//! Heap-backed fixed-capacity Ethernet frame storage and checked ticket
//! tracker (Task 2.1).
//!
//! This module is the future RX/TX packet-slot backing for
//! [`EthernetDevice`](super::EthernetDevice). It is transport-neutral: slots
//! hold plain frame bytes, a length and an optional TX ticket, and never a
//! `NetBufPtr`, descriptor, ring pointer or driver token. Construction
//! allocates the whole backing directly on the heap; no data-path operation
//! allocates afterwards.

use alloc::{boxed::Box, vec};

use smoltcp::wire::EthernetFrame;

use crate::consts::STANDARD_MTU;

/// Maximum ordinary Ethernet frame size: 1500-byte MTU plus the 14-byte
/// Ethernet header. Every currently supported frame fits in this bound.
pub(crate) const MAX_FRAME_SIZE: usize = STANDARD_MTU + EthernetFrame::<&[u8]>::header_len();

/// Upper bound on simultaneously live TX tickets.
pub(crate) const MAX_LIVE_TICKETS: usize = 128;

/// Result of a fixed-frame queue mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueueError {
    /// The queue holds `CAP` frames already; nothing was copied.
    Full,
    /// The offered frame exceeds [`MAX_FRAME_SIZE`]; nothing was copied.
    TooLarge,
}

/// Result of a ticket allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TicketError {
    /// The live-ticket backing is full (`MAX_LIVE_TICKETS`).
    LiveFull,
    /// The monotonic counter cannot advance past `u64::MAX`.
    CounterExhausted,
}

/// A fixed-capacity FIFO of complete frames over one heap-direct backing.
///
/// `CAP` entries each own a fixed `MAX_FRAME_SIZE`-byte region plus an
/// optional `Meta` value and an optional TX ticket. `new()` allocates the
/// whole storage on the heap; enqueue/peek/pop never allocate.
pub(crate) struct FixedFrameQueue<const CAP: usize, Meta: Copy = ()> {
    /// `CAP * MAX_FRAME_SIZE` contiguous bytes; entry `i` owns
    /// `storage[i * MAX_FRAME_SIZE .. (i + 1) * MAX_FRAME_SIZE]`.
    storage: Box<[u8]>,
    /// Per-entry occupied length; valid only for occupied ring positions.
    lengths: Box<[u16]>,
    /// Per-entry optional metadata.
    metas: Box<[Meta]>,
    /// Per-entry optional TX ticket.
    tickets: Box<[Option<u64>]>,
    /// Index of the oldest occupied entry.
    head: usize,
    /// Number of occupied entries.
    len: usize,
    /// Maximum `len` ever reached.
    high_water: usize,
    /// Number of times the queue transitioned into full.
    full_events: u64,
    /// Successful enqueues/fills.
    enqueue_events: u64,
    /// Successful pops.
    dequeue_events: u64,
    /// Full→space transitions (pop while full).
    space_events: u64,
}

impl<const CAP: usize, Meta: Copy + Default> FixedFrameQueue<CAP, Meta> {
    /// Allocates the full backing on the heap.
    ///
    /// The heap allocation happens inside `vec!` before any slice is boxed, so
    /// no `[Frame; CAP]` array is ever materialized on the stack.
    pub(crate) fn new() -> Self {
        Self::new_with(Meta::default())
    }
}

impl<const CAP: usize, Meta: Copy> FixedFrameQueue<CAP, Meta> {
    /// Allocates the full backing on the heap, seeding every metadata slot
    /// with `meta` (used when `Meta` has no `Default`, e.g. `IpAddress`).
    pub(crate) fn new_with(meta: Meta) -> Self {
        Self {
            storage: vec![0u8; CAP * MAX_FRAME_SIZE].into_boxed_slice(),
            lengths: vec![0u16; CAP].into_boxed_slice(),
            metas: vec![meta; CAP].into_boxed_slice(),
            tickets: vec![None; CAP].into_boxed_slice(),
            head: 0,
            len: 0,
            high_water: 0,
            full_events: 0,
            enqueue_events: 0,
            dequeue_events: 0,
            space_events: 0,
        }
    }

    // Dormant-slot observers consumed by Iteration 004 queue service and
    // Iteration 005 telemetry; unused in the product polling path.
    #[allow(dead_code)]
    pub(crate) fn capacity(&self) -> usize {
        CAP
    }

    #[allow(dead_code)]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn is_full(&self) -> bool {
        self.len == CAP
    }

    #[allow(dead_code)]
    pub(crate) fn high_water(&self) -> usize {
        self.high_water
    }

    #[allow(dead_code)]
    pub(crate) fn full_events(&self) -> u64 {
        self.full_events
    }

    #[allow(dead_code)]
    pub(crate) fn enqueue_events(&self) -> u64 {
        self.enqueue_events
    }

    #[allow(dead_code)]
    pub(crate) fn dequeue_events(&self) -> u64 {
        self.dequeue_events
    }

    #[allow(dead_code)]
    pub(crate) fn space_events(&self) -> u64 {
        self.space_events
    }

    /// Side-effect-free exact preflight for a frame of `size` bytes.
    pub(crate) fn preflight(&self, size: usize) -> Result<(), QueueError> {
        if size > MAX_FRAME_SIZE {
            return Err(QueueError::TooLarge);
        }
        if self.is_full() {
            return Err(QueueError::Full);
        }
        Ok(())
    }

    /// Enqueues one complete frame. On error no byte, occupancy, metadata or
    /// ticket is changed.
    pub(crate) fn enqueue(
        &mut self,
        data: &[u8],
        meta: Meta,
        ticket: Option<u64>,
    ) -> Result<(), QueueError> {
        self.preflight(data.len())?;
        let slot = (self.head + self.len) % CAP;
        let base = slot * MAX_FRAME_SIZE;
        self.storage[base..base + data.len()].copy_from_slice(data);
        self.lengths[slot] = data.len() as u16;
        self.metas[slot] = meta;
        self.tickets[slot] = ticket;
        self.len += 1;
        self.high_water = self.high_water.max(self.len);
        self.enqueue_events += 1;
        if self.len == CAP {
            self.full_events += 1;
        }
        Ok(())
    }

    /// Writes a frame directly into a reserved vacant slot and publishes it
    /// atomically (Task 2.4/3.2 copier seam).
    ///
    /// `f` receives the full `MAX_FRAME_SIZE`-byte vacant region and must
    /// return the number of bytes actually written, or `Err(())` to abort.
    /// On abort or a too-large result, no occupancy, metadata, length or
    /// ticket is published. The caller should preflight `size` first so the
    /// region is only handed out when capacity exists.
    pub(crate) fn fill<F>(
        &mut self,
        meta: Meta,
        ticket: Option<u64>,
        f: F,
    ) -> Result<usize, QueueError>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, ()>,
    {
        if self.is_full() {
            return Err(QueueError::Full);
        }
        let slot = (self.head + self.len) % CAP;
        let base = slot * MAX_FRAME_SIZE;
        let region = &mut self.storage[base..base + MAX_FRAME_SIZE];
        let len = f(region).map_err(|_| QueueError::TooLarge)?;
        if len > MAX_FRAME_SIZE {
            return Err(QueueError::TooLarge);
        }
        self.lengths[slot] = len as u16;
        self.metas[slot] = meta;
        self.tickets[slot] = ticket;
        self.len += 1;
        self.high_water = self.high_water.max(self.len);
        self.enqueue_events += 1;
        if self.len == CAP {
            self.full_events += 1;
        }
        Ok(len)
    }

    /// Returns the oldest frame without mutating the queue.
    pub(crate) fn peek(&self) -> Option<&[u8]> {
        if self.is_empty() {
            return None;
        }
        let slot = self.head;
        let base = slot * MAX_FRAME_SIZE;
        Some(&self.storage[base..base + self.lengths[slot] as usize])
    }

    /// Returns the oldest frame, its metadata and its optional ticket without
    /// mutating the queue (Task 3.2 copier observation seam).
    pub(crate) fn peek_full(&self) -> Option<(Meta, Option<u64>, &[u8])> {
        if self.is_empty() {
            return None;
        }
        let slot = self.head;
        let base = slot * MAX_FRAME_SIZE;
        Some((
            self.metas[slot],
            self.tickets[slot],
            &self.storage[base..base + self.lengths[slot] as usize],
        ))
    }

    /// Returns the oldest frame and its metadata without mutating the queue.
    pub(crate) fn peek_meta(&self) -> Option<(Meta, &[u8])> {
        if self.is_empty() {
            return None;
        }
        let slot = self.head;
        let base = slot * MAX_FRAME_SIZE;
        Some((
            self.metas[slot],
            &self.storage[base..base + self.lengths[slot] as usize],
        ))
    }

    /// Removes the oldest frame.
    ///
    /// Returns `Some(true)` when the queue was full before the pop (one
    /// full→space transition), `Some(false)` otherwise, and `None` when empty.
    pub(crate) fn pop(&mut self) -> Option<bool> {
        if self.is_empty() {
            return None;
        }
        let was_full = self.is_full();
        self.tickets[self.head] = None;
        self.head = (self.head + 1) % CAP;
        self.len -= 1;
        self.dequeue_events += 1;
        if was_full {
            self.space_events += 1;
        }
        Some(was_full)
    }
}

/// Lifecycle state of one live TX ticket (D8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TicketState {
    /// The frame sits in the fixed TX slot; the driver has not accepted it.
    Queued,
    /// The driver accepted the frame; its completion is outstanding.
    DeviceOwned,
}

/// A checked monotonic TX ticket allocator with a fixed live-ticket backing.
pub(crate) struct TicketTracker {
    next: u64,
    live: Box<[Option<(u64, TicketState)>]>,
    live_len: usize,
    last_accepted: Option<u64>,
}

impl TicketTracker {
    pub(crate) fn new() -> Self {
        Self {
            next: 0,
            live: vec![None; MAX_LIVE_TICKETS].into_boxed_slice(),
            live_len: 0,
            last_accepted: None,
        }
    }

    /// Allocates the next ticket as `Queued`. Fails when the live backing is
    /// full or when the counter cannot advance past `u64::MAX`.
    pub(crate) fn alloc(&mut self) -> Result<u64, TicketError> {
        if self.live_len == MAX_LIVE_TICKETS {
            return Err(TicketError::LiveFull);
        }
        if self.next == u64::MAX {
            return Err(TicketError::CounterExhausted);
        }
        let ticket = self.next;
        self.next += 1;
        // The fixed backing always has room because live_len was checked.
        let slot = self
            .live
            .iter_mut()
            .position(|entry| entry.is_none())
            .expect("live backing has a free slot");
        self.live[slot] = Some((ticket, TicketState::Queued));
        self.live_len += 1;
        self.last_accepted = Some(ticket);
        Ok(ticket)
    }

    /// Side-effect-free preflight for the next allocation (Task 3.4 slot-mode
    /// preflight: readiness depends on live-set room and counter headroom, and
    /// must never mutate the tracker).
    pub(crate) fn can_alloc(&self) -> bool {
        self.live_len < MAX_LIVE_TICKETS && self.next != u64::MAX
    }

    /// Transitions one `Queued` ticket to `DeviceOwned` (D8). Returns `false`
    /// for an unknown ticket or a second transition: both are owner drift.
    pub(crate) fn mark_device_owned(&mut self, ticket: u64) -> bool {
        let Some(slot) = self
            .live
            .iter()
            .position(|entry| *entry == Some((ticket, TicketState::Queued)))
        else {
            return false;
        };
        self.live[slot] = Some((ticket, TicketState::DeviceOwned));
        true
    }

    /// Removes a `Queued` ticket whose slot-fill aborted before submission.
    pub(crate) fn release_queued(&mut self, ticket: u64) -> bool {
        self.remove(ticket, Some(TicketState::Queued))
    }

    /// Removes a `DeviceOwned` ticket whose completion was reclaimed (C4).
    /// A non-`DeviceOwned` or unknown cookie is owner drift, not a success.
    pub(crate) fn release_device_owned(&mut self, ticket: u64) -> bool {
        self.remove(ticket, Some(TicketState::DeviceOwned))
    }

    /// Whether any live ticket is at or before `target` (D8 flush predicate).
    pub(crate) fn has_live_at_or_before(&self, target: u64) -> bool {
        self.live
            .iter()
            .any(|entry| matches!(entry, Some((t, _)) if *t <= target))
    }

    /// D8 C4 flush completion: `None` (empty data plane) is always complete;
    /// `Some(target)` completes once no live ticket `<= target` remains.
    pub(crate) fn flush_done(&self, target: Option<u64>) -> bool {
        match target {
            None => true,
            Some(target) => !self.has_live_at_or_before(target),
        }
    }

    /// The most recently accepted ticket, used as the flush target source.
    pub(crate) fn last_accepted(&self) -> Option<u64> {
        self.last_accepted
    }

    /// Number of live tickets still waiting in TX slots (D8).
    #[allow(dead_code)]
    pub(crate) fn queued_len(&self) -> usize {
        self.live
            .iter()
            .filter(|entry| matches!(entry, Some((_, TicketState::Queued))))
            .count()
    }

    /// Number of live tickets submitted to the driver with outstanding
    /// completions (D8).
    #[allow(dead_code)]
    pub(crate) fn device_owned_len(&self) -> usize {
        self.live
            .iter()
            .filter(|entry| matches!(entry, Some((_, TicketState::DeviceOwned))))
            .count()
    }

    fn remove(&mut self, ticket: u64, expected: Option<TicketState>) -> bool {
        let Some(slot) = self.live.iter().position(|entry| {
            matches!(entry, Some((t, state)) if *t == ticket && expected.is_none_or(|e| *state == e))
        }) else {
            return false;
        };
        self.live[slot] = None;
        self.live_len -= 1;
        true
    }

    // Ticket lookup/observation used by Iteration 004 flush and Iteration 005
    // telemetry; unused in the product polling path.
    #[allow(dead_code)]
    pub(crate) fn contains(&self, ticket: u64) -> bool {
        self.live
            .iter()
            .any(|entry| matches!(entry, Some((t, _)) if *t == ticket))
    }

    #[allow(dead_code)]
    pub(crate) fn live_len(&self) -> usize {
        self.live_len
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;
    use core::mem::size_of;

    use super::*;
    use crate::device::test_alloc::alloc_count;

    fn full_frame(n: u8) -> Vec<u8> {
        vec![n; MAX_FRAME_SIZE]
    }

    #[test]
    fn new_is_empty_with_exact_capacity() {
        let q = FixedFrameQueue::<64>::new();
        assert_eq!(q.capacity(), 64);
        assert_eq!(q.len(), 0);
        assert!(q.is_empty());
        assert!(!q.is_full());
        assert_eq!(q.high_water(), 0);
        assert_eq!(q.full_events(), 0);
        assert_eq!(q.peek(), None);
    }

    #[test]
    fn preflight_exact_length_and_capacity() {
        let mut q = FixedFrameQueue::<64>::new();
        assert_eq!(q.preflight(0), Ok(()));
        assert_eq!(q.preflight(MAX_FRAME_SIZE), Ok(()));
        assert_eq!(q.preflight(MAX_FRAME_SIZE + 1), Err(QueueError::TooLarge));
        // Fill to capacity; preflight must observe Full without side effects.
        for i in 0..64 {
            q.enqueue(&[i as u8; 1], (), None).unwrap();
        }
        assert_eq!(q.preflight(1), Err(QueueError::Full));
        assert_eq!(q.len(), 64);
        assert_eq!(q.peek().unwrap(), &[0u8; 1]);
    }

    #[test]
    fn accepts_exact_max_frame() {
        let mut q = FixedFrameQueue::<4>::new();
        let frame = full_frame(7);
        q.enqueue(&frame, (), None).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q.peek().unwrap(), &frame[..]);
    }

    #[test]
    fn oversize_enqueue_changes_nothing() {
        let mut q = FixedFrameQueue::<4>::new();
        let ok = vec![1u8; 100];
        q.enqueue(&ok, (), Some(9)).unwrap();
        let too_large = vec![2u8; MAX_FRAME_SIZE + 1];
        assert_eq!(q.enqueue(&too_large, (), None), Err(QueueError::TooLarge));
        // No bytes, occupancy or ticket changed.
        assert_eq!(q.len(), 1);
        assert_eq!(q.peek().unwrap(), &ok[..]);
        assert_eq!(q.high_water(), 1);
        assert_eq!(q.full_events(), 0);
    }

    #[test]
    fn full_rejects_65th_without_side_effect() {
        let mut q = FixedFrameQueue::<64>::new();
        for i in 0..64 {
            q.enqueue(&[i as u8; 1], (), None).unwrap();
        }
        assert!(q.is_full());
        assert_eq!(q.high_water(), 64);
        assert_eq!(q.full_events(), 1);
        // The 65th frame is rejected and the oldest frame is untouched.
        assert_eq!(q.enqueue(&[99u8; 1], (), None), Err(QueueError::Full));
        assert_eq!(q.len(), 64);
        assert_eq!(q.peek().unwrap(), &[0u8; 1]);
        assert_eq!(q.high_water(), 64);
        assert_eq!(q.full_events(), 1);
    }

    #[test]
    fn failed_enqueue_preserves_failed_bytes_untouched() {
        let mut q = FixedFrameQueue::<2>::new();
        q.enqueue(&[1u8; 10], (), None).unwrap();
        // Fill fully with a frame that differs from the rejected one.
        q.enqueue(&[2u8; 20], (), None).unwrap();
        let probe = vec![3u8; 5];
        assert_eq!(q.enqueue(&probe, (), None), Err(QueueError::Full));
        assert_eq!(q.peek().unwrap(), &[1u8; 10]);
    }

    #[test]
    fn fill_publishes_length_meta_and_ticket_after_success() {
        let mut q = FixedFrameQueue::<4, u8>::new_with(0);
        let len = q
            .fill(9, Some(42), |region| {
                region[..5].copy_from_slice(&[7; 5]);
                Ok(5)
            })
            .unwrap();
        assert_eq!(len, 5);
        assert_eq!(q.len(), 1);
        let (meta, ticket, bytes) = q.peek_full().unwrap();
        assert_eq!(meta, 9);
        assert_eq!(ticket, Some(42));
        assert_eq!(bytes, &[7; 5]);
    }

    #[test]
    fn fill_abort_publishes_nothing() {
        let mut q = FixedFrameQueue::<2>::new();
        assert_eq!(q.fill((), Some(1), |_| Err(())), Err(QueueError::TooLarge));
        assert_eq!(q.len(), 0);
        assert_eq!(q.peek(), None);
    }

    #[test]
    fn fill_oversize_result_publishes_nothing() {
        let mut q = FixedFrameQueue::<2>::new();
        assert_eq!(
            q.fill((), Some(1), |_| Ok(MAX_FRAME_SIZE + 1)),
            Err(QueueError::TooLarge)
        );
        assert_eq!(q.len(), 0);
        assert_eq!(q.peek(), None);
    }

    #[test]
    fn fill_full_rejects_without_touching_head() {
        let mut q = FixedFrameQueue::<2>::new();
        q.enqueue(&[1u8; 3], (), None).unwrap();
        q.enqueue(&[2u8; 3], (), None).unwrap();
        assert_eq!(q.fill((), None, |_| Ok(1)), Err(QueueError::Full));
        assert_eq!(q.len(), 2);
        assert_eq!(q.peek().unwrap(), &[1u8; 3]);
    }

    #[test]
    fn fill_writes_directly_into_vacant_slot_storage() {
        let mut q = FixedFrameQueue::<2, u8>::new_with(0);
        q.enqueue(&[1u8; 2], 1u8, None).unwrap();
        // The vacant slot is the second region; a fill writes it in place.
        q.fill(3u8, None, |region| {
            region[..4].copy_from_slice(&[9; 4]);
            Ok(4)
        })
        .unwrap();
        assert_eq!(q.len(), 2);
        assert_eq!(q.peek().unwrap(), &[1u8; 2]);
        // First head is still the first frame after one pop.
        let _ = q.pop();
        let (_, _, second) = q.peek_full().unwrap();
        assert_eq!(second, &[9; 4]);
    }

    #[test]
    fn peek_full_reports_none_when_empty_and_preserves_order() {
        let mut q = FixedFrameQueue::<3, u8>::new_with(0);
        assert_eq!(q.peek_full(), None);
        q.enqueue(&[1u8; 1], 1u8, Some(10u64)).unwrap();
        q.enqueue(&[2u8; 1], 2u8, None).unwrap();
        let (meta, ticket, bytes) = q.peek_full().unwrap();
        assert_eq!(meta, 1u8);
        assert_eq!(ticket, Some(10u64));
        assert_eq!(bytes, &[1u8; 1]);
        let _ = q.pop();
        let (meta, ticket, bytes) = q.peek_full().unwrap();
        assert_eq!(meta, 2u8);
        assert_eq!(ticket, None);
        assert_eq!(bytes, &[2u8; 1]);
    }

    #[test]
    fn peek_is_read_only() {
        let mut q = FixedFrameQueue::<4>::new();
        q.enqueue(&[7u8; 8], (), None).unwrap();
        let first = q.peek().unwrap();
        let second = q.peek().unwrap();
        assert_eq!(first, second);
        assert_eq!(q.len(), 1);
        assert_eq!(q.high_water(), 1);
        // The backing bytes must not be overwritten by peeking.
        assert_eq!(first[0], 7);
    }

    #[test]
    fn pop_from_full_reports_one_space_transition() {
        let mut q = FixedFrameQueue::<2>::new();
        q.enqueue(&[1u8; 1], (), None).unwrap();
        q.enqueue(&[2u8; 1], (), None).unwrap();
        assert_eq!(q.pop(), Some(true));
        assert_eq!(q.pop(), Some(false));
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn repeated_wrap_preserves_fifo() {
        let mut q = FixedFrameQueue::<3>::new();
        for i in 0..3 {
            q.enqueue(&[i as u8; 1], (), None).unwrap();
        }
        assert_eq!(q.pop(), Some(true));
        assert_eq!(q.pop(), Some(false));
        q.enqueue(&[3u8; 1], (), None).unwrap();
        q.enqueue(&[4u8; 1], (), None).unwrap();
        // Head has wrapped; order must remain FIFO.
        let mut seen = Vec::new();
        while let Some(frame) = q.peek() {
            seen.push(frame[0]);
            q.pop();
        }
        assert_eq!(seen, vec![2, 3, 4]);
    }

    #[test]
    fn max_frame_wrap_preserves_fifo() {
        let mut q = FixedFrameQueue::<64>::new();
        for i in 0..64u8 {
            q.enqueue(&full_frame(i), (), None).unwrap();
        }
        for _ in 0..63 {
            q.pop();
        }
        for i in 0..63u8 {
            q.enqueue(&full_frame(i + 100), (), None).unwrap();
        }
        let mut seen = Vec::new();
        while let Some(frame) = q.peek() {
            seen.push(frame[0]);
            q.pop();
        }
        assert_eq!(seen.len(), 64);
        // Oldest remaining is 63, then the wrapped batch 100..162.
        assert_eq!(seen[0], 63);
        assert_eq!(seen[1], 100);
        assert_eq!(seen[63], 162);
    }

    #[test]
    fn size_proves_heap_direct_storage() {
        // The backing lives behind `Box`, so the struct is small; a stack
        // materialized `[Frame; 64]` would be ~97 KiB.
        assert!(size_of::<FixedFrameQueue<64>>() < 1024);
        // Sanity: the declared frame size matches the MTU + Ethernet header.
        assert_eq!(MAX_FRAME_SIZE, STANDARD_MTU + 14);
    }

    #[test]
    fn construction_allocates_but_data_path_does_not() {
        let before = alloc_count();
        let mut q = FixedFrameQueue::<64>::new();
        let mut tracker = TicketTracker::new();
        let after_construct = alloc_count();
        // Heap-direct construction must perform heap allocations.
        assert!(after_construct > before);

        let frame = full_frame(9);
        let frozen = alloc_count();
        q.enqueue(&frame, (), Some(1)).unwrap();
        q.enqueue(&[8u8; 5], (), None).unwrap();
        assert_eq!(q.peek(), Some(&frame[..]));
        let _ = q.pop();
        let _ = q.pop();
        let _ = q.preflight(10);
        let ticket = tracker.alloc().unwrap();
        tracker.release_queued(ticket);
        assert_eq!(
            alloc_count(),
            frozen,
            "data-path operations must not allocate"
        );
    }

    #[test]
    fn ticket_allocator_is_monotonic_and_checked() {
        let mut tracker = TicketTracker::new();
        let a = tracker.alloc().unwrap();
        let b = tracker.alloc().unwrap();
        assert_eq!(a, 0);
        assert_eq!(b, 1);
        assert!(tracker.contains(a));
        assert!(tracker.contains(b));
        assert_eq!(tracker.live_len(), 2);
        assert!(tracker.release_queued(a));
        assert!(!tracker.contains(a));
        assert!(!tracker.release_queued(a));
        assert_eq!(tracker.live_len(), 1);
    }

    #[test]
    fn ticket_backing_exhausts_at_128_live() {
        let mut tracker = TicketTracker::new();
        for _ in 0..MAX_LIVE_TICKETS {
            tracker.alloc().unwrap();
        }
        assert_eq!(tracker.live_len(), MAX_LIVE_TICKETS);
        assert_eq!(tracker.alloc(), Err(TicketError::LiveFull));
        // Releasing a ticket makes room again.
        tracker.release_queued(0);
        assert_eq!(tracker.alloc(), Ok(MAX_LIVE_TICKETS as u64));
    }

    #[test]
    fn ticket_counter_exhausts_at_u64_max() {
        let mut tracker = TicketTracker::new();
        tracker.next = u64::MAX;
        assert_eq!(tracker.alloc(), Err(TicketError::CounterExhausted));
        // No ticket was consumed.
        assert_eq!(tracker.live_len(), 0);
    }

    // ---- Task 4.1: ticket lifecycle states and target-scoped C4 flush ----

    #[test]
    fn ticket_alloc_sets_queued_state_and_last_accepted() {
        let mut tracker = TicketTracker::new();
        assert_eq!(tracker.last_accepted(), None);
        let a = tracker.alloc().unwrap();
        assert_eq!(tracker.last_accepted(), Some(a));
        assert_eq!(tracker.queued_len(), 1);
        assert_eq!(tracker.device_owned_len(), 0);
        let b = tracker.alloc().unwrap();
        assert_eq!(tracker.last_accepted(), Some(b));
        assert_eq!(tracker.queued_len(), 2);
        assert_eq!(tracker.live_len(), 2);
    }

    #[test]
    fn ticket_mark_device_owned_transitions_state_exactly_once() {
        let mut tracker = TicketTracker::new();
        let a = tracker.alloc().unwrap();
        // A Queued ticket can be submitted to the driver exactly once.
        assert!(tracker.mark_device_owned(a));
        assert_eq!(tracker.queued_len(), 0);
        assert_eq!(tracker.device_owned_len(), 1);
        // A second transition is drift: never a silent success.
        assert!(!tracker.mark_device_owned(a));
        assert_eq!(tracker.device_owned_len(), 1);
        // An unknown ticket cannot transition.
        assert!(!tracker.mark_device_owned(99));
    }

    #[test]
    fn reclaim_only_releases_device_owned_tickets() {
        let mut tracker = TicketTracker::new();
        let a = tracker.alloc().unwrap();
        // The ticket is still Queued (never submitted): a completion cookie
        // matching it is ownership drift, not a successful reclaim.
        assert!(!tracker.release_device_owned(a));
        assert_eq!(tracker.live_len(), 1);
        tracker.mark_device_owned(a);
        assert!(tracker.release_device_owned(a));
        assert_eq!(tracker.live_len(), 0);
        // A duplicate completion cookie is drift.
        assert!(!tracker.release_device_owned(a));
    }

    #[test]
    fn release_queued_ticket_cancels_pre_submit_abort() {
        let mut tracker = TicketTracker::new();
        let a = tracker.alloc().unwrap();
        // The generic release backs the slot-fill abort path where a Queued
        // ticket must be returned without a completion.
        assert!(tracker.release_queued(a));
        assert_eq!(tracker.live_len(), 0);
        assert_eq!(tracker.last_accepted(), Some(a));
        assert!(!tracker.release_queued(a));
    }

    #[test]
    fn flush_done_empty_data_plane_succeeds_immediately() {
        let tracker = TicketTracker::new();
        assert!(tracker.flush_done(None));
    }

    #[test]
    fn flush_done_queued_ticket_blocks_until_submit_and_reclaim() {
        let mut tracker = TicketTracker::new();
        let a = tracker.alloc().unwrap();
        // A Queued ticket is still live: the flush target is not satisfied.
        assert!(!tracker.flush_done(Some(a)));
        // Submitting keeps the ticket live until the completion is reclaimed.
        tracker.mark_device_owned(a);
        assert!(!tracker.flush_done(Some(a)));
        tracker.release_device_owned(a);
        assert!(tracker.flush_done(Some(a)));
    }

    #[test]
    fn flush_done_out_of_order_hole_blocks_until_all_target_tickets_reclaimed() {
        let mut tracker = TicketTracker::new();
        let a = tracker.alloc().unwrap(); // 0
        let b = tracker.alloc().unwrap(); // 1
        let c = tracker.alloc().unwrap(); // 2
        for t in [a, b, c] {
            tracker.mark_device_owned(t);
        }
        // Reclaim out of order: 2 then 0 leaves the hole at 1.
        assert!(tracker.release_device_owned(c));
        assert!(!tracker.flush_done(Some(c)));
        assert!(tracker.release_device_owned(a));
        assert!(!tracker.flush_done(Some(c)));
        assert!(tracker.release_device_owned(b));
        assert!(tracker.flush_done(Some(c)));
    }

    #[test]
    fn flush_done_post_target_tickets_do_not_block() {
        let mut tracker = TicketTracker::new();
        let a = tracker.alloc().unwrap(); // 0
        let b = tracker.alloc().unwrap(); // 1
        tracker.mark_device_owned(a);
        tracker.mark_device_owned(b);
        // Flush target 0: ticket 1 accepted after the target is irrelevant.
        assert!(!tracker.flush_done(Some(a)));
        tracker.release_device_owned(a);
        assert!(tracker.flush_done(Some(a)));
        assert!(!tracker.flush_done(Some(b)));
    }

    #[test]
    fn flush_done_never_mutates_live_set() {
        let mut tracker = TicketTracker::new();
        let a = tracker.alloc().unwrap();
        let b = tracker.alloc().unwrap();
        tracker.mark_device_owned(a);
        assert!(!tracker.flush_done(Some(a)));
        assert!(!tracker.flush_done(Some(b)));
        assert_eq!(tracker.live_len(), 2);
        assert_eq!(tracker.queued_len(), 1);
        assert_eq!(tracker.device_owned_len(), 1);
    }

    #[test]
    fn ticket_counter_exhaustion_keeps_flush_semantics() {
        let mut tracker = TicketTracker::new();
        let a = tracker.alloc().unwrap();
        tracker.mark_device_owned(a);
        tracker.next = u64::MAX;
        // The allocator refuses to advance past u64::MAX: no ticket aliases
        // the sentinel and flush completion still depends only on the live set.
        assert_eq!(tracker.alloc(), Err(TicketError::CounterExhausted));
        assert_eq!(tracker.last_accepted(), Some(a));
        assert!(!tracker.flush_done(Some(a)));
        tracker.release_device_owned(a);
        assert!(tracker.flush_done(Some(a)));
    }
}
