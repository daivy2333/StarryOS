Boot at 1970-01-01 00:00:00.433000508 UTC

[starry-d1] Lichee D1 fullbench command-entry mode
[UART INIT] D1 MMIO base=0xffffffc002500000 stride=4 IER=0x0 IIR=0xc1 LSR=0x20
[UART INIT] D1 UART IRQ 18 registered=true, buffers=64KBx2
[UART INIT] async UART ready
[kernel] Async UART driver initialized (D1)
[BENCH] Running startup benchmark...
[BENCH] Ring buffer write: 1108647.45 KB/s (102400 bytes in 0.09 ms)
[BENCH] Hardware line rate: 11.52 KB/s (115200 bps)
[BENCH] FIFO depth: 16 bytes
[BENCH] Ring buffer: 64 KB × 2 = 128 KB total
[BENCH] Driver struct: 288 bytes
[BENCH] Total memory: 128 KB
[BENCH] NAPI threshold: 16 consecutive reads
[BENCH] NAPI batch size: 64 bytes
[BENCH] Copier buffer size: 1024 bytes
[BENCH] IRQ count: 42
[BENCH] IRQ frequency: 465631.93 IRQ/s
[BENCH] Running RX ring buffer throughput test...
[BENCH] RX ring buffer read: 8303061.75 KB/s (65536 bytes in 0.01 ms)
[BENCH] Running RX ring buffer latency test...
[BENCH] RX latency (n=100): min=82ns avg=103ns P50=82ns P95=123ns P99=328ns
[BENCH] Startup benchmark complete
[BENCH] Note: actual throughput limited by UART line rate (11.52 KB/s)
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

=== [S10] TX Throughput Baseline (write + tcdrain each iteration) ===
  diag=S10 pre_section_stdout_drain_ms=5476 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=560 line_time_ms=542.5 kbps=11.15 line_rate_pct=96.8
  diag=drain-each-size-64 n=100 avg_ms=5.602 p50_ms=5.597 p95_ms=5.622 p99_ms=5.632 max_ms=5.632 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.04 p99_p50_ratio=1.01 max_p50_ratio=1.01
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2230 line_time_ms=2170.1 kbps=11.21 line_rate_pct=97.3
  diag=drain-each-size-256 n=100 avg_ms=22.304 p50_ms=21.981 p95_ms=22.005 p99_ms=54.103 max_ms=54.103 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=2.49 p99_p50_ratio=2.46 max_p50_ratio=2.46
  policy=drain-each size=1024 iters=100 bytes=102400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=8786 line_time_ms=8680.6 kbps=11.38 line_rate_pct=98.8
  diag=drain-each-size-1024 n=100 avg_ms=87.857 p50_ms=87.524 p95_ms=87.535 p99_ms=120.660 max_ms=120.660 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=1.39 p99_p50_ratio=1.38 max_p50_ratio=1.38

=== [S11] TX Enqueue Cost (write loop, final drain outside timing) ===
  diag=S11 pre_section_stdout_drain_ms=40 drain_errors=0 last_errno=0
  policy=no-drain size=64 iters=100 bytes=6400 short_writes=0 enqueue_ms=1 final_drain_ms=545 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=542.5 enqueue_kbps=4683.51
  diag=s11-txdbg-reset size=64 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=64 ioctl_rc=0 user_calls=100 user_req=6400 user_acc=6400 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=64 ioctl_rc=0 user_calls=100 user_req=6400 user_acc=6400 ring_pop_calls=7 ring_pop_bytes=6400 hw_send_calls=274435 hw_send_bytes=6400 hw_send_zero=274035 hw_send_max_chunk=16 no_progress_budget=399 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  policy=no-drain size=256 iters=100 bytes=25600 short_writes=0 enqueue_ms=1 final_drain_ms=2183 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=2170.1 enqueue_kbps=13009.24
  diag=s11-txdbg-reset size=256 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=256 ioctl_rc=0 user_calls=100 user_req=25600 user_acc=25600 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=256 ioctl_rc=0 user_calls=100 user_req=25600 user_acc=25600 ring_pop_calls=25 ring_pop_bytes=25600 hw_send_calls=1099866 hw_send_bytes=25600 hw_send_zero=1098266 hw_send_max_chunk=16 no_progress_budget=1599 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  policy=no-drain size=1024 iters=100 bytes=102400 short_writes=0 enqueue_ms=3101 final_drain_ms=5634 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=8680.6 enqueue_kbps=32.24
  diag=s11-txdbg-reset size=1024 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=1024 ioctl_rc=0 user_calls=512 user_req=124544 user_acc=102400 ring_pop_calls=37 ring_pop_bytes=37312 hw_send_calls=1560415 hw_send_bytes=36336 hw_send_zero=1558144 hw_send_max_chunk=16 no_progress_budget=2271 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=1 staged_bytes=976 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=1024 ioctl_rc=0 user_calls=512 user_req=124544 user_acc=102400 ring_pop_calls=101 ring_pop_bytes=102400 hw_send_calls=4400033 hw_send_bytes=102400 hw_send_zero=4393633 hw_send_max_chunk=16 no_progress_budget=6399 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1

=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===
  diag=S12 pre_section_stdout_drain_ms=88 drain_errors=0 last_errno=0
  policy=batch-drain size=64 iters=100 batch=8 drains=13 bytes=6400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=548 line_time_ms=542.5 kbps=11.39 line_rate_pct=98.8
  policy=batch-drain size=256 iters=100 batch=8 drains=13 bytes=25600 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=2202 line_time_ms=2170.1 kbps=11.35 line_rate_pct=98.5
  policy=batch-drain size=1024 iters=100 batch=8 drains=13 bytes=102400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=8755 line_time_ms=8680.6 kbps=11.42 line_rate_pct=99.1

=== [S13] TX writev Fragments (fragment aggregation witness) ===
  diag=S13 pre_section_stdout_drain_ms=22 drain_errors=0 last_errno=0
  policy=writev-drain-each fragments=4 fragment_size=64 total_size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2199 line_time_ms=2170.1 kbps=11.36 line_rate_pct=98.7

=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===
  diag=S14 pre_section_stdout_drain_ms=25 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=560 line_time_ms=542.5 kbps=11.15 line_rate_pct=96.8
  diag=break-even-size-64 n=100 avg_ms=5.604 p50_ms=5.600 p95_ms=5.625 p99_ms=5.627 max_ms=5.627 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.04 p99_p50_ratio=1.00 max_p50_ratio=1.00
  policy=drain-each size=128 iters=100 bytes=12800 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=1138 line_time_ms=1085.1 kbps=10.98 line_rate_pct=95.3
  diag=break-even-size-128 n=100 avg_ms=11.385 p50_ms=11.059 p95_ms=11.085 p99_ms=43.183 max_ms=43.183 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=3.98 p99_p50_ratio=3.90 max_p50_ratio=3.90
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2232 line_time_ms=2170.1 kbps=11.20 line_rate_pct=97.2
  diag=break-even-size-256 n=100 avg_ms=22.319 p50_ms=21.981 p95_ms=22.007 p99_ms=55.128 max_ms=55.128 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=2.54 p99_p50_ratio=2.51 max_p50_ratio=2.51

=== [S20] TX Latency (single byte + tcdrain) ===
  diag=S20 pre_section_stdout_drain_ms=37 drain_errors=0 last_errno=0
  diag=s20-single-byte n=100 avg_ms=0.187 p50_ms=0.187 p95_ms=0.189 p99_ms=0.223 max_ms=0.223 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=2.63 p99_p50_ratio=1.19 max_p50_ratio=1.19
  diag=S20 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S21] TX Latency FIFO Boundary Matrix ===
  diag=S21 pre_section_stdout_drain_ms=26 drain_errors=0 last_errno=0
  diag=s21-fifo-size-1 n=100 avg_ms=0.188 p50_ms=0.187 p95_ms=0.189 p99_ms=0.229 max_ms=0.229 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=2.70 p99_p50_ratio=1.22 max_p50_ratio=1.22
  diag=fifo-size-1 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-15 n=100 avg_ms=1.632 p50_ms=1.406 p95_ms=1.415 p99_ms=23.985 max_ms=23.985 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=18.86 p99_p50_ratio=17.06 max_p50_ratio=17.06
  diag=fifo-size-15 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-16 n=100 avg_ms=1.723 p50_ms=1.491 p95_ms=1.499 p99_ms=24.662 max_ms=24.662 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=18.18 p99_p50_ratio=16.54 max_p50_ratio=16.54
  diag=fifo-size-16 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-17 n=100 avg_ms=1.801 p50_ms=1.569 p95_ms=1.579 p99_ms=24.746 max_ms=24.746 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=17.17 p99_p50_ratio=15.77 max_p50_ratio=15.77
  diag=fifo-size-17 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-31 n=100 avg_ms=3.022 p50_ms=2.789 p95_ms=2.807 p99_ms=25.940 max_ms=25.940 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=9.87 p99_p50_ratio=9.30 max_p50_ratio=9.30
  diag=fifo-size-31 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-32 n=100 avg_ms=3.105 p50_ms=2.868 p95_ms=2.893 p99_ms=25.767 max_ms=25.767 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=9.50 p99_p50_ratio=8.99 max_p50_ratio=8.99
  diag=fifo-size-32 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-33 n=100 avg_ms=3.166 p50_ms=2.937 p95_ms=2.944 p99_ms=25.856 max_ms=25.856 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=9.24 p99_p50_ratio=8.80 max_p50_ratio=8.80
  diag=fifo-size-33 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-48 n=100 avg_ms=4.469 p50_ms=4.232 p95_ms=4.258 p99_ms=27.136 max_ms=27.136 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=6.67 p99_p50_ratio=6.41 max_p50_ratio=6.41
  diag=fifo-size-48 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-49 n=100 avg_ms=4.532 p50_ms=4.303 p95_ms=4.309 p99_ms=27.217 max_ms=27.217 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=6.55 p99_p50_ratio=6.33 max_p50_ratio=6.33
  diag=fifo-size-49 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S30] RX Empty Non-blocking Read (FIONBIO) ===
  diag=S30 pre_section_stdout_drain_ms=27 drain_errors=0 last_errno=0
  method=open-o-nonblock status=PASS result=EAGAIN
  method=ioctl-fionbio status=PASS result=EAGAIN

=== [S31] RX Fixed Payload Witness ===
  diag=S31 pre_section_stdout_drain_ms=12 drain_errors=0 last_errno=0
  status=SKIPPED reason=BENCH_RX_FIXED_BYTES=0

=== [S40] TX Counter Proxy Summary ===
  telemetry_available=1 ioctl_rc=0
  counter=user-push user_calls=2577 user_req=360536 user_acc=338201
  counter=ring-pop ring_pop_calls=1659 ring_pop_bytes=338108
  counter=hw-send hw_send_calls=13841840 hw_send_bytes=338108 hw_send_zero=13820215 hw_send_max_chunk=16
  counter=no-progress no_progress_budget=20171 slow_poll_exh=0 yield_exh=0
  counter=drain-state ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  proxy=derived bytes_per_user_call=131.2 bytes_per_ring_pop=203.8 bytes_per_hw_send=0.024 zero_per_kb=41856.2 no_progress_per_kb=61.1

Done.
[starry-d1] benchmark exited with code: 0
[starry-d1] halting.
