//! Paging of large response bodies and incremental UTF-8 decoding.

pub const PAGE_SIZE: usize = 64 * 1024;
pub const RAW_DISPLAY_CAP: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub index: u64,
    pub offset: u64,
    pub len: usize,
}

pub fn page_count(_body_len: u64) -> u64 {
    0
}

pub fn page(_body_len: u64, _index: u64) -> Option<Page> {
    None
}

#[derive(Debug, Clone, Default)]
pub struct Utf8Decoder {
    carry: Vec<u8>,
}

impl Utf8Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    pub fn finish(&mut self) -> String {
        std::mem::take(&mut self.carry);
        String::new()
    }
}
