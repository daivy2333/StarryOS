pub const ONLCR_BUF_SIZE: usize = 256;

pub struct OnlcrChunk {
    bytes: [u8; ONLCR_BUF_SIZE],
    source_at_prefix: [usize; ONLCR_BUF_SIZE + 1],
    output_len: usize,
    source_len: usize,
}

impl OnlcrChunk {
    pub fn new(source: &[u8], output_limit: usize) -> Self {
        let mut chunk = Self {
            bytes: [0; ONLCR_BUF_SIZE],
            source_at_prefix: [0; ONLCR_BUF_SIZE + 1],
            output_len: 0,
            source_len: 0,
        };
        let output_limit = output_limit.min(ONLCR_BUF_SIZE);
        for &byte in source {
            if !chunk.push(byte, output_limit) {
                break;
            }
        }
        chunk
    }

    fn push(&mut self, byte: u8, output_limit: usize) -> bool {
        let mapped_len = if byte == b'\n' { 2 } else { 1 };
        if self.output_len + mapped_len > output_limit {
            return false;
        }
        if byte == b'\n' {
            self.push_mapped_byte(b'\r');
        }
        self.push_mapped_byte(byte);
        self.source_len += 1;
        self.source_at_prefix[self.output_len] = self.source_len;
        true
    }

    fn push_mapped_byte(&mut self, byte: u8) {
        self.bytes[self.output_len] = byte;
        self.output_len += 1;
        self.source_at_prefix[self.output_len] = self.source_len;
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.output_len]
    }

    pub fn source_len(&self) -> usize {
        self.source_len
    }

    pub fn accepted_source_len(&self, written: usize) -> usize {
        self.source_at_prefix[written.min(self.output_len)]
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ShortWriteAction {
    Return(usize),
    WouldBlock,
    Wait,
}

pub fn classify_short_write(
    written: usize,
    requested: usize,
    nonblocking: bool,
    waits_for_completion: bool,
) -> ShortWriteAction {
    debug_assert!(written < requested);
    if nonblocking {
        return if written == 0 {
            ShortWriteAction::WouldBlock
        } else {
            ShortWriteAction::Return(written)
        };
    }
    if waits_for_completion {
        ShortWriteAction::Wait
    } else {
        ShortWriteAction::Return(written)
    }
}
