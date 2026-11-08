//! Batch download helpers (XDM-style batch downloader).
//!
//! Expands URL patterns such as
//! `https://site/gallery[001:120].jpg` (zero-padding preserved) and
//! `https://site/file[1,4,7].zip`, and parses pasted line lists.

use crate::error::{Error, Result};

/// Expand one pattern into concrete URLs. Multiple bracket groups are allowed.
pub fn expand_pattern(pattern: &str) -> Result<Vec<String>> {
    let mut out = vec![String::new()];
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            // find matching ']'
            let close = match pattern[i + 1..].find(']') {
                Some(p) => i + 1 + p,
                None => return Err(Error::Task(format!("unbalanced '[' in pattern"))),
            };
            let body = &pattern[i + 1..close];
            let alts: Vec<String> = if body.contains(':') {
                let parts: Vec<&str> = body.splitn(2, ':').collect();
                let (from, to) = (parts[0].trim(), parts[1].trim());
                if from.len() != to.len() {
                    return Err(Error::Task(format!(
                        "range endpoints must have equal width: [{}:{}]",
                        from, to
                    )));
                }
                let pad = from.len();
                let a: u64 = from
                    .parse()
                    .map_err(|_| Error::Task(format!("bad range start '{}'", from)))?;
                let b: u64 = to
                    .parse()
                    .map_err(|_| Error::Task(format!("bad range end '{}'", to)))?;
                if a > b || b - a > 100_000 {
                    return Err(Error::Task(format!("bad or oversized range [{}:{}]", from, to)));
                }
                (a..=b)
                    .map(|n| format!("{:0width$}", n, width = pad))
                    .collect()
            } else if body.contains(',') {
                body.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            } else {
                return Err(Error::Task(format!(
                    "bracket group '{}' must be [a:b] or [a,b,c]",
                    body
                )));
            };
            if alts.is_empty() {
                return Err(Error::Task(format!("empty bracket group '{}'", body)));
            }
            if alts.len() * out.len() > 200_000 {
                return Err(Error::Task("pattern expands to too many URLs".into()));
            }
            let mut next = Vec::with_capacity(out.len() * alts.len());
            for prefix in &out {
                for alt in &alts {
                    let mut s = prefix.clone();
                    s.push_str(alt);
                    next.push(s);
                }
            }
            out = next;
            i = close + 1;
        } else {
            let ch = pattern[i..].chars().next().unwrap();
            for s in out.iter_mut() {
                s.push(ch);
            }
            i += ch.len_utf8();
        }
    }
    out.retain(|s| !s.is_empty());
    if out.is_empty() {
        return Err(Error::Task("empty pattern".into()));
    }
    Ok(out)
}

/// Parse a pasted list: one URL per line, `#` comments and blank lines skipped.
pub fn parse_url_list(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_range() {
        let urls = expand_pattern("https://x.test/pic[1:3].jpg").unwrap();
        assert_eq!(
            urls,
            vec![
                "https://x.test/pic1.jpg",
                "https://x.test/pic2.jpg",
                "https://x.test/pic3.jpg"
            ]
        );
    }

    #[test]
    fn zero_padding_preserved() {
        let urls = expand_pattern("https://x.test/p[001:002].png").unwrap();
        assert_eq!(
            urls,
            vec!["https://x.test/p001.png", "https://x.test/p002.png"]
        );
    }

    #[test]
    fn enum_list() {
        let urls = expand_pattern("https://x.test/f[1,4,7].zip").unwrap();
        assert_eq!(urls.len(), 3);
        assert!(urls.contains(&"https://x.test/f4.zip".to_string()));
    }

    #[test]
    fn two_groups_cartesian() {
        let urls = expand_pattern("https://x.test/[1:2]/a[1:2].bin").unwrap();
        assert_eq!(urls.len(), 4);
        assert!(urls.contains(&"https://x.test/2/a1.bin".to_string()));
    }

    #[test]
    fn no_bracket_passthrough() {
        let urls = expand_pattern("https://x.test/file.zip").unwrap();
        assert_eq!(urls, vec!["https://x.test/file.zip".to_string()]);
    }

    #[test]
    fn errors() {
        assert!(expand_pattern("x[3:1].y").is_err());
        assert!(expand_pattern("x[a:b].y").is_err());
        assert!(expand_pattern("x[1").is_err());
        assert!(expand_pattern("x[1,2].y[9:2].z").is_err());
        assert!(expand_pattern("").is_err());
    }

    #[test]
    fn big_range_rejected() {
        assert!(expand_pattern("x[1:500000].y").is_err());
    }

    #[test]
    fn url_list_parsing() {
        let text = "# comment\nhttps://a.test/1\n\n  https://a.test/2  \n#x\nhttps://a.test/3\n";
        assert_eq!(parse_url_list(text).len(), 3);
    }
}
