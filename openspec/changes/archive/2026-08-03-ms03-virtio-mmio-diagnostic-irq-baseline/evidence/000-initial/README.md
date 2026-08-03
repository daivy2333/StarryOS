# MS03 Iteration 000 — Evidence Index

- Date: 2026-08-03
- Branch: net-k3
- Hardware: QEMU virt (riscv64), single-hart, VirtIO-MMIO net

## Gate Results

| Gate | Verification | Status |
|------|-------------|--------|
| G3.1 | `make LOG=info build` exit 0 | PASS |
| G3.2 | Boot: UART IRQ 10 device handler registered | PASS |
| G3.3 | Boot: VirtIO-MMIO net validated (magic/version/device_id=1) at 0x10007000 | PASS |
| G3.4 | Boot: Diagnostic IRQ 7 handler registered | PASS |
| G3.5 | idle probe (2000ms): total delta=0 | PASS |
| G3.6 | uart probe: net used_ring delta=0 | PASS |
| G3.7 | rx2 probe: used_delta=3, ack_delta=3 | PASS |
| G3.8 | tx2 probe: used_delta=2, ack_delta=2 | PASS |
| G3.9 | both probe: uart_irq+net concurrent | PASS |
| G3.10 | rx2 repeat: used_delta=2, ack_delta=2 (repeat delivery) | PASS |
| G5.1 | MS02 TCP/UDP 5555 regression | PASS |
| G5.2 | MS01 14/14 socket baseline regression | PASS |

All 12 gates PASS. No failures, no IRQ storm, no spurious net events,
UART and net isolated, repeat delivery proven.

## Evidence Files

| File | Content |
|------|---------|
| `build.log` | `make LOG=info build` full output |
| `regression-ms02.log` | MS02 TCP 5555 + UDP 5555 results |
| `ms01-regression.log` | MS01 14/14 socket baseline: PASS, 0 FAIL |
