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

use axdriver_net::{QueueEpoch, TxCookie};
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

/// Bounded stage identity carried by a
/// [`Fault`](TicketOutcome::Fault) terminal outcome (Task 2.1 / A1).
///
/// The codes mirror the D3 `recover_stage` diagnostic codes
/// (`crate::async_rx::recover_stage`) so a faulted ticket is diagnosable
/// without widening the frozen V1–V3 ABI. The mapping is pinned by a
/// stability test; do not renumber either side independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TicketFaultStage {
    /// Submit wait fault: a Queued frame was never accepted.
    SubmitWait,
    /// Completion wait fault: a DeviceOwned completion did not arrive.
    CompletionWait,
    /// Reclaim fault: a reclaimable completion could not be reaped.
    Reclaim,
    /// Quiesce stage fault (bounded DeviceOwned drain elapsed).
    Quiesce,
    /// Reset stage fault (status == 0 confirmation or `begin_recovery` failed).
    Reset,
    /// Reinitialize stage fault (queue/backing rebuild failed).
    Reinitialize,
    /// Ownership/identity/ledger drift detected; no reset attempted.
    OwnershipDrift,
    /// Unclassified fault.
    Unknown,
}

impl TicketFaultStage {
    /// The mirrored D3 diagnostic code (see `recover_stage`).
    pub(crate) fn code(self) -> u64 {
        match self {
            Self::SubmitWait => crate::async_rx::recover_stage::SUBMIT_WAIT,
            Self::CompletionWait => crate::async_rx::recover_stage::COMPLETION_WAIT,
            Self::Reclaim => crate::async_rx::recover_stage::RECLAIM,
            Self::Quiesce => crate::async_rx::recover_stage::QUIESCE,
            Self::Reset => crate::async_rx::recover_stage::RESET,
            Self::Reinitialize => crate::async_rx::recover_stage::REINITIALIZE,
            Self::OwnershipDrift => crate::async_rx::recover_stage::OWNERSHIP_DRIFT,
            Self::Unknown => crate::async_rx::recover_stage::UNKNOWN,
        }
    }
}

/// Terminal reason a ticket left the device-owner live set. A fresh ticket is
/// `Queued`, then `DeviceOwned`; it reaches exactly one terminal outcome:
/// [`Reclaimed`](TicketOutcome::Reclaimed) for a matched completion, or a
/// packet-loss outcome [`CancelledPreSubmit`](TicketOutcome::CancelledPreSubmit)
/// / [`ResetAborted`](TicketOutcome::ResetAborted) /
/// [`Fault`](TicketOutcome::Fault).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TicketOutcome {
    /// A matched `(epoch, ticket, DeviceOwned)` completion closed the ticket.
    Reclaimed,
    /// A recovery cancelled the ticket while still `Queued` (never submitted).
    CancelledPreSubmit,
    /// A confirmed device reset closed the ticket while it was `DeviceOwned`.
    ResetAborted,
    /// An ownership or device fault terminated the ticket; the payload is the
    /// bounded stage identity the owner committed for the fault.
    Fault(TicketFaultStage),
}

/// Whether a target-scoped C4 flush may complete, given the epoch-scoped
/// ticket ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlushState {
    /// Every ticket `<= target` in the current epoch reached a terminal
    /// outcome and every one of them was [`Reclaimed`](TicketOutcome::Reclaimed).
    Done,
    /// Some ticket `<= target` is still live (`Queued`/`DeviceOwned`).
    Pending,
    /// A ticket `<= target` in the current epoch ended in a non-Reclaimed
    /// packet-loss outcome; the flush can never succeed and must fail stably.
    Lost(TicketOutcome),
}

/// A checked monotonic TX ticket allocator with a fixed live-ticket backing.
///
/// Each live owner is bound to the device-reset [`QueueEpoch`] it was
/// allocated under, so a completion cookie can only close the current-epoch
/// ticket with the matching ticket. Terminal outcomes are recorded in a
/// bounded, epoch-scoped summary (a single monotonic "first lost ticket"), so
/// a flush can distinguish a successful reclaim from a cancelled/reset/fault
/// packet loss without unbounded history.
pub(crate) struct TicketTracker {
    /// The device-reset epoch every live ticket currently belongs to.
    epoch: QueueEpoch,
    next: u64,
    live: Box<[Option<(QueueEpoch, u64, TicketState)>]>,
    live_len: usize,
    last_accepted: Option<u64>,
    /// Lowest ticket in the *current epoch* that ended non-Reclaimed; `None`
    /// when no packet loss occurred yet. A flush whose target `>=` this ticket
    /// must fail. Reset when the epoch advances.
    first_lost: Option<(u64, TicketOutcome)>,
}

impl TicketTracker {
    pub(crate) fn new() -> Self {
        Self {
            epoch: QueueEpoch::MIN,
            next: 0,
            live: vec![None; MAX_LIVE_TICKETS].into_boxed_slice(),
            live_len: 0,
            last_accepted: None,
            first_lost: None,
        }
    }

    /// The device-reset epoch every ticket currently belongs to.
    pub(crate) fn current_epoch(&self) -> QueueEpoch {
        self.epoch
    }

    /// Advances to `next_epoch`, requiring the live set already be empty and
    /// re-seeding the bounded loss summary for the new generation. Only the
    /// recovery owner calls this after confirming the device stopped and every
    /// old ticket was closed as [`ResetAborted`](TicketOutcome::ResetAborted)
    /// (the queue task is the caller that holds them).
    pub(crate) fn advance_epoch(&mut self, next_epoch: QueueEpoch) {
        debug_assert_eq!(self.live_len, 0, "advance requires an empty live set");
        self.epoch = next_epoch;
        self.first_lost = None;
    }

    /// Allocates the next ticket as `Queued` in the current epoch. Fails when
    /// the live backing is full or when the counter cannot advance past
    /// `u64::MAX`.
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
        self.live[slot] = Some((self.epoch, ticket, TicketState::Queued));
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
            .position(|entry| *entry == Some((self.epoch, ticket, TicketState::Queued)))
        else {
            return false;
        };
        self.live[slot] = Some((self.epoch, ticket, TicketState::DeviceOwned));
        true
    }

    /// Removes a `Queued` ticket whose slot-fill aborted before submission.
    /// This is a pre-submit packet loss, recorded so a flush can fail
    /// instead of reporting a silent success.
    pub(crate) fn release_queued(&mut self, ticket: u64) -> bool {
        if self.remove(ticket, Some(TicketState::Queued)) {
            self.record_lost(ticket, TicketOutcome::CancelledPreSubmit);
            true
        } else {
            false
        }
    }

    /// Removes a `DeviceOwned` ticket whose completion was reclaimed (C4),
    /// matching the cookie epoch and ticket exactly. A non-`DeviceOwned`,
    /// unknown, stale-epoch or duplicate cookie is owner drift, not a success.
    pub(crate) fn release_device_owned(&mut self, cookie: TxCookie) -> bool {
        let Some(slot) = self.live.iter().position(|entry| {
            matches!(entry, Some((e, t, TicketState::DeviceOwned)) if *e == cookie.epoch() && *t == cookie.ticket())
        }) else {
            return false;
        };
        self.live[slot] = None;
        self.live_len -= 1;
        true
    }

    /// Recovery cancellation: atomically cancels every `Queued` ticket of the
    /// current epoch as [`CancelledPreSubmit`](TicketOutcome::CancelledPreSubmit),
    /// returning the number cancelled. `DeviceOwned` tickets are untouched and
    /// keep their owner. The single Service-guard caller linearizes this with
    /// submit, so no ticket is both submitted and excluded.
    pub(crate) fn cancel_queued(&mut self) -> usize {
        let mut cancelled = 0usize;
        for entry in self.live.iter_mut() {
            if let Some((e, t, TicketState::Queued)) = entry
                && *e == self.epoch
            {
                let ticket = *t;
                *entry = None;
                self.live_len -= 1;
                match self.first_lost {
                    Some((current, _)) if current <= ticket => {}
                    _ => self.first_lost = Some((ticket, TicketOutcome::CancelledPreSubmit)),
                }
                cancelled += 1;
            }
        }
        cancelled
    }

    /// Recovery close: after a confirmed `status == 0`, closes every remaining
    /// `DeviceOwned` ticket of the current epoch as
    /// [`ResetAborted`](TicketOutcome::ResetAborted), returning the count.
    /// `Queued` tickets were already cancelled by the recovery owner.
    pub(crate) fn close_device_owned(&mut self) -> usize {
        let mut closed = 0usize;
        for entry in self.live.iter_mut() {
            if let Some((e, t, TicketState::DeviceOwned)) = entry
                && *e == self.epoch
            {
                let ticket = *t;
                *entry = None;
                self.live_len -= 1;
                match self.first_lost {
                    Some((current, _)) if current <= ticket => {}
                    _ => self.first_lost = Some((ticket, TicketOutcome::ResetAborted)),
                }
                closed += 1;
            }
        }
        closed
    }

    /// Recovery fault closure: terminates every `DeviceOwned` ticket of the
    /// current epoch as [`Fault`](TicketOutcome::Fault) with the committed
    /// `stage` identity, returning the count. Unlike a confirmed-recovery
    /// close, the driver backing is NOT released — the recovery holder keeps
    /// it quarantined — so the tickets are removed from the live set (a flush
    /// fails stably instead of pending forever) but the adapter-side
    /// DMA/buffer ownership is untouched. Used by the owner when it commits a
    /// resident `Faulted` without a confirmed reset (F4).
    pub(crate) fn fault_outstanding(&mut self, stage: TicketFaultStage) -> usize {
        let mut faulted = 0usize;
        for entry in self.live.iter_mut() {
            if let Some((e, t, TicketState::DeviceOwned)) = entry
                && *e == self.epoch
            {
                let ticket = *t;
                *entry = None;
                self.live_len -= 1;
                match self.first_lost {
                    Some((current, _)) if current <= ticket => {}
                    _ => self.first_lost = Some((ticket, TicketOutcome::Fault(stage))),
                }
                faulted += 1;
            }
        }
        faulted
    }

    /// Whether any live ticket is at or before `target` (D8 flush predicate).
    pub(crate) fn has_live_at_or_before(&self, target: u64) -> bool {
        self.live
            .iter()
            .any(|entry| matches!(entry, Some((_, t, _)) if *t <= target))
    }

    /// D8 C4 flush state. `None` (empty data plane) is always `Done`;
    /// `Some(target)` is `Lost` when a packet-loss outcome occurred at a ticket
    /// `<= target` in the current epoch, `Pending` while a live ticket
    /// `<= target` remains, and only `Done` when every ticket `<= target` was
    /// reclaimed.
    pub(crate) fn flush_state(&self, target: Option<u64>) -> FlushState {
        match target {
            None => FlushState::Done,
            Some(target) => {
                if let Some((lost, outcome)) = self.first_lost
                    && lost <= target
                {
                    return FlushState::Lost(outcome);
                }
                if self.has_live_at_or_before(target) {
                    FlushState::Pending
                } else {
                    FlushState::Done
                }
            }
        }
    }

    /// Whether a target-scoped flush is complete: `true` exactly when
    /// [`Self::flush_state`] is `Done`. Test-only convenience over
    /// [`Self::flush_state`]; production paths use `flush_state` directly so a
    /// lost outcome is never conflated with success.
    #[cfg(test)]
    pub(crate) fn flush_done(&self, target: Option<u64>) -> bool {
        matches!(self.flush_state(target), FlushState::Done)
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
            .filter(|entry| matches!(entry, Some((_, _, TicketState::Queued))))
            .count()
    }

    /// Number of live tickets submitted to the driver with outstanding
    /// completions (D8).
    #[allow(dead_code)]
    pub(crate) fn device_owned_len(&self) -> usize {
        self.live
            .iter()
            .filter(|entry| matches!(entry, Some((_, _, TicketState::DeviceOwned))))
            .count()
    }

    /// Records the lowest non-Reclaimed ticket of the current epoch so a
    /// flush can fail closed. Keeps the bounded min across the generation.
    fn record_lost(&mut self, ticket: u64, outcome: TicketOutcome) {
        match self.first_lost {
            Some((current, _)) if current <= ticket => {}
            _ => self.first_lost = Some((ticket, outcome)),
        }
    }

    fn remove(&mut self, ticket: u64, expected: Option<TicketState>) -> bool {
        let Some(slot) = self.live.iter().position(|entry| {
            matches!(entry, Some((e, t, state)) if *t == ticket && *e == self.epoch && expected.is_none_or(|exp| *state == exp))
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
            .any(|entry| matches!(entry, Some((_, t, _)) if *t == ticket))
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
        assert!(!tracker.release_device_owned(TxCookie::new(a)));
        assert_eq!(tracker.live_len(), 1);
        tracker.mark_device_owned(a);
        assert!(tracker.release_device_owned(TxCookie::new(a)));
        assert_eq!(tracker.live_len(), 0);
        // A duplicate completion cookie is drift.
        assert!(!tracker.release_device_owned(TxCookie::new(a)));
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
        tracker.release_device_owned(TxCookie::new(a));
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
        assert!(tracker.release_device_owned(TxCookie::new(c)));
        assert!(!tracker.flush_done(Some(c)));
        assert!(tracker.release_device_owned(TxCookie::new(a)));
        assert!(!tracker.flush_done(Some(c)));
        assert!(tracker.release_device_owned(TxCookie::new(b)));
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
        tracker.release_device_owned(TxCookie::new(a));
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
        tracker.release_device_owned(TxCookie::new(a));
        assert!(tracker.flush_done(Some(a)));
    }

    mod task21 {
        use super::*;

        fn next_epoch(e: QueueEpoch) -> QueueEpoch {
            e.advance().expect("epoch headroom")
        }

        #[test]
        fn reclaim_requires_current_epoch_and_device_owned() {
            let mut t = TicketTracker::new();
            let a = t.alloc().unwrap();
            let e1 = next_epoch(t.current_epoch());
            assert!(!t.release_device_owned(TxCookie::with_epoch(e1, a)));
            assert_eq!(t.live_len(), 1);
            assert!(!t.release_device_owned(TxCookie::with_epoch(t.current_epoch(), a)));
            assert_eq!(t.live_len(), 1);
            t.mark_device_owned(a);
            assert!(!t.release_device_owned(TxCookie::with_epoch(t.current_epoch(), 999)));
            assert!(!t.release_device_owned(TxCookie::with_epoch(e1, a)));
            assert!(t.release_device_owned(TxCookie::with_epoch(t.current_epoch(), a)));
            assert_eq!(t.live_len(), 0);
            assert!(!t.release_device_owned(TxCookie::with_epoch(t.current_epoch(), a)));
            assert_eq!(t.flush_state(Some(a)), FlushState::Done);
        }

        #[test]
        fn cancel_queued_is_exactly_once_and_flushes_lost() {
            let mut t = TicketTracker::new();
            let a = t.alloc().unwrap();
            let b = t.alloc().unwrap();
            t.mark_device_owned(a);
            assert_eq!(t.cancel_queued(), 1);
            assert_eq!(t.queued_len(), 0);
            assert_eq!(t.device_owned_len(), 1);
            assert_eq!(t.live_len(), 1);
            assert_eq!(t.cancel_queued(), 0);
            assert_eq!(
                t.flush_state(Some(b)),
                FlushState::Lost(TicketOutcome::CancelledPreSubmit)
            );
            assert!(t.release_device_owned(TxCookie::with_epoch(t.current_epoch(), a)));
        }

        #[test]
        fn device_owned_cancel_is_rejected_not_aborted() {
            let mut t = TicketTracker::new();
            let a = t.alloc().unwrap();
            t.mark_device_owned(a);
            assert!(!t.release_queued(a));
            assert_eq!(t.live_len(), 1);
            assert_eq!(t.device_owned_len(), 1);
            assert_eq!(t.flush_state(Some(a)), FlushState::Pending);
            assert!(t.release_device_owned(TxCookie::with_epoch(t.current_epoch(), a)));
            assert_eq!(t.flush_state(Some(a)), FlushState::Done);
        }

        #[test]
        fn close_device_owned_marks_reset_aborted_and_fails_flush() {
            let mut t = TicketTracker::new();
            let a = t.alloc().unwrap();
            let b = t.alloc().unwrap();
            t.mark_device_owned(a);
            t.mark_device_owned(b);
            assert_eq!(t.close_device_owned(), 2);
            assert_eq!(t.live_len(), 0);
            assert_eq!(
                t.flush_state(Some(b)),
                FlushState::Lost(TicketOutcome::ResetAborted)
            );
            assert_eq!(
                t.flush_state(Some(a)),
                FlushState::Lost(TicketOutcome::ResetAborted)
            );
        }

        #[test]
        fn flush_outcome_is_min_lost_not_unbounded_history() {
            let mut t = TicketTracker::new();
            let a = t.alloc().unwrap();
            let b = t.alloc().unwrap();
            let c = t.alloc().unwrap();
            t.mark_device_owned(a);
            t.mark_device_owned(c);
            assert!(t.release_device_owned(TxCookie::with_epoch(t.current_epoch(), a)));
            assert!(t.release_device_owned(TxCookie::with_epoch(t.current_epoch(), c)));
            assert_eq!(t.cancel_queued(), 1);
            assert_eq!(t.flush_state(Some(a)), FlushState::Done);
            assert_eq!(
                t.flush_state(Some(b)),
                FlushState::Lost(TicketOutcome::CancelledPreSubmit)
            );
            assert_eq!(
                t.flush_state(Some(c)),
                FlushState::Lost(TicketOutcome::CancelledPreSubmit)
            );
        }

        #[test]
        fn epoch_advance_clears_loss_and_fresh_generation_flushes_clean() {
            let mut t = TicketTracker::new();
            let a = t.alloc().unwrap();
            t.mark_device_owned(a);
            assert_eq!(t.close_device_owned(), 1);
            let e1 = t.current_epoch().advance().expect("headroom");
            t.advance_epoch(e1);
            assert_eq!(t.current_epoch(), e1);
            assert_eq!(t.live_len(), 0);
            assert_eq!(t.flush_state(None), FlushState::Done);
            let b = t.alloc().unwrap();
            assert_eq!(t.flush_state(Some(b)), FlushState::Pending);
            t.mark_device_owned(b);
            assert!(t.release_device_owned(TxCookie::with_epoch(e1, b)));
            assert_eq!(t.flush_state(Some(b)), FlushState::Done);
        }

        #[test]
        fn stale_epoch_cookie_cannot_meet_new_generation_flush() {
            let mut t = TicketTracker::new();
            let a = t.alloc().unwrap();
            t.mark_device_owned(a);
            let old_epoch = t.current_epoch();
            assert_eq!(t.close_device_owned(), 1);
            let e1 = old_epoch.advance().expect("headroom");
            t.advance_epoch(e1);
            assert!(!t.release_device_owned(TxCookie::with_epoch(old_epoch, a)));
        }

        #[test]
        fn loss_summary_is_bounded_to_first_lost() {
            let mut t = TicketTracker::new();
            let a = t.alloc().unwrap();
            assert_eq!(t.cancel_queued(), 1);
            assert_eq!(t.first_lost, Some((a, TicketOutcome::CancelledPreSubmit)));
            let _b = t.alloc().unwrap();
            assert_eq!(t.cancel_queued(), 1);
            assert_eq!(t.first_lost, Some((a, TicketOutcome::CancelledPreSubmit)));
        }

        #[test]
        fn fault_closure_closes_device_owned_as_fault_and_fails_flush_stably() {
            // F4 / A1 / A5: on a resident fault without a confirmed reset, the
            // owner terminates every DeviceOwned ticket as `Fault` (backing
            // stays quarantined in the adapter). A flush at or after the
            // faulted ticket must fail stably — never read a false success
            // from the faulted generation's ledger.
            let mut t = TicketTracker::new();
            let a = t.alloc().unwrap();
            let b = t.alloc().unwrap();
            t.mark_device_owned(b);
            assert_eq!(t.fault_outstanding(TicketFaultStage::OwnershipDrift), 1);
            assert_eq!(t.live_len(), 1);
            assert_eq!(
                t.flush_state(Some(b)),
                FlushState::Lost(TicketOutcome::Fault(TicketFaultStage::OwnershipDrift))
            );
            assert_eq!(t.flush_state(Some(a)), FlushState::Pending);
            assert_eq!(t.flush_state(None), FlushState::Done);
        }

        #[test]
        fn fault_outcome_carries_the_committed_stage() {
            // A1: the Fault terminal must preserve the stage identity the
            // owner committed, so a flush can diagnose which bounded stage
            // failed instead of an undifferentiated fault.
            let mut t = TicketTracker::new();
            let a = t.alloc().unwrap();
            t.mark_device_owned(a);
            assert_eq!(t.fault_outstanding(TicketFaultStage::Reset), 1);
            assert_eq!(
                t.flush_state(Some(a)),
                FlushState::Lost(TicketOutcome::Fault(TicketFaultStage::Reset))
            );
        }

        #[test]
        fn fault_outstanding_keeps_min_ticket_across_stages() {
            // A1: the bounded loss summary keeps the first (lowest) lost
            // ticket together with the stage the owner committed for it.
            let mut t = TicketTracker::new();
            let a = t.alloc().unwrap();
            let b = t.alloc().unwrap();
            t.mark_device_owned(a);
            t.mark_device_owned(b);
            assert_eq!(t.fault_outstanding(TicketFaultStage::Reinitialize), 2);
            assert_eq!(
                t.flush_state(Some(b)),
                FlushState::Lost(TicketOutcome::Fault(TicketFaultStage::Reinitialize))
            );
        }

        #[test]
        fn ticket_fault_stage_codes_mirror_recover_stage() {
            // The ticket fault stages are the bounded mirror of the D3
            // recover_stage diagnostic codes; both sides must stay in sync.
            use crate::async_rx::recover_stage;
            assert_eq!(
                TicketFaultStage::SubmitWait.code(),
                recover_stage::SUBMIT_WAIT
            );
            assert_eq!(
                TicketFaultStage::CompletionWait.code(),
                recover_stage::COMPLETION_WAIT
            );
            assert_eq!(TicketFaultStage::Reclaim.code(), recover_stage::RECLAIM);
            assert_eq!(TicketFaultStage::Quiesce.code(), recover_stage::QUIESCE);
            assert_eq!(TicketFaultStage::Reset.code(), recover_stage::RESET);
            assert_eq!(
                TicketFaultStage::Reinitialize.code(),
                recover_stage::REINITIALIZE
            );
            assert_eq!(
                TicketFaultStage::OwnershipDrift.code(),
                recover_stage::OWNERSHIP_DRIFT
            );
            assert_eq!(TicketFaultStage::Unknown.code(), recover_stage::UNKNOWN);
        }

        #[test]
        fn terminal_outcomes_are_distinct_and_stable() {
            // Task 2.1 / F6: the four ticket outcomes are distinct and each
            // maps to a stable flush terminal, so no outcome is silently
            // conflated with another when a flush re-checks later.
            assert_ne!(TicketOutcome::Reclaimed, TicketOutcome::CancelledPreSubmit);
            assert_ne!(
                TicketOutcome::CancelledPreSubmit,
                TicketOutcome::ResetAborted
            );
            assert_ne!(
                TicketOutcome::ResetAborted,
                TicketOutcome::Fault(TicketFaultStage::OwnershipDrift)
            );
            assert_ne!(
                TicketOutcome::Fault(TicketFaultStage::OwnershipDrift),
                TicketOutcome::Fault(TicketFaultStage::Reset)
            );
            assert!(matches!(
                FlushState::Lost(TicketOutcome::Reclaimed),
                FlushState::Lost(_)
            ));
        }
    }
}
