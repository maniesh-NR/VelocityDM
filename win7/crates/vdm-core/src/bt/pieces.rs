//! Piece store: tracks availability across connected peers, assembles blocks
//! into pieces, verifies SHA-1, applies per-file selection priorities
//! (Motrix-style) and picks the next block (rarest-first or sequential).

use crate::bt::metainfo::{sha1_20, Metainfo};
use crate::error::{Error, Result};
use std::sync::Arc;

/// Result of feeding a block into the piece store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddOutcome {
    Partial,
    Complete(Vec<u8>),
    Corrupted,
    Duplicate,
    Invalid,
}

/// Standard BitTorrent block size.
pub const BLOCK_LEN: u32 = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockPick {
    /// Ask the peer for this block.
    Request { piece: u32, offset: u32, len: u32 },
    /// Nothing to request from this peer right now.
    None,
}

struct PartialPiece {
    data: Vec<u8>,
    blocks_done: Vec<bool>,
}

pub struct PieceStore {
    pub meta: Arc<Metainfo>,
    pub have: Vec<bool>,
    /// Number of connected peers that have each piece.
    pub availability: Vec<u32>,
    partials: Vec<Option<PartialPiece>>,
    /// Per-torrent-file priority; 0 = skipped.
    pub file_priorities: Vec<u8>,
    needed: Vec<bool>,
    /// Completed piece bytes among needed pieces (progress numerator).
    pub have_bytes: u64,
    needed_bytes_total: u64,
    sequential: bool,
}

impl PieceStore {
    pub fn new(
        meta: Arc<Metainfo>,
        file_priorities: Vec<u8>,
        sequential: bool,
    ) -> Result<PieceStore> {
        if file_priorities.len() != meta.files.len() {
            return Err(Error::Torrent(format!(
                "priority vector length {} != file count {}",
                file_priorities.len(),
                meta.files.len()
            )));
        }
        let n = meta.piece_count();
        let mut needed = vec![false; n];
        for i in 0..n {
            needed[i] = meta
                .files_overlapping_piece(i as u32)
                .iter()
                .any(|&fi| file_priorities[fi] > 0);
        }
        let needed_bytes_total: u64 = (0..n as u32)
            .filter(|&i| needed[i as usize])
            .map(|i| meta.piece_len_at(i))
            .sum();
        Ok(PieceStore {
            meta,
            have: vec![false; n],
            availability: vec![0; n],
            partials: (0..n).map(|_| None).collect(),
            file_priorities,
            needed,
            have_bytes: 0,
            needed_bytes_total,
            sequential,
        })
    }

    pub fn needed_bytes(&self) -> u64 {
        self.needed_bytes_total - self.have_bytes
    }

    pub fn total_needed_bytes(&self) -> u64 {
        self.needed_bytes_total
    }

    pub fn all_needed_have(&self) -> bool {
        (0..self.have.len()).all(|i| !self.needed[i] || self.have[i])
    }

    /// A peer connected with an optional initial bitfield.
    pub fn peer_add(&mut self, bitfield: Option<&[u8]>) {
        match bitfield {
            Some(bf) => {
                for i in 0..self.have.len() {
                    if bitfield_has(bf, i as u32) {
                        self.availability[i] += 1;
                    }
                }
            }
            None => {
                // assume the peer has everything (older clients without bitfield)
                for a in self.availability.iter_mut() {
                    *a += 1;
                }
            }
        }
    }

    pub fn peer_remove(&mut self, bitfield: Option<&[u8]>) {
        match bitfield {
            Some(bf) => {
                for i in 0..self.have.len() {
                    if bitfield_has(bf, i as u32) {
                        self.availability[i] = self.availability[i].saturating_sub(1);
                    }
                }
            }
            None => {
                for a in self.availability.iter_mut() {
                    *a = a.saturating_sub(1);
                }
            }
        }
    }

    pub fn peer_have(&mut self, piece: u32) {
        if let Some(a) = self.availability.get_mut(piece as usize) {
            *a += 1;
        }
    }

    pub fn blocks_in_piece(&self, piece: u32) -> u32 {
        let len = self.meta.piece_len_at(piece);
        ((len + BLOCK_LEN as u64 - 1) / BLOCK_LEN as u64) as u32
    }

    /// Pick the next block to request from a peer whose bitfield is `peer_has`.
    pub fn next_block(&mut self, peer_has: &dyn Fn(u32) -> bool) -> BlockPick {
        // 1) continue a partial piece this peer can serve
        let candidates: Vec<u32> = (0..self.have.len() as u32)
            .filter(|&i| self.needed[i as usize] && !self.have[i as usize] && peer_has(i))
            .collect();
        if candidates.is_empty() {
            return BlockPick::None;
        }
        for &i in &candidates {
            if let Some(p) = &self.partials[i as usize] {
                if let Some(b) = (0..p.blocks_done.len()).find(|&b| !p.blocks_done[b]) {
                    return BlockPick::Request {
                        piece: i,
                        offset: b as u32 * BLOCK_LEN,
                        len: self.block_len(i, b as u32),
                    };
                }
            }
        }
        // 2) pick a new piece: rarest-first (or sequential)
        let choice = if self.sequential {
            candidates.iter().copied().min()
        } else {
            candidates
                .iter()
                .copied()
                .filter(|&i| self.partials[i as usize].is_none())
                .min_by_key(|&i| (self.availability[i as usize], i))
                .or_else(|| candidates.iter().copied().min())
        };
        match choice {
            Some(i) => {
                let plen = self.meta.piece_len_at(i) as usize;
                self.partials[i as usize] = Some(PartialPiece {
                    data: vec![0u8; plen],
                    blocks_done: vec![false; self.blocks_in_piece(i) as usize],
                });
                BlockPick::Request {
                    piece: i,
                    offset: 0,
                    len: self.block_len(i, 0),
                }
            }
            None => BlockPick::None,
        }
    }

    fn block_len(&self, piece: u32, block: u32) -> u32 {
        let plen = self.meta.piece_len_at(piece);
        let off = block as u64 * BLOCK_LEN as u64;
        (plen.saturating_sub(off)).min(BLOCK_LEN as u64) as u32
    }

    /// Feed a received block. Returns the verified piece data when it completes.
    pub fn add_block(&mut self, piece: u32, offset: u32, data: &[u8]) -> AddOutcome {
        let plen = self.meta.piece_len_at(piece);
        if offset as u64 + data.len() as u64 > plen {
            return AddOutcome::Invalid;
        }
        if self.have.get(piece as usize).copied().unwrap_or(false) {
            return AddOutcome::Duplicate;
        }
        let block = offset / BLOCK_LEN;
        let partial = match self
            .partials
            .get_mut(piece as usize)
            .and_then(|p| p.as_mut())
        {
            Some(p) => p,
            None => {
                // unsolicited block; start a partial implicitly
                let plen_s = plen as usize;
                self.partials[piece as usize] = Some(PartialPiece {
                    data: vec![0u8; plen_s],
                    blocks_done: vec![false; self.blocks_in_piece(piece) as usize],
                });
                self.partials[piece as usize].as_mut().unwrap()
            }
        };
        if block as usize >= partial.blocks_done.len() || partial.blocks_done[block as usize] {
            return AddOutcome::Duplicate;
        }
        let start = offset as usize;
        partial.data[start..start + data.len()].copy_from_slice(data);
        partial.blocks_done[block as usize] = true;
        if !partial.blocks_done.iter().all(|&b| b) {
            return AddOutcome::Partial;
        }
        // complete: verify
        let finished = self.partials[piece as usize].take().unwrap().data;
        if sha1_20(&finished) == self.meta.pieces[piece as usize] {
            AddOutcome::Complete(finished)
        } else {
            AddOutcome::Corrupted
        }
    }

    /// Mark a piece as downloaded and written (updates progress counters).
    pub fn mark_have(&mut self, piece: u32) {
        if let Some(h) = self.have.get_mut(piece as usize) {
            if *h {
                return;
            }
            *h = true;
        }
        self.partials[piece as usize] = None;
        if self.needed[piece as usize] {
            self.have_bytes += self.meta.piece_len_at(piece);
        }
    }

    /// Restore state (resume): mark pieces as present without re-hashing.
    pub fn restore_have(&mut self, pieces: &[u32]) {
        for &p in pieces {
            self.partials[p as usize] = None;
            if let Some(h) = self.have.get_mut(p as usize) {
                *h = true;
            }
            if self.needed[p as usize] {
                self.have_bytes += self.meta.piece_len_at(p);
            }
        }
    }

    /// Bitfield encoding of our `have` set.
    pub fn bitfield_bytes(&self) -> Vec<u8> {
        bitfield_encode(&self.have)
    }
}

pub fn bitfield_has(bf: &[u8], piece: u32) -> bool {
    let byte = (piece / 8) as usize;
    match bf.get(byte) {
        Some(&b) => (b >> (7 - (piece % 8))) & 1 == 1,
        None => false,
    }
}

pub fn bitfield_encode(have: &[bool]) -> Vec<u8> {
    let mut out = vec![0u8; (have.len() + 7) / 8];
    for (i, &h) in have.iter().enumerate() {
        if h {
            out[i / 8] |= 1 << (7 - (i % 8));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bt::metainfo::{create_torrent, CreateInput};

    fn test_store(
        dir: &std::path::Path,
        piece_len: u64,
        sizes: &[usize],
    ) -> (PieceStore, Vec<Vec<u8>>, Vec<u8>) {
        let mut inputs = Vec::new();
        let mut datas = Vec::new();
        for (i, &sz) in sizes.iter().enumerate() {
            let data: Vec<u8> = (0..sz).map(|j| (i * 31 + j) as u8).collect();
            let p = dir.join(format!("f{}.bin", i));
            std::fs::write(&p, &data).unwrap();
            inputs.push(CreateInput {
                path: vec![format!("f{}.bin", i)],
                fs_path: p,
            });
            datas.push(data);
        }
        let (meta, torrent) = create_torrent(
            "bundle",
            &inputs,
            piece_len,
            Some("http://t/ann"),
            &[],
            false,
        )
        .unwrap();
        let store = PieceStore::new(Arc::new(meta), vec![1; sizes.len()], false).unwrap();
        (store, datas, torrent)
    }

    #[test]
    fn bitfield_codec() {
        let have = vec![true, false, true, true, false, false, false, false, true];
        let bf = bitfield_encode(&have);
        assert_eq!(bf.len(), 2);
        for (i, h) in have.iter().enumerate() {
            assert_eq!(bitfield_has(&bf, i as u32), *h);
        }
        assert!(!bitfield_has(&bf, 100));
    }

    #[test]
    fn pick_rarest_first() {
        let dir = tempfile::tempdir().unwrap();
        // 4 pieces
        let (mut store, _d, _t) = test_store(dir.path(), 1024, &[4096]);
        // peers: p0 has piece 0+1+2; p1 has piece 0+2+3; p2 has 0+1+2+3
        store.peer_add(Some(&bitfield_encode(&[true, true, true, false])));
        store.peer_add(Some(&bitfield_encode(&[true, false, true, true])));
        store.peer_add(Some(&bitfield_encode(&[true, true, true, true])));
        // availability: p0=3, p1=2, p2=3, p3=2 -> rarest are 1 and 3; min by (avail, idx) = 1
        let all = |_p: u32| true;
        match store.next_block(&all) {
            BlockPick::Request { piece, offset, len } => {
                assert_eq!(piece, 1);
                assert_eq!((offset, len), (0, 1024));
            }
            other => panic!("expected request, got {:?}", other),
        }
    }

    #[test]
    fn pick_respects_peer_bitfield() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _d, _t) = test_store(dir.path(), 1024, &[4096]);
        let none = |p: u32| {
            let _ = p;
            false
        };
        assert_eq!(store.next_block(&none), BlockPick::None);
        let only3 = |p: u32| p == 3;
        match store.next_block(&only3) {
            BlockPick::Request { piece, .. } => assert_eq!(piece, 3),
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn partial_piece_continuation() {
        let dir = tempfile::tempdir().unwrap();
        // one 32 KiB piece made of two 16 KiB blocks
        let (mut store, datas, _t) = test_store(dir.path(), 32 * 1024, &[32 * 1024]);
        let all = |_p: u32| true;
        match store.next_block(&all) {
            BlockPick::Request { piece, offset, len } => {
                assert_eq!((piece, offset, len), (0, 0, 16384));
            }
            other => panic!("{:?}", other),
        }
        // feed the second block first -> Partial
        match store.add_block(0, 16384, &datas[0][16384..]) {
            AddOutcome::Partial => {}
            o => panic!("{:?}", o),
        }
        // next pick must continue the partial piece at block 0
        match store.next_block(&all) {
            BlockPick::Request { piece, offset, .. } => {
                assert_eq!((piece, offset), (0, 0));
            }
            other => panic!("{:?}", other),
        }
        match store.add_block(0, 0, &datas[0][0..16384]) {
            AddOutcome::Complete(data) => assert_eq!(data, datas[0]),
            o => panic!("{:?}", o),
        }
        store.mark_have(0);
        assert_eq!(store.have_bytes, 32 * 1024);
        assert_eq!(store.needed_bytes(), 0);
        assert!(store.all_needed_have());
        assert_eq!(store.next_block(&all), BlockPick::None);
    }

    #[test]
    fn corrupted_piece_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _d, _t) = test_store(dir.path(), 1024, &[1024]);
        let bad = vec![0xABu8; 1024];
        match store.add_block(0, 0, &bad) {
            AddOutcome::Corrupted => {}
            o => panic!("{:?}", o),
        }
        assert!(!store.have[0]);
        // can request again
        let all = |_p: u32| true;
        assert!(matches!(store.next_block(&all), BlockPick::Request { .. }));
    }

    #[test]
    fn file_priority_skips_pieces() {
        let dir = tempfile::tempdir().unwrap();
        // two files, 2 pieces each, disjoint
        let mut inputs = Vec::new();
        let d0 = vec![1u8; 2048];
        let d1 = vec![2u8; 2048];
        std::fs::write(dir.path().join("a.bin"), &d0).unwrap();
        std::fs::write(dir.path().join("b.bin"), &d1).unwrap();
        inputs.push(CreateInput {
            path: vec!["a.bin".into()],
            fs_path: dir.path().join("a.bin"),
        });
        inputs.push(CreateInput {
            path: vec!["b.bin".into()],
            fs_path: dir.path().join("b.bin"),
        });
        let (meta, _) =
            create_torrent("two", &inputs, 2048, Some("http://t/ann"), &[], false).unwrap();
        let mut store = PieceStore::new(Arc::new(meta), vec![1, 0], false).unwrap();
        assert_eq!(store.total_needed_bytes(), 2048);
        let only_b = |p: u32| p >= 2;
        assert_eq!(store.next_block(&only_b), BlockPick::None);
        let only_a = |p: u32| p < 2;
        assert!(matches!(
            store.next_block(&only_a),
            BlockPick::Request { piece: 0, .. }
        ));
    }

    #[test]
    fn invalid_block_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _d, _t) = test_store(dir.path(), 1024, &[1024]);
        assert_eq!(store.add_block(0, 0, &[0u8; 2048]), AddOutcome::Invalid);
        assert_eq!(store.add_block(99, 0, &[0u8; 16]), AddOutcome::Invalid);
    }

    #[test]
    fn duplicate_blocks_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, datas, _t) = test_store(dir.path(), 1024, &[1024]);
        assert!(matches!(
            store.add_block(0, 0, &datas[0]),
            AddOutcome::Complete(_)
        ));
        store.mark_have(0);
        assert_eq!(store.add_block(0, 0, &datas[0]), AddOutcome::Duplicate);
    }

    #[test]
    fn availability_updates() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _d, _t) = test_store(dir.path(), 1024, &[2048]);
        let bf = bitfield_encode(&[true, false]);
        store.peer_add(Some(&bf));
        assert_eq!(store.availability, vec![1, 0]);
        store.peer_have(1);
        assert_eq!(store.availability, vec![1, 1]);
        store.peer_remove(Some(&bf));
        assert_eq!(store.availability, vec![0, 1]);
    }

    #[test]
    fn restore_have_state() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _d, _t) = test_store(dir.path(), 1024, &[2048]);
        store.restore_have(&[0]);
        assert!(store.have[0]);
        assert_eq!(store.needed_bytes(), 1024);
    }
}
