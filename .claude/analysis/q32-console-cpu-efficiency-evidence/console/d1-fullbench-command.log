Boot at 1970-01-01 00:00:00.438960 UTC

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
Console Benchmark
==================

=== [S00] Benchmark Manifest ===
  version=q32-console-cpu-efficiency-20260722
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
  hart_count=not-available
  fstat_dev=major=5 minor=1
  source_revision=not-available
  source_dirty=not-available
  timer_source_detail=CLOCK_MONOTONIC
  clock_nanosleep_available=yes
  instret_source=/proc/instret
  bench_version_extra=q32-console-cpu-efficiency

=== [S05] Startup Ring ===
  status=SKIPPED reason=no-async-driver

  instret_read_overhead=15893
[  0.696189 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 3
=== [S10] TX Throughput Baseline (write + tcdrain each iteration) ===
  diag=S10 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=557 line_time_ms=542.5 kbps=11.21 line_rate_pct=97.3
  diag=drain-each-size-64 n=100 avg_ms=5.573 p50_ms=5.573 p95_ms=5.574 p99_ms=5.584 max_ms=5.584 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.03 p99_p50_ratio=1.00 max_p50_ratio=1.00
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2222 line_time_ms=2170.1 kbps=11.25 line_rate_pct=97.7
  diag=drain-each-size-256 n=100 avg_ms=22.218 p50_ms=22.218 p95_ms=22.219 p99_ms=22.261 max_ms=22.261 slow_gt10ms=100 slow_over_line_plus10ms=0 max_line_ratio=1.03 p99_p50_ratio=1.00 max_p50_ratio=1.00
  policy=drain-each size=1024 iters=100 bytes=102400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=8878 line_time_ms=8680.6 kbps=11.26 line_rate_pct=97.8
  diag=drain-each-size-1024 n=100 avg_ms=88.779 p50_ms=88.779 p95_ms=88.780 p99_ms=88.823 max_ms=88.823 slow_gt10ms=100 slow_over_line_plus10ms=0 max_line_ratio=1.02 p99_p50_ratio=1.00 max_p50_ratio=1.00

=== [S11] Blocking Transmit (write loop, final drain outside timing) ===
  diag=S11 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
[ 12.490406 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[ 13.055062 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
[ 13.065371 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  write_semantics=synchronous-blocking completion=final-tcdrain-after-loop policy=no-drain size=64 iters=100 bytes=6400 short_writes=0 enqueue_ms=554 final_drain_ms=10 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=542.5 enqueue_kbps=11.27
  diag=s11-txdbg-reset size=64 ioctl_rc=-1
  diag=s11-txdbg phase=enqueue size=64 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=64 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-derived size=64 submit_fraction=0.9817 producer_available=0.0183 total_time_ms=564 enqueue_time_ms=554
[ 13.167569 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[ 15.396230 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
[ 15.406535 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  write_semantics=synchronous-blocking completion=final-tcdrain-after-loop policy=no-drain size=256 iters=100 bytes=25600 short_writes=0 enqueue_ms=2218 final_drain_ms=10 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=2170.1 enqueue_kbps=11.27
  diag=s11-txdbg-reset size=256 ioctl_rc=-1
  diag=s11-txdbg phase=enqueue size=256 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=256 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-derived size=256 submit_fraction=0.9954 producer_available=0.0046 total_time_ms=2228 enqueue_time_ms=2218
[ 15.509599 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[ 24.394258 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
[ 24.404566 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  write_semantics=synchronous-blocking completion=final-tcdrain-after-loop policy=no-drain size=1024 iters=100 bytes=102400 short_writes=0 enqueue_ms=8874 final_drain_ms=10 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=8680.6 enqueue_kbps=11.27
  diag=s11-txdbg-reset size=1024 ioctl_rc=-1
  diag=s11-txdbg phase=enqueue size=1024 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=1024 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-derived size=1024 submit_fraction=0.9988 producer_available=0.0012 total_time_ms=8884 enqueue_time_ms=8874

=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===
  diag=S12 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=batch-drain size=64 iters=100 batch=8 drains=13 bytes=6400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=554 line_time_ms=542.5 kbps=11.26 line_rate_pct=97.8
  policy=batch-drain size=256 iters=100 batch=8 drains=13 bytes=25600 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=2219 line_time_ms=2170.1 kbps=11.27 line_rate_pct=97.8
  policy=batch-drain size=1024 iters=100 batch=8 drains=13 bytes=102400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=8875 line_time_ms=8680.6 kbps=11.27 line_rate_pct=97.8

=== [S13] TX writev Fragments (fragment aggregation witness) ===
  diag=S13 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=writev-drain-each fragments=4 fragment_size=64 total_size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2222 line_time_ms=2170.1 kbps=11.25 line_rate_pct=97.7

=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===
  diag=S14 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=557 line_time_ms=542.5 kbps=11.21 line_rate_pct=97.3
  diag=break-even-size-64 n=100 avg_ms=5.573 p50_ms=5.573 p95_ms=5.573 p99_ms=5.582 max_ms=5.582 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.03 p99_p50_ratio=1.00 max_p50_ratio=1.00
  policy=drain-each size=128 iters=100 bytes=12800 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=1112 line_time_ms=1085.1 kbps=11.24 line_rate_pct=97.5
  diag=break-even-size-128 n=100 avg_ms=11.122 p50_ms=11.122 p95_ms=11.123 p99_ms=11.169 max_ms=11.169 slow_gt10ms=100 slow_over_line_plus10ms=0 max_line_ratio=1.03 p99_p50_ratio=1.00 max_p50_ratio=1.00
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2222 line_time_ms=2170.1 kbps=11.25 line_rate_pct=97.7
  diag=break-even-size-256 n=100 avg_ms=22.218 p50_ms=22.218 p95_ms=22.219 p99_ms=22.261 max_ms=22.261 slow_gt10ms=100 slow_over_line_plus10ms=0 max_line_ratio=1.03 p99_p50_ratio=1.00 max_p50_ratio=1.00

=== [S20] TX Latency (single byte + tcdrain) ===
  diag=S20 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  diag=s20-single-byte n=100 avg_ms=0.110 p50_ms=0.109 p95_ms=0.110 p99_ms=0.117 max_ms=0.117 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.38 p99_p50_ratio=1.07 max_p50_ratio=1.07
  diag=S20 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S21] TX Latency FIFO Boundary Matrix ===
  diag=S21 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  diag=s21-fifo-size-1 n=100 avg_ms=0.110 p50_ms=0.109 p95_ms=0.110 p99_ms=0.122 max_ms=0.122 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.44 p99_p50_ratio=1.12 max_p50_ratio=1.12
  diag=fifo-size-1 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-15 n=100 avg_ms=1.324 p50_ms=1.324 p95_ms=1.325 p99_ms=1.418 max_ms=1.418 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.11 p99_p50_ratio=1.07 max_p50_ratio=1.07
  diag=fifo-size-15 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-16 n=100 avg_ms=1.411 p50_ms=1.410 p95_ms=1.412 p99_ms=1.504 max_ms=1.504 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.11 p99_p50_ratio=1.07 max_p50_ratio=1.07
  diag=fifo-size-16 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-17 n=100 avg_ms=1.498 p50_ms=1.497 p95_ms=1.498 p99_ms=1.591 max_ms=1.591 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.10 p99_p50_ratio=1.06 max_p50_ratio=1.06
  diag=fifo-size-17 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-31 n=100 avg_ms=2.712 p50_ms=2.711 p95_ms=2.712 p99_ms=2.805 max_ms=2.805 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.07 p99_p50_ratio=1.03 max_p50_ratio=1.03
  diag=fifo-size-31 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-32 n=100 avg_ms=2.799 p50_ms=2.798 p95_ms=2.802 p99_ms=2.891 max_ms=2.891 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.07 p99_p50_ratio=1.03 max_p50_ratio=1.03
  diag=fifo-size-32 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-33 n=100 avg_ms=2.885 p50_ms=2.884 p95_ms=2.885 p99_ms=2.978 max_ms=2.978 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.06 p99_p50_ratio=1.03 max_p50_ratio=1.03
  diag=fifo-size-33 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-48 n=100 avg_ms=4.186 p50_ms=4.185 p95_ms=4.187 p99_ms=4.278 max_ms=4.278 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.05 p99_p50_ratio=1.02 max_p50_ratio=1.02
  diag=fifo-size-48 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-49 n=100 avg_ms=4.273 p50_ms=4.272 p95_ms=4.273 p99_ms=4.364 max_ms=4.364 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.05 p99_p50_ratio=1.02 max_p50_ratio=1.02
  diag=fifo-size-49 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S30] RX Empty Non-blocking Read (FIONBIO) ===
  diag=S30 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  method=open-o-nonblock status=PASS result=EAGAIN
  method=ioctl-fionbio status=PASS result=EAGAIN

=== [S31] RX Fixed Payload Witness ===
  diag=S31 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  status=SKIPPED reason=BENCH_RX_FIXED_BYTES=0

=== [S41] TX CPU Work (instret: write start → final TEMT drain, 5 rounds) ===
  diag=S41 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  instret_read_overhead=15893
  s41-size=64 expected_bytes=6400 iters=100
[ 44.918382 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=64 round=1 completed=6400 expected=6400 reset_rc=-1 instret_begin=681483768 instret_end=689126499 instret_delta=7642731 instructions_per_byte=1194.18 instructions_per_write=76427 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=554
[ 45.510512 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=64 round=2 completed=6400 expected=6400 reset_rc=-1 instret_begin=689686175 instret_end=697329902 instret_delta=7643727 instructions_per_byte=1194.33 instructions_per_write=76437 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=554
[ 46.102649 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=64 round=3 completed=6400 expected=6400 reset_rc=-1 instret_begin=697891494 instret_end=705534561 instret_delta=7643067 instructions_per_byte=1194.23 instructions_per_write=76431 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=554
[ 46.694786 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=64 round=4 completed=6400 expected=6400 reset_rc=-1 instret_begin=706095960 instret_end=713739567 instret_delta=7643607 instructions_per_byte=1194.31 instructions_per_write=76436 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=554
[ 47.286923 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=64 round=5 completed=6400 expected=6400 reset_rc=-1 instret_begin=714301140 instret_end=721944348 instret_delta=7643208 instructions_per_byte=1194.25 instructions_per_write=76432 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=554
  diag=s41-summary size=64 valid_rounds=5 median_instructions_per_byte=1194.25 median_instructions_per_write=76432
[ 47.889035 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s41-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
  s41-size=256 expected_bytes=25600 iters=100
[ 47.912337 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=256 round=1 completed=25600 expected=25600 reset_rc=-1 instret_begin=723013635 instret_end=751308344 instret_delta=28294709 instructions_per_byte=1105.26 instructions_per_write=282947 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=2218
[ 50.168988 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=256 round=2 completed=25600 expected=25600 reset_rc=-1 instret_begin=751874695 instret_end=780169953 instret_delta=28295258 instructions_per_byte=1105.28 instructions_per_write=282953 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=2218
[ 52.425637 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=256 round=3 completed=25600 expected=25600 reset_rc=-1 instret_begin=780736596 instret_end=809031401 instret_delta=28294805 instructions_per_byte=1105.27 instructions_per_write=282948 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=2218
[ 54.682285 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=256 round=4 completed=25600 expected=25600 reset_rc=-1 instret_begin=809597792 instret_end=837892672 instret_delta=28294880 instructions_per_byte=1105.27 instructions_per_write=282949 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=2218
[ 56.938933 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=256 round=5 completed=25600 expected=25600 reset_rc=-1 instret_begin=838459223 instret_end=866754610 instret_delta=28295387 instructions_per_byte=1105.29 instructions_per_write=282954 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=2218
  diag=s41-summary size=256 valid_rounds=5 median_instructions_per_byte=1105.27 median_instructions_per_write=282949
[ 59.205725 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s41-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
  s41-size=1024 expected_bytes=102400 iters=100
[ 59.229200 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=1024 round=1 completed=102400 expected=102400 reset_rc=-1 instret_begin=867832770 instret_end=981038321 instret_delta=113205551 instructions_per_byte=1105.52 instructions_per_write=283014 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=400 duration_ms=8874
[ 68.142199 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=1024 round=2 completed=102400 expected=102400 reset_rc=-1 instret_begin=981608761 instret_end=1094811063 instret_delta=113202302 instructions_per_byte=1105.49 instructions_per_write=283006 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=400 duration_ms=8874
[ 77.055284 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=1024 round=3 completed=102400 expected=102400 reset_rc=-1 instret_begin=1095382645 instret_end=1208586050 instret_delta=113203405 instructions_per_byte=1105.50 instructions_per_write=283009 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=400 duration_ms=8874
[ 85.968455 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=1024 round=4 completed=102400 expected=102400 reset_rc=-1 instret_begin=1209159198 instret_end=1322360938 instret_delta=113201740 instructions_per_byte=1105.49 instructions_per_write=283004 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=400 duration_ms=8874
[ 94.881627 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=1024 round=5 completed=102400 expected=102400 reset_rc=-1 instret_begin=1322933806 instret_end=1436137358 instret_delta=113203552 instructions_per_byte=1105.50 instructions_per_write=283009 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=400 duration_ms=8874
  diag=s41-summary size=1024 valid_rounds=5 median_instructions_per_byte=1105.50 median_instructions_per_write=283009
[103.805040 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s41-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1

=== [S42] TX Compute Overlap (64B x 100, fixed window, 5 sample rounds) ===
  diag=S42 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  idle window_ms=542.535 window_ns=542534722 iters=271928 duration_ms=542.538 iters_per_sec=501215
  overlap payload=64 iters=100 warmup=1 sample_rounds=5 theoretical_line_time_ms=542.535
[104.396702 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[104.961533 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s42-sample round=1 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=554658.8 useful_iters=0 useful_work_per_ms=0 final_drain_ms=0.167 total_duration_ms=554.833 total_over_line_ratio=1.023 overlap_efficiency=0.0000 reset_rc=-1 drain_errors=0 leftover_ns=12131070
[105.552804 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s42-sample round=2 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=554656.5 useful_iters=0 useful_work_per_ms=0 final_drain_ms=0.167 total_duration_ms=554.831 total_over_line_ratio=1.023 overlap_efficiency=0.0000 reset_rc=-1 drain_errors=0 leftover_ns=12129278
[106.144069 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s42-sample round=3 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=554657.1 useful_iters=0 useful_work_per_ms=0 final_drain_ms=0.166 total_duration_ms=554.831 total_over_line_ratio=1.023 overlap_efficiency=0.0000 reset_rc=-1 drain_errors=0 leftover_ns=12130069
[106.735335 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s42-sample round=4 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=554656.2 useful_iters=0 useful_work_per_ms=0 final_drain_ms=0.167 total_duration_ms=554.831 total_over_line_ratio=1.023 overlap_efficiency=0.0000 reset_rc=-1 drain_errors=0 leftover_ns=12129278
[107.326602 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s42-sample round=5 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=554653.6 useful_iters=0 useful_work_per_ms=0 final_drain_ms=0.166 total_duration_ms=554.828 total_over_line_ratio=1.023 overlap_efficiency=0.0000 reset_rc=-1 drain_errors=0 leftover_ns=12126486
  diag=s42-summary valid_rounds=5 median_useful_iters=0 median_total_duration_ms=554.831 median_overlap_efficiency=0.0000
[107.928451 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s42-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1

=== [S43] Timer Wakeup Overshoot (5 idle groups + 5 loaded groups) ===
  diag=S43 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  s43-phase=idle groups=5 samples=50 interval_us=5000
  diag=s43-idle-group group=1 status=PASS collected=50 errors=0 valid=50 duration_ms=258 sample[0]=3430417 sample[1]=8424917 sample[2]=3440875
  s43-idle-group-summary n=50 errors=0 p50_ns=8423667 p95_ns=8424875 p99_ns=8425000 max_ns=8425000
  diag=s43-idle-group group=2 status=PASS collected=50 errors=0 valid=50 duration_ms=258 sample[0]=3770875 sample[1]=8769667 sample[2]=3785542
  s43-idle-group-summary n=50 errors=0 p50_ns=8768917 p95_ns=8770000 p99_ns=8770292 max_ns=8770292
  diag=s43-idle-group group=3 status=PASS collected=50 errors=0 valid=50 duration_ms=258 sample[0]=3773167 sample[1]=8771417 sample[2]=3787459
  s43-idle-group-summary n=50 errors=0 p50_ns=8770584 p95_ns=8771584 p99_ns=8772542 max_ns=8772542
  diag=s43-idle-group group=4 status=PASS collected=50 errors=0 valid=50 duration_ms=258 sample[0]=3772541 sample[1]=8770458 sample[2]=3786250
  s43-idle-group-summary n=50 errors=0 p50_ns=8769958 p95_ns=8771416 p99_ns=8771583 max_ns=8771583
  diag=s43-idle-group group=5 status=PASS collected=50 errors=0 valid=50 duration_ms=258 sample[0]=3772583 sample[1]=8770958 sample[2]=3787208
  s43-idle-group-summary n=50 errors=0 p50_ns=8770916 p95_ns=8771750 p99_ns=8772333 max_ns=8772333
  s43-phase=loaded groups=5 burst_bytes=4096 theoretical_line_time_ns=347222222
[109.372090 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s43-loaded-group group=1 status=not-applicable reason=no-overlap-window write_dur_ns=354980291 theoretical_line_time_ns=347222222
  diag=s43-loaded-group group=1 status=not-applicable reset_rc=-1 burst_written=4096 expected=4096 write_dur_ns=354980291 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=12 drain_ms=0 drain_errors=0 sample[0]=362045875 sample[1]=357055291 sample[2]=352063666
  s43-loaded-group-summary n=50 errors=0 p50_ns=242246666 p95_ns=352063666 p99_ns=362045875 max_ns=362045875
[109.785169 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s43-loaded-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
[109.804917 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s43-loaded-group group=2 status=not-applicable reason=no-overlap-window write_dur_ns=354983500 theoretical_line_time_ns=347222222
  diag=s43-loaded-group group=2 status=not-applicable reset_rc=-1 burst_written=4096 expected=4096 write_dur_ns=354983500 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=12 drain_ms=0 drain_errors=0 sample[0]=362044917 sample[1]=357054625 sample[2]=352063000
  s43-loaded-group-summary n=50 errors=0 p50_ns=242243875 p95_ns=352063000 p99_ns=362044917 max_ns=362044917
[110.217985 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s43-loaded-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
[110.237735 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s43-loaded-group group=3 status=not-applicable reason=no-overlap-window write_dur_ns=354985500 theoretical_line_time_ns=347222222
  diag=s43-loaded-group group=3 status=not-applicable reset_rc=-1 burst_written=4096 expected=4096 write_dur_ns=354985500 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=12 drain_ms=0 drain_errors=0 sample[0]=362046458 sample[1]=357056125 sample[2]=352064500
  s43-loaded-group-summary n=50 errors=0 p50_ns=242245583 p95_ns=352064500 p99_ns=362046458 max_ns=362046458
[110.650801 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s43-loaded-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
[110.670549 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s43-loaded-group group=4 status=not-applicable reason=no-overlap-window write_dur_ns=354986792 theoretical_line_time_ns=347222222
  diag=s43-loaded-group group=4 status=not-applicable reset_rc=-1 burst_written=4096 expected=4096 write_dur_ns=354986792 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=12 drain_ms=0 drain_errors=0 sample[0]=362046750 sample[1]=357056167 sample[2]=352064500
  s43-loaded-group-summary n=50 errors=0 p50_ns=242247125 p95_ns=352064500 p99_ns=362046750 max_ns=362046750
[111.083619 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s43-loaded-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
[111.103368 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s43-loaded-group group=5 status=not-applicable reason=no-overlap-window write_dur_ns=354985458 theoretical_line_time_ns=347222222
  diag=s43-loaded-group group=5 status=not-applicable reset_rc=-1 burst_written=4096 expected=4096 write_dur_ns=354985458 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=12 drain_ms=0 drain_errors=0 sample[0]=362046208 sample[1]=357055417 sample[2]=352063875
  s43-loaded-group-summary n=50 errors=0 p50_ns=242244292 p95_ns=352063875 p99_ns=362046208 max_ns=362046208
[111.516435 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s43-loaded-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
  diag=s43-idle-aggregate n=250 valid_groups=5 p50_ns=8423667 p95_ns=8771500 p99_ns=8771916 max_ns=8772542

=== [S40] TX Counter Proxy Summary ===
[111.549451 0:5 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 3
  status=UNSUPPORTED reason=backend-polling-console-no-telemetry
  proxy=not-available

Done.
[starry-d1] benchmark exited with code: 0
[starry-d1] halting.

