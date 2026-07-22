Boot at 1970-01-01 00:00:00.440562833 UTC

[starry-d1] Lichee D1 fullbench command-entry mode
[UART INIT] D1 MMIO base=0xffffffc002500000 stride=4 IER=0x0 IIR=0xc1 LSR=0x20
[UART INIT] D1 UART IRQ 18 registered=true, buffers=64KBx2
[UART INIT] async UART hardware initialized (copiers not started yet)
[kernel] Async UART driver initialized (D1)
[BENCH] Running startup benchmark...
[BENCH] Ring buffer write: 696287.92 KB/s (65536 bytes in 0.09 ms)
[BENCH] Hardware line rate: 11.52 KB/s (115200 bps)
[BENCH] FIFO depth: 16 bytes
[BENCH] Ring buffer: 64 KB × 2 = 128 KB total
[BENCH] Driver struct: 288 bytes
[BENCH] Total memory: 128 KB
[BENCH] NAPI threshold: 16 consecutive reads
[BENCH] NAPI batch size: 64 bytes
[BENCH] Copier buffer size: 1024 bytes
[BENCH] IRQ count: 0
[BENCH] Running RX ring buffer throughput test...
[BENCH] RX ring buffer read: 7384331.37 KB/s (65536 bytes in 0.01 ms)
[BENCH] Running RX ring buffer latency test...
[BENCH] RX latency (n=100): min=291ns avg=313ns P50=333ns P95=334ns P99=334ns
[BENCH] Startup benchmark complete
[BENCH] Note: actual throughput limited by UART line rate (11.52 KB/s)
[UART INIT] async UART copiers started
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
  version=q31-cpu-efficiency-20260721
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
  hart_count=not-available
  fstat_dev=major=5 minor=1
  source_revision=not-available
  source_dirty=not-available
  timer_source_detail=CLOCK_MONOTONIC
  clock_nanosleep_available=yes
  instret_source=/proc/instret
  bench_version_extra=q31-cpu-efficiency

  instret_read_overhead=15894
=== [S10] TX Throughput Baseline (write + tcdrain each iteration) ===
  diag=S10 pre_section_stdout_drain_ms=5642 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=570 line_time_ms=542.5 kbps=10.96 line_rate_pct=95.2
  diag=drain-each-size-64 n=100 avg_ms=5.696 p50_ms=5.696 p95_ms=5.698 p99_ms=5.710 max_ms=5.710 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.05 p99_p50_ratio=1.00 max_p50_ratio=1.00
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2267 line_time_ms=2170.1 kbps=11.02 line_rate_pct=95.7
  diag=drain-each-size-256 n=100 avg_ms=22.672 p50_ms=22.345 p95_ms=22.349 p99_ms=54.975 max_ms=54.975 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=2.53 p99_p50_ratio=2.46 max_p50_ratio=2.46
  policy=drain-each size=1024 iters=100 bytes=102400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=8928 line_time_ms=8680.6 kbps=11.20 line_rate_pct=97.2
  diag=drain-each-size-1024 n=100 avg_ms=89.281 p50_ms=88.944 p95_ms=88.963 p99_ms=122.611 max_ms=122.611 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=1.41 p99_p50_ratio=1.38 max_p50_ratio=1.38

=== [S11] TX Enqueue Cost (write loop, final drain outside timing) ===
  diag=S11 pre_section_stdout_drain_ms=40 drain_errors=0 last_errno=0
  policy=no-drain size=64 iters=100 bytes=6400 short_writes=0 enqueue_ms=1 final_drain_ms=554 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=542.5 enqueue_kbps=3845.17
  diag=s11-txdbg-reset size=64 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=64 ioctl_rc=0 user_calls=100 user_req=6400 user_acc=6400 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=64 ioctl_rc=0 user_calls=100 user_req=6400 user_acc=6400 ring_pop_calls=7 ring_pop_bytes=6400 hw_send_calls=274621 hw_send_bytes=6400 hw_send_zero=274221 hw_send_max_chunk=16 no_progress_budget=399 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-derived size=64 submit_fraction=0.0029 producer_available=0.9971 total_time_ms=556 enqueue_time_ms=1
  policy=no-drain size=256 iters=100 bytes=25600 short_writes=0 enqueue_ms=2 final_drain_ms=2218 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=2170.1 enqueue_kbps=11323.96
  diag=s11-txdbg-reset size=256 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=256 ioctl_rc=0 user_calls=100 user_req=25600 user_acc=25600 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=256 ioctl_rc=0 user_calls=100 user_req=25600 user_acc=25600 ring_pop_calls=25 ring_pop_bytes=25600 hw_send_calls=1100670 hw_send_bytes=25600 hw_send_zero=1099070 hw_send_max_chunk=16 no_progress_budget=1599 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-derived size=256 submit_fraction=0.0010 producer_available=0.9990 total_time_ms=2220 enqueue_time_ms=2
  policy=no-drain size=1024 iters=100 bytes=102400 short_writes=0 enqueue_ms=3198 final_drain_ms=5680 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=8680.6 enqueue_kbps=31.27
  diag=s11-txdbg-reset size=1024 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=1024 ioctl_rc=0 user_calls=512 user_req=125263 user_acc=102400 ring_pop_calls=37 ring_pop_bytes=37789 hw_send_calls=1584140 hw_send_bytes=36861 hw_send_zero=1581836 hw_send_max_chunk=16 no_progress_budget=2304 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=1 staged_bytes=928 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=1024 ioctl_rc=0 user_calls=512 user_req=125263 user_acc=102400 ring_pop_calls=101 ring_pop_bytes=102400 hw_send_calls=4403858 hw_send_bytes=102400 hw_send_zero=4397457 hw_send_max_chunk=16 no_progress_budget=6400 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-derived size=1024 submit_fraction=0.3602 producer_available=0.6398 total_time_ms=8878 enqueue_time_ms=3198

=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===
  diag=S12 pre_section_stdout_drain_ms=99 drain_errors=0 last_errno=0
  policy=batch-drain size=64 iters=100 batch=8 drains=13 bytes=6400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=558 line_time_ms=542.5 kbps=11.20 line_rate_pct=97.2
  policy=batch-drain size=256 iters=100 batch=8 drains=13 bytes=25600 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=2238 line_time_ms=2170.1 kbps=11.17 line_rate_pct=96.9
  policy=batch-drain size=1024 iters=100 batch=8 drains=13 bytes=102400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=8898 line_time_ms=8680.6 kbps=11.24 line_rate_pct=97.6

=== [S13] TX writev Fragments (fragment aggregation witness) ===
  diag=S13 pre_section_stdout_drain_ms=22 drain_errors=0 last_errno=0
  policy=writev-drain-each fragments=4 fragment_size=64 total_size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2235 line_time_ms=2170.1 kbps=11.18 line_rate_pct=97.1

=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===
  diag=S14 pre_section_stdout_drain_ms=25 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=569 line_time_ms=542.5 kbps=10.97 line_rate_pct=95.2
  diag=break-even-size-64 n=100 avg_ms=5.696 p50_ms=5.695 p95_ms=5.698 p99_ms=5.711 max_ms=5.711 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.05 p99_p50_ratio=1.00 max_p50_ratio=1.00
  policy=drain-each size=128 iters=100 bytes=12800 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=1157 line_time_ms=1085.1 kbps=10.80 line_rate_pct=93.7
  diag=break-even-size-128 n=100 avg_ms=11.572 p50_ms=11.246 p95_ms=11.257 p99_ms=43.875 max_ms=43.875 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=4.04 p99_p50_ratio=3.90 max_p50_ratio=3.90
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2268 line_time_ms=2170.1 kbps=11.02 line_rate_pct=95.7
  diag=break-even-size-256 n=100 avg_ms=22.681 p50_ms=22.344 p95_ms=22.347 p99_ms=56.013 max_ms=56.013 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=2.58 p99_p50_ratio=2.51 max_p50_ratio=2.51

=== [S20] TX Latency (single byte + tcdrain) ===
  diag=S20 pre_section_stdout_drain_ms=38 drain_errors=0 last_errno=0
  diag=s20-single-byte n=100 avg_ms=0.189 p50_ms=0.190 p95_ms=0.191 p99_ms=0.220 max_ms=0.220 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=2.60 p99_p50_ratio=1.16 max_p50_ratio=1.16
  diag=S20 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S21] TX Latency FIFO Boundary Matrix ===
  diag=S21 pre_section_stdout_drain_ms=26 drain_errors=0 last_errno=0
  diag=s21-fifo-size-1 n=100 avg_ms=0.191 p50_ms=0.191 p95_ms=0.192 p99_ms=0.233 max_ms=0.233 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=2.75 p99_p50_ratio=1.22 max_p50_ratio=1.22
  diag=fifo-size-1 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-15 n=100 avg_ms=1.662 p50_ms=1.429 p95_ms=1.449 p99_ms=24.385 max_ms=24.385 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=19.18 p99_p50_ratio=17.07 max_p50_ratio=17.07
  diag=fifo-size-15 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-16 n=100 avg_ms=1.754 p50_ms=1.517 p95_ms=1.532 p99_ms=25.085 max_ms=25.085 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=18.49 p99_p50_ratio=16.54 max_p50_ratio=16.54
  diag=fifo-size-16 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-17 n=100 avg_ms=1.838 p50_ms=1.598 p95_ms=1.620 p99_ms=25.150 max_ms=25.150 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=17.45 p99_p50_ratio=15.74 max_p50_ratio=15.74
  diag=fifo-size-17 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-31 n=100 avg_ms=3.069 p50_ms=2.834 p95_ms=2.835 p99_ms=26.380 max_ms=26.380 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=10.04 p99_p50_ratio=9.31 max_p50_ratio=9.31
  diag=fifo-size-31 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-32 n=100 avg_ms=3.153 p50_ms=2.919 p95_ms=2.922 p99_ms=26.291 max_ms=26.291 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=9.69 p99_p50_ratio=9.01 max_p50_ratio=9.01
  diag=fifo-size-32 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-33 n=100 avg_ms=3.224 p50_ms=2.986 p95_ms=3.006 p99_ms=26.293 max_ms=26.293 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=9.40 p99_p50_ratio=8.81 max_p50_ratio=8.81
  diag=fifo-size-33 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-48 n=100 avg_ms=4.540 p50_ms=4.307 p95_ms=4.309 p99_ms=27.600 max_ms=27.600 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=6.78 p99_p50_ratio=6.41 max_p50_ratio=6.41
  diag=fifo-size-48 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-49 n=100 avg_ms=4.608 p50_ms=4.372 p95_ms=4.394 p99_ms=27.675 max_ms=27.675 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=6.66 p99_p50_ratio=6.33 max_p50_ratio=6.33
  diag=fifo-size-49 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S30] RX Empty Non-blocking Read (FIONBIO) ===
  diag=S30 pre_section_stdout_drain_ms=28 drain_errors=0 last_errno=0
  method=open-o-nonblock status=PASS result=EAGAIN
  method=ioctl-fionbio status=PASS result=EAGAIN

=== [S31] RX Fixed Payload Witness ===
  diag=S31 pre_section_stdout_drain_ms=12 drain_errors=0 last_errno=0
  status=SKIPPED reason=BENCH_RX_FIXED_BYTES=0

=== [S41] TX CPU Work (instret: write start → final TEMT drain, 5 rounds) ===
  diag=S41 pre_section_stdout_drain_ms=11 drain_errors=0 last_errno=0
  instret_read_overhead=16013
  s41-size=64 expected_bytes=6400 iters=100
  diag=s41-valid size=64 round=1 completed=6400 expected=6400 reset_rc=0 instret_begin=18385156725 instret_end=18595195728 instret_delta=210039003 instructions_per_byte=32818.59 instructions_per_write=2100390 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=556
  diag=s41-valid size=64 round=2 completed=6400 expected=6400 reset_rc=0 instret_begin=18605781465 instret_end=18815816459 instret_delta=210034994 instructions_per_byte=32817.97 instructions_per_write=2100350 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=556
  diag=s41-valid size=64 round=3 completed=6400 expected=6400 reset_rc=0 instret_begin=18826397287 instret_end=19036742997 instret_delta=210345710 instructions_per_byte=32866.52 instructions_per_write=2082631 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=101 duration_ms=556
  diag=s41-valid size=64 round=4 completed=6400 expected=6400 reset_rc=0 instret_begin=19047320445 instret_end=19257356156 instret_delta=210035711 instructions_per_byte=32818.08 instructions_per_write=2100357 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=556
  diag=s41-valid size=64 round=5 completed=6400 expected=6400 reset_rc=0 instret_begin=19267936105 instret_end=19477960282 instret_delta=210024177 instructions_per_byte=32816.28 instructions_per_write=2100242 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=556
  diag=s41-summary size=64 valid_rounds=5 median_instructions_per_byte=32818.08 median_instructions_per_write=2100350
  s41-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=1.1 hw_send_zero_per_kb=43874.6 ring_pop_bytes_per_kb=1024.0 no_progress_per_kb=63.8 bytes_per_hw_send=0.023 bytes_per_ring_pop=914.3 hw_send_calls=274616 hw_send_bytes=6400 hw_send_zero=274216 ring_pop_calls=7 ring_pop_bytes=6400 user_push_calls=103 user_push_acc=6841
  s41-size=256 expected_bytes=25600 iters=100
  diag=s41-valid size=256 round=1 completed=25600 expected=25600 reset_rc=0 instret_begin=19505435296 instret_end=20344914183 instret_delta=839478887 instructions_per_byte=32792.14 instructions_per_write=8394789 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=2221
  diag=s41-valid size=256 round=2 completed=25600 expected=25600 reset_rc=0 instret_begin=20355510270 instret_end=21195287518 instret_delta=839777248 instructions_per_byte=32803.80 instructions_per_write=8314626 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=101 duration_ms=2221
  diag=s41-valid size=256 round=3 completed=25600 expected=25600 reset_rc=0 instret_begin=21205883545 instret_end=22045364707 instret_delta=839481162 instructions_per_byte=32792.23 instructions_per_write=8394812 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=2221
  diag=s41-valid size=256 round=4 completed=25600 expected=25600 reset_rc=0 instret_begin=22055960751 instret_end=22895417590 instret_delta=839456839 instructions_per_byte=32791.28 instructions_per_write=8394568 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=2221
  diag=s41-valid size=256 round=5 completed=25600 expected=25600 reset_rc=0 instret_begin=22906013016 instret_end=23745826869 instret_delta=839813853 instructions_per_byte=32805.23 instructions_per_write=8314989 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=101 duration_ms=2221
  diag=s41-summary size=256 valid_rounds=5 median_instructions_per_byte=32792.23 median_instructions_per_write=8394568
  s41-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=1.0 hw_send_zero_per_kb=43980.8 ring_pop_bytes_per_kb=1024.0 no_progress_per_kb=64.0 bytes_per_hw_send=0.023 bytes_per_ring_pop=984.6 hw_send_calls=1101122 hw_send_bytes=25600 hw_send_zero=1099521 ring_pop_calls=26 ring_pop_bytes=25600 user_push_calls=104 user_push_acc=26046
  s41-size=1024 expected_bytes=102400 iters=100
  diag=s41-valid size=1024 round=1 completed=102400 expected=102400 reset_rc=0 instret_begin=23773822096 instret_end=28344801677 instret_delta=4570979581 instructions_per_byte=44638.47 instructions_per_write=36599 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=124894 duration_ms=11847
  diag=s41-valid size=1024 round=2 completed=102400 expected=102400 reset_rc=0 instret_begin=28355395422 instret_end=32945091052 instret_delta=4589695630 instructions_per_byte=44821.25 instructions_per_write=36101 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=127134 duration_ms=11890
  diag=s41-valid size=1024 round=3 completed=102400 expected=102400 reset_rc=0 instret_begin=32955683951 instret_end=37523430531 instret_delta=4567746580 instructions_per_byte=44606.90 instructions_per_write=36678 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=124538 duration_ms=11841
  diag=s41-valid size=1024 round=4 completed=102400 expected=102400 reset_rc=0 instret_begin=37534026758 instret_end=42119002732 instret_delta=4584975974 instructions_per_byte=44775.16 instructions_per_write=34316 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=133612 duration_ms=11939
  diag=s41-valid size=1024 round=5 completed=102400 expected=102400 reset_rc=0 instret_begin=42129595691 instret_end=46708470918 instret_delta=4578875227 instructions_per_byte=44715.58 instructions_per_write=36382 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=125854 duration_ms=11893
  diag=s41-summary size=1024 valid_rounds=5 median_instructions_per_byte=44715.58 median_instructions_per_write=36382
  s41-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=1.0 hw_send_zero_per_kb=43566.6 ring_pop_bytes_per_kb=1024.0 no_progress_per_kb=64.0 bytes_per_hw_send=0.023 bytes_per_ring_pop=1013.9 hw_send_calls=4363059 hw_send_bytes=102400 hw_send_zero=4356658 ring_pop_calls=101 ring_pop_bytes=102400 user_push_calls=412 user_push_acc=102851

=== [S42] TX Compute Overlap (64B x 100, fixed window, 5 sample rounds) ===
  diag=S42 pre_section_stdout_drain_ms=77 drain_errors=0 last_errno=0
  idle window_ms=542.535 window_ns=542534722 iters=272612 duration_ms=542.538 iters_per_sec=502475
  overlap payload=64 iters=100 warmup=1 sample_rounds=5 theoretical_line_time_ms=542.535
  diag=s42-sample round=1 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=1679.4 useful_iters=145932 useful_work_per_ms=174 final_drain_ms=298.190 total_duration_ms=840.727 total_over_line_ratio=1.550 overlap_efficiency=0.5353 reset_rc=0 drain_errors=0 leftover_ns=2445
  diag=s42-sample round=2 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=1584.5 useful_iters=145876 useful_work_per_ms=174 final_drain_ms=298.191 total_duration_ms=840.729 total_over_line_ratio=1.550 overlap_efficiency=0.5351 reset_rc=0 drain_errors=0 leftover_ns=3486
  diag=s42-sample round=3 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=1697.9 useful_iters=145535 useful_work_per_ms=174 final_drain_ms=248.172 total_duration_ms=838.392 total_over_line_ratio=1.545 overlap_efficiency=0.5339 reset_rc=0 drain_errors=0 leftover_ns=47685862
  diag=s42-sample round=4 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=1571.2 useful_iters=146362 useful_work_per_ms=174 final_drain_ms=298.190 total_duration_ms=840.728 total_over_line_ratio=1.550 overlap_efficiency=0.5369 reset_rc=0 drain_errors=0 leftover_ns=3236
  diag=s42-sample round=5 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=1660.8 useful_iters=146108 useful_work_per_ms=174 final_drain_ms=298.190 total_duration_ms=840.728 total_over_line_ratio=1.550 overlap_efficiency=0.5360 reset_rc=0 drain_errors=0 leftover_ns=3570
  diag=s42-summary valid_rounds=5 median_useful_iters=145932 median_total_duration_ms=840.728 median_overlap_efficiency=0.5353
  s42-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=1.1 hw_send_zero_per_kb=43373.6 ring_pop_bytes_per_kb=1024.0 no_progress_per_kb=63.8 bytes_per_hw_send=0.024 bytes_per_ring_pop=914.3 hw_send_calls=271485 hw_send_bytes=6400 hw_send_zero=271085 ring_pop_calls=7 ring_pop_bytes=6400 user_push_calls=103 user_push_acc=6835

=== [S43] Timer Wakeup Overshoot (5 idle groups + 5 loaded groups) ===
  diag=S43 pre_section_stdout_drain_ms=74 drain_errors=0 last_errno=0
  s43-phase=idle groups=5 samples=50 interval_us=5000
  diag=s43-idle-group group=1 status=PASS collected=50 errors=0 valid=50 duration_ms=254 sample[0]=9535000 sample[1]=4550833 sample[2]=9534375
  s43-idle-group-summary n=50 errors=0 p50_ns=9532750 p95_ns=9533792 p99_ns=9535000 max_ns=9535000
  diag=s43-idle-group group=2 status=PASS collected=50 errors=0 valid=50 duration_ms=259 sample[0]=15846084 sample[1]=10862000 sample[2]=5870334
  s43-idle-group-summary n=50 errors=0 p50_ns=9807959 p95_ns=9809625 p99_ns=15846084 max_ns=15846084
  diag=s43-idle-group group=3 status=PASS collected=50 errors=0 valid=50 duration_ms=259 sample[0]=15847000 sample[1]=10862209 sample[2]=5870375
  s43-idle-group-summary n=50 errors=0 p50_ns=9820584 p95_ns=9822209 p99_ns=15847000 max_ns=15847000
  diag=s43-idle-group group=4 status=PASS collected=50 errors=0 valid=50 duration_ms=259 sample[0]=15847250 sample[1]=10862917 sample[2]=5871584
  s43-idle-group-summary n=50 errors=0 p50_ns=9822167 p95_ns=9823667 p99_ns=15847250 max_ns=15847250
  diag=s43-idle-group group=5 status=PASS collected=50 errors=0 valid=50 duration_ms=259 sample[0]=15846667 sample[1]=10862500 sample[2]=5871625
  s43-idle-group-summary n=50 errors=0 p50_ns=9821000 p95_ns=9822209 p99_ns=15846667 max_ns=15846667
  s43-phase=loaded groups=5 burst_bytes=4096 theoretical_line_time_ns=347222222
  diag=s43-loaded-group group=1 status=PASS reset_rc=0 burst_written=4096 expected=4096 write_dur_ns=499334 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=290 drain_ms=64 drain_errors=0 sample[0]=35757292 sample[1]=30772959 sample[2]=25781625
  s43-loaded-group-summary n=50 errors=0 p50_ns=25781625 p95_ns=45756709 p99_ns=45757084 max_ns=45757084
  s43-loaded-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=1.0 hw_send_zero_per_kb=43738.2 ring_pop_bytes_per_kb=1024.0 no_progress_per_kb=63.8 bytes_per_hw_send=0.023 bytes_per_ring_pop=1024.0 hw_send_calls=175209 hw_send_bytes=4096 hw_send_zero=174953 ring_pop_calls=4 ring_pop_bytes=4096 user_push_calls=19 user_push_acc=4489
  diag=s43-loaded-group group=2 status=PASS reset_rc=0 burst_written=4096 expected=4096 write_dur_ns=517875 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=299 drain_ms=55 drain_errors=0 sample[0]=44628958 sample[1]=39644583 sample[2]=34652875
  s43-loaded-group-summary n=50 errors=0 p50_ns=29661000 p95_ns=49626000 p99_ns=49626625 max_ns=49626625
  s43-loaded-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=1.0 hw_send_zero_per_kb=43739.8 ring_pop_bytes_per_kb=1024.0 no_progress_per_kb=63.8 bytes_per_hw_send=0.023 bytes_per_ring_pop=1024.0 hw_send_calls=175215 hw_send_bytes=4096 hw_send_zero=174959 ring_pop_calls=4 ring_pop_bytes=4096 user_push_calls=19 user_push_acc=4489
  diag=s43-loaded-group group=3 status=PASS reset_rc=0 burst_written=4096 expected=4096 write_dur_ns=509292 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=297 drain_ms=57 drain_errors=0 sample[0]=43475708 sample[1]=38491458 sample[2]=33500375
  s43-loaded-group-summary n=50 errors=0 p50_ns=28508500 p95_ns=48473333 p99_ns=48473583 max_ns=48473583
  s43-loaded-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=1.0 hw_send_zero_per_kb=43740.2 ring_pop_bytes_per_kb=1024.0 no_progress_per_kb=63.8 bytes_per_hw_send=0.023 bytes_per_ring_pop=1024.0 hw_send_calls=175217 hw_send_bytes=4096 hw_send_zero=174961 ring_pop_calls=4 ring_pop_bytes=4096 user_push_calls=19 user_push_acc=4489
  diag=s43-loaded-group group=4 status=PASS reset_rc=0 burst_written=4096 expected=4096 write_dur_ns=525625 incomplete_logical=0 syscall_calls=17 collected=50 errors=0 valid=50 sample_duration_ms=296 drain_ms=58 drain_errors=0 sample[0]=42335625 sample[1]=37351459 sample[2]=32359792
  s43-loaded-group-summary n=50 errors=0 p50_ns=27368042 p95_ns=47332334 p99_ns=47332875 max_ns=47332875
  s43-loaded-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=1.2 hw_send_zero_per_kb=43810.0 ring_pop_bytes_per_kb=1024.0 no_progress_per_kb=64.0 bytes_per_hw_send=0.023 bytes_per_ring_pop=819.2 hw_send_calls=175497 hw_send_bytes=4096 hw_send_zero=175240 ring_pop_calls=5 ring_pop_bytes=4096 user_push_calls=20 user_push_acc=4489
  diag=s43-loaded-group group=5 status=PASS reset_rc=0 burst_written=4096 expected=4096 write_dur_ns=488583 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=295 drain_ms=59 drain_errors=0 sample[0]=41263375 sample[1]=36278958 sample[2]=31287375
  s43-loaded-group-summary n=50 errors=0 p50_ns=26295458 p95_ns=46260667 p99_ns=46261250 max_ns=46261250
  s43-loaded-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=1.0 hw_send_zero_per_kb=43741.0 ring_pop_bytes_per_kb=1024.0 no_progress_per_kb=63.8 bytes_per_hw_send=0.023 bytes_per_ring_pop=1024.0 hw_send_calls=175220 hw_send_bytes=4096 hw_send_zero=174964 ring_pop_calls=4 ring_pop_bytes=4096 user_push_calls=19 user_push_acc=4489
  diag=s43-loaded-aggregate n=250 valid_groups=5 p50_ns=25781625 p95_ns=47332334 p99_ns=49626000 max_ns=49626625
  diag=s43-idle-aggregate n=250 valid_groups=5 p50_ns=9532750 p95_ns=9823084 p99_ns=15846667 max_ns=15847250

=== [S40] TX Counter Proxy Summary ===
  telemetry_available=1 ioctl_rc=0
  counter=user-push user_calls=25 user_req=5118 user_acc=5118
  counter=ring-pop ring_pop_calls=4 ring_pop_bytes=4096
  counter=hw-send hw_send_calls=175220 hw_send_bytes=4096 hw_send_zero=174964 hw_send_max_chunk=16
  counter=no-progress no_progress_budget=255 slow_poll_exh=0 yield_exh=0
  counter=drain-state ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  proxy=derived bytes_per_user_call=204.7 bytes_per_ring_pop=1024.0 bytes_per_hw_send=0.023 zero_per_kb=43741.0 no_progress_per_kb=63.8

Done.
[starry-d1] benchmark exited with code: 0
[starry-d1] halting.

