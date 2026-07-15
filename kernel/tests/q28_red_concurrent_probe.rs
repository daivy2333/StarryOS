// SPDX-License-Identifier: MIT OR Apache-2.0

extern crate alloc;

#[path = "../src/drivers/serialized_writer.rs"]
mod serialized_writer;

#[cfg(feature = "smp")]
use std::sync::Barrier;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};

use serialized_writer::SerializedWriter;

#[cfg(feature = "smp")]
const ROUNDS: usize = 128;

struct ProbeWriter {
    in_write: Arc<AtomicBool>,
    reentries: Arc<AtomicUsize>,
    stream: Vec<u8>,
}

impl ProbeWriter {
    fn write_prefix(&mut self, bytes: &[u8], limit: usize) -> usize {
        if self.in_write.swap(true, Ordering::AcqRel) {
            self.reentries.fetch_add(1, Ordering::Relaxed);
        }
        thread::yield_now();
        let accepted = bytes.len().min(limit);
        self.stream.extend_from_slice(&bytes[..accepted]);
        self.in_write.store(false, Ordering::Release);
        accepted
    }
}

#[test]
#[cfg(feature = "smp")]
fn cloned_writers_serialize_accepted_prefixes() {
    let in_write = Arc::new(AtomicBool::new(false));
    let reentries = Arc::new(AtomicUsize::new(0));
    let writer = SerializedWriter::new(ProbeWriter {
        in_write: Arc::clone(&in_write),
        reentries: Arc::clone(&reentries),
        stream: Vec::new(),
    });
    let echo_writer = writer.clone();
    let observer = writer.clone();

    let barrier = Arc::new(Barrier::new(3));
    let direct_barrier = Arc::clone(&barrier);
    let direct = thread::spawn(move || {
        direct_barrier.wait();
        for _ in 0..ROUNDS {
            assert_eq!(writer.with_lock(|raw| raw.write_prefix(b"AAAA", 3)), 3);
        }
    });
    let echo_barrier = Arc::clone(&barrier);
    let echo = thread::spawn(move || {
        echo_barrier.wait();
        for _ in 0..ROUNDS {
            assert_eq!(echo_writer.with_lock(|raw| raw.write_prefix(b"BBBB", 2)), 2);
        }
    });

    barrier.wait();
    direct.join().unwrap();
    echo.join().unwrap();

    assert_eq!(reentries.load(Ordering::Acquire), 0);
    observer.with_lock(|raw| {
        let mut direct_count = 0;
        let mut echo_count = 0;
        let mut remaining = raw.stream.as_slice();
        while !remaining.is_empty() {
            if let Some(rest) = remaining.strip_prefix(b"AAA") {
                direct_count += 1;
                remaining = rest;
            } else if let Some(rest) = remaining.strip_prefix(b"BB") {
                echo_count += 1;
                remaining = rest;
            } else {
                panic!("accepted prefixes were interleaved: {remaining:?}");
            }
        }
        assert_eq!(direct_count, ROUNDS);
        assert_eq!(echo_count, ROUNDS);
    });
}

#[test]
fn cloned_writers_share_one_stream_without_byte_interleaving() {
    let writer = SerializedWriter::new(ProbeWriter {
        in_write: Arc::new(AtomicBool::new(false)),
        reentries: Arc::new(AtomicUsize::new(0)),
        stream: Vec::new(),
    });
    let echo_writer = writer.clone();

    assert_eq!(writer.with_lock(|raw| raw.write_prefix(b"direct", 6)), 6);
    assert_eq!(echo_writer.with_lock(|raw| raw.write_prefix(b"echo", 4)), 4);
    writer.with_lock(|raw| assert_eq!(raw.stream, b"directecho"));
}
