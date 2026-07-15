#[path = "../src/pseudofs/dev/tty/write.rs"]
mod write;

use write::{OnlcrChunk, ShortWriteAction, classify_short_write};

struct FakeWriter {
    capacity: usize,
    calls: usize,
}

impl FakeWriter {
    fn write(&mut self, requested: usize) -> usize {
        self.calls += 1;
        requested.min(self.capacity)
    }
}

#[test]
fn onlcr_respects_complete_source_boundaries() {
    let cases: &[(&[u8], usize, &[u8], usize)] = &[
        (b"\n", 0, b"", 0),
        (b"\n", 1, b"", 0),
        (b"\n", 2, b"\r\n", 1),
        (b"a\n", 2, b"a", 1),
        (b"a\n", 3, b"a\r\n", 2),
        (b"\n\n", 3, b"\r\n", 1),
    ];

    for &(source, capacity, expected, consumed) in cases {
        let chunk = OnlcrChunk::new(source, capacity);
        assert_eq!(chunk.bytes(), expected);
        assert_eq!(chunk.source_len(), consumed);
    }
}

#[test]
fn onlcr_stops_before_the_256_byte_boundary() {
    let mut source = [b'a'; 256];
    source[255] = b'\n';

    let chunk = OnlcrChunk::new(&source, 256);

    assert_eq!(chunk.bytes(), &[b'a'; 255]);
    assert_eq!(chunk.source_len(), 255);
}

#[test]
fn onlcr_partial_output_counts_only_complete_source_bytes() {
    let chunk = OnlcrChunk::new(b"a\nb", 4);

    assert_eq!(chunk.accepted_source_len(1), 1);
    assert_eq!(chunk.accepted_source_len(2), 1);
    assert_eq!(chunk.accepted_source_len(3), 2);
    assert_eq!(chunk.accepted_source_len(4), 3);
}

#[test]
fn onlcr_retries_without_duplicate_or_missing_bytes() {
    let source = [b'a', b'\n', b'b', b'\n'];
    let mut consumed = 0;
    let mut output = Vec::new();

    for capacity in [1, 2, 1, 3] {
        let chunk = OnlcrChunk::new(&source[consumed..], capacity);
        output.extend_from_slice(chunk.bytes());
        consumed += chunk.source_len();
    }

    assert_eq!(consumed, source.len());
    assert_eq!(output, b"a\r\nb\r\n");
}

#[test]
fn short_write_policy_preserves_pty_and_waits_for_uart() {
    assert_eq!(
        classify_short_write(3, 8, false, false),
        ShortWriteAction::Return(3)
    );
    assert_eq!(
        classify_short_write(3, 8, false, true),
        ShortWriteAction::Wait
    );
    assert_eq!(
        classify_short_write(3, 8, true, true),
        ShortWriteAction::Return(3)
    );
    assert_eq!(
        classify_short_write(0, 8, true, true),
        ShortWriteAction::WouldBlock
    );
}

#[test]
fn fake_writer_waits_only_after_a_blocking_uart_short_write() {
    let mut writer = FakeWriter {
        capacity: 0,
        calls: 0,
    };
    let written = writer.write(8);
    assert_eq!(
        classify_short_write(written, 8, false, true),
        ShortWriteAction::Wait
    );
    assert_eq!(writer.calls, 1);

    writer.capacity = 8;
    assert_eq!(writer.write(8), 8);
    assert_eq!(writer.calls, 2);

    writer.capacity = 0;
    let written = writer.write(8);
    assert_eq!(
        classify_short_write(written, 8, false, false),
        ShortWriteAction::Return(0)
    );
    assert_eq!(writer.calls, 3);
}
