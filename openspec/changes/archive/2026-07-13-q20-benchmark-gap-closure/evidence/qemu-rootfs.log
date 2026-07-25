qemu
Boot at 2026-07-12 12:09:50.004429200 UTC

[  0.480416 0 axnet_ng:139]   No vsock device found!
[  0.480997 0 axdisplay:26]   No display device found!
[UART INIT] ✅ iomap OK: UART MMIO at VA:0xffffffc010000000
[UART INIT] Trying raw read at base+5 (stride 1, LSR)...
[UART INIT] ✅ Raw LSR read: 0x60
[UART INIT] Trying uart_16550 crate access...
[UART INIT] FCR: FIFO enabled=true, trigger level via ISR bits 7-6
[UART INIT] async UART ready
[kernel] Async UART driver initialized
[BENCH] Running startup benchmark...
[BENCH] Ring buffer write: 403388.46 KB/s (102400 bytes in 0.25 ms)
[BENCH] Hardware line rate: 11.52 KB/s (115200 bps)
[BENCH] FIFO depth: 16 bytes
[BENCH] Ring buffer: 64 KB × 2 = 128 KB total
[BENCH] Driver struct: 152 bytes
[BENCH] Total memory: 128 KB
[BENCH] NAPI threshold: 16 consecutive reads
[BENCH] NAPI batch size: 64 bytes
[BENCH] Copier buffer size: 1024 bytes
[BENCH] IRQ count: 0
[BENCH] Running RX ring buffer throughput test...
[BENCH] RX ring buffer read: 1052631.58 KB/s (65536 bytes in 0.06 ms)
[BENCH] Running RX ring buffer latency test...
[BENCH] RX latency (n=100): min=100ns avg=272ns P50=200ns P95=200ns P99=11800ns
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
  diag=S10 pre_section_stdout_drain_ms=5 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=44 line_time_ms=542.5 kbps=141.04 line_rate_pct=1224.3
  diag=drain-each-size-64 n=100 avg_ms=0.437 p50_ms=0.438 p95_ms=0.632 p99_ms=0.965 max_ms=0.965 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.18 p99_p50_ratio=2.20 max_p50_ratio=2.20
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=140 line_time_ms=2170.1 kbps=177.79 line_rate_pct=1543.3
  diag=drain-each-size-256 n=100 avg_ms=1.400 p50_ms=1.388 p95_ms=2.389 p99_ms=3.064 max_ms=3.064 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.14 p99_p50_ratio=2.21 max_p50_ratio=2.21
  policy=drain-each size=1024 iters=100 bytes=102400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=571 line_time_ms=8680.6 kbps=175.03 line_rate_pct=1519.4
  diag=drain-each-size-1024 n=100 avg_ms=5.704 p50_ms=5.753 p95_ms=7.272 p99_ms=8.958 max_ms=8.958 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.10 p99_p50_ratio=1.56 max_p50_ratio=1.56

=== [S11] TX Enqueue Cost (write loop, final drain outside timing) ===
  diag=S11 pre_section_stdout_drain_ms=2 drain_errors=0 last_errno=0
  policy=no-drain size=64 iters=100 bytes=6400 short_writes=0 enqueue_ms=1 final_drain_ms=32 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=542.5 enqueue_kbps=6039.23
  diag=s11-txdbg-reset size=64 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=64 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=64 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  policy=no-drain size=256 iters=100 bytes=25600 short_writes=0 enqueue_ms=1 final_drain_ms=137 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=2170.1 enqueue_kbps=14034.69
  diag=s11-txdbg-reset size=256 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=256 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=256 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  policy=no-drain size=1024 iters=100 bytes=65536 short_writes=36 enqueue_ms=3 final_drain_ms=350 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=5555.6 enqueue_kbps=18170.98
  diag=s11-txdbg-reset size=1024 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=1024 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=1024 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1

=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===
  diag=S12 pre_section_stdout_drain_ms=6 drain_errors=0 last_errno=0
  policy=batch-drain size=64 iters=100 batch=8 drains=13 bytes=6400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=37 line_time_ms=542.5 kbps=165.66 line_rate_pct=1438.0
  policy=batch-drain size=256 iters=100 batch=8 drains=13 bytes=25600 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=137 line_time_ms=2170.1 kbps=181.94 line_rate_pct=1579.4
  policy=batch-drain size=1024 iters=100 batch=8 drains=13 bytes=102400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=563 line_time_ms=8680.6 kbps=177.43 line_rate_pct=1540.2

=== [S13] TX writev Fragments (fragment aggregation witness) ===
  diag=S13 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  policy=writev-drain-each fragments=4 fragment_size=64 total_size=256 iters=100 bytes=25554 short_writes=1 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=159 line_time_ms=2166.2 kbps=156.50 line_rate_pct=1358.5

=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===
  diag=S14 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=50 line_time_ms=542.5 kbps=124.24 line_rate_pct=1078.4
  diag=break-even-size-64 n=100 avg_ms=0.494 p50_ms=0.501 p95_ms=0.654 p99_ms=0.744 max_ms=0.744 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.14 p99_p50_ratio=1.49 max_p50_ratio=1.49
  policy=drain-each size=128 iters=100 bytes=12800 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=91 line_time_ms=1085.1 kbps=136.58 line_rate_pct=1185.6
  diag=break-even-size-128 n=100 avg_ms=0.908 p50_ms=0.876 p95_ms=1.246 p99_ms=3.031 max_ms=3.031 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.28 p99_p50_ratio=3.46 max_p50_ratio=3.46
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=155 line_time_ms=2170.1 kbps=160.96 line_rate_pct=1397.2
  diag=break-even-size-256 n=100 avg_ms=1.545 p50_ms=1.580 p95_ms=2.186 p99_ms=2.631 max_ms=2.631 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.12 p99_p50_ratio=1.66 max_p50_ratio=1.66

=== [S20] TX Latency (single byte + tcdrain) ===
  diag=S20 pre_section_stdout_drain_ms=4 drain_errors=0 last_errno=0
  diag=s20-single-byte n=100 avg_ms=0.168 p50_ms=0.162 p95_ms=0.227 p99_ms=0.301 max_ms=0.301 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=3.56 p99_p50_ratio=1.86 max_p50_ratio=1.86
  diag=S20 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S21] TX Latency FIFO Boundary Matrix ===
  diag=S21 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  diag=s21-fifo-size-1 n=100 avg_ms=0.173 p50_ms=0.166 p95_ms=0.212 p99_ms=0.353 max_ms=0.353 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=4.16 p99_p50_ratio=2.13 max_p50_ratio=2.13
  diag=fifo-size-1 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-15 n=100 avg_ms=0.221 p50_ms=0.207 p95_ms=0.259 p99_ms=1.484 max_ms=1.484 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.17 p99_p50_ratio=7.18 max_p50_ratio=7.18
  diag=fifo-size-15 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-16 n=100 avg_ms=0.241 p50_ms=0.231 p95_ms=0.297 p99_ms=1.515 max_ms=1.515 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.12 p99_p50_ratio=6.57 max_p50_ratio=6.57
  diag=fifo-size-16 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-17 n=100 avg_ms=0.254 p50_ms=0.225 p95_ms=0.320 p99_ms=2.290 max_ms=2.290 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.59 p99_p50_ratio=10.16 max_p50_ratio=10.16
  diag=fifo-size-17 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-31 n=100 avg_ms=0.347 p50_ms=0.333 p95_ms=0.464 p99_ms=2.129 max_ms=2.129 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.81 p99_p50_ratio=6.39 max_p50_ratio=6.39
  diag=fifo-size-31 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-32 n=100 avg_ms=0.333 p50_ms=0.314 p95_ms=0.491 p99_ms=1.515 max_ms=1.515 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.56 p99_p50_ratio=4.83 max_p50_ratio=4.83
  diag=fifo-size-32 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-33 n=99 avg_ms=0.315 p50_ms=0.306 p95_ms=0.460 p99_ms=0.969 max_ms=0.969 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.35 p99_p50_ratio=3.17 max_p50_ratio=3.17
  diag=fifo-size-33 drain_calls=99 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-48 n=100 avg_ms=0.413 p50_ms=0.391 p95_ms=0.583 p99_ms=1.760 max_ms=1.760 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.43 p99_p50_ratio=4.50 max_p50_ratio=4.50
  diag=fifo-size-48 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-49 n=100 avg_ms=0.417 p50_ms=0.387 p95_ms=0.609 p99_ms=1.564 max_ms=1.564 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.38 p99_p50_ratio=4.05 max_p50_ratio=4.05
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