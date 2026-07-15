Boot at 2026-07-15 11:04:19.004605 UTC

[  0.494670 0 axnet_ng:139]   No vsock device found!
[  0.495545 0 axdisplay:26]   No display device found!
[UART INIT] ✅ iomap OK: UART MMIO at VA:0xffffffc010000000
[UART INIT] Trying raw read at base+5 (stride 1, LSR)...
[UART INIT] ✅ Raw LSR read: 0x60
[UART INIT] Trying uart_16550 crate access...
[UART INIT] FCR: FIFO enabled=true, trigger level via ISR bits 7-6
[UART INIT] async UART ready
[kernel] Async UART driver initialized
[BENCH] Running startup benchmark...
[BENCH] Ring buffer write: 275150.47 KB/s (65536 bytes in 0.23 ms)
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
[BENCH] RX ring buffer read: 1045751.63 KB/s (65536 bytes in 0.06 ms)
[BENCH] Running RX ring buffer latency test...
[BENCH] RX latency (n=100): min=100ns avg=316ns P50=200ns P95=200ns P99=14300ns
[BENCH] Startup benchmark complete
[BENCH] Note: actual throughput limited by UART line rate (11.52 KB/s)
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
  diag=S10 pre_section_stdout_drain_ms=3 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=38 line_time_ms=542.5 kbps=164.10 line_rate_pct=1424.5
  diag=drain-each-size-64 n=100 avg_ms=0.375 p50_ms=0.384 p95_ms=0.501 p99_ms=0.773 max_ms=0.773 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.14 p99_p50_ratio=2.01 max_p50_ratio=2.01
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=123 line_time_ms=2170.1 kbps=202.58 line_rate_pct=1758.5
  diag=drain-each-size-256 n=100 avg_ms=1.228 p50_ms=1.238 p95_ms=1.899 p99_ms=2.777 max_ms=2.777 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.13 p99_p50_ratio=2.24 max_p50_ratio=2.24
  policy=drain-each size=1024 iters=100 bytes=102400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=448 line_time_ms=8680.6 kbps=222.86 line_rate_pct=1934.6
  diag=drain-each-size-1024 n=100 avg_ms=4.478 p50_ms=4.488 p95_ms=5.722 p99_ms=6.736 max_ms=6.736 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.08 p99_p50_ratio=1.50 max_p50_ratio=1.50

=== [S11] TX Enqueue Cost (write loop, final drain outside timing) ===
  diag=S11 pre_section_stdout_drain_ms=2 drain_errors=0 last_errno=0
  policy=no-drain size=64 iters=100 bytes=6400 short_writes=0 enqueue_ms=0 final_drain_ms=29 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=542.5 enqueue_kbps=7736.11
  diag=s11-txdbg-reset size=64 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=64 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=64 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  policy=no-drain size=256 iters=100 bytes=25600 short_writes=0 enqueue_ms=1 final_drain_ms=115 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=2170.1 enqueue_kbps=24960.06
  diag=s11-txdbg-reset size=256 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=256 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=256 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  policy=no-drain size=1024 iters=100 bytes=102400 short_writes=0 enqueue_ms=200 final_drain_ms=265 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=8680.6 enqueue_kbps=498.59
  diag=s11-txdbg-reset size=1024 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=1024 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=1 staged_bytes=144 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=1024 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1

=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===
  diag=S12 pre_section_stdout_drain_ms=3 drain_errors=0 last_errno=0
  policy=batch-drain size=64 iters=100 batch=8 drains=13 bytes=6400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=30 line_time_ms=542.5 kbps=208.16 line_rate_pct=1806.9
  policy=batch-drain size=256 iters=100 batch=8 drains=13 bytes=25600 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=112 line_time_ms=2170.1 kbps=222.04 line_rate_pct=1927.4
  policy=batch-drain size=1024 iters=100 batch=8 drains=13 bytes=102400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=462 line_time_ms=8680.6 kbps=216.10 line_rate_pct=1875.9

=== [S13] TX writev Fragments (fragment aggregation witness) ===
  diag=S13 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  policy=writev-drain-each fragments=4 fragment_size=64 total_size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=132 line_time_ms=2170.1 kbps=188.61 line_rate_pct=1637.2

=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===
  diag=S14 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=45 line_time_ms=542.5 kbps=135.92 line_rate_pct=1179.8
  diag=break-even-size-64 n=100 avg_ms=0.452 p50_ms=0.441 p95_ms=0.699 p99_ms=1.214 max_ms=1.214 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.22 p99_p50_ratio=2.75 max_p50_ratio=2.75
  policy=drain-each size=128 iters=100 bytes=12800 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=69 line_time_ms=1085.1 kbps=179.02 line_rate_pct=1554.0
  diag=break-even-size-128 n=100 avg_ms=0.692 p50_ms=0.699 p95_ms=1.026 p99_ms=2.087 max_ms=2.087 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.19 p99_p50_ratio=2.98 max_p50_ratio=2.98
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=132 line_time_ms=2170.1 kbps=188.24 line_rate_pct=1634.0
  diag=break-even-size-256 n=100 avg_ms=1.320 p50_ms=1.357 p95_ms=1.871 p99_ms=3.231 max_ms=3.231 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.15 p99_p50_ratio=2.38 max_p50_ratio=2.38

=== [S20] TX Latency (single byte + tcdrain) ===
  diag=S20 pre_section_stdout_drain_ms=2 drain_errors=0 last_errno=0
  diag=s20-single-byte n=100 avg_ms=0.157 p50_ms=0.151 p95_ms=0.189 p99_ms=0.267 max_ms=0.267 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=3.15 p99_p50_ratio=1.77 max_p50_ratio=1.77
  diag=S20 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S21] TX Latency FIFO Boundary Matrix ===
  diag=S21 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  diag=s21-fifo-size-1 n=100 avg_ms=0.157 p50_ms=0.150 p95_ms=0.220 p99_ms=0.365 max_ms=0.365 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=4.31 p99_p50_ratio=2.44 max_p50_ratio=2.44
  diag=fifo-size-1 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-15 n=100 avg_ms=0.203 p50_ms=0.197 p95_ms=0.243 p99_ms=0.617 max_ms=0.617 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.49 p99_p50_ratio=3.13 max_p50_ratio=3.13
  diag=fifo-size-15 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-16 n=100 avg_ms=0.211 p50_ms=0.193 p95_ms=0.284 p99_ms=1.403 max_ms=1.403 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.03 p99_p50_ratio=7.26 max_p50_ratio=7.26
  diag=fifo-size-16 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-17 n=100 avg_ms=0.217 p50_ms=0.199 p95_ms=0.272 p99_ms=1.622 max_ms=1.622 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.13 p99_p50_ratio=8.17 max_p50_ratio=8.17
  diag=fifo-size-17 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-31 n=100 avg_ms=0.297 p50_ms=0.287 p95_ms=0.419 p99_ms=0.676 max_ms=0.676 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.26 p99_p50_ratio=2.36 max_p50_ratio=2.36
  diag=fifo-size-31 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-32 n=100 avg_ms=0.261 p50_ms=0.255 p95_ms=0.343 p99_ms=0.655 max_ms=0.655 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.24 p99_p50_ratio=2.57 max_p50_ratio=2.57
  diag=fifo-size-32 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-33 n=100 avg_ms=0.290 p50_ms=0.277 p95_ms=0.377 p99_ms=1.452 max_ms=1.452 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.52 p99_p50_ratio=5.24 max_p50_ratio=5.24
  diag=fifo-size-33 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-48 n=100 avg_ms=0.380 p50_ms=0.369 p95_ms=0.576 p99_ms=1.175 max_ms=1.175 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.29 p99_p50_ratio=3.19 max_p50_ratio=3.19
  diag=fifo-size-48 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-49 n=100 avg_ms=0.414 p50_ms=0.405 p95_ms=0.582 p99_ms=1.632 max_ms=1.632 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.39 p99_p50_ratio=4.03 max_p50_ratio=4.03
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