Boot at 1970-01-01 00:00:00.433326212 UTC

[starry-d1] Lichee D1 fullbench command-entry mode
[UART INIT] D1 MMIO base=0xffffffc002500000 stride=4 IER=0x0 IIR=0xc1 LSR=0x20
[UART INIT] D1 UART IRQ 18 registered=true, buffers=64KBx2
[UART INIT] async UART hardware initialized (copiers not started yet)
[kernel] Async UART driver initialized (D1)
[BENCH] Running startup benchmark...
[BENCH] Ring buffer write: 738399.06 KB/s (65536 bytes in 0.09 ms)
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
[BENCH] RX ring buffer read: 8259130.21 KB/s (65536 bytes in 0.01 ms)
[BENCH] Running RX ring buffer latency test...
[BENCH] RX latency (n=100): min=82ns avg=101ns P50=82ns P95=123ns P99=123ns
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
  diag=S10 pre_section_stdout_drain_ms=5579 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=560 line_time_ms=542.5 kbps=11.16 line_rate_pct=96.9
  diag=drain-each-size-64 n=100 avg_ms=5.597 p50_ms=5.596 p95_ms=5.600 p99_ms=5.611 max_ms=5.611 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.03 p99_p50_ratio=1.00 max_p50_ratio=1.00
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2230 line_time_ms=2170.1 kbps=11.21 line_rate_pct=97.3
  diag=drain-each-size-256 n=100 avg_ms=22.301 p50_ms=21.980 p95_ms=21.984 p99_ms=54.105 max_ms=54.105 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=2.49 p99_p50_ratio=2.46 max_p50_ratio=2.46
  policy=drain-each size=1024 iters=100 bytes=102400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=8787 line_time_ms=8680.6 kbps=11.38 line_rate_pct=98.8
  diag=drain-each-size-1024 n=100 avg_ms=87.873 p50_ms=87.542 p95_ms=87.552 p99_ms=120.667 max_ms=120.667 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=1.39 p99_p50_ratio=1.38 max_p50_ratio=1.38

=== [S11] TX Enqueue Cost (write loop, final drain outside timing) ===
  diag=S11 pre_section_stdout_drain_ms=40 drain_errors=0 last_errno=0
  policy=no-drain size=64 iters=100 bytes=6400 short_writes=0 enqueue_ms=1 final_drain_ms=545 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=542.5 enqueue_kbps=4282.12
  diag=s11-txdbg-reset size=64 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=64 ioctl_rc=0 user_calls=100 user_req=6400 user_acc=6400 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=64 ioctl_rc=0 user_calls=100 user_req=6400 user_acc=6400 ring_pop_calls=7 ring_pop_bytes=6400 hw_send_calls=274278 hw_send_bytes=6400 hw_send_zero=273878 hw_send_max_chunk=16 no_progress_budget=399 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  policy=no-drain size=256 iters=100 bytes=25600 short_writes=0 enqueue_ms=2 final_drain_ms=2183 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=2170.1 enqueue_kbps=12171.27
  diag=s11-txdbg-reset size=256 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=256 ioctl_rc=0 user_calls=100 user_req=25600 user_acc=25600 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=256 ioctl_rc=0 user_calls=100 user_req=25600 user_acc=25600 ring_pop_calls=25 ring_pop_bytes=25600 hw_send_calls=1099234 hw_send_bytes=25600 hw_send_zero=1097634 hw_send_max_chunk=16 no_progress_budget=1599 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  policy=no-drain size=1024 iters=100 bytes=102400 short_writes=0 enqueue_ms=3097 final_drain_ms=5639 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=8680.6 enqueue_kbps=32.29
  diag=s11-txdbg-reset size=1024 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=1024 ioctl_rc=0 user_calls=512 user_req=124544 user_acc=102400 ring_pop_calls=37 ring_pop_bytes=37312 hw_send_calls=1557317 hw_send_bytes=36288 hw_send_zero=1555049 hw_send_max_chunk=16 no_progress_budget=2268 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=1 staged_bytes=1024 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=1024 ioctl_rc=0 user_calls=512 user_req=124544 user_acc=102400 ring_pop_calls=101 ring_pop_bytes=102400 hw_send_calls=4397961 hw_send_bytes=102400 hw_send_zero=4391561 hw_send_max_chunk=16 no_progress_budget=6399 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1

=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===
  diag=S12 pre_section_stdout_drain_ms=88 drain_errors=0 last_errno=0
  policy=batch-drain size=64 iters=100 batch=8 drains=13 bytes=6400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=548 line_time_ms=542.5 kbps=11.39 line_rate_pct=98.8
  policy=batch-drain size=256 iters=100 batch=8 drains=13 bytes=25600 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=2202 line_time_ms=2170.1 kbps=11.35 line_rate_pct=98.5
  policy=batch-drain size=1024 iters=100 batch=8 drains=13 bytes=102400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=8756 line_time_ms=8680.6 kbps=11.42 line_rate_pct=99.1

=== [S13] TX writev Fragments (fragment aggregation witness) ===
  diag=S13 pre_section_stdout_drain_ms=22 drain_errors=0 last_errno=0
  policy=writev-drain-each fragments=4 fragment_size=64 total_size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2198 line_time_ms=2170.1 kbps=11.37 line_rate_pct=98.7

=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===
  diag=S14 pre_section_stdout_drain_ms=25 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=560 line_time_ms=542.5 kbps=11.16 line_rate_pct=96.9
  diag=break-even-size-64 n=100 avg_ms=5.596 p50_ms=5.596 p95_ms=5.601 p99_ms=5.606 max_ms=5.606 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.03 p99_p50_ratio=1.00 max_p50_ratio=1.00
  policy=drain-each size=128 iters=100 bytes=12800 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=1138 line_time_ms=1085.1 kbps=10.98 line_rate_pct=95.3
  diag=break-even-size-128 n=100 avg_ms=11.379 p50_ms=11.058 p95_ms=11.060 p99_ms=43.175 max_ms=43.175 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=3.98 p99_p50_ratio=3.90 max_p50_ratio=3.90
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2231 line_time_ms=2170.1 kbps=11.20 line_rate_pct=97.2
  diag=break-even-size-256 n=100 avg_ms=22.312 p50_ms=21.980 p95_ms=21.991 p99_ms=55.131 max_ms=55.131 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=2.54 p99_p50_ratio=2.51 max_p50_ratio=2.51

=== [S20] TX Latency (single byte + tcdrain) ===
  diag=S20 pre_section_stdout_drain_ms=37 drain_errors=0 last_errno=0
  diag=s20-single-byte n=100 avg_ms=0.191 p50_ms=0.191 p95_ms=0.193 p99_ms=0.223 max_ms=0.223 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=2.64 p99_p50_ratio=1.17 max_p50_ratio=1.17
  diag=S20 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S21] TX Latency FIFO Boundary Matrix ===
  diag=S21 pre_section_stdout_drain_ms=26 drain_errors=0 last_errno=0
  diag=s21-fifo-size-1 n=100 avg_ms=0.191 p50_ms=0.190 p95_ms=0.192 p99_ms=0.228 max_ms=0.228 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=2.69 p99_p50_ratio=1.20 max_p50_ratio=1.20
  diag=fifo-size-1 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-15 n=100 avg_ms=1.737 p50_ms=1.410 p95_ms=1.419 p99_ms=23.988 max_ms=23.988 slow_gt10ms=2 slow_over_line_plus10ms=2 max_line_ratio=18.86 p99_p50_ratio=17.01 max_p50_ratio=17.01
  diag=fifo-size-15 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-16 n=100 avg_ms=1.728 p50_ms=1.494 p95_ms=1.505 p99_ms=24.662 max_ms=24.662 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=18.18 p99_p50_ratio=16.51 max_p50_ratio=16.51
  diag=fifo-size-16 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-17 n=100 avg_ms=1.805 p50_ms=1.572 p95_ms=1.582 p99_ms=24.743 max_ms=24.743 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=17.17 p99_p50_ratio=15.74 max_p50_ratio=15.74
  diag=fifo-size-17 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-31 n=100 avg_ms=3.010 p50_ms=2.779 p95_ms=2.783 p99_ms=25.946 max_ms=25.946 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=9.87 p99_p50_ratio=9.34 max_p50_ratio=9.34
  diag=fifo-size-31 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-32 n=100 avg_ms=3.093 p50_ms=2.864 p95_ms=2.867 p99_ms=25.772 max_ms=25.772 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=9.50 p99_p50_ratio=9.00 max_p50_ratio=9.00
  diag=fifo-size-32 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-33 n=100 avg_ms=3.168 p50_ms=2.936 p95_ms=2.953 p99_ms=25.857 max_ms=25.857 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=9.24 p99_p50_ratio=8.81 max_p50_ratio=8.81
  diag=fifo-size-33 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-48 n=100 avg_ms=4.459 p50_ms=4.230 p95_ms=4.234 p99_ms=27.144 max_ms=27.144 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=6.67 p99_p50_ratio=6.42 max_p50_ratio=6.42
  diag=fifo-size-48 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-49 n=100 avg_ms=4.538 p50_ms=4.309 p95_ms=4.321 p99_ms=27.220 max_ms=27.220 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=6.55 p99_p50_ratio=6.32 max_p50_ratio=6.32
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
  counter=user-push user_calls=2577 user_req=360540 user_acc=338202
  counter=ring-pop ring_pop_calls=1659 ring_pop_bytes=338109
  counter=hw-send hw_send_calls=13834811 hw_send_bytes=338109 hw_send_zero=13813186 hw_send_max_chunk=16
  counter=no-progress no_progress_budget=20171 slow_poll_exh=0 yield_exh=0
  counter=drain-state ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  proxy=derived bytes_per_user_call=131.2 bytes_per_ring_pop=203.8 bytes_per_hw_send=0.024 zero_per_kb=41834.7 no_progress_per_kb=61.1

Done.
[starry-d1] benchmark exited with code: 0
[starry-d1] halting.

