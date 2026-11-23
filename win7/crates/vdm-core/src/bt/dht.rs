//! DHT (BEP-5) trackerless peer discovery — KRPC codec, XOR routing table,
//! announce tokens, iterative `get_peers` lookup with a client-side announce.
//!
//! Engine-internal: the BT engine creates one DHT per magnet download,
//! lazily at `download_blocking` entry, and drops it with the download —
//! torrent-file tasks and non-BT engines never bind a DHT socket (the idle
//! no-op promise, observable via [`ACTIVE_SOCKETS`]).
//!
//! Public-network contract: the production bootstrap list is the three
//! public BEP-5 routers, resolved by DNS at runtime only, one bootstrap
//! round per run plus one retry. Tests inject the local mini-DHT via
//! `Dht::start_with_bootstrap` — the test suite never touches the public
//! network.

use crate::bt::bencode::{self, Value};
use crate::bt::metainfo::sha1_20;
use crate::error::{Error, Result};
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Live DHT sockets right now (test observable: 0 with no BT magnet task).
pub static ACTIVE_SOCKETS: AtomicUsize = AtomicUsize::new(0);

/// Bucket size / announce fan-out (BEP-5 K).
const K: usize = 8;
/// Iterative lookup concurrency (BEP-5 α).
const ALPHA: usize = 3;
/// Routing-table hard cap — the split-infinite-table degenerate guard.
const MAX_TABLE_ENTRIES: usize = 1024;
/// Outstanding transaction cap; new queries defer when reached.
const MAX_PENDING_TX: usize = 64;
/// Single retransmit after 2 s; the transaction is abandoned after it.
const RETRANSMIT_AFTER: Duration = Duration::from_secs(2);
const ABANDON_AFTER: Duration = Duration::from_secs(4);
/// Token secret rotation period.
const TOKEN_WINDOW_SECS: u64 = 300;
/// Lookup depth cap (rounds).
const MAX_LOOKUP_DEPTH: usize = 20;
/// Single-use token ledger cap per window.
const TOKEN_LEDGER_CAP: usize = 1024;
/// One bootstrap round per run + one retry.
const MAX_BOOTSTRAP_ATTEMPTS: usize = 2;
/// Public BEP-5 bootstrap routers (resolved lazily at runtime, never in tests).
const PUBLIC_BOOTSTRAP: [(&str, u16); 3] = [
    ("router.bittorrent.com", 6881),
    ("dht.transmissionbt.com", 6881),
    ("dht.libtorrent.org", 6881),
];

/// KRPC error codes we emit (BEP-5): bad announce token, unknown method.
const ERR_PROTOCOL: i64 = 203;
const ERR_METHOD_UNKNOWN: i64 = 204;

// ---------------------------------------------------------------------------
// Node identity
// ---------------------------------------------------------------------------

/// Path of the persisted DHT node id under a download directory.
pub fn node_id_path(out_dir: &Path) -> PathBuf {
    out_dir.join(".vdm").join("dht-node-id")
}

/// Load or create the 20-byte node id: atomic write (same-dir temp +
/// rename), tolerant read — a missing or corrupt file regenerates.
fn load_or_create_node_id(out_dir: &Path) -> [u8; 20] {
    let path = node_id_path(out_dir);
    if let Ok(bytes) = std::fs::read(&path) {
        if bytes.len() == 20 {
            let mut id = [0u8; 20];
            id.copy_from_slice(&bytes);
            return id;
        }
        eprintln!("dht-node-id: wrong length ({}) — regenerating", bytes.len());
    }
    let r = crate::util::random_bytes(20);
    let mut id = [0u8; 20];
    id.copy_from_slice(&r);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path_with_suffix(&path, ".tmp");
    if std::fs::write(&tmp, id).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
    id
}

/// Append a suffix without `with_extension` (it would replace "id").
fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

// ---------------------------------------------------------------------------
// KRPC (BEP-5 message encoding)
// ---------------------------------------------------------------------------

/// A query we can send or serve. `Other` covers unknown method names that
/// arrive from the network (answered with 204); we never send them ourselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KrpcQuery {
    Ping {
        id: [u8; 20],
    },
    FindNode {
        id: [u8; 20],
        target: [u8; 20],
    },
    GetPeers {
        id: [u8; 20],
        info_hash: [u8; 20],
    },
    AnnouncePeer {
        id: [u8; 20],
        info_hash: [u8; 20],
        port: u16,
        token: Vec<u8>,
        implied_port: bool,
    },
    Other(String),
}

/// A reply we can send or consume. A bare `{id}` reply is a ping reply and
/// doubles as the announce_peer ack (identical wire shape per BEP-5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KrpcReply {
    Ping {
        id: [u8; 20],
    },
    FindNode {
        id: [u8; 20],
        nodes: Vec<NodeInfo>,
    },
    GetPeers {
        id: [u8; 20],
        token: Vec<u8>,
        values: Vec<SocketAddr>,
        nodes: Vec<NodeInfo>,
    },
    AnnouncePeer {
        id: [u8; 20],
    },
}

/// One decoded KRPC datagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KrpcIn {
    Query {
        tid: [u8; 2],
        q: KrpcQuery,
    },
    Reply {
        tid: [u8; 2],
        r: KrpcReply,
    },
    Error {
        tid: [u8; 2],
        code: i64,
        message: String,
    },
}

/// A node as carried in compact `nodes` strings: 20-byte id + ip + port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NodeInfo {
    pub id: [u8; 20],
    pub addr: SocketAddr,
}

fn krpc_query(tid: &[u8; 2], q: &KrpcQuery) -> Vec<u8> {
    let (name, a): (&str, Vec<(Vec<u8>, Value)>) = match q {
        KrpcQuery::Ping { id } => ("ping", vec![(b"id".to_vec(), Value::Str(id.to_vec()))]),
        KrpcQuery::FindNode { id, target } => (
            "find_node",
            vec![
                (b"id".to_vec(), Value::Str(id.to_vec())),
                (b"target".to_vec(), Value::Str(target.to_vec())),
            ],
        ),
        KrpcQuery::GetPeers { id, info_hash } => (
            "get_peers",
            vec![
                (b"id".to_vec(), Value::Str(id.to_vec())),
                (b"info_hash".to_vec(), Value::Str(info_hash.to_vec())),
            ],
        ),
        KrpcQuery::AnnouncePeer {
            id,
            info_hash,
            port,
            token,
            implied_port,
        } => {
            let mut a = vec![
                (b"id".to_vec(), Value::Str(id.to_vec())),
                (b"info_hash".to_vec(), Value::Str(info_hash.to_vec())),
                (b"port".to_vec(), Value::Int(*port as i64)),
                (b"token".to_vec(), Value::Str(token.clone())),
            ];
            if *implied_port {
                a.push((b"implied_port".to_vec(), Value::Int(1)));
            }
            ("announce_peer", a)
        }
        KrpcQuery::Other(name) => (name.as_str(), Vec::new()),
    };
    bencode::encode(&Value::Dict(vec![
        (b"a".to_vec(), Value::Dict(a)),
        (b"q".to_vec(), Value::Str(name.as_bytes().to_vec())),
        (b"t".to_vec(), Value::Str(tid.to_vec())),
        (b"y".to_vec(), Value::Str(b"q".to_vec())),
    ]))
}

fn krpc_reply(tid: &[u8; 2], r: &KrpcReply) -> Vec<u8> {
    let rdict: Vec<(Vec<u8>, Value)> = match r {
        KrpcReply::Ping { id } | KrpcReply::AnnouncePeer { id } => {
            vec![(b"id".to_vec(), Value::Str(id.to_vec()))]
        }
        KrpcReply::FindNode { id, nodes } => vec![
            (b"id".to_vec(), Value::Str(id.to_vec())),
            (b"nodes".to_vec(), Value::Str(encode_nodes_compact(nodes))),
        ],
        KrpcReply::GetPeers {
            id,
            token,
            values,
            nodes,
        } => {
            let mut d = vec![
                (b"id".to_vec(), Value::Str(id.to_vec())),
                (b"token".to_vec(), Value::Str(token.clone())),
            ];
            if !values.is_empty() {
                let vs: Vec<Value> = values
                    .iter()
                    .filter_map(|a| encode_compact_peer(a).map(|p| Value::Str(p.to_vec())))
                    .collect();
                d.push((b"values".to_vec(), Value::List(vs)));
            }
            if !nodes.is_empty() {
                d.push((b"nodes".to_vec(), Value::Str(encode_nodes_compact(nodes))));
            }
            d
        }
    };
    bencode::encode(&Value::Dict(vec![
        (b"r".to_vec(), Value::Dict(rdict)),
        (b"t".to_vec(), Value::Str(tid.to_vec())),
        (b"y".to_vec(), Value::Str(b"r".to_vec())),
    ]))
}

fn krpc_error(tid: &[u8; 2], code: i64, message: &str) -> Vec<u8> {
    bencode::encode(&Value::Dict(vec![
        (
            b"e".to_vec(),
            Value::List(vec![
                Value::Int(code),
                Value::Str(message.as_bytes().to_vec()),
            ]),
        ),
        (b"t".to_vec(), Value::Str(tid.to_vec())),
        (b"y".to_vec(), Value::Str(b"e".to_vec())),
    ]))
}

/// Decode one KRPC datagram. Malformed messages return `None` — dropped
/// silently, never a panic (ids, hashes and compact strings are untrusted
/// network input and length-validated here).
pub(crate) fn decode_krpc(data: &[u8]) -> Option<KrpcIn> {
    let v = bencode::decode(data).ok()?;
    let tid: [u8; 2] = {
        let t = v.dict_get(b"t").and_then(|x| x.as_bytes())?;
        if t.len() != 2 {
            return None;
        }
        [t[0], t[1]]
    };
    let y = v.dict_get(b"y").and_then(|x| x.as_bytes())?;
    if y.len() != 1 {
        return None;
    }
    match y[0] {
        b'q' => {
            let qname =
                String::from_utf8_lossy(v.dict_get(b"q").and_then(|x| x.as_bytes())?).into_owned();
            let a = v.dict_get(b"a")?;
            let q = match qname.as_str() {
                "ping" => KrpcQuery::Ping {
                    id: get_id(a, b"id")?,
                },
                "find_node" => KrpcQuery::FindNode {
                    id: get_id(a, b"id")?,
                    target: get_id(a, b"target")?,
                },
                "get_peers" => KrpcQuery::GetPeers {
                    id: get_id(a, b"id")?,
                    info_hash: get_id(a, b"info_hash")?,
                },
                "announce_peer" => {
                    let port = get_u16(a, b"port")?;
                    let token = a.dict_get(b"token").and_then(|x| x.as_bytes())?.to_vec();
                    let implied_port =
                        a.dict_get(b"implied_port").and_then(|x| x.as_int()) == Some(1);
                    KrpcQuery::AnnouncePeer {
                        id: get_id(a, b"id")?,
                        info_hash: get_id(a, b"info_hash")?,
                        port,
                        token,
                        implied_port,
                    }
                }
                other => KrpcQuery::Other(other.to_string()),
            };
            Some(KrpcIn::Query { tid, q })
        }
        b'r' => {
            let r = v.dict_get(b"r")?;
            let id = get_id(r, b"id")?;
            let reply = if r.dict_get(b"token").and_then(|x| x.as_bytes()).is_some() {
                let token = r.dict_get(b"token").and_then(|x| x.as_bytes())?.to_vec();
                let values: Vec<SocketAddr> = match r.dict_get(b"values").and_then(|x| x.as_list())
                {
                    Some(items) => items
                        .iter()
                        .filter_map(|e| e.as_bytes())
                        .filter(|b| b.len() == 6)
                        .map(peer_from_compact)
                        .collect(),
                    None => Vec::new(),
                };
                let nodes = match r.dict_get(b"nodes").and_then(|x| x.as_bytes()) {
                    Some(n) => parse_compact_nodes(n),
                    None => Vec::new(),
                };
                KrpcReply::GetPeers {
                    id,
                    token,
                    values,
                    nodes,
                }
            } else if r.dict_get(b"nodes").and_then(|x| x.as_bytes()).is_some() {
                let nodes = r.dict_get(b"nodes").and_then(|x| x.as_bytes())?;
                KrpcReply::FindNode {
                    id,
                    nodes: parse_compact_nodes(nodes),
                }
            } else {
                KrpcReply::Ping { id }
            };
            Some(KrpcIn::Reply { tid, r: reply })
        }
        b'e' => {
            let list = v.dict_get(b"e").and_then(|x| x.as_list())?;
            let code = list.first().and_then(|c| c.as_int())?;
            let message = list
                .get(1)
                .and_then(|m| m.as_str_lossy())
                .unwrap_or_default();
            Some(KrpcIn::Error { tid, code, message })
        }
        _ => None,
    }
}

fn get_id(v: &Value, key: &[u8]) -> Option<[u8; 20]> {
    let b = v.dict_get(key).and_then(|x| x.as_bytes())?;
    if b.len() == 20 {
        let mut id = [0u8; 20];
        id.copy_from_slice(b);
        Some(id)
    } else {
        None
    }
}

fn get_u16(v: &Value, key: &[u8]) -> Option<u16> {
    v.dict_get(key)
        .and_then(|x| x.as_int())
        .and_then(|i| u16::try_from(i).ok())
}

// ---------------------------------------------------------------------------
// Compact encodings (BEP-5: node = 26 bytes, peer = 6 bytes)
// ---------------------------------------------------------------------------

fn peer_from_compact(b: &[u8]) -> SocketAddr {
    SocketAddr::from(([b[0], b[1], b[2], b[3]], u16::from_be_bytes([b[4], b[5]])))
}

/// Parse a compact peer string; a trailing partial entry is dropped.
/// (The `values` replies arrive as lists of 6-byte strings — this string
/// form is the tracker-compact shape; kept as the codec's test inverse.)
#[cfg(test)]
pub(crate) fn parse_compact_peers(data: &[u8]) -> Vec<SocketAddr> {
    data.chunks(6)
        .filter(|c| c.len() == 6)
        .map(peer_from_compact)
        .collect()
}

/// Parse a compact node string; a trailing partial entry is dropped.
pub(crate) fn parse_compact_nodes(data: &[u8]) -> Vec<NodeInfo> {
    data.chunks(26)
        .filter(|c| c.len() == 26)
        .map(|c| {
            let mut id = [0u8; 20];
            id.copy_from_slice(&c[..20]);
            NodeInfo {
                id,
                addr: peer_from_compact(&c[20..]),
            }
        })
        .collect()
}

/// Compact peer (IPv4 only — IPv6 peers have no 6-byte form).
pub(crate) fn encode_compact_peer(addr: &SocketAddr) -> Option<[u8; 6]> {
    match addr.ip() {
        IpAddr::V4(ip) => {
            let mut out = [0u8; 6];
            out[..4].copy_from_slice(&ip.octets());
            out[4..].copy_from_slice(&addr.port().to_be_bytes());
            Some(out)
        }
        IpAddr::V6(_) => None,
    }
}

pub(crate) fn encode_compact_node(id: &[u8; 20], addr: &SocketAddr) -> Option<[u8; 26]> {
    let peer = encode_compact_peer(addr)?;
    let mut out = [0u8; 26];
    out[..20].copy_from_slice(id);
    out[20..].copy_from_slice(&peer);
    Some(out)
}

fn encode_nodes_compact(nodes: &[NodeInfo]) -> Vec<u8> {
    let mut out = Vec::with_capacity(nodes.len() * 26);
    for n in nodes {
        if let Some(bytes) = encode_compact_node(&n.id, &n.addr) {
            out.extend_from_slice(&bytes);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// XOR distance / bucket math
// ---------------------------------------------------------------------------

pub(crate) fn xor_distance(a: &[u8; 20], b: &[u8; 20]) -> [u8; 20] {
    let mut d = [0u8; 20];
    for i in 0..20 {
        d[i] = a[i] ^ b[i];
    }
    d
}

/// Bucket index = leading zero bits of the XOR distance (0..=159).
pub(crate) fn bucket_index(distance: &[u8; 20]) -> usize {
    for (i, &b) in distance.iter().enumerate() {
        if b != 0 {
            return i * 8 + b.leading_zeros() as usize;
        }
    }
    160
}

/// Whether `a` is closer to `target` than `b` (byte-wise XOR comparison).
pub(crate) fn cmp_distance(target: &[u8; 20], a: &[u8; 20], b: &[u8; 20]) -> std::cmp::Ordering {
    for i in 0..20 {
        let da = target[i] ^ a[i];
        let db = target[i] ^ b[i];
        if da != db {
            return da.cmp(&db);
        }
    }
    std::cmp::Ordering::Equal
}

// ---------------------------------------------------------------------------
// Routing table (160 K-buckets)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub id: [u8; 20],
    pub addr: SocketAddr,
    pub last_seen: Instant,
}

/// Result of a candidate insert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InsertOutcome {
    /// Fresh node inserted into a bucket with room.
    Inserted,
    /// Known node refreshed (moved to its bucket's most-recent end).
    Refreshed,
    /// Our own id — never inserted.
    RejectedOwnId,
    /// Table at the hard cap — overflow is ignored.
    OverflowIgnored,
    /// Bucket full: the least-fresh incumbent must be pinged; only its
    /// failure may admit `candidate` (the poisoning guard).
    NeedsPingCheck { incumbent: Entry, candidate: Entry },
}

pub(crate) struct RoutingTable {
    our_id: [u8; 20],
    buckets: Vec<VecDeque<Entry>>,
    total: usize,
}

impl RoutingTable {
    pub(crate) fn new(our_id: [u8; 20]) -> RoutingTable {
        RoutingTable {
            our_id,
            buckets: (0..160).map(|_| VecDeque::new()).collect(),
            total: 0,
        }
    }

    pub(crate) fn total(&self) -> usize {
        self.total
    }

    pub(crate) fn insert(&mut self, id: [u8; 20], addr: SocketAddr) -> InsertOutcome {
        if id == self.our_id {
            return InsertOutcome::RejectedOwnId;
        }
        if self.total >= MAX_TABLE_ENTRIES {
            return InsertOutcome::OverflowIgnored;
        }
        let distance = xor_distance(&self.our_id, &id);
        let idx = bucket_index(&distance);
        if idx >= 160 {
            return InsertOutcome::RejectedOwnId;
        }
        let bucket = &mut self.buckets[idx];
        if let Some(pos) = bucket.iter().position(|e| e.id == id) {
            // duplicates merge: refresh + LRU move to the most-recent end
            let mut e = bucket.remove(pos).unwrap();
            e.addr = addr;
            e.last_seen = Instant::now();
            bucket.push_back(e);
            return InsertOutcome::Refreshed;
        }
        if bucket.len() < K {
            bucket.push_back(Entry {
                id,
                addr,
                last_seen: Instant::now(),
            });
            self.total += 1;
            return InsertOutcome::Inserted;
        }
        // full bucket: the least-fresh incumbent is the deque front
        InsertOutcome::NeedsPingCheck {
            incumbent: bucket.front().unwrap().clone(),
            candidate: Entry {
                id,
                addr,
                last_seen: Instant::now(),
            },
        }
    }

    /// Remove a node (after a failed ping).
    pub(crate) fn remove(&mut self, id: &[u8; 20]) {
        let distance = xor_distance(&self.our_id, id);
        let idx = bucket_index(&distance);
        if idx >= 160 {
            return;
        }
        let bucket = &mut self.buckets[idx];
        if let Some(pos) = bucket.iter().position(|e| &e.id == id) {
            bucket.remove(pos);
            self.total -= 1;
        }
    }

    /// Admit a candidate whose bucket's incumbent failed its ping check.
    pub(crate) fn admit_after_failure(&mut self, candidate: Entry) -> InsertOutcome {
        if candidate.id == self.our_id {
            return InsertOutcome::RejectedOwnId;
        }
        let distance = xor_distance(&self.our_id, &candidate.id);
        let idx = bucket_index(&distance);
        if idx >= 160 {
            return InsertOutcome::RejectedOwnId;
        }
        let bucket = &mut self.buckets[idx];
        if let Some(pos) = bucket.iter().position(|e| e.id == candidate.id) {
            let mut e = bucket.remove(pos).unwrap();
            e.addr = candidate.addr;
            e.last_seen = candidate.last_seen;
            bucket.push_back(e);
            return InsertOutcome::Refreshed;
        }
        if bucket.len() < K && self.total < MAX_TABLE_ENTRIES {
            bucket.push_back(candidate);
            self.total += 1;
            return InsertOutcome::Inserted;
        }
        InsertOutcome::OverflowIgnored
    }

    /// The `count` entries closest to `target` by XOR distance.
    pub(crate) fn closest(&self, target: &[u8; 20], count: usize) -> Vec<Entry> {
        let mut all: Vec<Entry> = self
            .buckets
            .iter()
            .flat_map(|b| b.iter().cloned())
            .collect();
        all.sort_by(|a, b| cmp_distance(target, &a.id, &b.id));
        all.truncate(count);
        all
    }
}

// ---------------------------------------------------------------------------
// Announce tokens
// ---------------------------------------------------------------------------

/// Single-use ledger for announce tokens, bounded by `cap` (oldest evicted).
struct SingleUseLedger {
    set: HashSet<[u8; 20]>,
    order: VecDeque<[u8; 20]>,
    cap: usize,
}

impl SingleUseLedger {
    fn new() -> SingleUseLedger {
        SingleUseLedger {
            set: HashSet::new(),
            order: VecDeque::new(),
            cap: TOKEN_LEDGER_CAP,
        }
    }
    fn contains(&self, token: &[u8; 20]) -> bool {
        self.set.contains(token)
    }
    fn record(&mut self, token: [u8; 20]) {
        if self.set.insert(token) {
            self.order.push_back(token);
            while self.order.len() > self.cap {
                if let Some(old) = self.order.pop_front() {
                    self.set.remove(&old);
                }
            }
        }
    }
}

/// Token store: a random 64-bit secret rotated every 5-minute window; a
/// token matches the current OR the immediately previous window and is
/// single-use per window. Tokens are never logged.
pub(crate) struct TokenStore {
    current_epoch: u64,
    current_secret: u64,
    /// Epoch of the previous window when the last rotation was contiguous
    /// (None after a multi-window jump — old tokens expire with it).
    previous_epoch: Option<u64>,
    previous_secret: u64,
    spent_current: SingleUseLedger,
    spent_previous: SingleUseLedger,
}

fn epoch_of(now_secs: u64) -> u64 {
    now_secs / TOKEN_WINDOW_SECS
}

fn random_u64() -> u64 {
    let b = crate::util::random_bytes(8);
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&b);
    u64::from_be_bytes(arr)
}

/// Token mix (ours, not an HMAC): SHA1(secret || info_hash || source ip).
fn token_mix(secret: u64, info_hash: &[u8; 20], ip: &IpAddr) -> [u8; 20] {
    let mut buf = Vec::with_capacity(8 + 20 + 16);
    buf.extend_from_slice(&secret.to_be_bytes());
    buf.extend_from_slice(info_hash);
    match ip {
        IpAddr::V4(v) => buf.extend_from_slice(&v.octets()),
        IpAddr::V6(v) => buf.extend_from_slice(&v.octets()),
    }
    sha1_20(&buf)
}

impl TokenStore {
    pub(crate) fn new(now_secs: u64) -> TokenStore {
        TokenStore {
            current_epoch: epoch_of(now_secs),
            current_secret: random_u64(),
            previous_epoch: None,
            previous_secret: 0,
            spent_current: SingleUseLedger::new(),
            spent_previous: SingleUseLedger::new(),
        }
    }

    fn ensure_epoch(&mut self, now_secs: u64) {
        let epoch = epoch_of(now_secs);
        if epoch <= self.current_epoch {
            return;
        }
        // rotate: the current window becomes the previous one only when the
        // clock advanced by exactly one window; a bigger jump expires it
        self.previous_epoch = if self.current_epoch + 1 == epoch {
            Some(self.current_epoch)
        } else {
            None
        };
        self.previous_secret = self.current_secret;
        self.spent_previous = std::mem::replace(&mut self.spent_current, SingleUseLedger::new());
        self.current_epoch = epoch;
        self.current_secret = random_u64();
    }

    /// Issue a token for `info_hash` to the given source address.
    pub(crate) fn issue(&mut self, info_hash: &[u8; 20], ip: &IpAddr, now_secs: u64) -> [u8; 20] {
        self.ensure_epoch(now_secs);
        token_mix(self.current_secret, info_hash, ip)
    }

    /// Validate and consume a token (single-use; current or previous window).
    pub(crate) fn check_consume(
        &mut self,
        info_hash: &[u8; 20],
        ip: &IpAddr,
        token: &[u8],
        now_secs: u64,
    ) -> bool {
        self.ensure_epoch(now_secs);
        let current = token_mix(self.current_secret, info_hash, ip);
        if token == current.as_slice() {
            if self.spent_current.contains(&current) {
                return false;
            }
            self.spent_current.record(current);
            return true;
        }
        if let Some(previous_epoch) = self.previous_epoch {
            if previous_epoch + 1 == self.current_epoch {
                let previous = token_mix(self.previous_secret, info_hash, ip);
                if token == previous.as_slice() {
                    if self.spent_previous.contains(&previous) {
                        return false;
                    }
                    self.spent_previous.record(previous);
                    return true;
                }
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// Node service
// ---------------------------------------------------------------------------

/// How a node bootstraps its routing table.
enum BootstrapKind {
    /// A fixed list (the local mini-DHT harness; an empty list never
    /// touches the public network).
    Fixed(Vec<SocketAddr>),
    /// The public BEP-5 routers — DNS-resolved at runtime only.
    Public,
}

struct PendingTx {
    /// The encoded query, kept for the single retransmit.
    wire: Vec<u8>,
    target: SocketAddr,
    /// Node id when known (None = bootstrap target; learned from the reply).
    target_id: Option<[u8; 20]>,
    kind: TxKind,
    sent_at: Instant,
    retransmitted: bool,
}

enum TxKind {
    /// Poisoning guard: if this ping fails, `candidate` enters the bucket.
    /// (Standalone pings and find_node queries are the deferred
    /// routing-table-refresh sub-stage — this stage's lookup is
    /// get_peers-driven.)
    PingCheck {
        candidate: Entry,
    },
    GetPeers {
        info_hash: [u8; 20],
        job: u64,
    },
    /// A client-side announce; the ack just refreshes the table.
    Announce,
}

/// Lookup phases (the terminal announce+deliver step is unified in
/// `finish_job`).
enum Phase {
    /// Issue bootstrap queries (skipped when the table is already warm).
    Bootstrap,
    /// Iterative get_peers rounds over the closest known nodes.
    Rounds,
    Done,
}

struct KnownNode {
    id: [u8; 20],
    addr: SocketAddr,
    /// Token this node returned for the lookup's info_hash, if any.
    token: Option<Vec<u8>>,
    queried: bool,
}

struct LookupJob {
    id: u64,
    info_hash: [u8; 20],
    announce_port: u16,
    deadline: Instant,
    phase: Phase,
    known: Vec<KnownNode>,
    best_distance: [u8; 20],
    round_new_closer: bool,
    rounds_run: usize,
    in_flight: usize,
    peers: Vec<SocketAddr>,
    result: Option<mpsc::Sender<Vec<SocketAddr>>>,
}

struct DhtInner {
    our_id: [u8; 20],
    table: RoutingTable,
    tokens: TokenStore,
    tid_next: u16,
    pending: HashMap<[u8; 2], PendingTx>,
    active: Option<LookupJob>,
    queue: VecDeque<LookupJob>,
    job_counter: u64,
    /// Peers we hold for an info_hash (announcements + registered locals).
    peers_by_hash: HashMap<[u8; 20], Vec<SocketAddr>>,
    bootstrap_kind: BootstrapKind,
    bootstrap_attempts: usize,
    resolved_public: Option<Vec<SocketAddr>>,
}

struct DhtImp {
    addr: SocketAddr,
    socket: UdpSocket,
    state: Mutex<DhtInner>,
    stop: AtomicBool,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// One DHT node. Cheap to clone (clones share the service thread; only the
/// owning instance stops it on drop).
pub struct Dht {
    imp: Arc<DhtImp>,
    handle: Option<JoinHandle<()>>,
}

impl Clone for Dht {
    fn clone(&self) -> Dht {
        Dht {
            imp: self.imp.clone(),
            handle: None,
        }
    }
}

impl Dht {
    /// Production node: bootstrap = the public BEP-5 routers, resolved
    /// lazily at runtime. One socket per download; dropped with it.
    pub fn start(out_dir: &Path, bind: &str) -> Result<Dht> {
        Dht::start_inner(out_dir, bind, BootstrapKind::Public)
    }

    /// Harness/config constructor. `bootstrap`: `None` = the public routers;
    /// `Some(addrs)` pins a fixed list (the mini-DHT e2e injects its
    /// loopback node; `Some(vec![])` never touches the public network).
    pub fn start_with_bootstrap(
        out_dir: &Path,
        bind: &str,
        bootstrap: Option<Vec<SocketAddr>>,
    ) -> Result<Dht> {
        let kind = match bootstrap {
            Some(v) => BootstrapKind::Fixed(v),
            None => BootstrapKind::Public,
        };
        Dht::start_inner(out_dir, bind, kind)
    }

    fn start_inner(out_dir: &Path, bind: &str, bootstrap: BootstrapKind) -> Result<Dht> {
        let socket = UdpSocket::bind(bind)
            .map_err(|e| Error::Torrent(format!("dht bind {}: {}", bind, e)))?;
        let addr = socket
            .local_addr()
            .map_err(|e| Error::Torrent(format!("dht local_addr: {}", e)))?;
        let our_id = load_or_create_node_id(out_dir);
        let state = DhtInner {
            our_id,
            table: RoutingTable::new(our_id),
            tokens: TokenStore::new(now_secs()),
            tid_next: 0,
            pending: HashMap::new(),
            active: None,
            queue: VecDeque::new(),
            job_counter: 0,
            peers_by_hash: HashMap::new(),
            bootstrap_kind: bootstrap,
            bootstrap_attempts: 0,
            resolved_public: None,
        };
        let imp = Arc::new(DhtImp {
            addr,
            socket,
            state: Mutex::new(state),
            stop: AtomicBool::new(false),
        });
        let loop_imp = imp.clone();
        let handle = std::thread::spawn(move || service_loop(loop_imp));
        ACTIVE_SOCKETS.fetch_add(1, Ordering::Relaxed);
        Ok(Dht {
            imp,
            handle: Some(handle),
        })
    }

    /// The bound local address.
    pub fn local_addr(&self) -> SocketAddr {
        self.imp.addr
    }

    /// Register a peer we hold locally for `info_hash` (harness hook, the
    /// same role `TrackerServer::register` plays for the tracker harness).
    pub fn register(&self, info_hash: [u8; 20], addr: SocketAddr) {
        let mut st = self.imp.state.lock().unwrap();
        let list = st.peers_by_hash.entry(info_hash).or_default();
        if !list.contains(&addr) {
            list.push(addr);
        }
    }

    /// Routing-table size (diagnostics / tests).
    pub fn table_len(&self) -> usize {
        self.imp.state.lock().unwrap().table.total()
    }

    /// Iterative `get_peers` lookup for `info_hash`, then the client-side
    /// `announce_peer` to the K closest token holders with `announce_port`.
    /// Blocking with `budget`; stop-aware. Returns the resolved peers
    /// (possibly empty when the lookup fails or finds none).
    pub fn lookup_peers(
        &self,
        info_hash: [u8; 20],
        announce_port: u16,
        budget: Duration,
    ) -> Vec<SocketAddr> {
        let (tx, rx) = mpsc::channel();
        {
            let mut st = self.imp.state.lock().unwrap();
            st.job_counter += 1;
            let job_id = st.job_counter;
            st.queue.push_back(LookupJob {
                id: job_id,
                info_hash,
                announce_port,
                deadline: Instant::now() + budget,
                phase: Phase::Bootstrap, // re-evaluated at the first advance
                known: Vec::new(),
                best_distance: [0xFF; 20],
                round_new_closer: false,
                rounds_run: 0,
                in_flight: 0,
                peers: Vec::new(),
                result: Some(tx),
            });
        }
        // small grace beyond the budget so the service thread can deliver
        // the result it computed at the job deadline
        let deadline = Instant::now() + budget + Duration::from_millis(500);
        loop {
            match rx.try_recv() {
                Ok(peers) => return peers,
                Err(mpsc::TryRecvError::Disconnected) => return Vec::new(),
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if self.imp.stop.load(Ordering::Relaxed) {
                return Vec::new();
            }
            if Instant::now() >= deadline {
                return Vec::new();
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// Stop the service thread (owner only; clones are inert).
    pub fn stop(&mut self) {
        if self.handle.is_none() {
            return;
        }
        self.imp.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
            ACTIVE_SOCKETS.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

impl Drop for Dht {
    fn drop(&mut self) {
        self.stop();
    }
}

fn service_loop(imp: Arc<DhtImp>) {
    let _ = imp.socket.set_nonblocking(true);
    let mut buf = vec![0u8; 2048];
    loop {
        if imp.stop.load(Ordering::Relaxed) {
            break;
        }
        // drain all datagrams currently queued on the socket
        loop {
            match imp.socket.recv_from(&mut buf) {
                Ok((n, src)) => {
                    let now = now_secs();
                    let mut st = imp.state.lock().unwrap();
                    handle_datagram(&mut st, &imp.socket, src, &buf[..n], now);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        {
            let mut st = imp.state.lock().unwrap();
            sweep_pending(&mut st, &imp.socket);
            advance_jobs(&mut st, &imp.socket);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn handle_datagram(st: &mut DhtInner, socket: &UdpSocket, src: SocketAddr, data: &[u8], now: u64) {
    let msg = match decode_krpc(data) {
        Some(m) => m,
        None => return, // malformed — dropped silently
    };
    match msg {
        KrpcIn::Query { tid, q } => {
            // a valid query proves liveness too
            let qid = match &q {
                KrpcQuery::Ping { id }
                | KrpcQuery::FindNode { id, .. }
                | KrpcQuery::GetPeers { id, .. }
                | KrpcQuery::AnnouncePeer { id, .. } => Some(*id),
                KrpcQuery::Other(_) => None,
            };
            if let Some(qid) = qid {
                insert_node(st, socket, qid, src);
            }
            match q {
                KrpcQuery::Ping { .. } => {
                    let wire = krpc_reply(&tid, &KrpcReply::Ping { id: st.our_id });
                    let _ = socket.send_to(&wire, src);
                }
                KrpcQuery::FindNode { target, .. } => {
                    let nodes: Vec<NodeInfo> = st
                        .table
                        .closest(&target, K)
                        .into_iter()
                        .map(|e| NodeInfo {
                            id: e.id,
                            addr: e.addr,
                        })
                        .collect();
                    let wire = krpc_reply(
                        &tid,
                        &KrpcReply::FindNode {
                            id: st.our_id,
                            nodes,
                        },
                    );
                    let _ = socket.send_to(&wire, src);
                }
                KrpcQuery::GetPeers { info_hash, .. } => {
                    let token = st.tokens.issue(&info_hash, &src.ip(), now);
                    let values: Vec<SocketAddr> = st
                        .peers_by_hash
                        .get(&info_hash)
                        .cloned()
                        .unwrap_or_default();
                    let nodes: Vec<NodeInfo> = if values.is_empty() {
                        st.table
                            .closest(&info_hash, K)
                            .into_iter()
                            .map(|e| NodeInfo {
                                id: e.id,
                                addr: e.addr,
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    let wire = krpc_reply(
                        &tid,
                        &KrpcReply::GetPeers {
                            id: st.our_id,
                            token: token.to_vec(),
                            values,
                            nodes,
                        },
                    );
                    let _ = socket.send_to(&wire, src);
                }
                KrpcQuery::AnnouncePeer {
                    info_hash,
                    port,
                    token,
                    implied_port,
                    ..
                } => {
                    let peer_port = if implied_port { src.port() } else { port };
                    if !st.tokens.check_consume(&info_hash, &src.ip(), &token, now) {
                        let wire = krpc_error(&tid, ERR_PROTOCOL, "protocol error: invalid token");
                        let _ = socket.send_to(&wire, src);
                        return;
                    }
                    let list = st.peers_by_hash.entry(info_hash).or_default();
                    let peer = SocketAddr::new(src.ip(), peer_port);
                    if !list.contains(&peer) {
                        list.push(peer);
                    }
                    let wire = krpc_reply(&tid, &KrpcReply::AnnouncePeer { id: st.our_id });
                    let _ = socket.send_to(&wire, src);
                }
                KrpcQuery::Other(name) => {
                    let wire = krpc_error(
                        &tid,
                        ERR_METHOD_UNKNOWN,
                        &format!("method unknown: {}", name),
                    );
                    let _ = socket.send_to(&wire, src);
                }
            }
        }
        KrpcIn::Reply { tid, r } => handle_reply(st, socket, src, tid, r),
        KrpcIn::Error { tid, .. } => {
            // negative answers carry no node id — release the transaction
            if let Some(tx) = st.pending.remove(&tid) {
                if let TxKind::GetPeers { job, .. } = tx.kind {
                    if let Some(job) = st.active.as_mut().filter(|j| j.id == job) {
                        job.in_flight = job.in_flight.saturating_sub(1);
                    }
                }
            }
        }
    }
}

fn handle_reply(
    st: &mut DhtInner,
    socket: &UdpSocket,
    src: SocketAddr,
    tid: [u8; 2],
    r: KrpcReply,
) {
    let tx = match st.pending.remove(&tid) {
        Some(tx) => tx,
        None => return, // unsolicited / fire-and-forget
    };
    // any valid reply proves liveness — refresh/insert the replier
    let reply_id = match &r {
        KrpcReply::Ping { id } => *id,
        KrpcReply::FindNode { id, .. } => *id,
        KrpcReply::GetPeers { id, .. } => *id,
        KrpcReply::AnnouncePeer { id } => *id,
    };
    insert_node(st, socket, reply_id, src);
    match tx.kind {
        TxKind::GetPeers { info_hash, job } => match r {
            KrpcReply::GetPeers {
                token,
                values,
                nodes,
                ..
            } => {
                job_on_get_peers_reply(st, job, info_hash, reply_id, src, token, values, nodes);
            }
            _ => {
                // bare ack — release the round slot
                if let Some(job) = st.active.as_mut().filter(|j| j.id == job) {
                    job.in_flight = job.in_flight.saturating_sub(1);
                }
            }
        },
        TxKind::PingCheck { candidate } => {
            // incumbent alive: refreshed above, the candidate is dropped
            let _ = candidate;
        }
        TxKind::Announce => {}
    }
}

#[allow(clippy::too_many_arguments)] // fixed BEP-5 field set
fn job_on_get_peers_reply(
    st: &mut DhtInner,
    job_id: u64,
    _info_hash: [u8; 20],
    reply_id: [u8; 20],
    src: SocketAddr,
    token: Vec<u8>,
    values: Vec<SocketAddr>,
    nodes: Vec<NodeInfo>,
) {
    let our_id = st.our_id;
    let job = match st.active.as_mut().filter(|j| j.id == job_id) {
        Some(j) => j,
        None => return,
    };
    // the replier itself (bootstrap targets arrive here unknown)
    if reply_id != our_id {
        let d = xor_distance(&job.info_hash, &reply_id);
        if cmp_distance(&job.info_hash, &reply_id, &job.best_distance) == std::cmp::Ordering::Less {
            job.best_distance = d;
            job.round_new_closer = true;
        }
        let tok = if token.is_empty() { None } else { Some(token) };
        if let Some(k) = job.known.iter_mut().find(|k| k.id == reply_id) {
            k.addr = src;
            if tok.is_some() {
                k.token = tok;
            }
        } else {
            job.known.push(KnownNode {
                id: reply_id,
                addr: src,
                token: tok,
                queried: true,
            });
        }
    }
    for n in nodes {
        if n.id == our_id || job.known.iter().any(|k| k.id == n.id) {
            continue;
        }
        let d = xor_distance(&job.info_hash, &n.id);
        if cmp_distance(&job.info_hash, &n.id, &job.best_distance) == std::cmp::Ordering::Less {
            job.best_distance = d;
            job.round_new_closer = true;
        }
        job.known.push(KnownNode {
            id: n.id,
            addr: n.addr,
            token: None,
            queried: false,
        });
    }
    for p in values {
        if !job.peers.contains(&p) {
            job.peers.push(p);
        }
    }
    job.in_flight = job.in_flight.saturating_sub(1);
}

/// Insert a node into the table; a full bucket triggers the poisoning-guard
/// ping instead of an unconditional replace.
fn insert_node(st: &mut DhtInner, socket: &UdpSocket, id: [u8; 20], addr: SocketAddr) {
    if id == st.our_id {
        return;
    }
    if let InsertOutcome::NeedsPingCheck {
        incumbent,
        candidate,
    } = st.table.insert(id, addr)
    {
        send_query(
            st,
            socket,
            incumbent.addr,
            Some(incumbent.id),
            TxKind::PingCheck { candidate },
            KrpcQuery::Ping { id: st.our_id },
        );
    }
}

fn send_query(
    st: &mut DhtInner,
    socket: &UdpSocket,
    target: SocketAddr,
    target_id: Option<[u8; 20]>,
    kind: TxKind,
    q: KrpcQuery,
) -> bool {
    if st.pending.len() >= MAX_PENDING_TX {
        return false; // deferred; the caller retries on a later sweep
    }
    let tid = alloc_tid(st);
    let wire = krpc_query(&tid, &q);
    match socket.send_to(&wire, target) {
        Ok(_) => {
            st.pending.insert(
                tid,
                PendingTx {
                    wire,
                    target,
                    target_id,
                    kind,
                    sent_at: Instant::now(),
                    retransmitted: false,
                },
            );
            true
        }
        Err(_) => false,
    }
}

fn alloc_tid(st: &mut DhtInner) -> [u8; 2] {
    loop {
        let tid = st.tid_next.to_be_bytes();
        st.tid_next = st.tid_next.wrapping_add(1);
        if !st.pending.contains_key(&tid) {
            return tid;
        }
    }
}

/// Single retransmit after 2 s; abandon the transaction after the
/// retransmit window (no unbounded retry loops).
fn sweep_pending(st: &mut DhtInner, socket: &UdpSocket) {
    let now = Instant::now();
    let mut expired: Vec<[u8; 2]> = Vec::new();
    for (tid, tx) in st.pending.iter_mut() {
        let elapsed = now.duration_since(tx.sent_at);
        if !tx.retransmitted && elapsed >= RETRANSMIT_AFTER {
            let _ = socket.send_to(&tx.wire, tx.target);
            tx.retransmitted = true;
        } else if tx.retransmitted && elapsed >= ABANDON_AFTER {
            expired.push(*tid);
        }
    }
    for tid in expired {
        if let Some(tx) = st.pending.remove(&tid) {
            on_tx_failed(st, tx);
        }
    }
}

fn on_tx_failed(st: &mut DhtInner, tx: PendingTx) {
    match tx.kind {
        TxKind::PingCheck { candidate } => {
            // the incumbent failed its ping — safe to admit the candidate
            if let Some(incumbent_id) = tx.target_id {
                st.table.remove(&incumbent_id);
            }
            st.table.admit_after_failure(candidate);
        }
        TxKind::GetPeers { job, .. } => {
            if let Some(node_id) = tx.target_id {
                st.table.remove(&node_id);
            }
            if let Some(job) = st.active.as_mut().filter(|j| j.id == job) {
                job.in_flight = job.in_flight.saturating_sub(1);
            }
        }
        TxKind::Announce => {}
    }
}

fn bootstrap_targets(st: &mut DhtInner) -> Vec<SocketAddr> {
    match &st.bootstrap_kind {
        BootstrapKind::Fixed(list) => list.clone(),
        BootstrapKind::Public => {
            if st.resolved_public.is_none() {
                let mut addrs = Vec::new();
                for (host, port) in PUBLIC_BOOTSTRAP {
                    if let Ok(it) = (host, port).to_socket_addrs() {
                        for a in it {
                            addrs.push(a);
                        }
                    }
                }
                st.resolved_public = Some(addrs);
            }
            st.resolved_public.clone().unwrap_or_default()
        }
    }
}

fn advance_jobs(st: &mut DhtInner, socket: &UdpSocket) {
    if st.active.is_none() {
        if let Some(mut job) = st.queue.pop_front() {
            // seed candidates from a warm routing table — no bootstrap needed
            let seeds = st.table.closest(&job.info_hash, 2 * K);
            for e in seeds {
                if !job.known.iter().any(|k| k.id == e.id) {
                    job.known.push(KnownNode {
                        id: e.id,
                        addr: e.addr,
                        token: None,
                        queried: false,
                    });
                }
            }
            if !job.known.is_empty() {
                job.phase = Phase::Rounds;
            }
            st.active = Some(job);
        }
    }
    let mut job = match st.active.take() {
        Some(j) => j,
        None => return,
    };
    if job.in_flight > 0 {
        st.active = Some(job);
        return;
    }
    // a `true` return from a phase step means the lookup terminated — the
    // announce + deliver step runs right here, never a silent drop
    let finished = if Instant::now() >= job.deadline {
        true // budget over — announce + deliver whatever was collected
    } else {
        match job.phase {
            Phase::Bootstrap => advance_bootstrap(st, socket, &mut job),
            Phase::Rounds => advance_rounds(st, socket, &mut job),
            Phase::Done => true,
        }
    };
    if finished {
        finish_job(st, socket, &mut job);
    } else {
        st.active = Some(job);
    }
}

fn advance_bootstrap(st: &mut DhtInner, socket: &UdpSocket, job: &mut LookupJob) -> bool {
    if !job.known.is_empty() {
        // the bootstrap round taught us nodes — iterate from here
        job.phase = Phase::Rounds;
        return false;
    }
    if st.bootstrap_attempts >= MAX_BOOTSTRAP_ATTEMPTS {
        return true; // one round + one retry — give up
    }
    let addrs = bootstrap_targets(st);
    if addrs.is_empty() {
        // nothing configured (or unresolvable): this counts as exhausted
        st.bootstrap_attempts = MAX_BOOTSTRAP_ATTEMPTS;
        return false;
    }
    let mut queued = 0usize;
    for addr in addrs {
        if send_query(
            st,
            socket,
            addr,
            None,
            TxKind::GetPeers {
                info_hash: job.info_hash,
                job: job.id,
            },
            KrpcQuery::GetPeers {
                id: st.our_id,
                info_hash: job.info_hash,
            },
        ) {
            queued += 1;
        }
    }
    if queued == 0 {
        return false; // transaction cap — retry on a later sweep
    }
    st.bootstrap_attempts += 1;
    job.in_flight = queued;
    false
}

fn advance_rounds(st: &mut DhtInner, socket: &UdpSocket, job: &mut LookupJob) -> bool {
    // termination: depth cap, or the last completed round yielded no node
    // closer to the info_hash than everything known before it
    if job.rounds_run > 0 && (!job.round_new_closer || job.rounds_run >= MAX_LOOKUP_DEPTH) {
        return true;
    }
    // next round: the α closest unqueried candidates
    let mut cands: Vec<([u8; 20], SocketAddr)> = job
        .known
        .iter()
        .filter(|k| !k.queried)
        .map(|k| (k.id, k.addr))
        .collect();
    cands.sort_by(|a, b| cmp_distance(&job.info_hash, &a.0, &b.0));
    cands.truncate(ALPHA);
    if cands.is_empty() {
        return true;
    }
    job.round_new_closer = false;
    let mut queued = 0usize;
    for (id, addr) in cands {
        if send_query(
            st,
            socket,
            addr,
            Some(id),
            TxKind::GetPeers {
                info_hash: job.info_hash,
                job: job.id,
            },
            KrpcQuery::GetPeers {
                id: st.our_id,
                info_hash: job.info_hash,
            },
        ) {
            if let Some(k) = job.known.iter_mut().find(|k| k.id == id) {
                k.queried = true;
            }
            queued += 1;
        }
    }
    if queued == 0 {
        return false; // transaction cap — retry on a later sweep
    }
    job.rounds_run += 1;
    job.in_flight = queued;
    false
}

/// Client-side announce to the K closest token holders, then deliver the
/// resolved peers to the waiting caller and retire the job.
fn finish_job(st: &mut DhtInner, socket: &UdpSocket, job: &mut LookupJob) {
    let mut holders: Vec<([u8; 20], SocketAddr, Vec<u8>)> = job
        .known
        .iter()
        .filter(|k| k.token.is_some())
        .map(|k| (k.id, k.addr, k.token.clone().unwrap_or_default()))
        .collect();
    holders.sort_by(|a, b| cmp_distance(&job.info_hash, &a.0, &b.0));
    for (id, addr, token) in holders.into_iter().take(K) {
        let _ = send_query(
            st,
            socket,
            addr,
            Some(id),
            TxKind::Announce,
            KrpcQuery::AnnouncePeer {
                id: st.our_id,
                info_hash: job.info_hash,
                port: job.announce_port,
                token,
                implied_port: false,
            },
        );
    }
    if let Some(sender) = job.result.take() {
        let _ = sender.send(job.peers.clone());
    }
    job.phase = Phase::Done;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    const SAMPLE_ID: [u8; 20] = *b"abcdefghij0123456789";
    const SAMPLE_TID: [u8; 2] = *b"aa";

    fn addr(port: u8) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port as u16))
    }

    #[test]
    fn krpc_bep5_literal_samples() {
        // ping query — the BEP-5 §Message encoding sample, byte-exact
        let wire = krpc_query(&SAMPLE_TID, &KrpcQuery::Ping { id: SAMPLE_ID });
        assert_eq!(
            wire,
            b"d1:ad2:id20:abcdefghij0123456789e1:q4:ping1:t2:aa1:y1:qe"
        );
        // ping reply — same sample set
        let rid: [u8; 20] = *b"mnopqrstuvwxyz123456";
        let rwire = krpc_reply(&SAMPLE_TID, &KrpcReply::Ping { id: rid });
        assert_eq!(rwire, b"d1:rd2:id20:mnopqrstuvwxyz123456e1:t2:aa1:y1:re");
        // error reply — sample shape (message text ours)
        let ewire = krpc_error(&SAMPLE_TID, 201, "foobar");
        assert_eq!(ewire, b"d1:eli201e6:foobare1:t2:aa1:y1:ee");
        // get_peers reply carrying values (hand-built literal)
        let peer: SocketAddr = "127.0.0.1:80".parse().unwrap();
        let g = krpc_reply(
            &SAMPLE_TID,
            &KrpcReply::GetPeers {
                id: SAMPLE_ID,
                token: b"tok1".to_vec(),
                values: vec![peer],
                nodes: Vec::new(),
            },
        );
        assert_eq!(
            g,
            b"d1:rd2:id20:abcdefghij01234567895:token4:tok16:valuesl6:\x7f\x00\x00\x01\x00\x50ee1:t2:aa1:y1:re"
        );
        // and the literals decode back
        match decode_krpc(b"d1:ad2:id20:abcdefghij0123456789e1:q4:ping1:t2:aa1:y1:qe") {
            Some(KrpcIn::Query {
                tid,
                q: KrpcQuery::Ping { id },
            }) => {
                assert_eq!(tid, SAMPLE_TID);
                assert_eq!(id, SAMPLE_ID);
            }
            other => panic!("bad decode: {:?}", other),
        }
        match decode_krpc(
            b"d1:rd2:id20:abcdefghij01234567895:token4:tok16:valuesl6:\x7f\x00\x00\x01\x00\x50ee1:t2:aa1:y1:re",
        ) {
            Some(KrpcIn::Reply {
                tid,
                r: KrpcReply::GetPeers { values, .. },
            }) => {
                assert_eq!(tid, SAMPLE_TID);
                assert_eq!(values, vec![peer]);
            }
            other => panic!("bad decode: {:?}", other),
        }
    }

    #[test]
    fn krpc_error_literals_roundtrip() {
        for (code, msg) in [
            (201i64, "generic error"),
            (202, "server error"),
            (ERR_PROTOCOL, "protocol error: invalid token"),
            (ERR_METHOD_UNKNOWN, "method unknown: x"),
        ] {
            let wire = krpc_error(&[0x01, 0x02], code, msg);
            match decode_krpc(&wire) {
                Some(KrpcIn::Error {
                    tid,
                    code: c,
                    message,
                }) => {
                    assert_eq!(tid, [1, 2]);
                    assert_eq!(c, code);
                    assert_eq!(message, msg);
                }
                other => panic!("bad decode: {:?}", other),
            }
        }
    }

    #[test]
    fn krpc_roundtrip_all_kinds() {
        let cases: Vec<KrpcQuery> = vec![
            KrpcQuery::Ping { id: SAMPLE_ID },
            KrpcQuery::FindNode {
                id: SAMPLE_ID,
                target: [9u8; 20],
            },
            KrpcQuery::GetPeers {
                id: SAMPLE_ID,
                info_hash: [3u8; 20],
            },
            KrpcQuery::AnnouncePeer {
                id: SAMPLE_ID,
                info_hash: [3u8; 20],
                port: 6881,
                token: b"tok".to_vec(),
                implied_port: true,
            },
        ];
        for q in &cases {
            let wire = krpc_query(&SAMPLE_TID, q);
            match decode_krpc(&wire) {
                Some(KrpcIn::Query { tid, q: back }) => {
                    assert_eq!(tid, SAMPLE_TID);
                    assert_eq!(&back, q);
                }
                other => panic!("bad decode: {:?}", other),
            }
        }
        let peer: SocketAddr = "10.1.2.3:6881".parse().unwrap();
        let replies: Vec<KrpcReply> = vec![
            KrpcReply::Ping { id: SAMPLE_ID },
            KrpcReply::FindNode {
                id: SAMPLE_ID,
                nodes: vec![NodeInfo {
                    id: [7u8; 20],
                    addr: peer,
                }],
            },
            KrpcReply::GetPeers {
                id: SAMPLE_ID,
                token: b"t".to_vec(),
                values: vec![peer],
                nodes: Vec::new(),
            },
        ];
        for r in &replies {
            let wire = krpc_reply(&SAMPLE_TID, r);
            match decode_krpc(&wire) {
                Some(KrpcIn::Reply { tid, r: back }) => {
                    assert_eq!(tid, SAMPLE_TID);
                    assert_eq!(&back, r);
                }
                other => panic!("bad decode: {:?}", other),
            }
        }
        // the announce ack is a bare {id} reply — the same wire shape as a
        // ping reply per BEP-5; decode yields Ping
        let ack = krpc_reply(&SAMPLE_TID, &KrpcReply::AnnouncePeer { id: SAMPLE_ID });
        match decode_krpc(&ack) {
            Some(KrpcIn::Reply {
                tid,
                r: KrpcReply::Ping { id },
            }) => {
                assert_eq!(tid, SAMPLE_TID);
                assert_eq!(id, SAMPLE_ID);
            }
            other => panic!("bad decode: {:?}", other),
        }
    }

    #[test]
    fn krpc_malformed_dropped() {
        // truncated
        assert!(decode_krpc(b"d1:ad2:id20:abcdefghij01234567").is_none());
        // trailing bytes after the document
        assert!(decode_krpc(b"d1:ad2:id20:abcdefghij01234567891:q4:ping1:t2:aa1:y1:qee").is_none());
        // transaction id with the wrong length
        assert!(
            decode_krpc(b"d1:ad2:id20:abcdefghij01234567891:q4:ping1:t3:aaay1:y1:qe").is_none()
        );
        // missing y
        assert!(decode_krpc(b"d1:ad2:id20:abcdefghij01234567891:q4:ping1:t2:aae").is_none());
        // short id
        assert!(decode_krpc(b"d1:ad2:id10:abcdefghij1:q4:ping1:t2:aa1:y1:qe").is_none());
        // garbage
        assert!(decode_krpc(b"not-krpc-at-all").is_none());
        // announce_peer without a port
        assert!(decode_krpc(
            b"d1:ad2:id20:abcdefghij01234567899:info_hash20:abcdefghij01234567895:token1:t1:q13:announce_peer1:t2:aa1:y1:qe"
        )
        .is_none());
    }

    #[test]
    fn compact_codecs() {
        let a: SocketAddr = "10.0.0.1:6881".parse().unwrap();
        let p = encode_compact_peer(&a).unwrap();
        assert_eq!(parse_compact_peers(&p), vec![a]);
        // a trailing partial entry is dropped
        let mut bad = p.to_vec();
        bad.push(0x01);
        assert_eq!(parse_compact_peers(&bad), vec![a]);
        assert_eq!(parse_compact_peers(&[]), Vec::<SocketAddr>::new());
        // IPv6 peers have no compact form
        let v6: SocketAddr = "[::1]:6881".parse().unwrap();
        assert!(encode_compact_peer(&v6).is_none());
        // nodes
        let id = [7u8; 20];
        let n = encode_compact_node(&id, &a).unwrap();
        assert_eq!(parse_compact_nodes(&n), vec![NodeInfo { id, addr: a }]);
        let mut badn = n.to_vec();
        badn.extend_from_slice(&[0, 0, 0]);
        assert_eq!(parse_compact_nodes(&badn), vec![NodeInfo { id, addr: a }]);
        assert_eq!(parse_compact_nodes(&[]), Vec::<NodeInfo>::new());
    }

    #[test]
    fn xor_and_bucket_math() {
        let a = [0u8; 20];
        let b = {
            let mut x = [0u8; 20];
            x[0] = 0x80;
            x
        };
        let c = {
            let mut x = [0u8; 20];
            x[19] = 0x01;
            x
        };
        let d = {
            let mut x = [0u8; 20];
            x[19] = 0x40;
            x
        };
        assert_eq!(bucket_index(&xor_distance(&a, &b)), 0);
        assert_eq!(bucket_index(&xor_distance(&a, &c)), 159);
        assert_eq!(bucket_index(&xor_distance(&a, &d)), 153);
        // b differs in the top bit (distance 0x80…) — farther than c,
        // whose distance is a single low bit in the last byte (…01)
        assert_eq!(cmp_distance(&a, &c, &b), Ordering::Less);
        assert_eq!(cmp_distance(&a, &c, &c), Ordering::Equal);
        assert_eq!(cmp_distance(&a, &b, &c), Ordering::Greater);
    }

    #[test]
    fn routing_table_ops() {
        let our = [0u8; 20];
        let mut t = RoutingTable::new(our);
        // our own id is never inserted
        assert_eq!(t.insert(our, addr(1)), InsertOutcome::RejectedOwnId);
        // ids differing only in the top bit of the last byte share bucket 152
        let mk = |b: u8| {
            let mut id = [0u8; 20];
            id[19] = b;
            id
        };
        for b in 0x80..=0x87u8 {
            assert_eq!(t.insert(mk(b), addr(b)), InsertOutcome::Inserted);
        }
        assert_eq!(t.total(), 8);
        // duplicates merge + LRU refresh
        assert_eq!(t.insert(mk(0x80), addr(0x80)), InsertOutcome::Refreshed);
        assert_eq!(t.total(), 8);
        // a full bucket demands a ping of the least-fresh incumbent
        match t.insert(mk(0x88), addr(0x88)) {
            InsertOutcome::NeedsPingCheck {
                incumbent,
                candidate,
            } => {
                // 0x80 moved to the back by the refresh → front is 0x81
                assert_eq!(incumbent.id, mk(0x81));
                assert_eq!(candidate.id, mk(0x88));
                // the incumbent fails the ping: remove + admit
                t.remove(&incumbent.id);
                assert_eq!(t.admit_after_failure(candidate), InsertOutcome::Inserted);
                assert_eq!(t.total(), 8);
            }
            other => panic!("expected NeedsPingCheck, got {:?}", other),
        }
        // closest-by-distance selection
        let near = mk(0x80); // distance 0x80… — closest possible here
        let entries = t.closest(&near, 1);
        assert_eq!(entries[0].id, near);
    }

    #[test]
    fn routing_table_overflow_cap() {
        let our = [0u8; 20];
        let mut t = RoutingTable::new(our);
        let mut inserted = 0usize;
        // fill 128 buckets (first nonzero byte at position 0..=15) x 8 ids
        for idx in 0..128usize {
            let p = idx / 8;
            let l = (idx % 8) as u32;
            for v in 1u8..=8 {
                let mut id = [0u8; 20];
                id[p] = 0xFFu8 >> l;
                id[19] = v;
                match t.insert(id, addr(v)) {
                    InsertOutcome::Inserted => inserted += 1,
                    other => panic!("unexpected outcome filling: {:?}", other),
                }
            }
        }
        assert_eq!(inserted, 1024);
        assert_eq!(t.total(), 1024);
        // beyond the cap: overflow is ignored even with bucket room
        let mut id = [0u8; 20];
        id[16] = 0xFF; // bucket 128, empty — but the table is capped
        assert_eq!(t.insert(id, addr(1)), InsertOutcome::OverflowIgnored);
        assert_eq!(t.total(), 1024);
    }

    #[test]
    fn token_rotation_single_use() {
        let hash = [1u8; 20];
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        let other_ip: IpAddr = "127.0.0.2".parse().unwrap();
        let mut ts = TokenStore::new(100);
        let t1 = ts.issue(&hash, &ip, 100);
        assert!(ts.check_consume(&hash, &ip, &t1, 100)); // valid once
        assert!(!ts.check_consume(&hash, &ip, &t1, 100)); // replay rejected
                                                          // bound to the source ip
        assert!(!ts.check_consume(&hash, &other_ip, &t1, 100));
        // bound to the info_hash
        assert!(!ts.check_consume(&[2u8; 20], &ip, &t1, 100));
        // re-issuing in the SAME window yields identical bytes — still spent
        let t1b = ts.issue(&hash, &ip, 100);
        assert_eq!(t1b, t1);
        assert!(!ts.check_consume(&hash, &ip, &t1b, 400)); // previous window, used
                                                           // an UNconsumed pair stays valid through the previous window…
        let hash_b = [5u8; 20];
        let t_b = ts.issue(&hash_b, &other_ip, 100);
        assert!(ts.check_consume(&hash_b, &other_ip, &t_b, 400));
        // …exactly once
        assert!(!ts.check_consume(&hash_b, &other_ip, &t_b, 400));
        // window 2: t_b is beyond the previous window — expired
        assert!(!ts.check_consume(&hash_b, &other_ip, &t_b, 700));
        // a multi-window jump invalidates the old previous window entirely
        let t3 = ts.issue(&hash, &ip, 400);
        assert!(!ts.check_consume(&hash, &ip, &t3, 1_500));
    }

    #[test]
    fn node_id_file_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let id1 = load_or_create_node_id(dir.path());
        let file = node_id_path(dir.path());
        assert_eq!(std::fs::read(&file).unwrap().len(), 20);
        // reload is stable
        let id2 = load_or_create_node_id(dir.path());
        assert_eq!(id1, id2);
        // corrupt file → regenerated, never fatal
        std::fs::write(&file, b"short").unwrap();
        let _id3 = load_or_create_node_id(dir.path());
        assert_eq!(std::fs::read(&file).unwrap().len(), 20);
    }
}
