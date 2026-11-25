//! DASH (.mpd) download support (S6): bounded MPEG-DASH manifest parsing,
//! representation selection, segment addressing (SegmentTemplate /
//! SegmentTimeline / SegmentList / SegmentBase@indexRange + SIDX), and
//! sequential ordered assembly over one persistent connection.
//!
//! From-scratch conformance to the public ISO/IEC 23009-1 (MPEG-DASH MPD)
//! and ISO/IEC 14496-12 (SIDX box) shapes. Gate 0015
//! (`harness/ASSESSMENTS/0015-s6-dash.md`): static manifests only, first
//! Period only, video-or-loud-fail selection, a 100,000-segment cap (the
//! zip-bomb analog guard), checked arithmetic everywhere, and a bounded
//! in-module XML tokenizer whose DOCTYPE is skipped as literal markup —
//! entity expansion is structurally impossible (billion-laughs defense).

use crate::error::{Error, Result};
use crate::http::{ClientCfg, Conn, Request, Url};
use crate::token_bucket::TokenBucket;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Maximum nesting depth the tokenizer will walk (deeper ⇒ classified
/// error — the iterative scanner never recurses, so this bounds memory,
/// not the call stack).
const XML_MAX_DEPTH: usize = 128;

/// Attribute values are capped at 4 KiB (gate 0015 ATKR #6).
const XML_MAX_ATTR: usize = 4096;

/// Total media-segment cap per representation (gate 0015: the zip-bomb
/// analog guard; applies to timeline `@r` expansion and duration-based
/// counts alike).
const MAX_SEGMENTS: u64 = 100_000;

/// `SegmentBase@indexRange` index fetch cap.
const SIDX_FETCH_CAP: usize = 8 * 1024 * 1024;

/// Per-segment body cap (mirrors the HLS segment cap).
const SEGMENT_BODY_CAP: usize = 512 * 1024 * 1024;

// ---------------------------------------------------------------------------
// XML tokenizer (ours, in-module; gate 0015 ATKR #1/#6)
// ---------------------------------------------------------------------------

/// One scanned markup tag. Text between tags is validated for entity
/// references (undefined `&name;` ⇒ error) but otherwise skipped — the
/// MPD grammar is element-shaped.
#[derive(Debug)]
enum Tag {
    Open {
        name: String,
        attrs: Vec<(String, String)>,
        self_closed: bool,
    },
    Close(String),
    Eof,
}

/// Minimal non-validating XML scanner. Namespace prefixes are stripped
/// (local names only). `<!DOCTYPE ...>` is skipped as literal markup and
/// `<!ENTITY>` never defines an expansion, so an undefined `&name;`
/// reference is a classified error — entity expansion is structurally
/// impossible. Depth is tracked here and the scanner never recurses.
struct Scanner<'a> {
    src: &'a str,
    pos: usize,
    depth: usize,
}

impl<'a> Scanner<'a> {
    fn new(src: &'a str) -> Self {
        Scanner {
            src,
            pos: 0,
            depth: 0,
        }
    }

    fn err(what: &str) -> Error {
        Error::Dash(format!("bad manifest XML: {}", what))
    }

    /// Skip the next markup construct (declaration, comment, CDATA,
    /// processing instruction, DOCTYPE — literal, never expanded).
    fn skip_misc(&mut self) -> Result<bool> {
        let rest = &self.src[self.pos..];
        if rest.starts_with("<?") {
            let end = rest[2..]
                .find("?>")
                .map(|i| i + 4)
                .ok_or_else(|| Self::err("unterminated processing instruction"))?;
            self.pos += end;
            return Ok(true);
        }
        if rest.starts_with("<!--") {
            let end = rest[4..]
                .find("-->")
                .map(|i| i + 7)
                .ok_or_else(|| Self::err("unterminated comment"))?;
            self.pos += end;
            return Ok(true);
        }
        if rest.starts_with("<![CDATA[") {
            let end = rest[9..]
                .find("]]>")
                .map(|i| i + 12)
                .ok_or_else(|| Self::err("unterminated CDATA"))?;
            self.pos += end;
            return Ok(true);
        }
        if rest.starts_with("<!") {
            // DOCTYPE (with optional internal subset) — skipped as literal
            // markup; no entity table is ever built.
            let bytes = rest.as_bytes();
            let mut i = 2;
            let mut bracket = 0usize;
            while i < bytes.len() {
                match bytes[i] {
                    b'[' => bracket += 1,
                    b']' => bracket = bracket.saturating_sub(1),
                    b'>' if bracket == 0 => {
                        self.pos += i + 1;
                        return Ok(true);
                    }
                    _ => {}
                }
                i += 1;
            }
            return Err(Self::err("unterminated DOCTYPE"));
        }
        Ok(false)
    }

    /// Validate entity references in a text run. The five named entities
    /// and numeric character references are legal; anything else is the
    /// classified "undefined entity" error.
    fn validate_text_entities(&self, text: &str) -> Result<()> {
        let mut rest = text;
        while let Some(amp) = rest.find('&') {
            let Some(semi) = rest[amp..].find(';').map(|i| amp + i) else {
                return Err(Self::err("unterminated entity reference"));
            };
            if Self::decode_entity(&rest[amp + 1..semi])?.is_none() {
                return Err(Self::err("undefined entity reference"));
            }
            rest = &rest[semi + 1..];
        }
        Ok(())
    }

    fn decode_entity(name: &str) -> Result<Option<char>> {
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            n if n.len() > 2 && (n.starts_with("#x") || n.starts_with("#X")) => {
                u32::from_str_radix(&n[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            n if n.len() > 1
                && n.as_bytes()[1].is_ascii_digit()
                && n[1..].chars().all(|c| c.is_ascii_digit()) =>
            {
                n[1..].parse::<u32>().ok().and_then(char::from_u32)
            }
            _ => None,
        };
        Ok(decoded)
    }

    fn decode_entities(v: &str) -> Result<String> {
        let mut out = String::with_capacity(v.len());
        let mut rest = v;
        while let Some(amp) = rest.find('&') {
            let Some(semi) = rest[amp..].find(';').map(|i| amp + i) else {
                return Err(Self::err("unterminated entity reference"));
            };
            match Self::decode_entity(&rest[amp + 1..semi])? {
                Some(c) => {
                    out.push_str(&rest[..amp]);
                    out.push(c);
                }
                None => return Err(Self::err("undefined entity reference")),
            }
            rest = &rest[semi + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    fn local_name(raw: &str) -> String {
        match raw.rsplit_once(':') {
            Some((_, local)) if !local.is_empty() => local.to_string(),
            _ => raw.to_string(),
        }
    }

    /// Scan the next tag. Text runs between tags are entity-validated then
    /// skipped. Nesting depth is tracked here — the ONLY depth accounting,
    /// so unknown-element skipping is inherently iterative and capped.
    fn next_tag(&mut self) -> Result<Tag> {
        loop {
            // skip whitespace/text between tags, entity-validating the run
            let text_start = self.pos;
            while self.pos < self.src.len() && self.src.as_bytes()[self.pos] != b'<' {
                self.pos += 1;
            }
            if text_start < self.pos {
                self.validate_text_entities(&self.src[text_start..self.pos])?;
            }
            if self.pos >= self.src.len() {
                return Ok(Tag::Eof);
            }
            if self.skip_misc()? {
                continue;
            }
            let rest = &self.src[self.pos..];
            if let Some(close) = rest.strip_prefix("</") {
                let end = close
                    .find('>')
                    .ok_or_else(|| Self::err("unterminated close tag"))?;
                let raw = close[..end].trim();
                if raw.is_empty() {
                    return Err(Self::err("empty close tag name"));
                }
                self.pos += 2 + end + 1;
                if self.depth == 0 {
                    return Err(Self::err("unbalanced close tag"));
                }
                self.depth -= 1;
                return Ok(Tag::Close(Self::local_name(raw)));
            }
            // open tag: name + attributes
            let bytes = rest.as_bytes();
            let mut i = 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric()
                    || bytes[i] == b'_'
                    || bytes[i] == b':'
                    || bytes[i] == b'-'
                    || bytes[i] == b'.')
            {
                i += 1;
            }
            if i == 1 {
                return Err(Self::err("malformed start tag"));
            }
            let raw_name = &rest[1..i];
            let name = Self::local_name(raw_name);
            let mut attrs: Vec<(String, String)> = Vec::new();
            let mut self_closed = false;
            loop {
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i >= bytes.len() {
                    return Err(Self::err("unterminated start tag"));
                }
                if bytes[i] == b'>' {
                    i += 1;
                    break;
                }
                if bytes[i] == b'/' {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'>' {
                        self_closed = true;
                        i += 2;
                        break;
                    }
                    return Err(Self::err("malformed self-close"));
                }
                // attribute name
                let name_start = i;
                while i < bytes.len()
                    && bytes[i] != b'='
                    && bytes[i] != b'>'
                    && bytes[i] != b'/'
                    && !bytes[i].is_ascii_whitespace()
                {
                    i += 1;
                }
                let attr_name = rest[name_start..i].trim().to_string();
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i >= bytes.len() || bytes[i] != b'=' {
                    return Err(Self::err("attribute without value"));
                }
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i >= bytes.len() || (bytes[i] != b'"' && bytes[i] != b'\'') {
                    return Err(Self::err("attribute value not quoted"));
                }
                let quote = bytes[i] as char;
                i += 1;
                let val_start = i;
                while i < bytes.len() && bytes[i] != quote as u8 {
                    i += 1;
                }
                if i >= bytes.len() {
                    return Err(Self::err("unterminated attribute value"));
                }
                let raw_val = &rest[val_start..i];
                if raw_val.len() > XML_MAX_ATTR {
                    return Err(Self::err("attribute value exceeds 4 KiB"));
                }
                i += 1;
                attrs.push((attr_name, Self::decode_entities(raw_val)?));
            }
            self.pos += i;
            if !self_closed {
                self.depth += 1;
                if self.depth > XML_MAX_DEPTH {
                    return Err(Self::err("nesting depth exceeds 128"));
                }
            }
            return Ok(Tag::Open {
                name,
                attrs,
                self_closed,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// ISO-8601 duration (bounded subset: PTnHnMnS used by MPD attributes)
// ---------------------------------------------------------------------------

/// Parse an ISO-8601 duration (`PT…`, hours/minutes/seconds) into whole
/// seconds. Sub-second components round the current unit UP (ceil) so the
/// derived segment count never under-counts; the choice is recorded here.
/// Bounded input (≤ 32 chars), checked arithmetic, junk ⇒ classified error.
fn parse_iso_duration(s: &str) -> Result<u64> {
    let t = s.trim();
    if t.is_empty() || t.len() > 32 {
        return Err(Error::Dash(format!("bad duration '{}'", s)));
    }
    let Some(rest) = t.strip_prefix('P') else {
        return Err(Error::Dash(format!("bad duration '{}'", s)));
    };
    // Week form (PnW)
    if let Some(weeks_part) = rest.strip_suffix('W') {
        let weeks: u64 = weeks_part
            .parse()
            .map_err(|_| Error::Dash(format!("bad duration '{}'", s)))?;
        return weeks
            .checked_mul(7 * 86400)
            .filter(|v| *v > 0)
            .ok_or_else(|| Error::Dash(format!("bad duration '{}'", s)));
    }
    let Some(rest) = rest.strip_prefix('T') else {
        return Err(Error::Dash(format!("bad duration '{}'", s)));
    };
    if rest.is_empty() {
        return Err(Error::Dash(format!("bad duration '{}'", s)));
    }
    let mut total: u64 = 0;
    let mut num = String::new();
    let mut frac_seen = false;
    let mut unit_order = 0u8; // 1=H, 2=M, 3=S — must be ascending
    for c in rest.chars() {
        match c {
            '0'..='9' => {
                if frac_seen {
                    return Err(Error::Dash(format!("bad duration '{}'", s)));
                }
                if num.len() > 18 {
                    return Err(Error::Dash(format!("duration overflow '{}'", s)));
                }
                num.push(c);
            }
            '.' | ',' => {
                if num.is_empty() || frac_seen {
                    return Err(Error::Dash(format!("bad duration '{}'", s)));
                }
                frac_seen = true; // any fraction rounds the current unit up
            }
            'H' | 'M' | 'S' => {
                let n: u64 = if frac_seen {
                    1 // ceil of a fractional value
                } else {
                    num.parse()
                        .map_err(|_| Error::Dash(format!("bad duration '{}'", s)))?
                };
                num.clear();
                frac_seen = false;
                let order = match c {
                    'H' => 1u8,
                    'M' => 2,
                    _ => 3,
                };
                if order <= unit_order {
                    return Err(Error::Dash(format!("bad duration '{}'", s)));
                }
                unit_order = order;
                let mult = match c {
                    'H' => 3600u64,
                    'M' => 60,
                    _ => 1,
                };
                total = total
                    .checked_add(
                        n.checked_mul(mult)
                            .ok_or_else(|| Error::Dash(format!("duration overflow '{}'", s)))?,
                    )
                    .ok_or_else(|| Error::Dash(format!("duration overflow '{}'", s)))?;
            }
            _ => return Err(Error::Dash(format!("bad duration '{}'", s))),
        }
    }
    if !num.is_empty() {
        return Err(Error::Dash(format!("bad duration '{}'", s)));
    }
    if total == 0 {
        return Err(Error::Dash(format!("bad duration '{}'", s)));
    }
    Ok(total)
}

// ---------------------------------------------------------------------------
// MPD model
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
struct Template {
    initialization: Option<String>,
    media: Option<String>,
    timescale: Option<u64>,
    duration: Option<u64>,
    start_number: Option<u64>,
    presentation_time_offset: Option<u64>,
    timeline: Option<Vec<TlS>>,
}

#[derive(Debug, Clone, Copy)]
struct TlS {
    t: Option<u64>,
    d: u64,
    r: i64,
}

#[derive(Debug, Default, Clone)]
struct SegmentList {
    init_url: Option<String>,
    init_range: Option<(u64, u64)>,
    media: Vec<(String, Option<(u64, u64)>)>,
}

#[derive(Debug, Default, Clone)]
struct SegmentBase {
    index_range: Option<(u64, u64)>,
}

#[derive(Debug, Default, Clone)]
struct Representation {
    id: String,
    bandwidth: Option<u64>,
    base: Option<String>,
    template: Option<Template>,
    list: Option<SegmentList>,
    sbase: Option<SegmentBase>,
}

#[derive(Debug, Default, Clone)]
struct AdaptationSet {
    content_type: Option<String>,
    mime_type: Option<String>,
    has_protection: bool,
    base: Option<String>,
    template: Option<Template>,
    list: Option<SegmentList>,
    sbase: Option<SegmentBase>,
    components: Vec<String>,
    reps: Vec<Representation>,
}

#[derive(Debug, Default, Clone)]
struct Period {
    duration: Option<u64>,
    base: Option<String>,
    template: Option<Template>,
    sets: Vec<AdaptationSet>,
}

#[derive(Debug, Default)]
struct Mpd {
    dynamic: bool,
    media_presentation_duration: Option<u64>,
    base: Option<String>,
    periods: Vec<Period>,
}

/// Which model level a child-bearing element (SegmentTemplate,
/// SegmentList, SegmentTimeline) attached to.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Owner {
    Period,
    Set,
    Rep,
}

/// Parse the DASH byte-range form (`SegmentURL@mediaRange`,
/// `SegmentBase@indexRange`, `Initialization@range`): RFC 7233
/// `first-last`, inclusive. No `len@off` alias, no open-ended form, and
/// no other attribute name is accepted (the attributes are exactly
/// `mediaRange` / `indexRange` / `range`). Junk ⇒ classified error.
fn parse_byte_range_form(s: &str) -> Result<(u64, u64)> {
    let (first_s, last_s) = match s.split_once('-') {
        Some((f, l)) => (f, Some(l)),
        None => (s, None),
    };
    let first: u64 = first_s
        .trim()
        .parse()
        .map_err(|_| Error::Dash(format!("bad byte range '{}'", s)))?;
    let Some(last_s) = last_s else {
        return Err(Error::Dash(format!(
            "open-ended byte range '{}' is not supported",
            s
        )));
    };
    let last: u64 = last_s
        .trim()
        .parse()
        .map_err(|_| Error::Dash(format!("bad byte range '{}'", s)))?;
    if last < first {
        return Err(Error::Dash(format!("bad byte range '{}'", s)));
    }
    Ok((first, last - first + 1))
}

fn parse_u64_attr(attrs: &[(String, String)], name: &str) -> Result<Option<u64>> {
    match attrs.iter().find(|(k, _)| k == name) {
        Some((_, v)) => {
            let t = v.trim();
            if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit()) {
                return Err(Error::Dash(format!("bad {} '{}'", name, v)));
            }
            let n: u64 = t
                .parse()
                .map_err(|_| Error::Dash(format!("bad {} '{}'", name, v)))?;
            Ok(Some(n))
        }
        None => Ok(None),
    }
}

/// Merge a child template over its nearest ancestor (child wins per
/// attribute; `None` fields inherit).
fn merge_template(child: Option<Template>, parent: Option<&Template>) -> Option<Template> {
    match (child, parent) {
        (Some(c), Some(p)) => {
            let mut m = p.clone();
            if c.initialization.is_some() {
                m.initialization = c.initialization;
            }
            if c.media.is_some() {
                m.media = c.media;
            }
            if c.timescale.is_some() {
                m.timescale = c.timescale;
            }
            if c.duration.is_some() {
                m.duration = c.duration;
            }
            if c.start_number.is_some() {
                m.start_number = c.start_number;
            }
            if c.presentation_time_offset.is_some() {
                m.presentation_time_offset = c.presentation_time_offset;
            }
            if c.timeline.is_some() {
                m.timeline = c.timeline;
            }
            Some(m)
        }
        (Some(c), None) => Some(c),
        (None, Some(p)) => Some(p.clone()),
        (None, None) => None,
    }
}

/// Read the text content of a simple text-valued element
/// (`<BaseURL>…</BaseURL>`) directly off the scanner (entity-decoded),
/// consuming the close tag.
fn read_element_text(sc: &mut Scanner, name: &str) -> Result<String> {
    let mut out = String::new();
    loop {
        let start = sc.pos;
        while sc.pos < sc.src.len() && sc.src.as_bytes()[sc.pos] != b'<' {
            sc.pos += 1;
        }
        let raw = &sc.src[start..sc.pos];
        if !raw.is_empty() {
            sc.validate_text_entities(raw)?;
            out.push_str(&Scanner::decode_entities(raw)?);
        }
        if sc.pos >= sc.src.len() {
            return Err(Scanner::err("unterminated element"));
        }
        if sc.skip_misc()? {
            continue;
        }
        let rest = &sc.src[sc.pos..];
        let Some(close) = rest.strip_prefix("</") else {
            return Err(Scanner::err("unexpected markup inside text-only element"));
        };
        let end = close
            .find('>')
            .ok_or_else(|| Scanner::err("unterminated close tag"))?;
        let raw = close[..end].trim();
        let local = Scanner::local_name(raw);
        sc.pos += 2 + end + 1;
        if sc.depth == 0 {
            return Err(Scanner::err("unbalanced close tag"));
        }
        sc.depth -= 1;
        if local == name {
            return Ok(out);
        }
        return Err(Scanner::err("mismatched close tag"));
    }
}

/// Walk the token stream into the MPD model. Unknown elements are skipped
/// by construction (their tags simply produce no model state) — the
/// scanner's depth cap keeps the walk bounded and iterative.
fn parse_mpd(text: &str) -> Result<Mpd> {
    let mut sc = Scanner::new(text);
    let mut mpd = Mpd::default();
    let mut saw_mpd = false;
    let mut cur_period: Option<Period> = None;
    let mut cur_set: Option<AdaptationSet> = None;
    let mut cur_rep: Option<Representation> = None;
    // open-element context stack (element local names)
    let mut stack: Vec<String> = Vec::new();
    // in-flight SegmentTimeline S-children + their owning template level
    let mut tl_buf: Option<Vec<TlS>> = None;
    let mut tl_owner: Option<Owner> = None;
    // in-flight SegmentList children + their owning level
    let mut list_buf: Option<SegmentList> = None;
    let mut list_owner: Option<Owner> = None;

    macro_rules! attach_tl {
        ($tl:expr) => {
            match tl_owner {
                Some(Owner::Rep) => {
                    if let Some(r) = cur_rep.as_mut() {
                        if let Some(t) = r.template.as_mut() {
                            t.timeline = Some($tl);
                        }
                    }
                }
                Some(Owner::Set) => {
                    if let Some(s) = cur_set.as_mut() {
                        if let Some(t) = s.template.as_mut() {
                            t.timeline = Some($tl);
                        }
                    }
                }
                Some(Owner::Period) => {
                    if let Some(p) = cur_period.as_mut() {
                        if let Some(t) = p.template.as_mut() {
                            t.timeline = Some($tl);
                        }
                    }
                }
                None => {
                    return Err(Error::Dash(
                        "SegmentTimeline outside a SegmentTemplate".into(),
                    ))
                }
            }
        };
    }

    macro_rules! attach_list {
        ($sl:expr) => {
            match list_owner {
                Some(Owner::Rep) => {
                    if let Some(r) = cur_rep.as_mut() {
                        r.list = Some($sl);
                    }
                }
                Some(Owner::Set) => {
                    if let Some(s) = cur_set.as_mut() {
                        s.list = Some($sl);
                    }
                }
                Some(Owner::Period) => {
                    return Err(Error::Dash(
                        "SegmentList at Period level is not supported".into(),
                    ))
                }
                None => return Err(Error::Dash("SegmentList outside a representation".into())),
            }
        };
    }

    loop {
        match sc.next_tag()? {
            Tag::Eof => break,
            Tag::Close(name) => {
                let top = stack.pop();
                if top.as_deref() != Some(name.as_str()) {
                    return Err(Scanner::err("mismatched close tag"));
                }
                match name.as_str() {
                    "MPD" => saw_mpd = true,
                    "Period" => {
                        if let Some(p) = cur_period.take() {
                            mpd.periods.push(p);
                        }
                    }
                    "AdaptationSet" => {
                        if let (Some(p), Some(s)) = (cur_period.as_mut(), cur_set.take()) {
                            p.sets.push(s);
                        }
                    }
                    "Representation" => {
                        if let (Some(s), Some(r)) = (cur_set.as_mut(), cur_rep.take()) {
                            s.reps.push(r);
                        }
                    }
                    "SegmentTimeline" => {
                        if let Some(tl) = tl_buf.take() {
                            attach_tl!(tl);
                        }
                        tl_owner = None;
                    }
                    "SegmentList" => {
                        if let Some(sl) = list_buf.take() {
                            attach_list!(sl);
                        }
                        list_owner = None;
                    }
                    _ => {}
                }
            }
            Tag::Open {
                name,
                attrs,
                self_closed,
            } => {
                // xlink:href on MPD-relevant elements ⇒ loud-fail
                if attrs.iter().any(|(k, _)| k == "xlink:href") {
                    return Err(Error::Dash(
                        "xlink remote references are not supported".into(),
                    ));
                }
                match name.as_str() {
                    "MPD" => {
                        saw_mpd = true;
                        let typ = attrs
                            .iter()
                            .find(|(k, _)| k == "type")
                            .map(|(_, v)| v.trim().to_ascii_lowercase());
                        match typ.as_deref() {
                            Some("static") | None => {}
                            Some("dynamic") => {
                                return Err(Error::Dash(
                                    "live DASH (dynamic manifest) is not supported".into(),
                                ));
                            }
                            Some(other) => {
                                return Err(Error::Dash(format!("bad MPD@type '{}'", other)))
                            }
                        }
                        if typ.is_none() && attrs.iter().any(|(k, _)| k == "minimumUpdatePeriod") {
                            return Err(Error::Dash(
                                "live DASH (dynamic manifest) is not supported".into(),
                            ));
                        }
                        mpd.media_presentation_duration =
                            match attrs.iter().find(|(k, _)| k == "mediaPresentationDuration") {
                                Some((_, v)) => Some(parse_iso_duration(v)?),
                                None => None,
                            };
                    }
                    "BaseURL" => {
                        let url_text = read_element_text(&mut sc, "BaseURL")?;
                        let t = url_text.trim().to_string();
                        if !t.is_empty() {
                            match stack.last().map(|s| s.as_str()) {
                                Some("MPD") => mpd.base = Some(t),
                                Some("Period") => {
                                    if let Some(p) = cur_period.as_mut() {
                                        p.base = Some(t);
                                    }
                                }
                                Some("AdaptationSet") => {
                                    if let Some(s) = cur_set.as_mut() {
                                        s.base = Some(t);
                                    }
                                }
                                Some("Representation") => {
                                    if let Some(r) = cur_rep.as_mut() {
                                        r.base = Some(t);
                                    }
                                }
                                _ => {}
                            }
                        }
                        continue; // element fully consumed (no stack push)
                    }
                    "Period" => {
                        let dur = match attrs.iter().find(|(k, _)| k == "duration") {
                            Some((_, v)) => Some(parse_iso_duration(v)?),
                            None => None,
                        };
                        cur_period = Some(Period {
                            duration: dur,
                            ..Default::default()
                        });
                    }
                    "AdaptationSet" => {
                        let ct = attrs
                            .iter()
                            .find(|(k, _)| k == "contentType")
                            .map(|(_, v)| v.clone());
                        let mt = attrs
                            .iter()
                            .find(|(k, _)| k == "mimeType")
                            .map(|(_, v)| v.clone());
                        cur_set = Some(AdaptationSet {
                            content_type: ct,
                            mime_type: mt,
                            ..Default::default()
                        });
                    }
                    "ContentComponent" => {
                        if let Some(s) = cur_set.as_mut() {
                            if let Some((_, v)) = attrs.iter().find(|(k, _)| k == "contentType") {
                                s.components.push(v.clone());
                            }
                        }
                    }
                    "ContentProtection" => {
                        if let Some(s) = cur_set.as_mut() {
                            s.has_protection = true;
                        }
                    }
                    "Representation" => {
                        let id = attrs
                            .iter()
                            .find(|(k, _)| k == "id")
                            .map(|(_, v)| v.clone())
                            .unwrap_or_default();
                        let bw = parse_u64_attr(&attrs, "bandwidth")?;
                        cur_rep = Some(Representation {
                            id,
                            bandwidth: bw,
                            ..Default::default()
                        });
                    }
                    "SegmentTemplate" => {
                        let mut t = Template::default();
                        for (k, v) in &attrs {
                            match k.as_str() {
                                "initialization" => t.initialization = Some(v.clone()),
                                "media" => t.media = Some(v.clone()),
                                "timescale" => t.timescale = parse_u64_attr(&attrs, "timescale")?,
                                "duration" => t.duration = parse_u64_attr(&attrs, "duration")?,
                                "startNumber" => {
                                    t.start_number = parse_u64_attr(&attrs, "startNumber")?
                                }
                                "presentationTimeOffset" => {
                                    t.presentation_time_offset =
                                        parse_u64_attr(&attrs, "presentationTimeOffset")?
                                }
                                _ => {}
                            }
                        }
                        let owner =
                            match stack.last().map(|s| s.as_str()) {
                                Some("Period") => Owner::Period,
                                Some("AdaptationSet") => Owner::Set,
                                Some("Representation") => Owner::Rep,
                                _ => return Err(Error::Dash(
                                    "SegmentTemplate outside Period/AdaptationSet/Representation"
                                        .into(),
                                )),
                            };
                        match owner {
                            Owner::Period => {
                                if let Some(p) = cur_period.as_mut() {
                                    p.template = Some(t);
                                }
                            }
                            Owner::Set => {
                                if let Some(s) = cur_set.as_mut() {
                                    s.template = Some(t);
                                }
                            }
                            Owner::Rep => {
                                if let Some(r) = cur_rep.as_mut() {
                                    r.template = Some(t);
                                }
                            }
                        }
                        // a following SegmentTimeline attaches to this level
                        tl_owner = Some(owner);
                    }
                    "SegmentTimeline" => {
                        tl_buf = Some(Vec::new());
                    }
                    "S" => {
                        if let Some(tl) = tl_buf.as_mut() {
                            let t = parse_u64_attr(&attrs, "t")?;
                            let d = parse_u64_attr(&attrs, "d")?
                                .ok_or_else(|| Error::Dash("timeline S@d missing".into()))?;
                            if d == 0 {
                                return Err(Error::Dash("timeline S@d=0".into()));
                            }
                            let r = match attrs.iter().find(|(k, _)| k == "r") {
                                Some((_, v)) => {
                                    let t = v.trim();
                                    let neg = t.starts_with('-');
                                    let n: u64 = t
                                        .trim_start_matches('-')
                                        .parse()
                                        .map_err(|_| Error::Dash("bad timeline @r".into()))?;
                                    if neg {
                                        if n == 1 {
                                            -1i64
                                        } else {
                                            return Err(Error::Dash(
                                                "timeline @r < -1 is invalid".into(),
                                            ));
                                        }
                                    } else {
                                        n.min(i64::MAX as u64) as i64
                                    }
                                }
                                None => 0,
                            };
                            tl.push(TlS { t, d, r });
                        }
                    }
                    "SegmentList" => {
                        let owner = match stack.last().map(|s| s.as_str()) {
                            Some("AdaptationSet") => Owner::Set,
                            Some("Representation") => Owner::Rep,
                            _ => {
                                return Err(Error::Dash(
                                    "SegmentList outside AdaptationSet/Representation".into(),
                                ))
                            }
                        };
                        list_buf = Some(SegmentList::default());
                        list_owner = Some(owner);
                    }
                    "Initialization" => {
                        let src = attrs
                            .iter()
                            .find(|(k, _)| k == "sourceURL")
                            .map(|(_, v)| v.clone());
                        let rng = match attrs.iter().find(|(k, _)| k == "range") {
                            Some((_, v)) => Some(parse_byte_range_form(v)?),
                            None => None,
                        };
                        if let Some(sl) = list_buf.as_mut() {
                            sl.init_url = src;
                            sl.init_range = rng;
                        }
                    }
                    "SegmentURL" => {
                        let media = attrs
                            .iter()
                            .find(|(k, _)| k == "media")
                            .map(|(_, v)| v.clone());
                        let rng = match attrs.iter().find(|(k, _)| k == "mediaRange") {
                            Some((_, v)) => Some(parse_byte_range_form(v)?),
                            None => None,
                        };
                        if let Some(sl) = list_buf.as_mut() {
                            if let Some(m) = media {
                                sl.media.push((m, rng));
                            }
                        }
                    }
                    "SegmentBase" => {
                        let idx = match attrs.iter().find(|(k, _)| k == "indexRange") {
                            Some((_, v)) => Some(parse_byte_range_form(v)?),
                            None => None,
                        };
                        let sb = SegmentBase { index_range: idx };
                        match stack.last().map(|s| s.as_str()) {
                            Some("AdaptationSet") => {
                                if let Some(s) = cur_set.as_mut() {
                                    s.sbase = Some(sb);
                                }
                            }
                            Some("Representation") => {
                                if let Some(r) = cur_rep.as_mut() {
                                    r.sbase = Some(sb);
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
                if !self_closed {
                    stack.push(name);
                } else {
                    // self-closed structural elements flush immediately
                    // (no Close event will follow)
                    match name.as_str() {
                        "Period" => {
                            if let Some(p) = cur_period.take() {
                                mpd.periods.push(p);
                            }
                        }
                        "AdaptationSet" => {
                            if let (Some(p), Some(s)) = (cur_period.as_mut(), cur_set.take()) {
                                p.sets.push(s);
                            }
                        }
                        "Representation" => {
                            if let (Some(s), Some(r)) = (cur_set.as_mut(), cur_rep.take()) {
                                s.reps.push(r);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    if !saw_mpd {
        return Err(Scanner::err("no MPD root element"));
    }
    if mpd.periods.is_empty() {
        return Err(Error::Dash("manifest has no Period".into()));
    }
    Ok(mpd)
}

// ---------------------------------------------------------------------------
// Representation selection (gate 0015: choose_variant semantics exactly)
// ---------------------------------------------------------------------------

/// The `hls::choose_variant` semantics on `bandwidth`: best/worst/nearest
/// with ties resolving to the EARLIER document position. When no
/// representation carries a bandwidth, document order wins (first = best).
fn choose_representation<'a>(
    reps: &'a [Representation],
    quality: &str,
) -> Result<&'a Representation> {
    let q = quality.trim().to_ascii_lowercase();
    if reps.is_empty() {
        return Err(Error::Dash("no representations".into()));
    }
    let pick = match q.as_str() {
        "" | "best" | "highest" => reps
            .iter()
            .enumerate()
            .filter(|(_, r)| r.bandwidth.is_some())
            .max_by(|a, b| {
                let (av, bv) = (a.1.bandwidth.unwrap(), b.1.bandwidth.unwrap());
                if av == bv {
                    b.0.cmp(&a.0) // tie → earlier position (smaller index)
                } else {
                    av.cmp(&bv)
                }
            }),
        "worst" | "lowest" => reps
            .iter()
            .enumerate()
            .filter(|(_, r)| r.bandwidth.is_some())
            .min_by(|a, b| {
                let (av, bv) = (a.1.bandwidth.unwrap(), b.1.bandwidth.unwrap());
                if av == bv {
                    a.0.cmp(&b.0) // tie → earlier position (smaller index)
                } else {
                    av.cmp(&bv)
                }
            }),
        other => {
            let want: u64 = other.parse().map_err(|_| {
                Error::Dash(format!(
                    "bad quality '{}' (use best|worst|bandwidth)",
                    quality
                ))
            })?;
            reps.iter()
                .enumerate()
                .filter(|(_, r)| r.bandwidth.is_some())
                .min_by(|a, b| {
                    let (av, bv) = (
                        a.1.bandwidth.unwrap().abs_diff(want),
                        b.1.bandwidth.unwrap().abs_diff(want),
                    );
                    if av == bv {
                        a.0.cmp(&b.0) // tie → earlier position (smaller index)
                    } else {
                        av.cmp(&bv)
                    }
                })
        }
    };
    pick.map(|p| p.1)
        .or_else(|| reps.first())
        .ok_or_else(|| Error::Dash("no representations".into()))
}

// ---------------------------------------------------------------------------
// Template enumeration + expansion
// ---------------------------------------------------------------------------

/// One enumerated media segment (number + `$Time$` value in timescale
/// units) before URL expansion.
struct Numbered {
    number: u64,
    time: u64,
}

/// Enumerate duration-based numbering: count = ceil(period-length ×
/// timescale ÷ @duration); `$Time$` = PTO(=0) + (N − startNumber) ×
/// @duration (checked math; gate 0015 FALS #2).
fn enumerate_duration(
    period_len: u64,
    timescale: u64,
    duration: u64,
    start_number: u64,
) -> Result<Vec<Numbered>> {
    if duration == 0 || timescale == 0 {
        return Err(Error::Dash("duration/timescale of 0".into()));
    }
    let num = (period_len as u128)
        .checked_mul(timescale as u128)
        .ok_or_else(|| Error::Dash("period duration overflow".into()))?;
    let count = num.div_ceil(duration as u128); // ceil(period×ts ÷ duration)
    if count > MAX_SEGMENTS as u128 {
        return Err(Error::Dash("manifest enumerates too many segments".into()));
    }
    let count = count as u64;
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let time = (i as u128)
            .checked_mul(duration as u128)
            .and_then(|v| u64::try_from(v).ok())
            .ok_or_else(|| Error::Dash("segment time overflow".into()))?;
        out.push(Numbered {
            number: start_number + i,
            time,
        });
    }
    Ok(out)
}

/// Enumerate a SegmentTimeline: `@t` absent = previous S end (0 for the
/// first S); `@r ≥ 0` repeats; `@r = -1` repeats until the Period end
/// (loud-fail when no Period duration is derivable); `@r < -1` invalid.
/// The 100,000 cap applies to the expansion.
fn enumerate_timeline(tl: &[TlS], period_len: Option<u64>) -> Result<Vec<Numbered>> {
    let mut out: Vec<u64> = Vec::new(); // the $Time$ values, in order
    let mut prev_end = 0u64;
    for s in tl {
        let start = s.t.unwrap_or(prev_end);
        let total: u64 = if s.r >= 0 {
            (s.r as u64)
                .checked_add(1)
                .ok_or_else(|| Error::Dash("timeline repeat overflow".into()))?
        } else {
            // @r = -1: repeat until the Period end (the total segment count
            // covering [start, period_len), ceil-divided)
            let Some(pl) = period_len else {
                return Err(Error::Dash(
                    "@r=-1 timeline without a derivable Period duration".into(),
                ));
            };
            if start >= pl {
                0
            } else {
                (pl - start).div_ceil(s.d)
            }
        };
        let mut t = start;
        for _ in 0..total {
            if out.len() as u64 >= MAX_SEGMENTS {
                return Err(Error::Dash("manifest enumerates too many segments".into()));
            }
            out.push(t);
            t = t
                .checked_add(s.d)
                .ok_or_else(|| Error::Dash("timeline time overflow".into()))?;
        }
        prev_end = t;
    }
    if out.is_empty() {
        return Err(Error::Dash("empty segment timeline".into()));
    }
    Ok(out
        .into_iter()
        .map(|time| Numbered {
            number: 0, // numbering applied by the caller against startNumber
            time,
        })
        .collect())
}

/// Expand one template string against one numbered entry. `$$` unescapes;
/// `$Name%0Nd$` zero-pads (width ≤ 16); an identifier the representation
/// cannot supply is a loud-fail (never a literal `$…$` in a URL).
fn expand_template(
    tmpl: &str,
    rep_id: &str,
    bandwidth: Option<u64>,
    number: u64,
    time: u64,
    init_only: bool,
) -> Result<String> {
    let mut out = String::with_capacity(tmpl.len() + 16);
    let mut rest = tmpl;
    while let Some(dollar) = rest.find('$') {
        out.push_str(&rest[..dollar]);
        let after = &rest[dollar + 1..];
        if let Some(stripped) = after.strip_prefix('$') {
            out.push('$');
            rest = stripped;
            continue;
        }
        let Some(end) = after.find('$') else {
            return Err(Error::Dash("unterminated template identifier".into()));
        };
        let ident = &after[..end];
        rest = &after[end + 1..];
        // split an optional zero-pad format tag: %0Nd
        let (field, pad): (&str, Option<usize>) = match ident.split_once('%') {
            Some((f, fmt)) => {
                let Some(width) = fmt
                    .strip_prefix('0')
                    .and_then(|w| w.strip_suffix('d'))
                    .and_then(|w| w.parse::<usize>().ok())
                else {
                    return Err(Error::Dash(format!("bad template format '${}$'", ident)));
                };
                if width > 16 {
                    return Err(Error::Dash("template zero-pad width exceeds 16".into()));
                }
                (f, Some(width))
            }
            None => (ident, None),
        };
        let value: String = match field {
            "RepresentationID" => {
                if rep_id.is_empty() {
                    return Err(Error::Dash(
                        "template needs $RepresentationID$ but the representation has no id".into(),
                    ));
                }
                if pad.is_some() {
                    // 23009-1: $RepresentationID$ does not support format tags
                    return Err(Error::Dash(
                        "$RepresentationID$ format tag is not allowed".into(),
                    ));
                }
                rep_id.to_string()
            }
            "Bandwidth" => match bandwidth {
                Some(b) => padded(b, pad),
                None => {
                    return Err(Error::Dash(
                        "template needs $Bandwidth$ but the representation has no bandwidth".into(),
                    ))
                }
            },
            "Number" => {
                if init_only {
                    return Err(Error::Dash(
                        "initialization template may not reference $Number$".into(),
                    ));
                }
                padded(number, pad)
            }
            "Time" => {
                if init_only {
                    return Err(Error::Dash(
                        "initialization template may not reference $Time$".into(),
                    ));
                }
                padded(time, pad)
            }
            other => {
                return Err(Error::Dash(format!(
                    "unsupported template identifier ${}$",
                    other
                )));
            }
        };
        out.push_str(&value);
    }
    out.push_str(rest);
    Ok(out)
}

fn padded(v: u64, pad: Option<usize>) -> String {
    match pad {
        Some(w) => format!("{:0w$}", v, w = w),
        None => v.to_string(),
    }
}

// ---------------------------------------------------------------------------
// SIDX (ISO/IEC 14496-12 §8.16.3) — SegmentBase@indexRange index box
// ---------------------------------------------------------------------------

pub struct Sidx {
    pub timescale: u32,
    pub earliest_presentation_time: u64,
    pub first_offset: u64,
    /// The declared total box size (anchors the first referenced range:
    /// first range starts at end-of-box + first_offset).
    pub box_size: u64,
    /// One (referenced_size, subsegment_duration) pair per reference.
    pub references: Vec<(u64, u64)>,
}

fn be_u32(b: &[u8], off: usize) -> Result<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| Error::Dash("SIDX box truncated".into()))
}

fn be_u64(b: &[u8], off: usize) -> Result<u64> {
    b.get(off..off + 8)
        .map(|s| {
            let mut a = [0u8; 8];
            a.copy_from_slice(s);
            u64::from_be_bytes(a)
        })
        .ok_or_else(|| Error::Dash("SIDX box truncated".into()))
}

/// Parse an `sidx` box from fetched index bytes. Byte-accurate: the box
/// size claim must fit the buffer and the consumed bytes must equal it.
/// `reference_count` is capped at 100,000 (gate 0015 ATKR #3).
pub fn parse_sidx(buf: &[u8]) -> Result<Sidx> {
    if buf.len() < 12 {
        return Err(Error::Dash("SIDX box truncated".into()));
    }
    let size32 = be_u32(buf, 0)? as u64;
    if &buf[4..8] != b"sidx" {
        return Err(Error::Dash(
            "index range does not contain an sidx box".into(),
        ));
    }
    let mut off = 8usize;
    let declared: u64 = if size32 == 1 {
        let l = be_u64(buf, off)?;
        off += 8;
        l
    } else if size32 == 0 {
        buf.len() as u64 // box extends to the end of the buffer
    } else {
        size32
    };
    if declared > buf.len() as u64 {
        return Err(Error::Dash(
            "SIDX box size exceeds the fetched index".into(),
        ));
    }
    let version = buf[off];
    off += 4; // version + flags
    off += 4; // reference_ID
    let timescale = be_u32(buf, off)?;
    off += 4;
    if timescale == 0 {
        return Err(Error::Dash("SIDX timescale is 0".into()));
    }
    let (ept, first_offset) = if version == 1 {
        (be_u64(buf, off)?, be_u64(buf, off + 8)?)
    } else {
        (be_u32(buf, off)? as u64, be_u32(buf, off + 4)? as u64)
    };
    off += if version == 1 { 16 } else { 8 };
    off += 16; // reserved
    let rc = buf
        .get(off..off + 2)
        .ok_or_else(|| Error::Dash("SIDX box truncated".into()))?;
    let ref_count = u16::from_be_bytes([rc[0], rc[1]]) as u64;
    off += 2;
    if ref_count > MAX_SEGMENTS {
        return Err(Error::Dash("manifest enumerates too many segments".into()));
    }
    let mut references = Vec::with_capacity(ref_count as usize);
    for _ in 0..ref_count {
        let word = be_u32(buf, off)?;
        let referenced_size = (word & 0x7FFF_FFFF) as u64;
        let duration = be_u32(buf, off + 4)? as u64;
        let sap_word = be_u32(buf, off + 8)?;
        let sap_type = (sap_word >> 28) & 0x7;
        // ISO/IEC 14496-12 §8.16.3: the SAP_delta_time field follows only
        // when SAP_type is 1 or 2 (a 20-byte entry); otherwise 12 bytes.
        off += 12 + if sap_type == 1 || sap_type == 2 { 4 } else { 0 };
        if off as u64 > declared {
            return Err(Error::Dash(
                "SIDX references exceed the declared box size".into(),
            ));
        }
        references.push((referenced_size, duration));
    }
    if off as u64 != declared {
        return Err(Error::Dash(
            "SIDX box size does not match consumed bytes".into(),
        ));
    }
    Ok(Sidx {
        timescale,
        earliest_presentation_time: ept,
        first_offset,
        box_size: declared,
        references,
    })
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

/// One fetchable piece of the stream.
#[derive(Debug, Clone)]
pub struct SegRef {
    pub url: Url,
    /// `(offset, length)` byte sub-range of the resource; `None` = the
    /// whole resource. A ranged fetch is a 206-or-fail byte-range GET.
    pub range: Option<(u64, u64)>,
}

/// The media-segment source shape after planning.
#[derive(Debug, Clone)]
pub enum MediaSource {
    /// Fully resolved media segment references, in order.
    Resolved(Vec<SegRef>),
    /// `SegmentBase@indexRange`: the exec layer fetches the index range,
    /// parses the SIDX box, and derives the referenced byte ranges.
    SegmentBase { url: Url, index: (u64, u64) },
}

/// The executable plan produced by [`plan`].
#[derive(Debug, Clone)]
pub struct DashPlan {
    /// Initialization segment(s), written before the media segments.
    pub init: Vec<SegRef>,
    pub media: MediaSource,
    /// Exact byte total when every piece's length is derivable up-front
    /// (ranged init/list/indexRange forms); `None` degrades progress to an
    /// estimate (pinned behavior, both cases).
    pub exact_total: Option<u64>,
    /// Completion notice (e.g. first-of-N-periods download).
    pub notice: Option<String>,
}

fn resolve_chain(base: &Url, parts: &[&Option<String>]) -> Result<Url> {
    let mut url = base.clone();
    for p in parts {
        if let Some(s) = p {
            if !s.trim().is_empty() {
                url = url.join(s.trim())?;
            }
        }
    }
    Ok(url)
}

/// Plan a DASH download from manifest text. Static manifests only; first
/// Period only; the chosen adaptation set is video (ContentComponent-aware),
/// then audio, else a loud-fail; the quality choice mirrors
/// `hls::choose_variant` exactly on `bandwidth`.
pub fn plan(text: &str, manifest_url: &Url, quality: &str) -> Result<DashPlan> {
    if text.len() > 16 * 1024 * 1024 {
        return Err(Error::Dash("manifest exceeds 16 MiB".into()));
    }
    let mpd = parse_mpd(text)?;
    if mpd.dynamic {
        return Err(Error::Dash(
            "live DASH (dynamic manifest) is not supported".into(),
        ));
    }
    let Some(period) = mpd.periods.first().cloned() else {
        return Err(Error::Dash("manifest has no Period".into()));
    };
    let period_count = mpd.periods.len();
    // period length in seconds: Period@duration else MPD@mediaPresentationDuration
    let period_len = period.duration.or(mpd.media_presentation_duration);

    // ---- adaptation-set selection (video first, then audio) ----
    fn typed(ct: &Option<String>, mt: &Option<String>, comps: &[String], want: &str) -> bool {
        ct.as_deref()
            .map(|s| s.to_ascii_lowercase().contains(want))
            .or_else(|| mt.as_deref().map(|s| s.to_ascii_lowercase().contains(want)))
            .or_else(|| {
                comps
                    .iter()
                    .find(|c| c.to_ascii_lowercase().contains(want))
                    .map(|_| true)
            })
            .unwrap_or(false)
    }
    let set = mpd
        .periods
        .first()
        .and_then(|p| {
            p.sets
                .iter()
                .find(|s| typed(&s.content_type, &s.mime_type, &s.components, "video"))
        })
        .or_else(|| {
            mpd.periods.first().and_then(|p| {
                p.sets
                    .iter()
                    .find(|s| typed(&s.content_type, &s.mime_type, &s.components, "audio"))
            })
        })
        .ok_or_else(|| Error::Dash("cannot determine stream type".into()))?;
    if set.has_protection {
        return Err(Error::Dash(
            "DRM-protected DASH (ContentProtection) is not supported".into(),
        ));
    }
    if set.reps.is_empty() {
        return Err(Error::Dash(
            "the selected adaptation set has no representations".into(),
        ));
    }
    let rep = choose_representation(&set.reps, quality)?;

    // ---- effective template / list / base (three-level inheritance) ----
    let template = merge_template(
        merge_template(rep.template.clone(), set.template.as_ref()),
        period.template.as_ref(),
    );
    let list = rep.list.clone().or_else(|| set.list.clone());
    let sbase = rep.sbase.clone().or_else(|| set.sbase.clone());
    let rep_base = resolve_chain(
        manifest_url,
        &[&mpd.base, &period.base, &set.base, &rep.base],
    )?;

    let mut init: Vec<SegRef> = Vec::new();
    let media;

    match (list, sbase, template) {
        // ---- SegmentList: fetched as listed ----
        (Some(sl), _, _) => {
            if let Some(iu) = &sl.init_url {
                let url = rep_base.join(iu.trim())?;
                init.push(SegRef {
                    url,
                    range: sl.init_range,
                });
            }
            if sl.media.is_empty() {
                return Err(Error::Dash("SegmentList has no media segments".into()));
            }
            if sl.media.len() as u64 > MAX_SEGMENTS {
                return Err(Error::Dash("manifest enumerates too many segments".into()));
            }
            let mut refs = Vec::with_capacity(sl.media.len());
            for (m, rng) in &sl.media {
                let url = rep_base.join(m.trim())?;
                refs.push(SegRef { url, range: *rng });
            }
            media = MediaSource::Resolved(refs);
        }
        // ---- SegmentBase@indexRange: the exec layer resolves via SIDX ----
        (None, Some(sb), _) => {
            let Some(index) = sb.index_range else {
                return Err(Error::Dash(
                    "SegmentBase without indexRange is not supported".into(),
                ));
            };
            media = MediaSource::SegmentBase {
                url: rep_base.clone(),
                index,
            };
        }
        // ---- SegmentTemplate: duration-based or timeline ----
        (None, None, Some(t)) => {
            let timescale = t.timescale.unwrap_or(1);
            if timescale == 0 {
                return Err(Error::Dash("timescale is 0".into()));
            }
            let pto = t.presentation_time_offset.unwrap_or(0);
            if pto != 0 {
                return Err(Error::Dash(
                    "presentationTimeOffset (PTO) != 0 is not supported".into(),
                ));
            }
            let Some(media_tmpl) = t.media.clone() else {
                return Err(Error::Dash("SegmentTemplate has no media attribute".into()));
            };
            // initialization
            if let Some(init_tmpl) = &t.initialization {
                // the init template may carry only $RepresentationID$/$Bandwidth$
                let expanded = expand_template(init_tmpl, &rep.id, rep.bandwidth, 0, 0, true)?;
                let url = rep_base.join(expanded.trim())?;
                init.push(SegRef { url, range: None });
            }
            let start_number = t.start_number.unwrap_or(1);
            let numbered = match &t.timeline {
                Some(tl) if !tl.is_empty() => {
                    let times = enumerate_timeline(tl, period_len)?;
                    times
                        .into_iter()
                        .enumerate()
                        .map(|(i, mut n)| {
                            n.number = start_number + i as u64;
                            n
                        })
                        .collect::<Vec<_>>()
                }
                _ => {
                    let Some(dur) = t.duration else {
                        return Err(Error::Dash(
                            "media template has neither @duration nor a SegmentTimeline".into(),
                        ));
                    };
                    if dur == 0 {
                        return Err(Error::Dash("SegmentTemplate @duration is 0".into()));
                    }
                    let Some(pl) = period_len else {
                        return Err(Error::Dash(
                            "no Period@duration or mediaPresentationDuration to derive the segment count"
                                .into(),
                        ));
                    };
                    enumerate_duration(pl, timescale, dur, start_number)?
                }
            };
            if numbered.len() as u64 > MAX_SEGMENTS {
                return Err(Error::Dash("manifest enumerates too many segments".into()));
            }
            let mut refs = Vec::with_capacity(numbered.len());
            for n in &numbered {
                let expanded =
                    expand_template(&media_tmpl, &rep.id, rep.bandwidth, n.number, n.time, false)?;
                let url = rep_base.join(expanded.trim())?;
                refs.push(SegRef { url, range: None });
            }
            media = MediaSource::Resolved(refs);
        }
        (None, None, None) => {
            return Err(Error::Dash(
                "no SegmentTemplate, SegmentList, or SegmentBase addressing".into(),
            ));
        }
    }

    // ---- exact total only when every length is derivable ----
    let media_all_ranged = match &media {
        MediaSource::Resolved(refs) => !refs.is_empty() && refs.iter().all(|r| r.range.is_some()),
        MediaSource::SegmentBase { .. } => true, // SIDX sizes are exact
    };
    let exact_total = if media_all_ranged && init.iter().all(|r| r.range.is_some()) {
        let init_sum: u64 = init.iter().filter_map(|r| r.range).map(|(_, l)| l).sum();
        let media_sum: u64 = match &media {
            MediaSource::Resolved(refs) => {
                refs.iter().filter_map(|r| r.range).map(|(_, l)| l).sum()
            }
            MediaSource::SegmentBase { .. } => 0, // filled post-SIDX by the exec layer
        };
        Some(init_sum + media_sum)
    } else {
        None
    };

    let notice = if period_count > 1 {
        Some(format!("downloaded first period of {}", period_count))
    } else {
        None
    };

    Ok(DashPlan {
        init,
        media,
        exact_total,
        notice,
    })
}

/// Resolve a `MediaSource::SegmentBase` into ordered media `SegRef`s once
/// the SIDX index bytes are fetched. The first referenced range starts at
/// (end of the sidx box) + first_offset; each next range continues after
/// the previous (`checked_add` on all offset+size math — overflow ⇒
/// classified error).
pub fn resolve_segment_base(rep_url: &Url, index: (u64, u64), sidx: &Sidx) -> Result<Vec<SegRef>> {
    let (ioff, _ilen) = index;
    let mut cursor = sidx
        .box_size
        .checked_add(sidx.first_offset)
        .and_then(|v| v.checked_add(ioff))
        .ok_or_else(|| Error::Dash("SIDX offset overflow".into()))?;
    let mut out = Vec::with_capacity(sidx.references.len());
    for (size, _dur) in &sidx.references {
        let end = cursor
            .checked_add(*size)
            .ok_or_else(|| Error::Dash("SIDX range overflow".into()))?;
        out.push(SegRef {
            url: rep_url.clone(),
            range: Some((cursor, *size)),
        });
        cursor = end;
    }
    if out.is_empty() {
        return Err(Error::Dash("SIDX box has no references".into()));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Execution (sequential over ONE persistent connection — a deliberate
// deviation from run_hls's 4 concurrent workers, recorded in the gate)
// ---------------------------------------------------------------------------

/// Fetch the manifest text (HLS playlist budget: 16 MiB, `fetch_text`
/// semantics — but classified `Error::Dash`, never `Error::Hls`).
fn fetch_manifest(cfg: &ClientCfg, url: &Url) -> Result<String> {
    let mut conn = Conn::connect(url, cfg)?;
    let mut resp = conn.request(&Request::get(url), cfg)?;
    if resp.status != 200 {
        return Err(Error::Dash(format!(
            "manifest fetch returned {}",
            resp.status
        )));
    }
    let body = resp.read_body(16 * 1024 * 1024)?;
    String::from_utf8(body).map_err(|_| Error::Dash("manifest is not utf-8".into()))
}

/// Fetch one piece, reusing a keep-alive connection. The reuse key is
/// **scheme + host + port** (gate 0015 ATKR #5: the DASH code does not
/// inherit the HLS key's scheme-blindness). Ranged fetches expect
/// `206 Partial Content` and fail on a short body — never degraded to a
/// whole-resource read.
fn fetch_seg_ref(
    cfg: &ClientCfg,
    r: &SegRef,
    rate: Option<&Arc<TokenBucket>>,
    conn: &mut Option<Conn>,
    conn_target: &mut Option<String>,
) -> Result<Vec<u8>> {
    let target = format!("{}://{}:{}", r.url.scheme, r.url.host, r.url.port);
    if conn.is_some() && conn_target.as_deref() != Some(target.as_str()) {
        *conn = None;
    }
    let mut request = Request::get(&r.url);
    if let Some((offset, len)) = r.range {
        request = request.header("Range", &format!("bytes={}-{}", offset, offset + len - 1));
    }
    let mut resp = match conn.as_mut() {
        Some(c) => match c.request(&request, cfg) {
            Ok(resp) => resp,
            Err(_) => {
                // stale keep-alive: retry once on a fresh connection
                *conn = None;
                let mut fresh = Conn::connect(&r.url, cfg)?;
                let resp = fresh.request(&request, cfg)?;
                *conn = Some(fresh);
                resp
            }
        },
        None => {
            let mut fresh = Conn::connect(&r.url, cfg)?;
            let resp = fresh.request(&request, cfg)?;
            *conn = Some(fresh);
            resp
        }
    };
    *conn_target = Some(target);
    let want_status = if r.range.is_some() { 206 } else { 200 };
    if resp.status != want_status {
        let code = resp.status;
        let _ = resp.read_body(4096);
        if let Some(c) = resp.into_conn() {
            *conn = Some(c);
        } else {
            *conn = None;
        }
        return Err(Error::Dash(format!("segment returned {}", code)));
    }
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = {
            let mut br = resp.body_reader();
            Read::read(&mut br, &mut buf)?
        };
        if n == 0 {
            break;
        }
        if let Some(bucket) = rate {
            bucket.wait_for(n as u64);
        }
        out.extend_from_slice(&buf[..n]);
        if out.len() > SEGMENT_BODY_CAP {
            return Err(Error::Dash("segment exceeds 512 MiB".into()));
        }
    }
    if let Some((_, len)) = r.range {
        if out.len() as u64 != len {
            *conn = None; // body framing untrusted; drop the connection
            return Err(Error::Dash(format!(
                "byte-range segment short body: wanted {} bytes, got {}",
                len,
                out.len()
            )));
        }
    }
    Ok(out)
}

fn wait_resume(pause: &AtomicBool, stop: &AtomicBool) -> Result<()> {
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(Error::Dash("stopped".into()));
        }
        if pause.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(200));
            continue;
        }
        return Ok(());
    }
}

/// Run the DASH download: fetch the manifest, plan, resolve SegmentBase
/// via SIDX when present, then write init + media segments IN ORDER over
/// one persistent connection. In-process pause sleeps the fetch loop and
/// continues mid-stream; a restarted task truncates the `.part` and starts
/// from zero (no `.vdmctl` for DASH — documented, not silent).
///
/// Returns the completion notice, if the manifest produced one.
#[allow(clippy::too_many_arguments)]
pub fn download_dash(
    cfg: &ClientCfg,
    manifest_url: &Url,
    quality: &str,
    out: &Path,
    rate: Option<&Arc<TokenBucket>>,
    pause: &AtomicBool,
    stop: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Option<String>> {
    let text = fetch_manifest(cfg, manifest_url)?;
    let plan = plan(&text, manifest_url, quality)?;

    // resolve SegmentBase media via the SIDX index
    let media_refs: Vec<SegRef> = match &plan.media {
        MediaSource::Resolved(refs) => refs.clone(),
        MediaSource::SegmentBase { url, index } => {
            let (ioff, ilen) = *index;
            if ilen as usize > SIDX_FETCH_CAP {
                return Err(Error::Dash("index range exceeds 8 MiB".into()));
            }
            let seg = SegRef {
                url: url.clone(),
                range: Some((ioff, ilen)),
            };
            let mut conn = None;
            let mut conn_target = None;
            let bytes = fetch_seg_ref(cfg, &seg, None, &mut conn, &mut conn_target)?;
            let sidx = parse_sidx(&bytes)?;
            resolve_segment_base(url, (ioff, ilen), &sidx)?
        }
    };

    let total: Option<u64> = match plan.exact_total {
        // SegmentBase totals gain their media sum only after resolution
        Some(_) if matches!(plan.media, MediaSource::SegmentBase { .. }) => {
            let init_sum: u64 = plan
                .init
                .iter()
                .filter_map(|r| r.range)
                .map(|(_, l)| l)
                .sum();
            let media_sum: u64 = media_refs
                .iter()
                .filter_map(|r| r.range)
                .map(|(_, l)| l)
                .sum();
            Some(init_sum + media_sum)
        }
        other => other,
    };

    let part_path = crate::engine::state::ResumeState::part_path(out);
    let mut writer = std::io::BufWriter::new(
        std::fs::File::create(&part_path)
            .map_err(|e| Error::Dash(format!("create part {}: {}", part_path.display(), e)))?,
    );

    let mut conn: Option<Conn> = None;
    let mut conn_target: Option<String> = None;
    let mut written: u64 = 0;
    let media_count = media_refs.len() as u64;

    // init segment(s) first — their bytes precede every media segment
    for r in &plan.init {
        wait_resume(pause, stop)?;
        let data = fetch_seg_ref(cfg, r, rate, &mut conn, &mut conn_target)?;
        writer
            .write_all(&data)
            .map_err(|e| Error::Dash(format!("write: {}", e)))?;
        written += data.len() as u64;
        if let Some(t) = total {
            progress(written.min(t), t);
        }
    }
    let init_bytes = written;

    // media segments, sequential; when the total is unknown, estimate from
    // the first media segment's length × count (the HLS estimate shape)
    let mut est_total = total;
    for r in &media_refs {
        wait_resume(pause, stop)?;
        let data = fetch_seg_ref(cfg, r, rate, &mut conn, &mut conn_target)?;
        if est_total.is_none() {
            est_total = Some(init_bytes + (data.len() as u64) * media_count);
        }
        writer
            .write_all(&data)
            .map_err(|e| Error::Dash(format!("write: {}", e)))?;
        written += data.len() as u64;
        let est = est_total.unwrap_or(written);
        progress(written.min(est), est.max(written));
    }

    writer
        .flush()
        .map_err(|e| Error::Dash(format!("flush: {}", e)))?;
    drop(writer);
    std::fs::rename(&part_path, out).map_err(|e| {
        Error::Dash(format!(
            "rename {} -> {}: {}",
            part_path.display(),
            out.display(),
            e
        ))
    })?;
    Ok(plan.notice)
}

// ---------------------------------------------------------------------------
// Tests (gate 0015 spec §Tests — parsing/planning level)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("http://127.0.0.1:9/video.mpd").unwrap()
    }

    fn plan_ok(text: &str, quality: &str) -> DashPlan {
        plan(text, &base(), quality).unwrap()
    }

    fn plan_err(text: &str, quality: &str) -> String {
        plan(text, &base(), quality).err().unwrap().to_string()
    }

    const HDR: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" type=\"static\" mediaPresentationDuration=\"PT10S\" minBufferTime=\"PT2S\">";

    fn template_mpd(extra: &str, tmpl_attrs: &str) -> String {
        format!(
            "{HDR}\n<Period id=\"p0\" duration=\"PT10S\">\n\
<AdaptationSet contentType=\"video\" mimeType=\"video/mp4\">\n\
<SegmentTemplate {} timescale=\"1\">{} </SegmentTemplate>\n\
<Representation id=\"v0\" bandwidth=\"1000000\"/>\n\
</AdaptationSet>\n</Period>\n</MPD>\n",
            tmpl_attrs, extra
        )
    }

    // ---- tokenizer ----

    #[test]
    fn tokenizer_entities_numeric_refs_comments_cdata_pi_doctype() {
        let mpd = format!(
            "<!-- a comment --><?xml-stylesheet href=\"x.css\"?><!DOCTYPE MPD [<!ENTITY xxe SYSTEM \"file:///etc/passwd\">]>{HDR}\n<Period duration=\"PT1S\"><AdaptationSet contentType=\"video\"><SegmentTemplate media=\"seg-$Number$.m4s\" duration=\"1\"/><Representation id=\"a&amp;b&#65;&#x42;\" bandwidth=\"1\"/></AdaptationSet></Period>\n</MPD>"
        );
        let p = plan_ok(&mpd, "best");
        assert_eq!(p.media_resolved().len(), 1);
        // tokenizer detail assertions:
        let mut sc = Scanner::new("<a x=\"&amp;&lt;&gt;&quot;&apos;&#65;&#x42;\"/>");
        match sc.next_tag().unwrap() {
            Tag::Open { attrs, .. } => {
                assert_eq!(attrs[0].1, "&<>\"'AB");
            }
            _ => panic!("expected open"),
        }
        // CDATA content is skipped wholesale (its & is not an entity)
        let mut sc = Scanner::new("<r><![CDATA[ &notanentity; ]]></r>");
        assert!(matches!(sc.next_tag().unwrap(), Tag::Open { .. }));
        assert!(matches!(sc.next_tag().unwrap(), Tag::Close(_)));
    }

    #[test]
    fn tokenizer_undefined_entity_is_error() {
        let mpd = format!(
            "{HDR}\n<Period duration=\"PT1S\"><AdaptationSet contentType=\"video\"><Representation id=\"&nope;\" bandwidth=\"1\"/></AdaptationSet></Period>\n</MPD>"
        );
        assert!(plan_err(&mpd, "best").contains("undefined entity"));
        // undefined entity in a text run too
        let mpd2 = format!(
            "{HDR}\n<Period duration=\"PT1S\"><AdaptationSet contentType=\"video\">&badref;<Representation id=\"v\" bandwidth=\"1\"/></AdaptationSet></Period>\n</MPD>"
        );
        assert!(plan_err(&mpd2, "best").contains("undefined entity"));
    }

    #[test]
    fn tokenizer_depth_cap_and_malformed() {
        // 130 nested unknown elements ⇒ depth-cap error, no recursion
        let mut mpd =
            format!("{HDR}\n<Period duration=\"PT1S\"><AdaptationSet contentType=\"video\">");
        for _ in 0..130 {
            mpd.push_str("<z><y>");
        }
        mpd.push_str("<Representation id=\"v\" bandwidth=\"1\"/>");
        assert!(plan_err(&mpd, "best").contains("depth"));
        // malformed: attribute value not quoted
        let mpd2 = format!(
            "{HDR}\n<Period duration=PT1S><AdaptationSet contentType=\"video\"><Representation id=\"v\" bandwidth=\"1\"/></AdaptationSet></Period>\n</MPD>"
        );
        assert!(plan_err(&mpd2, "best").contains("bad manifest XML"));
        // malformed: unterminated tag
        let mpd3 = format!("{HDR}\n<Period duration=\"PT1S\"\n");
        assert!(plan_err(&mpd3, "best").contains("bad manifest XML"));
    }

    #[test]
    fn tokenizer_4kib_attr_cap() {
        let big = "x".repeat(5000);
        let mpd = format!(
            "{HDR}\n<Period duration=\"PT1S\"><AdaptationSet contentType=\"{}\"><Representation id=\"v\" bandwidth=\"1\"/></AdaptationSet></Period>\n</MPD>",
            big
        );
        assert!(plan_err(&mpd, "best").contains("4 KiB"));
    }

    // ---- MPD-level loud-fails ----

    #[test]
    fn dynamic_mpd_loud_fails() {
        let mpd = format!(
            "<?xml version=\"1.0\"?><MPD type=\"dynamic\" minimumUpdatePeriod=\"PT2S\"><Period duration=\"PT1S\"><AdaptationSet contentType=\"video\"><Representation id=\"v\" bandwidth=\"1\"/></AdaptationSet></Period></MPD>"
        );
        assert!(plan_err(&mpd, "best").contains("live DASH"));
        // minimumUpdatePeriod without type also means live
        let mpd2 = format!(
            "<MPD minimumUpdatePeriod=\"PT2S\"><Period duration=\"PT1S\"><AdaptationSet contentType=\"video\"><Representation id=\"v\" bandwidth=\"1\"/></AdaptationSet></Period></MPD>"
        );
        assert!(plan_err(&mpd2, "best").contains("live DASH"));
    }

    #[test]
    fn drm_loud_fails_and_undeterminable() {
        let mpd = format!(
            "{HDR}\n<Period duration=\"PT1S\"><AdaptationSet contentType=\"video\"><ContentProtection schemeIdUri=\"urn:mpeg:dash:mp4protection:2011\"/><Representation id=\"v\" bandwidth=\"1\"/></AdaptationSet></Period>\n</MPD>"
        );
        assert!(plan_err(&mpd, "best").contains("DRM-protected"));
        let mpd2 = format!(
            "{HDR}\n<Period duration=\"PT1S\"><AdaptationSet><Representation id=\"v\" bandwidth=\"1\"/></AdaptationSet></Period>\n</MPD>"
        );
        assert!(plan_err(&mpd2, "best").contains("cannot determine stream type"));
    }

    // ---- selection ----

    fn sel_mpd(reps: &[(&str, u64)], set_attrs: &str, component: Option<&str>) -> String {
        let comp = match component {
            Some(c) => format!("<ContentComponent contentType=\"{}\"/>", c),
            None => String::new(),
        };
        let mut reps_xml = String::new();
        for (id, bw) in reps {
            reps_xml.push_str(&format!(
                "<Representation id=\"{}\" bandwidth=\"{}\"/>",
                id, bw
            ));
        }
        format!(
            "{HDR}\n<Period duration=\"PT1S\"><AdaptationSet {}><SegmentTemplate media=\"seg-$Bandwidth$.m4s\" duration=\"1\"/>{}{}</AdaptationSet></Period>\n</MPD>",
            set_attrs, comp, reps_xml
        )
    }

    #[test]
    fn selection_best_worst_nearest_tie_earlier() {
        let reps = [
            ("r1", 1_000u64),
            ("r2", 2_000),
            ("r3", 2_000),
            ("r4", 5_000),
        ];
        let mpd = sel_mpd(&reps, "contentType=\"video\"", None);
        assert_eq!(plan_ok(&mpd, "best").media_resolved().len(), 1);
        let _ = mpd;
        let pick = |q: &str| {
            let p = plan(&mpd, &base(), q).unwrap();
            let refs = p.media_resolved();
            refs[0]
                .url
                .path_and_query
                .split('/')
                .next_back()
                .unwrap()
                .to_string()
        };
        assert_eq!(pick("best"), "seg-5000.m4s");
        assert_eq!(pick("worst"), "seg-1000.m4s");
        // nearest to 2500: r2 and r3 tie at 500 — the EARLIER wins
        assert_eq!(pick("2500"), "seg-2000.m4s");
        assert!(plan_err(&mpd, "junk!").contains("bad quality"));
    }

    #[test]
    fn selection_contentcomponent_only_typing_audio_only() {
        // the video type is declared ONLY on the child ContentComponent
        let mpd = sel_mpd(&[("v", 1)], "", Some("video"));
        assert!(plan(&mpd, &base(), "best").is_ok());
        // audio-only set: selected as audio
        let audio = sel_mpd(&[("a", 1)], "contentType=\"audio/mp4\"", None);
        assert!(plan(&audio, &base(), "best").is_ok());
    }

    // ---- template expansion ----

    #[test]
    fn template_duration_count_and_zero_pad_and_dollar() {
        // 10 s period, 2 s segments, timescale 1 → 5 segments; startNumber 3
        let mpd = template_mpd(
            "",
            "initialization=\"init-$Bandwidth$.mp4\" media=\"seg-$Number%05d$-dollar$$suffix.m4s\" duration=\"2\" startNumber=\"3\"",
        );
        // `$$` unescapes to a literal `$` — legal, no identifier
        let p = plan_ok(&mpd, "best");
        assert!(p.init.len() == 1);
        let refs = p.media_resolved();
        assert_eq!(refs.len(), 5);
        assert!(refs[0]
            .url
            .path_and_query
            .contains("seg-00003-dollar$suffix.m4s"));
        assert!(refs[4]
            .url
            .path_and_query
            .contains("seg-00007-dollar$suffix.m4s"));
    }

    #[test]
    fn template_time_values_duration_based() {
        // $Time$ = (N − startNumber) × duration (timescale units)
        let mpd = template_mpd("", "media=\"seg-$Time$.m4s\" duration=\"2\"");
        let p = plan_ok(&mpd, "best");
        let refs = p.media_resolved();
        assert!(refs[0].url.path_and_query.contains("seg-0."));
        assert!(refs[3].url.path_and_query.contains("seg-6."));
    }

    #[test]
    fn template_timeline_t_gaps_r_repeats() {
        // S t=0 d=10 r=1 (t=0,10) + S t=100 d=5 (t=100) — @t gap respected
        let mpd = template_mpd(
            "<SegmentTimeline><S t=\"0\" d=\"10\" r=\"1\"/><S t=\"100\" d=\"5\"/></SegmentTimeline>",
            "media=\"seg-$Time$.m4s\"",
        );
        let p = plan_ok(&mpd, "best");
        let refs = p.media_resolved();
        assert_eq!(refs.len(), 3);
        assert!(refs[0].url.path_and_query.contains("seg-0."));
        assert!(refs[1].url.path_and_query.contains("seg-10."));
        assert!(refs[2].url.path_and_query.contains("seg-100."));
        // absent @t continues from the previous S end
        let mpd2 = template_mpd(
            "<SegmentTimeline><S t=\"0\" d=\"10\"/><S d=\"5\" r=\"2\"/></SegmentTimeline>",
            "media=\"seg-$Time$.m4s\"",
        );
        let p2 = plan_ok(&mpd2, "best");
        let refs2 = p2.media_resolved();
        assert_eq!(refs2.len(), 4);
        // absent @t CONTINUES from the previous S end (10), so the second
        // S (d=5, r=2) yields 10, 15, 20
        assert!(refs2[2].url.path_and_query.contains("seg-15."));
        assert!(refs2[3].url.path_and_query.contains("seg-20."));
    }

    #[test]
    fn template_r_minus_one() {
        // @r=-1 with a derivable Period end (10 s period, d=4, t=0):
        // ceil(10/4) = 3 segments (0, 4, 8)
        let mpd = template_mpd(
            "<SegmentTimeline><S t=\"0\" d=\"4\" r=\"-1\"/></SegmentTimeline>",
            "media=\"seg-$Time$.m4s\"",
        );
        let p = plan_ok(&mpd, "best");
        assert_eq!(p.media_resolved().len(), 3);
        // @r=-1 WITHOUT a derivable period end ⇒ loud-fail
        let mpd2 = format!(
            "<?xml version=\"1.0\"?><MPD type=\"static\"><Period><AdaptationSet contentType=\"video\"><SegmentTemplate media=\"seg-$Time$.m4s\" timescale=\"1\"><SegmentTimeline><S t=\"0\" d=\"4\" r=\"-1\"/></SegmentTimeline></SegmentTemplate><Representation id=\"v\" bandwidth=\"1\"/></AdaptationSet></Period></MPD>"
        );
        assert!(plan_err(&mpd2, "best").contains("derivable Period duration"));
    }

    #[test]
    fn template_init_restriction_and_pto() {
        // $Number$ in the initialization template ⇒ loud-fail
        let mpd = template_mpd(
            "",
            "initialization=\"init-$Number$.mp4\" media=\"seg-$Number$.m4s\" duration=\"2\"",
        );
        assert!(plan_err(&mpd, "best").contains("initialization template"));
        // PTO != 0 ⇒ loud-fail
        let mpd2 = template_mpd(
            "",
            "media=\"seg-$Number$.m4s\" duration=\"2\" presentationTimeOffset=\"10\"",
        );
        assert!(plan_err(&mpd2, "best").contains("presentationTimeOffset"));
        // absent initialization attribute = media-only (legal)
        let mpd3 = template_mpd("", "media=\"seg-$Number$.m4s\" duration=\"2\"");
        let p = plan_ok(&mpd3, "best");
        assert!(p.init.is_empty());
        assert_eq!(p.media_resolved().len(), 5);
    }

    #[test]
    fn template_missing_identifier_and_count_cap() {
        // $Bandwidth$ with no bandwidth on the rep ⇒ loud-fail
        let mpd = format!(
            "{HDR}\n<Period duration=\"PT1S\"><AdaptationSet contentType=\"video\"><SegmentTemplate media=\"seg-$Bandwidth$.m4s\" duration=\"2\"/><Representation id=\"v\"/></AdaptationSet></Period>\n</MPD>"
        );
        assert!(plan_err(&mpd, "best").contains("no bandwidth"));
        // a template with neither @duration nor a timeline ⇒ loud-fail
        let mpd2 = template_mpd("", "media=\"seg-$Number$.m4s\"");
        assert!(plan_err(&mpd2, "best").contains("neither @duration"));
        // duration-based count beyond the 100,000 cap ⇒ loud-fail
        let mpd3 = template_mpd("", "media=\"seg-$Number$.m4s\" duration=\"1\"");
        // period = PT10S so this is only 10 — force the cap via a huge period
        let mpd4 = format!(
            "{HDR}\n<Period duration=\"PT400000H\"><AdaptationSet contentType=\"video\"><SegmentTemplate media=\"seg-$Number$.m4s\" duration=\"1\" timescale=\"1\"/><Representation id=\"v\" bandwidth=\"1\"/></AdaptationSet></Period>\n</MPD>"
        );
        assert!(plan_err(&mpd4, "best").contains("too many segments"));
        let _ = mpd3;
    }

    // ---- SegmentList + SegmentBase ----

    #[test]
    fn segment_list_with_media_range() {
        let mpd = format!(
            "{HDR}\n<Period duration=\"PT10S\"><AdaptationSet contentType=\"video\"><SegmentList duration=\"10\"><Initialization sourceURL=\"init.mp4\" range=\"0-99\"/><SegmentURL media=\"s1.m4s\" mediaRange=\"100-199\"/><SegmentURL media=\"s2.m4s\"/></SegmentList><Representation id=\"v\" bandwidth=\"1\"/></AdaptationSet></Period>\n</MPD>"
        );
        let p = plan_ok(&mpd, "best");
        assert_eq!(p.init.len(), 1);
        assert_eq!(p.init[0].range, Some((0, 100)));
        let refs = p.media_resolved();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].range, Some((100, 100)));
        assert_eq!(refs[1].range, None);
        // exact total only when every piece is ranged: s2 is unranged → None
        assert_eq!(p.exact_total, None);
    }

    #[test]
    fn segment_list_media_range_no_alias() {
        // the alias `range` on SegmentURL is NOT accepted (attribute is
        // mediaRange) — an unknown attribute is ignored, so the segment is
        // unranged; the parse itself must not error
        let mpd = format!(
            "{HDR}\n<Period duration=\"PT10S\"><AdaptationSet contentType=\"video\"><SegmentList duration=\"10\"><SegmentURL media=\"s1.m4s\" range=\"100-199\"/></SegmentList><Representation id=\"v\" bandwidth=\"1\"/></AdaptationSet></Period>\n</MPD>"
        );
        let p = plan_ok(&mpd, "best");
        let refs = p.media_resolved();
        assert_eq!(refs[0].range, None);
    }

    fn craft_sidx(version: u8, ref_count: u16, first_offset: u64, sizes: &[u32]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"sidx");
        b.push(version);
        b.extend_from_slice(&[0, 0, 0]); // flags
        b.extend_from_slice(&1u32.to_be_bytes()); // reference_ID
        b.extend_from_slice(&1000u32.to_be_bytes()); // timescale
        if version == 1 {
            b.extend_from_slice(&0u64.to_be_bytes()); // EPT
            b.extend_from_slice(&first_offset.to_be_bytes());
        } else {
            b.extend_from_slice(&(0u32).to_be_bytes()); // EPT
            b.extend_from_slice(&(first_offset as u32).to_be_bytes());
        }
        b.extend_from_slice(&[0u8; 16]); // reserved
        b.extend_from_slice(&ref_count.to_be_bytes());
        for s in sizes {
            b.extend_from_slice(&s.to_be_bytes()); // reference_type=0 | size
            b.extend_from_slice(&10u32.to_be_bytes()); // duration
            b.extend_from_slice(&0u32.to_be_bytes()); // SAP: type 0 (12-byte entry)
        }
        let total = (b.len() + 4) as u32;
        let mut out = (total).to_be_bytes().to_vec();
        out.extend_from_slice(&b);
        out
    }

    #[test]
    fn sidx_parse_v0_v1_first_offset_caps() {
        // v0, first_offset = 100, two references
        let buf = craft_sidx(0, 2, 100, &[500, 600]);
        let s = parse_sidx(&buf).unwrap();
        assert_eq!(s.timescale, 1000);
        assert_eq!(s.first_offset, 100);
        assert_eq!(s.references.len(), 2);
        assert_eq!(s.references[0], (500, 10));
        assert_eq!(s.box_size, buf.len() as u64);
        // v1 with a large first_offset
        let buf1 = craft_sidx(1, 1, 5_000_000_000u64, &[700]);
        let s1 = parse_sidx(&buf1).unwrap();
        assert_eq!(s1.first_offset, 5_000_000_000);
        assert_eq!(s1.references.len(), 1);
        // truncated box ⇒ error
        assert!(parse_sidx(&buf[..buf.len() - 4]).is_err());
        // reference_count is a u16 → the 100,000 cap is STRUCTURAL (u16
        // can never exceed it; the parse-side check is defensive only).
        // A 65,535-count box with zero entry bytes fails on the first
        // truncated entry read.
        let big = craft_sidx(0, 65_535, 0, &[]);
        assert!(parse_sidx(&big)
            .err()
            .unwrap()
            .to_string()
            .contains("truncated"));
        // not an sidx box ⇒ error
        assert!(parse_sidx(&[0u8; 32]).is_err());
    }

    #[test]
    fn sidx_resolve_anchors_after_box() {
        let buf = craft_sidx(0, 2, 100, &[500, 600]);
        let s = parse_sidx(&buf).unwrap();
        let url = base();
        let refs = resolve_segment_base(&url, (1000, buf.len() as u64), &s).unwrap();
        // first range starts at (index offset) + box size + first_offset
        assert_eq!(refs[0].range, Some((1000 + buf.len() as u64 + 100, 500)));
        // each next continues after the previous
        assert_eq!(
            refs[1].range,
            Some((1000 + buf.len() as u64 + 100 + 500, 600))
        );
    }

    // ---- exact-total accounting ----

    #[test]
    fn exact_total_when_all_ranged() {
        // SegmentList with all ranges: init 0-99 (100) + 100-199 (100)
        let mpd = format!(
            "{HDR}\n<Period duration=\"PT10S\"><AdaptationSet contentType=\"video\"><SegmentList duration=\"10\"><Initialization sourceURL=\"init.mp4\" range=\"0-99\"/><SegmentURL media=\"s1.m4s\" mediaRange=\"100-199\"/></SegmentList><Representation id=\"v\" bandwidth=\"1\"/></AdaptationSet></Period>\n</MPD>"
        );
        let p = plan_ok(&mpd, "best");
        assert_eq!(p.exact_total, Some(200));
    }
}

#[cfg(test)]
impl DashPlan {
    /// Test helper: the resolved media refs (panics on SegmentBase plans).
    fn media_resolved(&self) -> &[SegRef] {
        match &self.media {
            MediaSource::Resolved(r) => r,
            MediaSource::SegmentBase { .. } => panic!("segment-base plan"),
        }
    }
}
