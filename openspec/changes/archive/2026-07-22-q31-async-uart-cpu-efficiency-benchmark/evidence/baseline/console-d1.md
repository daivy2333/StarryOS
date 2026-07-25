Boot at 1970-01-01 00:00:00.431224265 UTC

[starry-d1] Lichee D1 fullbench command-entry mode
[starry-d1] log_label=lichee-memory-root-command
[starry-d1] target_mode=lichee-d1-fullbench-command
[starry-d1] startup_chain=android-boot-image -> memory-root /bin/benchmark -> eager_elf_mapping (equivalent_command_entry)
[starry-d1] root_provider=d1-memory-root-path
[starry-d1] shell_status=SKIPPED: no known-good static /bin/sh
[starry-d1] equivalent_entry=/bin/benchmark
[starry-d1] Initializing populated memory rootfs...
[starry-d1] root_provider=d1-memory-root-path requested_path=/bin/benchmark resolved=true
[starry-d1] evidence_path=/init.sh resolved=true (not executed, shell unavailable)
[starry-d1] argv_evidence=kernel-side-construction argv=/bin/benchmark,--q19c-m2-command-entry
[starry-d1] envp_count=0 (kernel-side construction)
[starry-d1] stdio=/dev/console
[starry-d1] note=user-observed-argv-not-claimed (payload does not print argc/argv; see q19c-m2-m3-acceptance-alignment §D4)
[starry-d1] Loading /bin/benchmark via path eager loader (command-entry)...
[starry-d1] stage=loaded-process-command-entry requested_path=/bin/benchmark spawned=true
[starry-d1] benchmark process spawned (command-entry), waiting...
UART Async Benchmark
====================

=== [S00] Benchmark Manifest ===
  version=q19c-m0-20260703
  backend=polling-console
  target_mode=lichee-d1-fullbench
  startup_chain=android-boot-image -> memory-root /bin/benchmark -> eager_elf_mapping
  root_provider=d1-memory-root-path
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

[  0.656433 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 3
=== [S10] TX Throughput Baseline (write + tcdrain each iteration) ===
  diag=S10 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=548 line_time_ms=542.5 kbps=11.40 line_rate_pct=99.0
  diag=drain-each-size-64 n=100 avg_ms=5.480 p50_ms=5.480 p95_ms=5.481 p99_ms=5.493 max_ms=5.493 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.01 p99_p50_ratio=1.00 max_p50_ratio=1.00
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2186 line_time_ms=2170.1 kbps=11.44 line_rate_pct=99.3
  diag=drain-each-size-256 n=100 avg_ms=21.860 p50_ms=21.859 p95_ms=21.860 p99_ms=21.915 max_ms=21.915 slow_gt10ms=100 slow_over_line_plus10ms=0 max_line_ratio=1.01 p99_p50_ratio=1.00 max_p50_ratio=1.00
  policy=drain-each size=1024 iters=100 bytes=102400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=8735 line_time_ms=8680.6 kbps=11.45 line_rate_pct=99.4
  diag=drain-each-size-1024 n=100 avg_ms=87.356 p50_ms=87.355 p95_ms=87.356 p99_ms=87.413 max_ms=87.413 slow_gt10ms=100 slow_over_line_plus10ms=0 max_line_ratio=1.01 p99_p50_ratio=1.00 max_p50_ratio=1.00

=== [S11] Blocking Transmit (write loop, final drain outside timing) ===
  diag=S11 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
[ 12.260598 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[ 12.816216 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
[ 12.826363 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  policy=no-drain size=64 iters=100 bytes=6400 short_writes=0 enqueue_ms=545 final_drain_ms=10 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=542.5 enqueue_kbps=11.45
  diag=s11-txdbg-reset size=64 ioctl_rc=-1
  diag=s11-txdbg phase=enqueue size=64 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=64 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
[ 12.910807 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[ 15.103801 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
[ 15.113949 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  policy=no-drain size=256 iters=100 bytes=25600 short_writes=0 enqueue_ms=2183 final_drain_ms=10 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=2170.1 enqueue_kbps=11.45
  diag=s11-txdbg-reset size=256 ioctl_rc=-1
  diag=s11-txdbg phase=enqueue size=256 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=256 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
[ 15.198990 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[ 23.941488 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
[ 23.951635 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  policy=no-drain size=1024 iters=100 bytes=102400 short_writes=0 enqueue_ms=8732 final_drain_ms=10 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=8680.6 enqueue_kbps=11.45
  diag=s11-txdbg-reset size=1024 ioctl_rc=-1
  diag=s11-txdbg phase=enqueue size=1024 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=1024 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0

=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===
  diag=S12 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=batch-drain size=64 iters=100 batch=8 drains=13 bytes=6400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=546 line_time_ms=542.5 kbps=11.45 line_rate_pct=99.4
  policy=batch-drain size=256 iters=100 batch=8 drains=13 bytes=25600 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=2183 line_time_ms=2170.1 kbps=11.45 line_rate_pct=99.4
  policy=batch-drain size=1024 iters=100 batch=8 drains=13 bytes=102400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=8733 line_time_ms=8680.6 kbps=11.45 line_rate_pct=99.4

=== [S13] TX writev Fragments (fragment aggregation witness) ===
  diag=S13 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=writev-drain-each fragments=4 fragment_size=64 total_size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2186 line_time_ms=2170.1 kbps=11.43 line_rate_pct=99.3

=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===
  diag=S14 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=548 line_time_ms=542.5 kbps=11.40 line_rate_pct=99.0
  diag=break-even-size-64 n=100 avg_ms=5.480 p50_ms=5.480 p95_ms=5.481 p99_ms=5.493 max_ms=5.493 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.01 p99_p50_ratio=1.00 max_p50_ratio=1.00
  policy=drain-each size=128 iters=100 bytes=12800 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=1094 line_time_ms=1085.1 kbps=11.42 line_rate_pct=99.2
  diag=break-even-size-128 n=100 avg_ms=10.940 p50_ms=10.940 p95_ms=10.940 p99_ms=11.002 max_ms=11.002 slow_gt10ms=100 slow_over_line_plus10ms=0 max_line_ratio=1.01 p99_p50_ratio=1.01 max_p50_ratio=1.01
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2186 line_time_ms=2170.1 kbps=11.44 line_rate_pct=99.3
  diag=break-even-size-256 n=100 avg_ms=21.860 p50_ms=21.859 p95_ms=21.859 p99_ms=21.919 max_ms=21.919 slow_gt10ms=100 slow_over_line_plus10ms=0 max_line_ratio=1.01 p99_p50_ratio=1.00 max_p50_ratio=1.00

=== [S20] TX Latency (single byte + tcdrain) ===
  diag=S20 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  diag=s20-single-byte n=100 avg_ms=0.106 p50_ms=0.106 p95_ms=0.106 p99_ms=0.112 max_ms=0.112 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.32 p99_p50_ratio=1.05 max_p50_ratio=1.05
  diag=S20 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S21] TX Latency FIFO Boundary Matrix ===
  diag=S21 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  diag=s21-fifo-size-1 n=100 avg_ms=0.106 p50_ms=0.106 p95_ms=0.106 p99_ms=0.118 max_ms=0.118 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.40 p99_p50_ratio=1.12 max_p50_ratio=1.12
  diag=fifo-size-1 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-15 n=100 avg_ms=1.301 p50_ms=1.300 p95_ms=1.301 p99_ms=1.398 max_ms=1.398 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.10 p99_p50_ratio=1.08 max_p50_ratio=1.08
  diag=fifo-size-15 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-16 n=100 avg_ms=1.386 p50_ms=1.385 p95_ms=1.386 p99_ms=1.483 max_ms=1.483 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.09 p99_p50_ratio=1.07 max_p50_ratio=1.07
  diag=fifo-size-16 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-17 n=100 avg_ms=1.471 p50_ms=1.470 p95_ms=1.471 p99_ms=1.569 max_ms=1.569 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.09 p99_p50_ratio=1.07 max_p50_ratio=1.07
  diag=fifo-size-17 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-31 n=100 avg_ms=2.666 p50_ms=2.665 p95_ms=2.666 p99_ms=2.761 max_ms=2.761 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.05 p99_p50_ratio=1.04 max_p50_ratio=1.04
  diag=fifo-size-31 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-32 n=100 avg_ms=2.751 p50_ms=2.750 p95_ms=2.751 p99_ms=2.847 max_ms=2.847 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.05 p99_p50_ratio=1.04 max_p50_ratio=1.04
  diag=fifo-size-32 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-33 n=100 avg_ms=2.836 p50_ms=2.836 p95_ms=2.836 p99_ms=2.932 max_ms=2.932 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.05 p99_p50_ratio=1.03 max_p50_ratio=1.03
  diag=fifo-size-33 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-48 n=100 avg_ms=4.116 p50_ms=4.115 p95_ms=4.116 p99_ms=4.212 max_ms=4.212 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.04 p99_p50_ratio=1.02 max_p50_ratio=1.02
  diag=fifo-size-48 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-49 n=100 avg_ms=4.202 p50_ms=4.201 p95_ms=4.201 p99_ms=4.297 max_ms=4.297 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.03 p99_p50_ratio=1.02 max_p50_ratio=1.02
  diag=fifo-size-49 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S30] RX Empty Non-blocking Read (FIONBIO) ===
  status=UNSUPPORTED reason=D1-UART-RX-not-implemented

=== [S31] RX Fixed Payload Witness ===
  diag=S31 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  status=SKIPPED reason=BENCH_RX_FIXED_BYTES=0

=== [S40] TX Counter Proxy Summary ===
[ 44.088540 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 3
  status=UNSUPPORTED reason=backend-polling-console-no-telemetry
  proxy=not-available

Done.
[starry-d1] benchmark exited with code: 0
[starry-d1] halting.

