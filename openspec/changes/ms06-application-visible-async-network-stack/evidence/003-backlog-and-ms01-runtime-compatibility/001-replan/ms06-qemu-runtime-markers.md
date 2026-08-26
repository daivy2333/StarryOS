# QEMU Runtime Markers — Cycle 001-replan（T2.8-R1）

- Captured at: 2026-08-26
- Revision: `4396d264` (net-k3) + Cycle 001-replan 工作树实现
- Kernel: `StarryOS_riscv64-qemu-virt.bin`（2026-08-26 12:16 构建）
- Runner: 用户手工输入，guest shell 串口输出原样摘录（无自动化驱动）
- Verdict: diagnostic single PASS / fork PASS / MS01 PASS（14/14, exit 0）

## 1. Diagnostic single

```text
starry:~# /tmp/ms01_diag single
MS01_LOOPBACK_DIAGNOSTIC_START single
PHASE: single-listen
PHASE: single-connect
PHASE: single-connect-poll
PHASE: single-accept-poll
PHASE: single-accept
PHASE: single-send
PHASE: single-recv
PASS: single-loopback
MS01_LOOPBACK_DIAGNOSTIC_END single
```

## 2. Diagnostic fork

```text
starry:~# /tmp/ms01_diag fork
MS01_LOOPBACK_DIAGNOSTIC_START fork
PHASE: fork-listen
PHASE: fork-child-spawn
PHASE: fork-parent-accept-poll
PHASE: fork-child-connect
PHASE: fork-child-send
PHASE: fork-child-done
PHASE: fork-parent-accept
PHASE: fork-parent-recv
PASS: fork-loopback
MS01_LOOPBACK_DIAGNOSTIC_END fork
```

## 3. MS01 socket baseline（含显式进程退出码）

```text
starry:~# /tmp/ms01_test; echo MS01_EXIT:$?
MS01_SOCKET_BASELINE_START
PHASE: tcp-accept parent-accept-poll
PHASE: tcp-accept child-connect
PHASE: tcp-accept child-send
PHASE: tcp-accept child-done
PHASE: tcp-accept parent-accept
PHASE: tcp-accept parent-recv
PASS: tcp-accept
PASS: tcp-adjacent
PHASE: tcp-512cap listen
PHASE: tcp-512cap connect
PHASE: tcp-512cap accept-refill
PHASE: tcp-512-recovery connect
PHASE: tcp-512cap drain
PASS: tcp-512cap: accepted 512 of 512 initial connections
PASS: tcp-512-recovery
PASS: tcp-relisten
PASS: udp-bidi
PASS: tcp-nonblock-accept
PASS: udp-nonblock
PASS: poll-readiness
PASS: udp-source: 127.0.0.1:49153
PASS: bind-getsockname: port 18012
PASS: bind-ephemeral: port 49675
PASS: bind-conflict: EADDRINUSE
PASS: bind-close-cleanup
MS01_SOCKET_BASELINE_END
MS01_EXIT:0
```

## 判定摘要

| 检查 | 观测 | 要求 | 结论 |
|---|---|---|---|
| diagnostic single | `PASS: single-loopback` + START/END | PASS + exit 0 | PASS |
| diagnostic fork | `PASS: fork-loopback` + START/END | PASS + exit 0 | PASS |
| MS01 marker 集合 | 1×START、14 unique PASS、0×FAIL、1×END | 同左 | PASS |
| MS01 显式退出码 | `MS01_EXIT:0` | 0 | PASS |
| 本 Cycle 缺陷关闭 | `tcp-adjacent` 双相邻客户端建立 | 第二 SYN 不被同批 RST | PASS |

适用限制：单 hart QEMU VirtIO-MMIO 软件设备模型；不覆盖 SMP、真板、DMA/cache 与性能结论。
