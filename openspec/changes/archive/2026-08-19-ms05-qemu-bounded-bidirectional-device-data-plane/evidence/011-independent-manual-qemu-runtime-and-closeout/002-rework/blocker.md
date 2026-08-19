# BLOCKED — MS05 Iteration 011 / Cycle 002 — descriptor-full and host-stimulus

- Change: ms05-qemu-bounded-bidirectional-device-data-plane
- Iteration: 011-independent-manual-qemu-runtime-and-closeout-review
- Cycle: 002-rework
- Origin: act-added (user manual QEMU return, Gate 6)
- Artifact: `qemu-ms05-serial.log` in this Cycle Evidence root
- Revision: 2af394e6 (net-k3); artifacts per `artifacts.sha256` (6/6 OK)

## Symptom summary

Manual QEMU MS05 session (`qemu-ms05-serial.log`): PASS only for `snapshot`
and `flush`. All four network modes fail:

| Mode | Marker | Observation |
|---|---|---|
| tx-only | `MS05 FAIL reason=handshake` + `host_received=0` | guest PRE already shows a fresh `last_accepted` rollover; host did not complete handshake |
| bidirectional | `MS05 FAIL` `host_received=0` | guest rx_received=96, host_received=0 |
| slot-full | `MS05 FAIL` `host_received=0` | hold=1 active, but full condition never reached |
| descriptor-full (×2) | `MS05 FAIL reason=full-deadline` | hold=2 active; full condition never reached |
| flush | `MS05 PASS` `host_received=96` | only network mode to pass |

## Probe diagnostic root cause (descriptor-full full-deadline)

`run_held_mode("descriptor-full", MS05_CTL_HOLD_RECLAIM)` requires
`ms05_descriptor_full_proved()`:

```c
full->tx_buffer_available == 0 &&
full->tx_buffer_inflight == MS05_QS &&      // 64
full->tx_descriptor_available == 0 &&
full->tx_descriptor_inflight == MS05_QS &&  // 64
full->tx_again > held->tx_again;
```

Every PRE/HELD snapshot shows `buf_avail=64 buf_inflight=0 desc_avail=64
desc_inflight=0 tx_again=0` and `tx_submit == tx_comp == tx_reclaim`
(ledger closing immediately). The TX path therefore never accumulates 64
in-flight buffers/descriptors and `tx_again` stays 0, so the FULL predicate can
never become true within `MS05_FULL_DEADLINE_MS` (1200ms) → `full-deadline`.

Same pattern for slot-full: `ms05_slot_full_proved` (slot occupancy/backpressure)
also never reached, `host_received=0`.

## Secondary observation (R44 host timing)

`host_received=0` on every host-assisted mode except flush; tx-only failed at
`handshake`. Consistent with the user's report that the host stimulus process
exited/interrupted before the guest connected (the R44 manual exchange window /
host `EXCHANGE_TIMEOUT` / grace window was too short for manual QEMU input).

## Classification

- descriptor-full `full-deadline` is a probe/product test-predicate failure
  (the TX backpressure condition is unreachable under this workload/QS), NOT an
  environment block. Tx/rx data path itself works (flush + bidirectional
  guest-rx pass).
- Host stimulus timing is an R44 manual-orchestration problem; also to be
  addressed next round.

## State / recovery

- Cycle 002 Act Response status: `reported` -> `blocked`.
- Activities completed before the blocker: Gates 1-5 (schema-v2 identity,
  automatic 44/44 qualification, six-artifact freeze, exact four-session
  handoff + static audit); manual qemu-ms05 serial partially returned.
- Required manual files still outstanding: wget/ms04/network session raw files
  were not all returned; this round is blocked on descriptor-full.
- Recovery: return to `openspec-plan` for the next round to (a) fix the
  descriptor-full / slot-full FULL-predicate reachability (QS vs in-flight
  accumulation, HOLD semantics, `tx_again` backpressure trigger) and/or adjust
  the deadline/test strategy, and (b) lengthen the host-stimulus exchange /
  R44 manual input window. Re-verify after a fresh automatic qualification and
  artifact freeze.
