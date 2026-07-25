Boot at 2026-07-22 07:20:34.004646200 UTC

[  0.491879 0 axnet_ng:139]   No vsock device found!
[  0.492621 0 axdisplay:26]   No display device found!
Welcome to Starry OS!
SHLVL=1
HOME=/root
PWD=/

Use apk to install packages.

starry:~# cd /bin
starry:/bin# ./benchmark
Console Benchmark
==================

=== [S00] Benchmark Manifest ===
  version=q32-console-cpu-efficiency-20260722
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

  instret_read_overhead=1984339
[  7.696179 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 3
=== [S10] TX Throughput Baseline (write + tcdrain each iteration) ===
  diag=S10 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=35 line_time_ms=542.5 kbps=175.07 line_rate_pct=1519.7
  diag=drain-each-size-64 n=100 avg_ms=0.354 p50_ms=0.369 p95_ms=0.570 p99_ms=0.958 max_ms=0.958 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.18 p99_p50_ratio=2.60 max_p50_ratio=2.60
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=140 line_time_ms=2170.1 kbps=178.15 line_rate_pct=1546.5
  diag=drain-each-size-256 n=100 avg_ms=1.399 p50_ms=1.431 p95_ms=2.012 p99_ms=2.400 max_ms=2.400 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.11 p99_p50_ratio=1.68 max_p50_ratio=1.68
  policy=drain-each size=1024 iters=100 bytes=102400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=587 line_time_ms=8680.6 kbps=170.17 line_rate_pct=1477.2
  diag=drain-each-size-1024 n=100 avg_ms=5.871 p50_ms=5.967 p95_ms=7.935 p99_ms=8.867 max_ms=8.867 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.10 p99_p50_ratio=1.49 max_p50_ratio=1.49

=== [S11] Blocking Transmit (write loop, final drain outside timing) ===
  diag=S11 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
[  8.472260 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[  8.514639 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
[  8.515776 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  write_semantics=synchronous-blocking completion=final-tcdrain-after-loop policy=no-drain size=64 iters=100 bytes=6400 short_writes=0 enqueue_ms=41 final_drain_ms=1 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=542.5 enqueue_kbps=151.03
  diag=s11-txdbg-reset size=64 ioctl_rc=-1
  diag=s11-txdbg phase=enqueue size=64 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=64 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-derived size=64 submit_fraction=0.9725 producer_available=0.0275 total_time_ms=42 enqueue_time_ms=41
[  8.524356 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[  8.683978 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
[  8.684264 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  write_semantics=synchronous-blocking completion=final-tcdrain-after-loop policy=no-drain size=256 iters=100 bytes=25600 short_writes=0 enqueue_ms=158 final_drain_ms=0 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=2170.1 enqueue_kbps=157.36
  diag=s11-txdbg-reset size=256 ioctl_rc=-1
  diag=s11-txdbg phase=enqueue size=256 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=256 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-derived size=256 submit_fraction=0.9980 producer_available=0.0020 total_time_ms=159 enqueue_time_ms=158
[  8.691240 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[  9.316525 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
[  9.317566 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  write_semantics=synchronous-blocking completion=final-tcdrain-after-loop policy=no-drain size=1024 iters=100 bytes=102400 short_writes=0 enqueue_ms=624 final_drain_ms=1 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=8680.6 enqueue_kbps=160.00
  diag=s11-txdbg-reset size=1024 ioctl_rc=-1
  diag=s11-txdbg phase=enqueue size=1024 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=1024 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-derived size=1024 submit_fraction=0.9983 producer_available=0.0017 total_time_ms=626 enqueue_time_ms=624

=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===
  diag=S12 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=batch-drain size=64 iters=100 batch=8 drains=13 bytes=6400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=40 line_time_ms=542.5 kbps=155.95 line_rate_pct=1353.7
  policy=batch-drain size=256 iters=100 batch=8 drains=13 bytes=25600 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=149 line_time_ms=2170.1 kbps=166.79 line_rate_pct=1447.8
  policy=batch-drain size=1024 iters=100 batch=8 drains=13 bytes=102400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=619 line_time_ms=8680.6 kbps=161.50 line_rate_pct=1401.9

=== [S13] TX writev Fragments (fragment aggregation witness) ===
  diag=S13 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=writev-drain-each fragments=4 fragment_size=64 total_size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=146 line_time_ms=2170.1 kbps=170.67 line_rate_pct=1481.5

=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===
  diag=S14 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=37 line_time_ms=542.5 kbps=168.50 line_rate_pct=1462.6
  diag=break-even-size-64 n=100 avg_ms=0.367 p50_ms=0.398 p95_ms=0.569 p99_ms=0.766 max_ms=0.766 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.14 p99_p50_ratio=1.92 max_p50_ratio=1.92
  policy=drain-each size=128 iters=100 bytes=12800 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=78 line_time_ms=1085.1 kbps=158.64 line_rate_pct=1377.1
  diag=break-even-size-128 n=100 avg_ms=0.784 p50_ms=0.794 p95_ms=1.264 p99_ms=1.451 max_ms=1.451 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.13 p99_p50_ratio=1.83 max_p50_ratio=1.83
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=152 line_time_ms=2170.1 kbps=164.09 line_rate_pct=1424.4
  diag=break-even-size-256 n=100 avg_ms=1.519 p50_ms=1.522 p95_ms=2.267 p99_ms=3.451 max_ms=3.451 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.16 p99_p50_ratio=2.27 max_p50_ratio=2.27

=== [S20] TX Latency (single byte + tcdrain) ===
  diag=S20 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  diag=s20-single-byte n=100 avg_ms=0.039 p50_ms=0.038 p95_ms=0.040 p99_ms=0.062 max_ms=0.062 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.73 p99_p50_ratio=1.64 max_p50_ratio=1.64
  diag=S20 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S21] TX Latency FIFO Boundary Matrix ===
  diag=S21 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  diag=s21-fifo-size-1 n=100 avg_ms=0.039 p50_ms=0.038 p95_ms=0.049 p99_ms=0.090 max_ms=0.090 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.06 p99_p50_ratio=2.36 max_p50_ratio=2.36
  diag=fifo-size-1 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-15 n=100 avg_ms=0.113 p50_ms=0.115 p95_ms=0.178 p99_ms=0.274 max_ms=0.274 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.22 p99_p50_ratio=2.39 max_p50_ratio=2.39
  diag=fifo-size-15 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-16 n=100 avg_ms=0.107 p50_ms=0.117 p95_ms=0.188 p99_ms=0.208 max_ms=0.208 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.15 p99_p50_ratio=1.78 max_p50_ratio=1.78
  diag=fifo-size-16 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-17 n=100 avg_ms=0.117 p50_ms=0.115 p95_ms=0.202 p99_ms=0.273 max_ms=0.273 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.19 p99_p50_ratio=2.37 max_p50_ratio=2.37
  diag=fifo-size-17 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-31 n=100 avg_ms=0.212 p50_ms=0.218 p95_ms=0.283 p99_ms=0.658 max_ms=0.658 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.25 p99_p50_ratio=3.02 max_p50_ratio=3.02
  diag=fifo-size-31 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-32 n=100 avg_ms=0.196 p50_ms=0.210 p95_ms=0.284 p99_ms=0.376 max_ms=0.376 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.14 p99_p50_ratio=1.79 max_p50_ratio=1.79
  diag=fifo-size-32 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-33 n=100 avg_ms=0.218 p50_ms=0.231 p95_ms=0.320 p99_ms=0.631 max_ms=0.631 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.23 p99_p50_ratio=2.73 max_p50_ratio=2.73
  diag=fifo-size-33 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-48 n=100 avg_ms=0.302 p50_ms=0.310 p95_ms=0.478 p99_ms=1.001 max_ms=1.001 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.25 p99_p50_ratio=3.22 max_p50_ratio=3.22
  diag=fifo-size-48 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-49 n=100 avg_ms=0.320 p50_ms=0.329 p95_ms=0.478 p99_ms=0.587 max_ms=0.587 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.14 p99_p50_ratio=1.79 max_p50_ratio=1.79
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
  instret_read_overhead=218503
  s41-size=64 expected_bytes=6400 iters=100
[ 10.764175 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=64 round=1 completed=6400 expected=6400 reset_rc=-1 instret_begin=28188255276925 instret_end=28188348753741 instret_delta=93476816 instructions_per_byte=14605.75 instructions_per_write=934768 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=38
[ 10.806404 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=64 round=2 completed=6400 expected=6400 reset_rc=-1 instret_begin=28188357282346 instret_end=28188455486537 instret_delta=98204191 instructions_per_byte=15344.40 instructions_per_write=982042 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=40
[ 10.850500 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=64 round=3 completed=6400 expected=6400 reset_rc=-1 instret_begin=28188463634491 instret_end=28188558978779 instret_delta=95344288 instructions_per_byte=14897.55 instructions_per_write=953443 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=39
[ 10.893382 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=64 round=4 completed=6400 expected=6400 reset_rc=-1 instret_begin=28188567170107 instret_end=28188661083899 instret_delta=93913792 instructions_per_byte=14674.03 instructions_per_write=939138 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=38
[ 10.935395 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=64 round=5 completed=6400 expected=6400 reset_rc=-1 instret_begin=28188668270556 instret_end=28188762862187 instret_delta=94591631 instructions_per_byte=14779.94 instructions_per_write=945916 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=38
  diag=s41-summary size=64 valid_rounds=5 median_instructions_per_byte=14779.94 median_instructions_per_write=945916
[ 10.978396 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s41-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
  s41-size=256 expected_bytes=25600 iters=100
[ 10.980345 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=256 round=1 completed=25600 expected=25600 reset_rc=-1 instret_begin=28188778239912 instret_end=28189159104849 instret_delta=380864937 instructions_per_byte=14877.54 instructions_per_write=3808649 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=157
[ 11.140780 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=256 round=2 completed=25600 expected=25600 reset_rc=-1 instret_begin=28189165026562 instret_end=28189527861444 instret_delta=362834882 instructions_per_byte=14173.24 instructions_per_write=3628349 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=149
[ 11.293972 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=256 round=3 completed=25600 expected=25600 reset_rc=-1 instret_begin=28189536609086 instret_end=28189914256703 instret_delta=377647617 instructions_per_byte=14751.86 instructions_per_write=3776476 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=155
[ 11.452809 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=256 round=4 completed=25600 expected=25600 reset_rc=-1 instret_begin=28189923295819 instret_end=28190282850908 instret_delta=359555089 instructions_per_byte=14045.12 instructions_per_write=3595551 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=148
[ 11.605569 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=256 round=5 completed=25600 expected=25600 reset_rc=-1 instret_begin=28190289456054 instret_end=28190633794820 instret_delta=344338766 instructions_per_byte=13450.73 instructions_per_write=3443388 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=142
  diag=s41-summary size=256 valid_rounds=5 median_instructions_per_byte=14173.24 median_instructions_per_write=3628349
[ 11.751646 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s41-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
  s41-size=1024 expected_bytes=102400 iters=100
[ 11.753831 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=1024 round=1 completed=102400 expected=102400 reset_rc=-1 instret_begin=28190649226795 instret_end=28192108909915 instret_delta=1459683120 instructions_per_byte=14254.72 instructions_per_write=3649208 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=400 duration_ms=603
[ 12.360258 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=1024 round=2 completed=102400 expected=102400 reset_rc=-1 instret_begin=28192115673069 instret_end=28193540784105 instret_delta=1425111036 instructions_per_byte=13917.10 instructions_per_write=3562778 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=400 duration_ms=588
[ 12.952180 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=1024 round=3 completed=102400 expected=102400 reset_rc=-1 instret_begin=28193547665739 instret_end=28194933240356 instret_delta=1385574617 instructions_per_byte=13531.00 instructions_per_write=3463937 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=400 duration_ms=572
[ 13.527676 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=1024 round=4 completed=102400 expected=102400 reset_rc=-1 instret_begin=28194940125059 instret_end=28196313063695 instret_delta=1372938636 instructions_per_byte=13407.60 instructions_per_write=3432347 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=400 duration_ms=567
[ 14.097663 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=1024 round=5 completed=102400 expected=102400 reset_rc=-1 instret_begin=28196318247754 instret_end=28197727017312 instret_delta=1408769558 instructions_per_byte=13757.52 instructions_per_write=3521924 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=400 duration_ms=582
  diag=s41-summary size=1024 valid_rounds=5 median_instructions_per_byte=13757.52 median_instructions_per_write=3521924
[ 14.683349 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s41-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1

=== [S42] TX Compute Overlap (64B x 100, fixed window, 5 sample rounds) ===
  diag=S42 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  idle window_ms=542.535 window_ns=542534722 iters=265637 duration_ms=542.575 iters_per_sec=489586
  overlap payload=64 iters=100 warmup=1 sample_rounds=5 theoretical_line_time_ms=542.535
[ 15.230014 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[ 15.773401 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s42-sample round=1 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=33946.3 useful_iters=279398 useful_work_per_ms=515 final_drain_ms=0.025 total_duration_ms=542.563 total_over_line_ratio=1.000 overlap_efficiency=1.0518 reset_rc=-1 drain_errors=0 leftover_ns=3778
[ 16.318108 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s42-sample round=2 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=28744.9 useful_iters=277118 useful_work_per_ms=511 final_drain_ms=0.019 total_duration_ms=542.557 total_over_line_ratio=1.000 overlap_efficiency=1.0432 reset_rc=-1 drain_errors=0 leftover_ns=2778
[ 16.862438 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s42-sample round=3 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=28117.1 useful_iters=276616 useful_work_per_ms=510 final_drain_ms=0.023 total_duration_ms=542.562 total_over_line_ratio=1.000 overlap_efficiency=1.0413 reset_rc=-1 drain_errors=0 leftover_ns=3878
[ 17.406200 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s42-sample round=4 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=29498.6 useful_iters=280769 useful_work_per_ms=517 final_drain_ms=0.025 total_duration_ms=542.563 total_over_line_ratio=1.000 overlap_efficiency=1.0570 reset_rc=-1 drain_errors=0 leftover_ns=3178
[ 17.950444 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s42-sample round=5 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=29281.1 useful_iters=278018 useful_work_per_ms=512 final_drain_ms=0.018 total_duration_ms=542.555 total_over_line_ratio=1.000 overlap_efficiency=1.0466 reset_rc=-1 drain_errors=0 leftover_ns=2378
  diag=s42-summary valid_rounds=5 median_useful_iters=278018 median_total_duration_ms=542.562 median_overlap_efficiency=1.0466
[ 18.495121 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s42-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1

=== [S43] Timer Wakeup Overshoot (5 idle groups + 5 loaded groups) ===
  diag=S43 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  s43-phase=idle groups=5 samples=50 interval_us=5000
  diag=s43-idle-group group=1 status=PASS collected=50 errors=0 valid=50 duration_ms=256 sample[0]=3153000 sample[1]=6699300 sample[2]=1862500
  s43-idle-group-summary n=50 errors=0 p50_ns=6279600 p95_ns=6845800 p99_ns=7075600 max_ns=7075600
  diag=s43-idle-group group=2 status=PASS collected=50 errors=0 valid=50 duration_ms=259 sample[0]=3875600 sample[1]=9001200 sample[2]=4032700
  s43-idle-group-summary n=50 errors=0 p50_ns=8946600 p95_ns=9548200 p99_ns=9664100 max_ns=9664100
  diag=s43-idle-group group=3 status=PASS collected=50 errors=0 valid=50 duration_ms=259 sample[0]=3725600 sample[1]=8934200 sample[2]=3965600
  s43-idle-group-summary n=50 errors=0 p50_ns=8757700 p95_ns=9400200 p99_ns=9517000 max_ns=9517000
  diag=s43-idle-group group=4 status=PASS collected=50 errors=0 valid=50 duration_ms=258 sample[0]=3360100 sample[1]=8051000 sample[2]=3085600
  s43-idle-group-summary n=50 errors=0 p50_ns=8051000 p95_ns=8734600 p99_ns=8798500 max_ns=8798500
  diag=s43-idle-group group=5 status=PASS collected=50 errors=0 valid=50 duration_ms=259 sample[0]=4242900 sample[1]=9305200 sample[2]=4339300
  s43-idle-group-summary n=50 errors=0 p50_ns=9149500 p95_ns=9581800 p99_ns=9687500 max_ns=9687500
  s43-phase=loaded groups=5 burst_bytes=4096 theoretical_line_time_ns=347222222
[ 19.797435 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s43-loaded-group group=1 status=PASS reset_rc=-1 burst_written=4096 expected=4096 write_dur_ns=20061900 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=235 drain_ms=0 drain_errors=0 sample[0]=15107600 sample[1]=10116700 sample[2]=5123700
  s43-loaded-group-summary n=50 errors=0 p50_ns=5049600 p95_ns=9972400 p99_ns=15107600 max_ns=15107600
[ 20.056822 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s43-loaded-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
[ 20.059808 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s43-loaded-group group=2 status=PASS reset_rc=-1 burst_written=4096 expected=4096 write_dur_ns=20388100 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=232 drain_ms=0 drain_errors=0 sample[0]=15416100 sample[1]=10424500 sample[2]=5431300
  s43-loaded-group-summary n=50 errors=0 p50_ns=7416100 p95_ns=8118700 p99_ns=15416100 max_ns=15416100
[ 20.316391 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s43-loaded-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
[ 20.319635 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s43-loaded-group group=3 status=PASS reset_rc=-1 burst_written=4096 expected=4096 write_dur_ns=18269500 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=234 drain_ms=0 drain_errors=0 sample[0]=13293500 sample[1]=8302000 sample[2]=3308900
  s43-loaded-group-summary n=50 errors=0 p50_ns=7431800 p95_ns=8140300 p99_ns=13293500 max_ns=13293500
[ 20.576794 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s43-loaded-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
[ 20.578568 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s43-loaded-group group=4 status=PASS reset_rc=-1 burst_written=4096 expected=4096 write_dur_ns=16633900 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=238 drain_ms=0 drain_errors=0 sample[0]=11659400 sample[1]=6667800 sample[2]=1674800
  s43-loaded-group-summary n=50 errors=0 p50_ns=5040600 p95_ns=10067200 p99_ns=11659400 max_ns=11659400
[ 20.837719 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s43-loaded-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
[ 20.839026 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s43-loaded-group group=5 status=PASS reset_rc=-1 burst_written=4096 expected=4096 write_dur_ns=17725200 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=236 drain_ms=0 drain_errors=0 sample[0]=12751100 sample[1]=7760100 sample[2]=2767000
  s43-loaded-group-summary n=50 errors=0 p50_ns=7760100 p95_ns=9622600 p99_ns=12751100 max_ns=12751100
[ 21.096588 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s43-loaded-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
  diag=s43-loaded-aggregate n=250 valid_groups=5 p50_ns=5049600 p95_ns=9827400 p99_ns=13293500 max_ns=15416100
  diag=s43-idle-aggregate n=250 valid_groups=5 p50_ns=6279600 p95_ns=9473600 p99_ns=9634300 max_ns=9687500

=== [S40] TX Counter Proxy Summary ===
[ 21.099583 0:9 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 3
  status=UNSUPPORTED reason=backend-polling-console-no-telemetry
  proxy=not-available

Done.
starry:/bin#  
