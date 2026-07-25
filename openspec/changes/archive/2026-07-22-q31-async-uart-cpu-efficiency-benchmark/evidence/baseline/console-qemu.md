Boot at 2026-07-21 05:22:36.003759400 UTC

[  0.456965 0 axnet_ng:139]   No vsock device found!
[  0.457396 0 axdisplay:26]   No display device found!
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
  backend=polling-console
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
  tx_transmit_policy=blocking
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

=== [S05] Startup Ring ===
  status=SKIPPED reason=no-async-driver

[ 13.605165 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 3
=== [S10] TX Throughput Baseline (write + tcdrain each iteration) ===
  diag=S10 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=35 line_time_ms=542.5 kbps=177.17 line_rate_pct=1537.9
  diag=drain-each-size-64 n=100 avg_ms=0.349 p50_ms=0.379 p95_ms=0.620 p99_ms=0.771 max_ms=0.771 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.14 p99_p50_ratio=2.04 max_p50_ratio=2.04
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=136 line_time_ms=2170.1 kbps=183.12 line_rate_pct=1589.6
  diag=drain-each-size-256 n=100 avg_ms=1.362 p50_ms=1.444 p95_ms=1.917 p99_ms=2.625 max_ms=2.625 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.12 p99_p50_ratio=1.82 max_p50_ratio=1.82
  policy=drain-each size=1024 iters=100 bytes=102400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=591 line_time_ms=8680.6 kbps=169.08 line_rate_pct=1467.7
  diag=drain-each-size-1024 n=100 avg_ms=5.910 p50_ms=5.888 p95_ms=7.497 p99_ms=8.750 max_ms=8.750 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.10 p99_p50_ratio=1.49 max_p50_ratio=1.49

=== [S11] Blocking Transmit (write loop, final drain outside timing) ===
  diag=S11 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
[ 14.394506 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[ 14.435183 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
[ 14.436264 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  policy=no-drain size=64 iters=100 bytes=6400 short_writes=0 enqueue_ms=39 final_drain_ms=1 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=542.5 enqueue_kbps=157.67
  diag=s11-txdbg-reset size=64 ioctl_rc=-1
  diag=s11-txdbg phase=enqueue size=64 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=64 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
[ 14.442918 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[ 14.597321 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
[ 14.598623 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  policy=no-drain size=256 iters=100 bytes=25600 short_writes=0 enqueue_ms=153 final_drain_ms=1 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=2170.1 enqueue_kbps=163.09
  diag=s11-txdbg-reset size=256 ioctl_rc=-1
  diag=s11-txdbg phase=enqueue size=256 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=256 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
[ 14.605536 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[ 15.198794 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
[ 15.199736 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  policy=no-drain size=1024 iters=100 bytes=102400 short_writes=0 enqueue_ms=592 final_drain_ms=0 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=8680.6 enqueue_kbps=168.87
  diag=s11-txdbg-reset size=1024 ioctl_rc=-1
  diag=s11-txdbg phase=enqueue size=1024 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=1024 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0

=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===
  diag=S12 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=batch-drain size=64 iters=100 batch=8 drains=13 bytes=6400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=35 line_time_ms=542.5 kbps=174.38 line_rate_pct=1513.7
  policy=batch-drain size=256 iters=100 batch=8 drains=13 bytes=25600 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=139 line_time_ms=2170.1 kbps=179.03 line_rate_pct=1554.1
  policy=batch-drain size=1024 iters=100 batch=8 drains=13 bytes=102400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=604 line_time_ms=8680.6 kbps=165.47 line_rate_pct=1436.3

=== [S13] TX writev Fragments (fragment aggregation witness) ===
  diag=S13 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=writev-drain-each fragments=4 fragment_size=64 total_size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=154 line_time_ms=2170.1 kbps=161.37 line_rate_pct=1400.8

=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===
  diag=S14 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=40 line_time_ms=542.5 kbps=155.64 line_rate_pct=1351.0
  diag=break-even-size-64 n=100 avg_ms=0.398 p50_ms=0.390 p95_ms=0.702 p99_ms=1.367 max_ms=1.367 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.25 p99_p50_ratio=3.50 max_p50_ratio=3.50
  policy=drain-each size=128 iters=100 bytes=12800 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=78 line_time_ms=1085.1 kbps=159.94 line_rate_pct=1388.3
  diag=break-even-size-128 n=100 avg_ms=0.778 p50_ms=0.790 p95_ms=1.048 p99_ms=1.900 max_ms=1.900 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.18 p99_p50_ratio=2.40 max_p50_ratio=2.40
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=154 line_time_ms=2170.1 kbps=161.91 line_rate_pct=1405.5
  diag=break-even-size-256 n=100 avg_ms=1.540 p50_ms=1.526 p95_ms=2.335 p99_ms=3.695 max_ms=3.695 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.17 p99_p50_ratio=2.42 max_p50_ratio=2.42

=== [S20] TX Latency (single byte + tcdrain) ===
  diag=S20 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  diag=s20-single-byte n=100 avg_ms=0.037 p50_ms=0.037 p95_ms=0.039 p99_ms=0.082 max_ms=0.082 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.97 p99_p50_ratio=2.21 max_p50_ratio=2.21
  diag=S20 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S21] TX Latency FIFO Boundary Matrix ===
  diag=S21 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  diag=s21-fifo-size-1 n=100 avg_ms=0.036 p50_ms=0.036 p95_ms=0.039 p99_ms=0.084 max_ms=0.084 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.99 p99_p50_ratio=2.31 max_p50_ratio=2.31
  diag=fifo-size-1 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-15 n=100 avg_ms=0.100 p50_ms=0.105 p95_ms=0.157 p99_ms=0.485 max_ms=0.485 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.38 p99_p50_ratio=4.61 max_p50_ratio=4.61
  diag=fifo-size-15 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-16 n=100 avg_ms=0.112 p50_ms=0.118 p95_ms=0.161 p99_ms=0.235 max_ms=0.235 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.17 p99_p50_ratio=1.99 max_p50_ratio=1.99
  diag=fifo-size-16 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-17 n=100 avg_ms=0.117 p50_ms=0.120 p95_ms=0.176 p99_ms=0.269 max_ms=0.269 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.19 p99_p50_ratio=2.24 max_p50_ratio=2.24
  diag=fifo-size-17 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-31 n=100 avg_ms=0.200 p50_ms=0.204 p95_ms=0.309 p99_ms=0.374 max_ms=0.374 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.14 p99_p50_ratio=1.83 max_p50_ratio=1.83
  diag=fifo-size-31 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-32 n=100 avg_ms=0.203 p50_ms=0.208 p95_ms=0.317 p99_ms=0.645 max_ms=0.645 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.24 p99_p50_ratio=3.10 max_p50_ratio=3.10
  diag=fifo-size-32 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-33 n=100 avg_ms=0.211 p50_ms=0.220 p95_ms=0.305 p99_ms=0.391 max_ms=0.391 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.14 p99_p50_ratio=1.78 max_p50_ratio=1.78
  diag=fifo-size-33 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-48 n=100 avg_ms=0.302 p50_ms=0.297 p95_ms=0.502 p99_ms=0.610 max_ms=0.610 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.15 p99_p50_ratio=2.05 max_p50_ratio=2.05
  diag=fifo-size-48 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-49 n=100 avg_ms=0.312 p50_ms=0.321 p95_ms=0.480 p99_ms=0.640 max_ms=0.640 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.15 p99_p50_ratio=2.00 max_p50_ratio=2.00
  diag=fifo-size-49 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S30] RX Empty Non-blocking Read (FIONBIO) ===
  diag=S30 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  method=open-o-nonblock status=PASS result=EAGAIN
  method=ioctl-fionbio status=PASS result=EAGAIN

=== [S31] RX Fixed Payload Witness ===
  diag=S31 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  status=SKIPPED reason=BENCH_RX_FIXED_BYTES=0

=== [S40] TX Counter Proxy Summary ===
[ 16.624130 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 3
  status=UNSUPPORTED reason=backend-polling-console-no-telemetry
  proxy=not-available

Done.
starry:/bin# 