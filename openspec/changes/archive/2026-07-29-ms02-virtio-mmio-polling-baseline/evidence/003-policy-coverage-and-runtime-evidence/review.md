# Review: 003-policy-coverage-and-runtime-evidence

- Change: `ms02-virtio-mmio-polling-baseline`
- Iteration: `003-policy-coverage-and-runtime-evidence`
- Reviewer: openspec-act
- Captured at: 2026-07-29T20:30:00+08:00
- Revision: `efcf08124294d523ccab4d3569ea97fe31ed96c1` (worktree modified)

## Diff Surface

| File | Change |
|---|---|
| `crates/axnet/src/service.rs` | +44/-7: extracted `any_masked_device_requires_polling` helper, refactored `register_waker` to call it, added 4 mask×polling eligibility tests |

No out-of-scope files touched. No change to `Device` trait, `ethernet.rs`, `Cargo.toml`, `Makefile`, or `tests/ms02_guest_service.c` beyond iteration 002 baseline.

## Spec Compliance Review

### R3: No-IRQ synchronous progress (T1 Task Contract)

- **保持 4/4 GREEN as refactor witness**: The 4 original deadline tests pass
  unchanged after extraction. Verified at Gate 3 (pre-refactor 4/4) and
  Gate 5 (post-refactor 8/8).
- **helper 输入 mask 与按设备顺序排列的 polling capability**:
  `any_masked_device_requires_polling(mask: u32, polling_capabilities: impl IntoIterator<Item = bool>)`
  takes mask and an iterator of `requires_polling()` results in device order.
- **mask 外 polling device MUST NOT 触发 fallback**:
  `unmasked_polling_device_does_not_trigger_fallback` — mask=0b010,
  capabilities=[true, false] → false. Device 0 requires polling but bit 0
  is not set in mask.
- **mask 内非 polling device MUST NOT 触发 fallback**:
  `masked_non_polling_device_does_not_trigger_fallback` — mask=0b001,
  capabilities=[false] → false. Bit 0 is set but device 0 does not require
  polling.
- **mask 内 polling device MUST 触发 fallback**:
  `masked_polling_device_triggers_fallback` — mask=0b001,
  capabilities=[true] → true. Bit 0 set and device 0 requires polling.
- **mixed devices MUST 只由命中项决定**:
  `mixed_devices_only_masked_polling_decides` — two sub-cases:
  (1) mask=0b101, cap=[true,true,false] → true (device 0 masked+polling);
  (2) mask=0b101, cap=[false,true,false] → false (device 1 polling but
  unmasked, device 0/2 non-polling).
- **`register_waker` 必须复用 helper**: `register_waker` (service.rs:113-116)
  calls `any_masked_device_requires_polling(mask, self.router.devices.iter().map(|d| d.requires_polling()))`
  instead of inlining the `.any()` logic.
- **禁止改变 10ms / timer ownership / Device API**: `POLLING_FALLBACK` unchanged
  at `Duration::from_millis(10)`. `Service.timeout` ownership unchanged. `Device`
  trait unchanged.
- Status: PASS.

### R8: MS02 scope isolation

- No IRQ, PLIC, AtomicWaker, async queue task, or multi-waiter state introduced.
- No change to `Device` trait, `EthernetDevice`, `Cargo.toml`, or `Makefile`.
- Refactor only: `register_waker` behavior is semantically identical to the
  pre-refactor inline logic.
- MS01 socket behavior preserved: MS01 self-test PASS.
- Status: PASS.

## Code Quality Review

### Extracted helper: `any_masked_device_requires_polling`

- Pure function: no state, no side effects, no I/O.
- Signature uses `impl IntoIterator<Item = bool>` — accepts arrays, Vecs,
  iterators. Flexible for testing without real devices.
- Logic is identical to the original inline `.any()` in `register_waker`:
  `mask & (1 << i) != 0 && requires_polling`.
- Short-circuits on first match via `.any()`.
- Doc comment explains the non-obvious mask-bit-to-device-index contract.
- Status: PASS.

### `register_waker` refactor

- Before: inline `.enumerate().any(|(i, device)| mask & (1 << i) != 0 && device.requires_polling())`
- After: `any_masked_device_requires_polling(mask, self.router.devices.iter().map(|d| d.requires_polling()))`
- `.map(|d| d.requires_polling())` creates a lazy iterator; no allocation.
- `.then_some(timestamp + POLLING_FALLBACK)` unchanged.
- Device waker registration loop (lines 137-141) unchanged.
- Old timeout future drop (line 124) unchanged.
- Status: PASS.

### New tests

- Each test covers exactly one mask×polling combination.
- Test names describe the scenario, not the implementation.
- Tests use simple bit patterns (`0b001`, `0b010`, `0b101`) and small arrays.
- `mixed_devices_only_masked_polling_decides` tests both true and false
  outcomes in a single function, covering the decision boundary.
- Tests do not depend on real devices or QEMU.
- Status: PASS.

### Cross-task interaction

- T1 (policy tests) exercises the same `register_waker` path that T2
  (QEMU evidence) will verify at runtime. No conflict.
- T3 (ICMP feature) is unaffected — the helper only touches device
  selection, not protocol processing.
- Status: PASS.

## Findings Summary

| Severity | Finding | Resolution |
|---|---|---|
| Critical | None | N/A |
| Important | None | N/A |
| Minor | None | N/A |

## Regression Risk

- MS01 socket behavior: `register_waker` preserves the original protocol
  deadline path. For loopback-only or IRQ-device sockets, `polling_deadline`
  is `None` (no device requires polling), so `select_wake_deadline` returns
  the protocol deadline unchanged. MS01 self-test PASS confirms.
- smoltcp warnings: 11 pre-existing warnings remain; no new warnings
  introduced by this change (axnet compiles clean).

## Conclusion

Spec compliance: PASS.
Code quality: PASS.
No Critical, Important, or Minor findings.
