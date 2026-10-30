//! Paging of large response bodies and incremental UTF-8 decoding.

pub const PAGE_SIZE: usize = 64 * 1024;
pub const RAW_DISPLAY_CAP: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub index: u64,
    pub offset: u64,
    pub len: usize,
}

pub fn page_count(body_len: u64) -> u64 {
    body_len.div_ceil(PAGE_SIZE as u64)
}

pub fn page(body_len: u64, index: u64) -> Option<Page> {
    if index >= page_count(body_len) {
        return None;
    }
    let offset = index * PAGE_SIZE as u64;
    let len = (body_len - offset).min(PAGE_SIZE as u64) as usize;
    Some(Page { index, offset, len })
}

/// Incremental UTF-8 decoder that carries a split multibyte sequence across pushes.
#[derive(Debug, Clone, Default)]
pub struct Utf8Decoder {
    carry: Vec<u8>,
}

impl Utf8Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> String {
        let mut buf = std::mem::take(&mut self.carry);
        buf.extend_from_slice(bytes);
        let mut out = String::with_capacity(buf.len());
        let mut rest = buf.as_slice();
        loop {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    out.push_str(s);
                    break;
                }
                Err(e) => {
                    let (valid, tail) = rest.split_at(e.valid_up_to());
                    out.push_str(std::str::from_utf8(valid).unwrap_or_default());
                    match e.error_len() {
                        Some(n) => {
                            out.push('\u{FFFD}');
                            rest = &tail[n..];
                        }
                        None => {
                            self.carry = tail.to_vec();
                            break;
                        }
                    }
                }
            }
        }
        out
    }

    pub fn finish(&mut self) -> String {
        if std::mem::take(&mut self.carry).is_empty() {
            String::new()
        } else {
            "\u{FFFD}".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paging() {
        assert_eq!(page_count(0), 0);
        assert_eq!(page_count(1), 1);
        assert_eq!(page_count(PAGE_SIZE as u64), 1);
        assert_eq!(page_count(PAGE_SIZE as u64 + 1), 2);
        assert_eq!(page(0, 0), None);
        let n = PAGE_SIZE as u64 * 2 + 5;
        assert_eq!(
            page(n, 0),
            Some(Page {
                index: 0,
                offset: 0,
                len: PAGE_SIZE
            })
        );
        assert_eq!(
            page(n, 2),
            Some(Page {
                index: 2,
                offset: PAGE_SIZE as u64 * 2,
                len: 5
            })
        );
        assert_eq!(page(n, 3), None);
    }

    #[test]
    fn split_chars_never_produce_replacement() {
        for s in ["é", "€", "😀", "a€b😀c"] {
            let b = s.as_bytes();
            for cut in 0..=b.len() {
                let mut d = Utf8Decoder::default();
                let mut out = d.push(&b[..cut]);
                out.push_str(&d.push(&b[cut..]));
                out.push_str(&d.finish());
                assert_eq!(out, s, "{s} cut at {cut}");
            }
        }
    }

    #[test]
    fn invalid_bytes_and_dangling_tail() {
        let mut d = Utf8Decoder::default();
        assert_eq!(d.push(b"a\xFFb\xC3("), "a\u{FFFD}b\u{FFFD}(");
        assert_eq!(d.finish(), "");
        let mut d = Utf8Decoder::default();
        assert_eq!(d.push(&"€".as_bytes()[..2]), "");
        assert_eq!(d.finish(), "\u{FFFD}");
        assert_eq!(d.finish(), "");
    }
}
