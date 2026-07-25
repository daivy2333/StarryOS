Boot at 1970-01-01 00:00:00.433100261 UTC

[starry-d1] Lichee D1 fullbench command-entry mode
[UART INIT] D1 MMIO base=0xffffffc002500000 stride=4 IER=0x0 IIR=0xc1 LSR=0x20
[UART INIT] D1 UART IRQ 18 registered=true, buffers=64KBx2
[UART INIT] async UART hardware initialized (copiers not started yet)
[kernel] Async UART driver initialized (D1)
[BENCH] Running startup benchmark...
[BENCH] Ring buffer write: 718020.06 KB/s (65536 bytes in 0.09 ms)
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
[BENCH] RX ring buffer read: 8303061.75 KB/s (65536 bytes in 0.01 ms)
[BENCH] Running RX ring buffer latency test...
[BENCH] RX latency (n=100): min=82ns avg=100ns P50=82ns P95=123ns P99=123ns
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
  diag=S10 pre_section_stdout_drain_ms=5580 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=560 line_time_ms=542.5 kbps=11.15 line_rate_pct=96.8
  diag=drain-each-size-64 n=100 avg_ms=5.600 p50_ms=5.600 p95_ms=5.606 p99_ms=5.615 max_ms=5.615 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.03 p99_p50_ratio=1.00 max_p50_ratio=1.00
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2230 line_time_ms=2170.1 kbps=11.21 line_rate_pct=97.3
  diag=drain-each-size-256 n=100 avg_ms=22.304 p50_ms=21.983 p95_ms=21.994 p99_ms=54.095 max_ms=54.095 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=2.49 p99_p50_ratio=2.46 max_p50_ratio=2.46
  policy=drain-each size=1024 iters=100 bytes=102400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=8787 line_time_ms=8680.6 kbps=11.38 line_rate_pct=98.8
  diag=drain-each-size-1024 n=100 avg_ms=87.874 p50_ms=87.545 p95_ms=87.555 p99_ms=120.651 max_ms=120.651 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=1.39 p99_p50_ratio=1.38 max_p50_ratio=1.38

=== [S11] TX Enqueue Cost (write loop, final drain outside timing) ===
  diag=S11 pre_section_stdout_drain_ms=40 drain_errors=0 last_errno=0
  policy=no-drain size=64 iters=100 bytes=6400 short_writes=0 enqueue_ms=1 final_drain_ms=545 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=542.5 enqueue_kbps=3997.04
  diag=s11-txdbg-reset size=64 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=64 ioctl_rc=0 user_calls=100 user_req=6400 user_acc=6400 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=64 ioctl_rc=0 user_calls=100 user_req=6400 user_acc=6400 ring_pop_calls=7 ring_pop_bytes=6400 hw_send_calls=274448 hw_send_bytes=6400 hw_send_zero=274048 hw_send_max_chunk=16 no_progress_budget=399 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  policy=no-drain size=256 iters=100 bytes=25600 short_writes=0 enqueue_ms=2 final_drain_ms=2183 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=2170.1 enqueue_kbps=11280.08
  diag=s11-txdbg-reset size=256 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=256 ioctl_rc=0 user_calls=100 user_req=25600 user_acc=25600 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=256 ioctl_rc=0 user_calls=100 user_req=25600 user_acc=25600 ring_pop_calls=25 ring_pop_bytes=25600 hw_send_calls=1099928 hw_send_bytes=25600 hw_send_zero=1098328 hw_send_max_chunk=16 no_progress_budget=1599 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  policy=no-drain size=1024 iters=100 bytes=102400 short_writes=0 enqueue_ms=3146 final_drain_ms=5590 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=8680.6 enqueue_kbps=31.79
  diag=s11-txdbg-reset size=1024 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=1024 ioctl_rc=0 user_calls=512 user_req=124544 user_acc=102400 ring_pop_calls=37 ring_pop_bytes=37312 hw_send_calls=1582708 hw_send_bytes=36864 hw_send_zero=1580404 hw_send_max_chunk=16 no_progress_budget=2304 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=1 staged_bytes=448 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=1024 ioctl_rc=0 user_calls=512 user_req=124544 user_acc=102400 ring_pop_calls=101 ring_pop_bytes=102400 hw_send_calls=4400134 hw_send_bytes=102400 hw_send_zero=4393734 hw_send_max_chunk=16 no_progress_budget=6399 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1

=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===
  diag=S12 pre_section_stdout_drain_ms=88 drain_errors=0 last_errno=0
  policy=batch-drain size=64 iters=100 batch=8 drains=13 bytes=6400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=549 line_time_ms=542.5 kbps=11.38 line_rate_pct=98.8
  policy=batch-drain size=256 iters=100 batch=8 drains=13 bytes=25600 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=2202 line_time_ms=2170.1 kbps=11.35 line_rate_pct=98.5
  policy=batch-drain size=1024 iters=100 batch=8 drains=13 bytes=102400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=8756 line_time_ms=8680.6 kbps=11.42 line_rate_pct=99.1

=== [S13] TX writev Fragments (fragment aggregation witness) ===
  diag=S13 pre_section_stdout_drain_ms=22 drain_errors=0 last_errno=0
  policy=writev-drain-each fragments=4 fragment_size=64 total_size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2199 line_time_ms=2170.1 kbps=11.37 line_rate_pct=98.7

=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===
  diag=S14 pre_section_stdout_drain_ms=25 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=560 line_time_ms=542.5 kbps=11.15 line_rate_pct=96.8
  diag=break-even-size-64 n=100 avg_ms=5.600 p50_ms=5.599 p95_ms=5.608 p99_ms=5.618 max_ms=5.618 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.04 p99_p50_ratio=1.00 max_p50_ratio=1.00
  policy=drain-each size=128 iters=100 bytes=12800 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=1138 line_time_ms=1085.1 kbps=10.98 line_rate_pct=95.3
  diag=break-even-size-128 n=100 avg_ms=11.382 p50_ms=11.061 p95_ms=11.069 p99_ms=43.175 max_ms=43.175 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=3.98 p99_p50_ratio=3.90 max_p50_ratio=3.90
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2231 line_time_ms=2170.1 kbps=11.20 line_rate_pct=97.2
  diag=break-even-size-256 n=100 avg_ms=22.316 p50_ms=21.983 p95_ms=21.998 p99_ms=55.116 max_ms=55.116 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=2.54 p99_p50_ratio=2.51 max_p50_ratio=2.51

=== [S20] TX Latency (single byte + tcdrain) ===
  diag=S20 pre_section_stdout_drain_ms=37 drain_errors=0 last_errno=0
  diag=s20-single-byte n=100 avg_ms=0.192 p50_ms=0.191 p95_ms=0.193 p99_ms=0.238 max_ms=0.238 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=2.81 p99_p50_ratio=1.25 max_p50_ratio=1.25
  diag=S20 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S21] TX Latency FIFO Boundary Matrix ===
  diag=S21 pre_section_stdout_drain_ms=26 drain_errors=0 last_errno=0
  diag=s21-fifo-size-1 n=100 avg_ms=0.193 p50_ms=0.193 p95_ms=0.198 p99_ms=0.233 max_ms=0.233 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=2.75 p99_p50_ratio=1.21 max_p50_ratio=1.21
  diag=fifo-size-1 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-15 n=100 avg_ms=1.646 p50_ms=1.421 p95_ms=1.424 p99_ms=23.992 max_ms=23.992 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=18.87 p99_p50_ratio=16.89 max_p50_ratio=16.89
  diag=fifo-size-15 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-16 n=100 avg_ms=1.738 p50_ms=1.507 p95_ms=1.509 p99_ms=24.676 max_ms=24.676 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=18.19 p99_p50_ratio=16.38 max_p50_ratio=16.38
  diag=fifo-size-16 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-17 n=100 avg_ms=1.816 p50_ms=1.585 p95_ms=1.588 p99_ms=24.758 max_ms=24.758 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=17.18 p99_p50_ratio=15.62 max_p50_ratio=15.62
  diag=fifo-size-17 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-31 n=100 avg_ms=3.015 p50_ms=2.783 p95_ms=2.789 p99_ms=25.966 max_ms=25.966 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=9.88 p99_p50_ratio=9.33 max_p50_ratio=9.33
  diag=fifo-size-31 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-32 n=100 avg_ms=3.097 p50_ms=2.868 p95_ms=2.874 p99_ms=25.784 max_ms=25.784 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=9.51 p99_p50_ratio=8.99 max_p50_ratio=8.99
  diag=fifo-size-32 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-33 n=100 avg_ms=3.172 p50_ms=2.941 p95_ms=2.954 p99_ms=25.872 max_ms=25.872 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=9.25 p99_p50_ratio=8.80 max_p50_ratio=8.80
  diag=fifo-size-33 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-48 n=100 avg_ms=4.462 p50_ms=4.233 p95_ms=4.239 p99_ms=27.153 max_ms=27.153 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=6.67 p99_p50_ratio=6.41 max_p50_ratio=6.41
  diag=fifo-size-48 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-49 n=100 avg_ms=4.541 p50_ms=4.311 p95_ms=4.322 p99_ms=27.240 max_ms=27.240 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=6.56 p99_p50_ratio=6.32 max_p50_ratio=6.32
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
  counter=hw-send hw_send_calls=13842121 hw_send_bytes=338108 hw_send_zero=13820496 hw_send_max_chunk=16
  counter=no-progress no_progress_budget=20171 slow_poll_exh=0 yield_exh=0
  counter=drain-state ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  proxy=derived bytes_per_user_call=131.2 bytes_per_ring_pop=203.8 bytes_per_hw_send=0.024 zero_per_kb=41857.0 no_progress_per_kb=61.1

Done.
[starry-d1] benchmark exited with code: 0
[starry-d1] halting.

