Boot at 1970-01-01 00:00:00.432471198 UTC

[starry-d1] Lichee D1 userbench mode
[UART INIT] D1 MMIO base=0xffffffc002500000 stride=4 IER=0x0 IIR=0xc1 LSR=0x20
[UART INIT] D1 UART IRQ 18 registered=true, buffers=64KBx2
[UART INIT] async UART ready
[kernel] Async UART driver initialized (D1)
[BENCH] Running startup benchmark...
[BENCH] Ring buffer write: 1148316.57 KB/s (102400 bytes in 0.09 ms)
[BENCH] Hardware line rate: 11.52 KB/s (115200 bps)
[BENCH] FIFO depth: 16 bytes
[BENCH] Ring buffer: 64 KB × 2 = 128 KB total
[BENCH] Driver struct: 256 bytes
[BENCH] Total memory: 128 KB
[BENCH] NAPI threshold: 16 consecutive reads
[BENCH] NAPI batch size: 64 bytes
[BENCH] Copier buffer size: 1024 bytes
[BENCH] IRQ count: 6
[BENCH] IRQ frequency: 68898.99 IRQ/s
[BENCH] Running RX ring buffer throughput test...
[BENCH] RX ring buffer read: 8303061.75 KB/s (65536 bytes in 0.01 ms)
[BENCH] Running RX ring buffer latency test...
[BENCH] RX latency (n=100): min=82ns avg=103ns P50=123ns P95=123ns P99=123ns
[BENCH] Startup benchmark complete
[BENCH] Note: actual throughput limited by UART line rate (11.52 KB/s)
[starry-d1] Initializing memory rootfs...
[starry-d1] Loading embedded benchmark payload...
[starry-d1] benchmark process spawned, waiting...
UART Async Benchmark
====================

=== [S00] Benchmark Manifest ===
  version=q19c-m0-20260703
  target_mode=lichee-d1-userbench
  startup_chain=android-boot-image -> embedded benchmark.elf
  root_provider=d1-memory-root-embedded-payload
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
  diag=S10 pre_section_stdout_drain_ms=5668 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=561 line_time_ms=542.5 kbps=11.14 line_rate_pct=96.7
  diag=drain-each-size-64 n=100 avg_ms=5.610 p50_ms=5.610 p95_ms=5.612 p99_ms=5.621 max_ms=5.621 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.04
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2229 line_time_ms=2170.1 kbps=11.22 line_rate_pct=97.4
  diag=drain-each-size-256 n=100 avg_ms=22.288 p50_ms=21.999 p95_ms=22.001 p99_ms=50.872 max_ms=50.872 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=2.34
  policy=drain-each size=1024 iters=100 bytes=102400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=8805 line_time_ms=8680.6 kbps=11.36 line_rate_pct=98.6
  diag=drain-each-size-1024 n=100 avg_ms=88.056 p50_ms=87.558 p95_ms=87.587 p99_ms=117.447 max_ms=117.447 slow_gt10ms=100 slow_over_line_plus10ms=3 max_line_ratio=1.35
  policy=drain-each-recheck size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=591 line_time_ms=542.5 kbps=10.57 line_rate_pct=91.7
  diag=drain-each-recheck-size-64 n=100 avg_ms=5.911 p50_ms=5.608 p95_ms=5.611 p99_ms=35.932 max_ms=35.932 slow_gt10ms=1 slow_over_line_plus10ms=1 max_line_ratio=6.62

=== [S11] TX Enqueue Cost (write loop, final drain outside timing) ===
  diag=S11 pre_section_stdout_drain_ms=37 drain_errors=0 last_errno=0
  policy=no-drain size=64 iters=100 bytes=6400 short_writes=0 enqueue_ms=1 final_drain_ms=545 second_drain_ms=0 final_drain_rc=0 final_drain_errno=0 second_drain_rc=0 second_drain_errno=0 drain_calls=2 drain_errors=0 last_drain_errno=0 line_time_ms=542.5 enqueue_kbps=4368.88
  diag=s11-txdbg-reset size=64 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=64 ioctl_rc=0 user_calls=100 user_req=6400 user_acc=6400 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=64 ioctl_rc=0 user_calls=100 user_req=6400 user_acc=6400 ring_pop_calls=7 ring_pop_bytes=6400 hw_send_calls=13966 hw_send_bytes=6400 hw_send_zero=13566 hw_send_max_chunk=16 no_progress_budget=399 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=second-drain size=64 ioctl_rc=0 user_calls=100 user_req=6400 user_acc=6400 ring_pop_calls=7 ring_pop_bytes=6400 hw_send_calls=13966 hw_send_bytes=6400 hw_send_zero=13566 hw_send_max_chunk=16 no_progress_budget=399 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  policy=no-drain size=256 iters=100 bytes=25600 short_writes=0 enqueue_ms=3 final_drain_ms=2183 second_drain_ms=0 final_drain_rc=0 final_drain_errno=0 second_drain_rc=0 second_drain_errno=0 drain_calls=2 drain_errors=0 last_drain_errno=0 line_time_ms=2170.1 enqueue_kbps=8297.24
  diag=s11-txdbg-reset size=256 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=256 ioctl_rc=0 user_calls=100 user_req=25600 user_acc=25600 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=256 ioctl_rc=0 user_calls=100 user_req=25600 user_acc=25600 ring_pop_calls=25 ring_pop_bytes=25600 hw_send_calls=55966 hw_send_bytes=25600 hw_send_zero=54366 hw_send_max_chunk=16 no_progress_budget=1599 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=second-drain size=256 ioctl_rc=0 user_calls=100 user_req=25600 user_acc=25600 ring_pop_calls=25 ring_pop_bytes=25600 hw_send_calls=55966 hw_send_bytes=25600 hw_send_zero=54366 hw_send_max_chunk=16 no_progress_budget=1599 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  policy=no-drain size=1024 iters=100 bytes=65536 short_writes=36 enqueue_ms=7 final_drain_ms=5589 second_drain_ms=0 final_drain_rc=0 final_drain_errno=0 second_drain_rc=0 second_drain_errno=0 drain_calls=2 drain_errors=0 last_drain_errno=0 line_time_ms=5555.6 enqueue_kbps=9108.59
  diag=s11-txdbg-reset size=1024 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=1024 ioctl_rc=0 user_calls=293 user_req=74886 user_acc=65536 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=1024 ioctl_rc=0 user_calls=293 user_req=74886 user_acc=65536 ring_pop_calls=65 ring_pop_bytes=65536 hw_send_calls=143361 hw_send_bytes=65536 hw_send_zero=139264 hw_send_max_chunk=16 no_progress_budget=4096 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=second-drain size=1024 ioctl_rc=0 user_calls=293 user_req=74886 user_acc=65536 ring_pop_calls=65 ring_pop_bytes=65536 hw_send_calls=143361 hw_send_bytes=65536 hw_send_zero=139264 hw_send_max_chunk=16 no_progress_budget=4096 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1

=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===
  diag=S12 pre_section_stdout_drain_ms=111 drain_errors=0 last_errno=0
  policy=batch-drain size=64 iters=100 batch=8 drains=13 bytes=6400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=548 line_time_ms=542.5 kbps=11.38 line_rate_pct=98.8
  policy=batch-drain size=256 iters=100 batch=8 drains=13 bytes=25600 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=2203 line_time_ms=2170.1 kbps=11.35 line_rate_pct=98.5
  policy=batch-drain size=1024 iters=100 batch=8 drains=13 bytes=102400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=8759 line_time_ms=8680.6 kbps=11.42 line_rate_pct=99.1

=== [S13] TX writev Fragments (fragment aggregation witness) ===
  diag=S13 pre_section_stdout_drain_ms=22 drain_errors=0 last_errno=0
  policy=writev-drain-each fragments=4 fragment_size=64 total_size=256 iters=100 bytes=25431 short_writes=1 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2185 line_time_ms=2155.8 kbps=11.36 line_rate_pct=98.6

=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===
  diag=S14 pre_section_stdout_drain_ms=25 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=560 line_time_ms=542.5 kbps=11.14 line_rate_pct=96.7
  diag=break-even-size-64 n=100 avg_ms=5.607 p50_ms=5.607 p95_ms=5.611 p99_ms=5.620 max_ms=5.620 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.04
  policy=drain-each size=128 iters=100 bytes=12800 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=1136 line_time_ms=1085.1 kbps=11.00 line_rate_pct=95.5
  diag=break-even-size-128 n=100 avg_ms=11.361 p50_ms=11.072 p95_ms=11.075 p99_ms=39.945 max_ms=39.945 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=3.68
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2229 line_time_ms=2170.1 kbps=11.21 line_rate_pct=97.3
  diag=break-even-size-256 n=100 avg_ms=22.294 p50_ms=21.996 p95_ms=21.998 p99_ms=51.894 max_ms=51.894 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=2.39

=== [S20] TX Latency (single byte + tcdrain) ===
  diag=S20 pre_section_stdout_drain_ms=34 drain_errors=0 last_errno=0
  size=1 policy=drain-each n=100 avg_ms=0.183 p50_ms=0.182 p95_ms=0.183 p99_ms=0.222
  diag=S20 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S21] TX Latency FIFO Boundary Matrix ===
  diag=S21 pre_section_stdout_drain_ms=17 drain_errors=0 last_errno=0
  size=1 policy=drain-each n=100 avg_ms=0.183 p50_ms=0.183 p95_ms=0.184 p99_ms=0.231
  diag=fifo-size-1 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=15 policy=drain-each n=100 avg_ms=1.538 p50_ms=1.402 p95_ms=1.411 p99_ms=14.843
  diag=fifo-size-15 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=16 policy=drain-each n=100 avg_ms=1.627 p50_ms=1.488 p95_ms=1.497 p99_ms=15.204
  diag=fifo-size-16 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=17 policy=drain-each n=100 avg_ms=1.713 p50_ms=1.574 p95_ms=1.599 p99_ms=15.286
  diag=fifo-size-17 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=31 policy=drain-each n=99 avg_ms=2.941 p50_ms=2.790 p95_ms=2.795 p99_ms=16.482
  diag=fifo-size-31 drain_calls=99 drain_errors=0 last_drain_errno=0
  size=32 policy=drain-each n=100 avg_ms=3.011 p50_ms=2.876 p95_ms=2.880 p99_ms=16.398
  diag=fifo-size-32 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=33 policy=drain-each n=100 avg_ms=3.098 p50_ms=2.961 p95_ms=2.964 p99_ms=16.657
  diag=fifo-size-33 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=48 policy=drain-each n=100 avg_ms=4.377 p50_ms=4.240 p95_ms=4.244 p99_ms=17.936
  diag=fifo-size-48 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=49 policy=drain-each n=100 avg_ms=4.463 p50_ms=4.326 p95_ms=4.331 p99_ms=18.022
  diag=fifo-size-49 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S30] RX Empty Non-blocking Read (FIONBIO) ===
  diag=S30 pre_section_stdout_drain_ms=18 drain_errors=0 last_errno=0
  method=open-o-nonblock status=PASS result=EAGAIN
  method=ioctl-fionbio status=PASS result=EAGAIN

=== [S31] RX Fixed Payload Witness ===
  diag=S31 pre_section_stdout_drain_ms=12 drain_errors=0 last_errno=0
  status=SKIPPED reason=BENCH_RX_FIXED_BYTES=0

Done.
[starry-d1] benchmark exited with code: 0
[starry-d1] halting.

