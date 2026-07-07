D1:
Boot at 1970-01-01 00:00:00.432197728 UTC

[starry-d1] Lichee D1 userbench mode
[UART INIT] D1 MMIO base=0xffffffc002500000 stride=4 IER=0x0 IIR=0xc1 LSR=0x20
[UART INIT] D1 UART IRQ 18 registered=true, buffers=64KBx2
[UART INIT] async UART ready
[kernel] Async UART driver initialized (D1)
[BENCH] Running startup benchmark...
[BENCH] Ring buffer write: 1151569.59 KB/s (102400 bytes in 0.09 ms)
[BENCH] Hardware line rate: 11.52 KB/s (115200 bps)
[BENCH] FIFO depth: 16 bytes
[BENCH] Ring buffer: 64 KB × 2 = 128 KB total
[BENCH] Driver struct: 272 bytes
[BENCH] Total memory: 128 KB
[BENCH] NAPI threshold: 16 consecutive reads
[BENCH] NAPI batch size: 64 bytes
[BENCH] Copier buffer size: 1024 bytes
[BENCH] IRQ count: 43
[BENCH] IRQ frequency: 495174.92 IRQ/s
[BENCH] Running RX ring buffer throughput test...
[BENCH] RX ring buffer read: 8437706.00 KB/s (65536 bytes in 0.01 ms)
[BENCH] Running RX ring buffer latency test...
[BENCH] RX latency (n=100): min=82ns avg=106ns P50=123ns P95=123ns P99=246ns
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
  diag=S10 pre_section_stdout_drain_ms=5518 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=561 line_time_ms=542.5 kbps=11.13 line_rate_pct=96.6
  diag=drain-each-size-64 n=100 avg_ms=5.612 p50_ms=5.612 p95_ms=5.614 p99_ms=5.625 max_ms=5.625 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.04
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2229 line_time_ms=2170.1 kbps=11.21 line_rate_pct=97.3
  diag=drain-each-size-256 n=100 avg_ms=22.293 p50_ms=22.004 p95_ms=22.011 p99_ms=50.860 max_ms=50.860 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=2.34
  policy=drain-each size=1024 iters=100 bytes=102400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=8785 line_time_ms=8680.6 kbps=11.38 line_rate_pct=98.8
  diag=drain-each-size-1024 n=100 avg_ms=87.850 p50_ms=87.551 p95_ms=87.562 p99_ms=117.438 max_ms=117.438 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=1.35

=== [S11] TX Enqueue Cost (write loop, final drain outside timing) ===
  diag=S11 pre_section_stdout_drain_ms=36 drain_errors=0 last_errno=0
  policy=no-drain size=64 iters=100 bytes=6400 short_writes=0 enqueue_ms=1 final_drain_ms=545 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=542.5 enqueue_kbps=4239.36
  diag=s11-txdbg-reset size=64 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=64 ioctl_rc=0 user_calls=100 user_req=6400 user_acc=6400 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=64 ioctl_rc=0 user_calls=100 user_req=6400 user_acc=6400 ring_pop_calls=7 ring_pop_bytes=6400 hw_send_calls=274396 hw_send_bytes=6400 hw_send_zero=273996 hw_send_max_chunk=16 no_progress_budget=399 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  policy=no-drain size=256 iters=100 bytes=25600 short_writes=0 enqueue_ms=2 final_drain_ms=2183 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=2170.1 enqueue_kbps=9113.76
  diag=s11-txdbg-reset size=256 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=256 ioctl_rc=0 user_calls=100 user_req=25600 user_acc=25600 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=256 ioctl_rc=0 user_calls=100 user_req=25600 user_acc=25600 ring_pop_calls=25 ring_pop_bytes=25600 hw_send_calls=1099814 hw_send_bytes=25600 hw_send_zero=1098214 hw_send_max_chunk=16 no_progress_budget=1599 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  policy=no-drain size=1024 iters=100 bytes=65536 short_writes=36 enqueue_ms=6 final_drain_ms=5588 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=5555.6 enqueue_kbps=10015.88
  diag=s11-txdbg-reset size=1024 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=1024 ioctl_rc=0 user_calls=293 user_req=74944 user_acc=65536 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=1024 ioctl_rc=0 user_calls=293 user_req=74944 user_acc=65536 ring_pop_calls=65 ring_pop_bytes=65536 hw_send_calls=2816501 hw_send_bytes=65536 hw_send_zero=2812405 hw_send_max_chunk=16 no_progress_budget=4095 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1

=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===
  diag=S12 pre_section_stdout_drain_ms=85 drain_errors=0 last_errno=0
  policy=batch-drain size=64 iters=100 batch=8 drains=13 bytes=6400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=549 line_time_ms=542.5 kbps=11.38 line_rate_pct=98.8
  policy=batch-drain size=256 iters=100 batch=8 drains=13 bytes=25600 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=2203 line_time_ms=2170.1 kbps=11.35 line_rate_pct=98.5
  policy=batch-drain size=1024 iters=100 batch=8 drains=13 bytes=102400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=8758 line_time_ms=8680.6 kbps=11.42 line_rate_pct=99.1

=== [S13] TX writev Fragments (fragment aggregation witness) ===
  diag=S13 pre_section_stdout_drain_ms=22 drain_errors=0 last_errno=0
  policy=writev-drain-each fragments=4 fragment_size=64 total_size=256 iters=100 bytes=25429 short_writes=1 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2186 line_time_ms=2155.6 kbps=11.36 line_rate_pct=98.6

=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===
  diag=S14 pre_section_stdout_drain_ms=25 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=561 line_time_ms=542.5 kbps=11.13 line_rate_pct=96.6
  diag=break-even-size-64 n=100 avg_ms=5.611 p50_ms=5.611 p95_ms=5.614 p99_ms=5.622 max_ms=5.622 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.04
  policy=drain-each size=128 iters=100 bytes=12800 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=1136 line_time_ms=1085.1 kbps=11.00 line_rate_pct=95.4
  diag=break-even-size-128 n=100 avg_ms=11.365 p50_ms=11.077 p95_ms=11.082 p99_ms=39.947 max_ms=39.947 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=3.68
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=2230 line_time_ms=2170.1 kbps=11.21 line_rate_pct=97.3
  diag=break-even-size-256 n=100 avg_ms=22.303 p50_ms=22.004 p95_ms=22.012 p99_ms=51.884 max_ms=51.884 slow_gt10ms=100 slow_over_line_plus10ms=1 max_line_ratio=2.39

=== [S20] TX Latency (single byte + tcdrain) ===
  diag=S20 pre_section_stdout_drain_ms=34 drain_errors=0 last_errno=0
  size=1 policy=drain-each n=100 avg_ms=0.186 p50_ms=0.185 p95_ms=0.188 p99_ms=0.224
  diag=S20 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S21] TX Latency FIFO Boundary Matrix ===
  diag=S21 pre_section_stdout_drain_ms=17 drain_errors=0 last_errno=0
  size=1 policy=drain-each n=100 avg_ms=0.187 p50_ms=0.186 p95_ms=0.188 p99_ms=0.231
  diag=fifo-size-1 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=15 policy=drain-each n=100 avg_ms=1.538 p50_ms=1.403 p95_ms=1.412 p99_ms=14.862
  diag=fifo-size-15 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=16 policy=drain-each n=100 avg_ms=1.627 p50_ms=1.490 p95_ms=1.497 p99_ms=15.191
  diag=fifo-size-16 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=17 policy=drain-each n=100 avg_ms=1.706 p50_ms=1.568 p95_ms=1.571 p99_ms=15.274
  diag=fifo-size-17 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=31 policy=drain-each n=100 avg_ms=2.934 p50_ms=2.796 p95_ms=2.804 p99_ms=16.493
  diag=fifo-size-31 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=32 policy=drain-each n=100 avg_ms=3.017 p50_ms=2.881 p95_ms=2.889 p99_ms=16.558
  diag=fifo-size-32 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=33 policy=drain-each n=99 avg_ms=3.077 p50_ms=2.936 p95_ms=2.943 p99_ms=16.638
  diag=fifo-size-33 drain_calls=99 drain_errors=0 last_drain_errno=0
  size=48 policy=drain-each n=100 avg_ms=4.383 p50_ms=4.249 p95_ms=4.256 p99_ms=17.750
  diag=fifo-size-48 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=49 policy=drain-each n=100 avg_ms=4.439 p50_ms=4.301 p95_ms=4.312 p99_ms=18.006
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


qemu:
Boot at 2026-07-07 09:52:57.002900300 UTC

[  0.498463 0 axnet_ng:139]   No vsock device found!
[  0.498899 0 axdisplay:26]   No display device found!
[UART INIT] ✅ iomap OK: UART MMIO at VA:0xffffffc010000000
[UART INIT] Trying raw read at base+5 (stride 1, LSR)...
[UART INIT] ✅ Raw LSR read: 0x60
[UART INIT] Trying uart_16550 crate access...
[UART INIT] FCR: FIFO enabled=true, trigger level via ISR bits 7-6
[UART INIT] async UART ready
[kernel] Async UART driver initialized
[BENCH] Running startup benchmark...
[BENCH] Ring buffer write: 550055.01 KB/s (102400 bytes in 0.18 ms)
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
[BENCH] RX ring buffer read: 1205273.07 KB/s (65536 bytes in 0.05 ms)
[BENCH] Running RX ring buffer latency test...
[BENCH] RX latency (n=100): min=100ns avg=260ns P50=100ns P95=200ns P99=11600ns
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
  diag=S10 pre_section_stdout_drain_ms=2 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=40 line_time_ms=542.5 kbps=153.86 line_rate_pct=1335.6
  diag=drain-each-size-64 n=100 avg_ms=0.400 p50_ms=0.406 p95_ms=0.622 p99_ms=0.975 max_ms=0.975 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.18
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=149 line_time_ms=2170.1 kbps=167.20 line_rate_pct=1451.4
  diag=drain-each-size-256 n=100 avg_ms=1.488 p50_ms=1.479 p95_ms=2.410 p99_ms=3.308 max_ms=3.308 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.15
  policy=drain-each size=1024 iters=100 bytes=102400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=548 line_time_ms=8680.6 kbps=182.28 line_rate_pct=1582.3
  diag=drain-each-size-1024 n=100 avg_ms=5.476 p50_ms=5.552 p95_ms=7.388 p99_ms=8.474 max_ms=8.474 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.10

=== [S11] TX Enqueue Cost (write loop, final drain outside timing) ===
  diag=S11 pre_section_stdout_drain_ms=2 drain_errors=0 last_errno=0
  policy=no-drain size=64 iters=100 bytes=6400 short_writes=0 enqueue_ms=1 final_drain_ms=34 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=542.5 enqueue_kbps=5918.00
  policy=no-drain size=256 iters=100 bytes=25600 short_writes=0 enqueue_ms=1 final_drain_ms=147 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=2170.1 enqueue_kbps=13084.21
  policy=no-drain size=1024 iters=100 bytes=65311 short_writes=37 enqueue_ms=3 final_drain_ms=364 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=5536.5 enqueue_kbps=16363.15

=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===
  diag=S12 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  policy=batch-drain size=64 iters=100 batch=8 drains=13 bytes=6400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=39 line_time_ms=542.5 kbps=159.34 line_rate_pct=1383.1
  policy=batch-drain size=256 iters=100 batch=8 drains=13 bytes=25600 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=150 line_time_ms=2170.1 kbps=165.56 line_rate_pct=1437.2
  policy=batch-drain size=1024 iters=100 batch=8 drains=13 bytes=102400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=597 line_time_ms=8680.6 kbps=167.41 line_rate_pct=1453.2

=== [S13] TX writev Fragments (fragment aggregation witness) ===
  diag=S13 pre_section_stdout_drain_ms=2 drain_errors=0 last_errno=0
  policy=writev-drain-each fragments=4 fragment_size=64 total_size=256 iters=100 bytes=25563 short_writes=1 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=166 line_time_ms=2167.0 kbps=149.88 line_rate_pct=1301.1

=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===
  diag=S14 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=51 line_time_ms=542.5 kbps=121.90 line_rate_pct=1058.1
  diag=break-even-size-64 n=100 avg_ms=0.505 p50_ms=0.520 p95_ms=0.766 p99_ms=1.115 max_ms=1.115 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.21
  policy=drain-each size=128 iters=100 bytes=12800 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=86 line_time_ms=1085.1 kbps=143.98 line_rate_pct=1249.8
  diag=break-even-size-128 n=100 avg_ms=0.860 p50_ms=0.881 p95_ms=1.181 p99_ms=3.114 max_ms=3.114 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.29
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=157 line_time_ms=2170.1 kbps=159.23 line_rate_pct=1382.2
  diag=break-even-size-256 n=100 avg_ms=1.562 p50_ms=1.616 p95_ms=2.131 p99_ms=3.478 max_ms=3.478 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.16

=== [S20] TX Latency (single byte + tcdrain) ===
  diag=S20 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  size=1 policy=drain-each n=100 avg_ms=0.182 p50_ms=0.176 p95_ms=0.204 p99_ms=0.357
  diag=S20 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S21] TX Latency FIFO Boundary Matrix ===
  diag=S21 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  size=1 policy=drain-each n=100 avg_ms=0.168 p50_ms=0.164 p95_ms=0.187 p99_ms=0.378
  diag=fifo-size-1 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=15 policy=drain-each n=100 avg_ms=0.217 p50_ms=0.208 p95_ms=0.279 p99_ms=0.577
  diag=fifo-size-15 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=16 policy=drain-each n=100 avg_ms=0.252 p50_ms=0.239 p95_ms=0.359 p99_ms=0.964
  diag=fifo-size-16 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=17 policy=drain-each n=100 avg_ms=0.255 p50_ms=0.248 p95_ms=0.311 p99_ms=0.942
  diag=fifo-size-17 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=31 policy=drain-each n=100 avg_ms=0.328 p50_ms=0.314 p95_ms=0.450 p99_ms=1.343
  diag=fifo-size-31 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=32 policy=drain-each n=100 avg_ms=0.324 p50_ms=0.319 p95_ms=0.462 p99_ms=0.592
  diag=fifo-size-32 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=33 policy=drain-each n=100 avg_ms=0.337 p50_ms=0.338 p95_ms=0.451 p99_ms=1.206
  diag=fifo-size-33 drain_calls=100 drain_errors=0 last_drain_errno=0
  size=48 policy=drain-each n=99 avg_ms=0.411 p50_ms=0.399 p95_ms=0.591 p99_ms=0.938
  diag=fifo-size-48 drain_calls=99 drain_errors=0 last_drain_errno=0
  size=49 policy=drain-each n=100 avg_ms=0.485 p50_ms=0.473 p95_ms=0.730 p99_ms=1.750
  diag=fifo-size-49 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S30] RX Empty Non-blocking Read (FIONBIO) ===
  diag=S30 pre_section_stdout_drain_ms=2 drain_errors=0 last_errno=0
  method=open-o-nonblock status=PASS result=EAGAIN
  method=ioctl-fionbio status=PASS result=EAGAIN

=== [S31] RX Fixed Payload Witness ===
  diag=S31 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  status=SKIPPED reason=BENCH_RX_FIXED_BYTES=0

Done.
starry:/bin# 