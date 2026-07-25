Boot at 2026-07-20 09:43:05.004194800 UTC

[  0.512040 0 axnet_ng:139]   No vsock device found!
[  0.512776 0 axdisplay:26]   No display device found!
[UART INIT] ✅ iomap OK: UART MMIO at VA:0xffffffc010000000
[UART INIT] Trying raw read at base+5 (stride 1, LSR)...
[UART INIT] ✅ Raw LSR read: 0x60
[UART INIT] Trying uart_16550 crate access...
[UART INIT] FCR: FIFO enabled=true, trigger level via ISR bits 7-6
[UART INIT] async UART hardware initialized (copiers not started yet)
[kernel] Async UART driver initialized
[BENCH] Running startup benchmark...
[BENCH] Ring buffer write: 321446.51 KB/s (65536 bytes in 0.20 ms)
[BENCH] Hardware line rate: 11.52 KB/s (115200 bps)
[BENCH] FIFO depth: 16 bytes
[BENCH] Ring buffer: 64 KB × 2 = 128 KB total
[BENCH] Driver struct: 168 bytes
[BENCH] Total memory: 128 KB
[BENCH] NAPI threshold: 16 consecutive reads
[BENCH] NAPI batch size: 64 bytes
[BENCH] Copier buffer size: 1024 bytes
[BENCH] IRQ count: 0
[BENCH] Running RX ring buffer throughput test...
[BENCH] RX ring buffer read: 1019108.28 KB/s (65536 bytes in 0.06 ms)
[BENCH] Running RX ring buffer latency test...
[BENCH] RX latency (n=100): min=100ns avg=250ns P50=100ns P95=200ns P99=11600ns
[BENCH] Startup benchmark complete
[BENCH] Note: actual throughput limited by UART line rate (11.52 KB/s)
[UART INIT] async UART copiers started
Welcome to Starry OS!
SHLVL=1
HOME=/root
PWD=/

Use apk to install packages.

starry:~# cd /bin
starry:/bin# ./benchmark
UART Async Benchmark
====================

=== [S00] Benchmark Manifest ===
  version=q19c-m0-20260703
  target_mode=qemu-rootfs
  startup_chain=/bin/sh -c init.sh -> /bin/benchmark
  root_provider=qemu-virtio-ext4-rootfs
  device=/dev/console
  timer_source=CLOCK_MONOTONIC
  uart_line_rate=11.52 KB/s
  tx_throughput_sizes=64,256,1024
  tx_break_even_sizes=64,128,256
  tx_throughput_iters=100
  tx_baseline_drain_policy=tcdrain-after-each-write
  tx_enqueue_policy=no-drain-during-measure-final-tcdrain-after
  tx_batch_drain_every=8
  tx_writev_fragments=4
  tx_writev_fragment_size=64
  tx_latency_size=1
  tx_latency_iters=100
  fifo_matrix_sizes=1,15,16,17,31,32,33,48,49
  fifo_matrix_iters=100
  rx_mode=empty-nonblocking-eagain
  rx_fixed_bytes=0
  rx_fixed_timeout_ms=5000

=== [S10] TX Throughput Baseline (write + tcdrain each iteration) ===
  diag=S10 pre_section_stdout_drain_ms=4 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=41 line_time_ms=542.5 kbps=151.54 line_rate_pct=1315.5
  diag=drain-each-size-64 n=100 avg_ms=0.406 p50_ms=0.399 p95_ms=0.579 p99_ms=0.816 max_ms=0.816 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.15 p99_p50_ratio=2.05 max_p50_ratio=2.05
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=140 line_time_ms=2170.1 kbps=177.84 line_rate_pct=1543.8
  diag=drain-each-size-256 n=100 avg_ms=1.396 p50_ms=1.434 p95_ms=2.170 p99_ms=2.598 max_ms=2.598 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.12 p99_p50_ratio=1.81 max_p50_ratio=1.81
  policy=drain-each size=1024 iters=100 bytes=102400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=550 line_time_ms=8680.6 kbps=181.59 line_rate_pct=1576.3
  diag=drain-each-size-1024 n=100 avg_ms=5.495 p50_ms=5.459 p95_ms=7.271 p99_ms=8.408 max_ms=8.408 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.10 p99_p50_ratio=1.54 max_p50_ratio=1.54

=== [S11] TX Enqueue Cost (write loop, final drain outside timing) ===
  diag=S11 pre_section_stdout_drain_ms=3 drain_errors=0 last_errno=0
  policy=no-drain size=64 iters=100 bytes=6400 short_writes=0 enqueue_ms=1 final_drain_ms=32 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=542.5 enqueue_kbps=5223.13
  diag=s11-txdbg-reset size=64 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=64 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=64 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  policy=no-drain size=256 iters=100 bytes=25600 short_writes=0 enqueue_ms=1 final_drain_ms=125 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=2170.1 enqueue_kbps=23516.13
  diag=s11-txdbg-reset size=256 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=256 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=256 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  policy=no-drain size=1024 iters=100 bytes=102400 short_writes=0 enqueue_ms=197 final_drain_ms=316 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=8680.6 enqueue_kbps=506.81
  diag=s11-txdbg-reset size=1024 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=1024 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=1 staged_bytes=224 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=1024 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1

=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===
  diag=S12 pre_section_stdout_drain_ms=4 drain_errors=0 last_errno=0
  policy=batch-drain size=64 iters=100 batch=8 drains=13 bytes=6400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=36 line_time_ms=542.5 kbps=170.50 line_rate_pct=1480.0
  policy=batch-drain size=256 iters=100 batch=8 drains=13 bytes=25600 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=134 line_time_ms=2170.1 kbps=185.36 line_rate_pct=1609.1
  policy=batch-drain size=1024 iters=100 batch=8 drains=13 bytes=102400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=522 line_time_ms=8680.6 kbps=191.50 line_rate_pct=1662.3

=== [S13] TX writev Fragments (fragment aggregation witness) ===
  diag=S13 pre_section_stdout_drain_ms=3 drain_errors=0 last_errno=0
  policy=writev-drain-each fragments=4 fragment_size=64 total_size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=148 line_time_ms=2170.1 kbps=167.85 line_rate_pct=1457.0

=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===
  diag=S14 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=50 line_time_ms=542.5 kbps=124.22 line_rate_pct=1078.3
  diag=break-even-size-64 n=100 avg_ms=0.493 p50_ms=0.492 p95_ms=0.691 p99_ms=1.320 max_ms=1.320 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.24 p99_p50_ratio=2.69 max_p50_ratio=2.69
  policy=drain-each size=128 iters=100 bytes=12800 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=87 line_time_ms=1085.1 kbps=143.03 line_rate_pct=1241.6
  diag=break-even-size-128 n=100 avg_ms=0.865 p50_ms=0.847 p95_ms=1.400 p99_ms=2.495 max_ms=2.495 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.23 p99_p50_ratio=2.95 max_p50_ratio=2.95
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=140 line_time_ms=2170.1 kbps=177.31 line_rate_pct=1539.1
  diag=break-even-size-256 n=100 avg_ms=1.399 p50_ms=1.419 p95_ms=2.084 p99_ms=3.410 max_ms=3.410 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.16 p99_p50_ratio=2.40 max_p50_ratio=2.40

=== [S20] TX Latency (single byte + tcdrain) ===
  diag=S20 pre_section_stdout_drain_ms=2 drain_errors=0 last_errno=0
  diag=s20-single-byte n=100 avg_ms=0.176 p50_ms=0.171 p95_ms=0.213 p99_ms=0.278 max_ms=0.278 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=3.28 p99_p50_ratio=1.62 max_p50_ratio=1.62
  diag=S20 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S21] TX Latency FIFO Boundary Matrix ===
  diag=S21 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  diag=s21-fifo-size-1 n=100 avg_ms=0.194 p50_ms=0.186 p95_ms=0.228 p99_ms=0.486 max_ms=0.486 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=5.73 p99_p50_ratio=2.62 max_p50_ratio=2.62
  diag=fifo-size-1 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-15 n=100 avg_ms=0.248 p50_ms=0.226 p95_ms=0.297 p99_ms=1.731 max_ms=1.731 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.36 p99_p50_ratio=7.65 max_p50_ratio=7.65
  diag=fifo-size-15 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-16 n=100 avg_ms=0.223 p50_ms=0.201 p95_ms=0.274 p99_ms=1.515 max_ms=1.515 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.12 p99_p50_ratio=7.54 max_p50_ratio=7.54
  diag=fifo-size-16 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-17 n=100 avg_ms=0.252 p50_ms=0.237 p95_ms=0.322 p99_ms=1.340 max_ms=1.340 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.93 p99_p50_ratio=5.66 max_p50_ratio=5.66
  diag=fifo-size-17 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-31 n=100 avg_ms=0.318 p50_ms=0.305 p95_ms=0.427 p99_ms=1.539 max_ms=1.539 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.59 p99_p50_ratio=5.04 max_p50_ratio=5.04
  diag=fifo-size-31 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-32 n=100 avg_ms=0.341 p50_ms=0.323 p95_ms=0.506 p99_ms=1.589 max_ms=1.589 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.59 p99_p50_ratio=4.93 max_p50_ratio=4.93
  diag=fifo-size-32 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-33 n=100 avg_ms=0.316 p50_ms=0.303 p95_ms=0.459 p99_ms=1.530 max_ms=1.530 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.55 p99_p50_ratio=5.04 max_p50_ratio=5.04
  diag=fifo-size-33 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-48 n=100 avg_ms=0.376 p50_ms=0.365 p95_ms=0.531 p99_ms=1.162 max_ms=1.162 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.29 p99_p50_ratio=3.18 max_p50_ratio=3.18
  diag=fifo-size-48 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-49 n=100 avg_ms=0.425 p50_ms=0.412 p95_ms=0.598 p99_ms=1.644 max_ms=1.644 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.40 p99_p50_ratio=3.98 max_p50_ratio=3.98
  diag=fifo-size-49 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S30] RX Empty Non-blocking Read (FIONBIO) ===
  diag=S30 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  method=open-o-nonblock status=PASS result=EAGAIN
  method=ioctl-fionbio status=PASS result=EAGAIN

=== [S31] RX Fixed Payload Witness ===
  diag=S31 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  status=SKIPPED reason=BENCH_RX_FIXED_BYTES=0

=== [S40] TX Counter Proxy Summary ===
  telemetry_available=0 ioctl_rc=0
  counter=user-push user_calls=0 user_req=0 user_acc=0
  counter=ring-pop ring_pop_calls=0 ring_pop_bytes=0
  counter=hw-send hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0
  counter=no-progress no_progress_budget=0 slow_poll_exh=0 yield_exh=0
  counter=drain-state ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  proxy=derived status=not-available reason=telemetry-counters-are-zero

Done.
starry:/bin# 