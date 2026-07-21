Boot at 2026-07-21 13:57:13.004394200 UTC

[  0.460236 0 axnet_ng:139]   No vsock device found!
[  0.461045 0 axdisplay:26]   No display device found!
[UART INIT] ✅ iomap OK: UART MMIO at VA:0xffffffc010000000
[UART INIT] Trying raw read at base+5 (stride 1, LSR)...
[UART INIT] ✅ Raw LSR read: 0x60
[UART INIT] Trying uart_16550 crate access...
[UART INIT] FCR: FIFO enabled=true, trigger level via ISR bits 7-6
[UART INIT] async UART hardware initialized (copiers not started yet)
[kernel] Async UART driver initialized
[BENCH] Running startup benchmark...
[BENCH] Ring buffer write: 351648.35 KB/s (65536 bytes in 0.18 ms)
[BENCH] Hardware line rate: 11.52 KB/s (115200 bps)
[BENCH] FIFO depth: 16 bytes
[BENCH] Ring buffer: 64 KB × 2 = 128 KB total
[BENCH] Driver struct: 168 bytes
[BENCH] Total memory: 128 KB
[BENCH] NAPI threshold: 16 consecutive reads
[BENCH] NAPI batch size: 64 bytes
[BENCH] Copier buffer size: 1024 bytes
[BENCH] IRQ count: 0
[BENCH] Running RX ring buffer throughput test...
[BENCH] RX ring buffer read: 989180.83 KB/s (65536 bytes in 0.06 ms)
[BENCH] Running RX ring buffer latency test...
[BENCH] RX latency (n=100): min=100ns avg=278ns P50=200ns P95=200ns P99=12400ns
[BENCH] Startup benchmark complete
[BENCH] Note: actual throughput limited by UART line rate (11.52 KB/s)
[UART INIT] async UART copiers started
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
  version=q31-cpu-efficiency-20260721
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
  hart_count=not-available
  fstat_dev=major=5 minor=1
  source_revision=not-available
  source_dirty=not-available
  timer_source_detail=CLOCK_MONOTONIC
  clock_nanosleep_available=yes
  instret_source=/proc/instret
  bench_version_extra=q31-cpu-efficiency

  instret_read_overhead=2174577
=== [S10] TX Throughput Baseline (write + tcdrain each iteration) ===
  diag=S10 pre_section_stdout_drain_ms=5 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=44 line_time_ms=542.5 kbps=141.61 line_rate_pct=1229.3
  diag=drain-each-size-64 n=100 avg_ms=0.429 p50_ms=0.418 p95_ms=0.659 p99_ms=1.131 max_ms=1.131 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.21 p99_p50_ratio=2.71 max_p50_ratio=2.71
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=137 line_time_ms=2170.1 kbps=181.52 line_rate_pct=1575.7
  diag=drain-each-size-256 n=100 avg_ms=1.370 p50_ms=1.371 p95_ms=1.999 p99_ms=3.303 max_ms=3.303 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.15 p99_p50_ratio=2.41 max_p50_ratio=2.41
  policy=drain-each size=1024 iters=100 bytes=102400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=548 line_time_ms=8680.6 kbps=182.45 line_rate_pct=1583.8
  diag=drain-each-size-1024 n=100 avg_ms=5.472 p50_ms=5.382 p95_ms=6.694 p99_ms=8.034 max_ms=8.034 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.09 p99_p50_ratio=1.49 max_p50_ratio=1.49

=== [S11] TX Enqueue Cost (write loop, final drain outside timing) ===
  diag=S11 pre_section_stdout_drain_ms=2 drain_errors=0 last_errno=0
  policy=no-drain size=64 iters=100 bytes=6400 short_writes=0 enqueue_ms=0 final_drain_ms=33 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=542.5 enqueue_kbps=6864.36
  diag=s11-txdbg-reset size=64 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=64 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=64 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-derived size=64 submit_fraction=0.0268 producer_available=0.9732 total_time_ms=33 enqueue_time_ms=0
  policy=no-drain size=256 iters=100 bytes=25600 short_writes=0 enqueue_ms=0 final_drain_ms=134 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=2170.1 enqueue_kbps=25906.74
  diag=s11-txdbg-reset size=256 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=256 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=256 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-derived size=256 submit_fraction=0.0071 producer_available=0.9929 total_time_ms=134 enqueue_time_ms=0
  policy=no-drain size=1024 iters=100 bytes=102400 short_writes=0 enqueue_ms=197 final_drain_ms=340 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=8680.6 enqueue_kbps=506.23
  diag=s11-txdbg-reset size=1024 ioctl_rc=0
  diag=s11-txdbg phase=enqueue size=1024 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=1 staged_bytes=96 transmitter_empty=1
  diag=s11-txdbg phase=final-drain size=1024 ioctl_rc=0 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=1 copier_active=0 staged_bytes=0 transmitter_empty=1
  diag=s11-derived size=1024 submit_fraction=0.3672 producer_available=0.6328 total_time_ms=537 enqueue_time_ms=197

=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===
  diag=S12 pre_section_stdout_drain_ms=5 drain_errors=0 last_errno=0
  policy=batch-drain size=64 iters=100 batch=8 drains=13 bytes=6400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=36 line_time_ms=542.5 kbps=169.85 line_rate_pct=1474.4
  policy=batch-drain size=256 iters=100 batch=8 drains=13 bytes=25600 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=141 line_time_ms=2170.1 kbps=176.76 line_rate_pct=1534.4
  policy=batch-drain size=1024 iters=100 batch=8 drains=13 bytes=102400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=557 line_time_ms=8680.6 kbps=179.49 line_rate_pct=1558.1

=== [S13] TX writev Fragments (fragment aggregation witness) ===
  diag=S13 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  policy=writev-drain-each fragments=4 fragment_size=64 total_size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=147 line_time_ms=2170.1 kbps=169.55 line_rate_pct=1471.8

=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===
  diag=S14 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=48 line_time_ms=542.5 kbps=129.18 line_rate_pct=1121.3
  diag=break-even-size-64 n=100 avg_ms=0.476 p50_ms=0.486 p95_ms=0.629 p99_ms=0.975 max_ms=0.975 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.18 p99_p50_ratio=2.01 max_p50_ratio=2.01
  policy=drain-each size=128 iters=100 bytes=12800 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=86 line_time_ms=1085.1 kbps=144.11 line_rate_pct=1251.0
  diag=break-even-size-128 n=100 avg_ms=0.860 p50_ms=0.833 p95_ms=1.141 p99_ms=2.884 max_ms=2.884 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.27 p99_p50_ratio=3.46 max_p50_ratio=3.46
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=156 line_time_ms=2170.1 kbps=159.72 line_rate_pct=1386.5
  diag=break-even-size-256 n=100 avg_ms=1.557 p50_ms=1.544 p95_ms=2.177 p99_ms=3.759 max_ms=3.759 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.17 p99_p50_ratio=2.43 max_p50_ratio=2.43

=== [S20] TX Latency (single byte + tcdrain) ===
  diag=S20 pre_section_stdout_drain_ms=2 drain_errors=0 last_errno=0
  diag=s20-single-byte n=100 avg_ms=0.181 p50_ms=0.177 p95_ms=0.249 p99_ms=0.297 max_ms=0.297 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=3.51 p99_p50_ratio=1.68 max_p50_ratio=1.68
  diag=S20 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S21] TX Latency FIFO Boundary Matrix ===
  diag=S21 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  diag=s21-fifo-size-1 n=100 avg_ms=0.187 p50_ms=0.179 p95_ms=0.221 p99_ms=0.361 max_ms=0.361 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=4.26 p99_p50_ratio=2.02 max_p50_ratio=2.02
  diag=fifo-size-1 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-15 n=100 avg_ms=0.244 p50_ms=0.228 p95_ms=0.295 p99_ms=1.364 max_ms=1.364 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.07 p99_p50_ratio=5.98 max_p50_ratio=5.98
  diag=fifo-size-15 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-16 n=100 avg_ms=0.228 p50_ms=0.211 p95_ms=0.276 p99_ms=1.496 max_ms=1.496 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.10 p99_p50_ratio=7.08 max_p50_ratio=7.08
  diag=fifo-size-16 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-17 n=100 avg_ms=0.257 p50_ms=0.241 p95_ms=0.295 p99_ms=1.646 max_ms=1.646 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.14 p99_p50_ratio=6.82 max_p50_ratio=6.82
  diag=fifo-size-17 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-31 n=100 avg_ms=0.340 p50_ms=0.323 p95_ms=0.420 p99_ms=2.582 max_ms=2.582 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.98 p99_p50_ratio=8.00 max_p50_ratio=8.00
  diag=fifo-size-31 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-32 n=100 avg_ms=0.296 p50_ms=0.293 p95_ms=0.368 p99_ms=1.107 max_ms=1.107 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.41 p99_p50_ratio=3.77 max_p50_ratio=3.77
  diag=fifo-size-32 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-33 n=100 avg_ms=0.359 p50_ms=0.333 p95_ms=0.462 p99_ms=2.067 max_ms=2.067 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.74 p99_p50_ratio=6.20 max_p50_ratio=6.20
  diag=fifo-size-33 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-48 n=100 avg_ms=0.416 p50_ms=0.408 p95_ms=0.588 p99_ms=1.528 max_ms=1.528 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.38 p99_p50_ratio=3.74 max_p50_ratio=3.74
  diag=fifo-size-48 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-49 n=100 avg_ms=0.393 p50_ms=0.387 p95_ms=0.552 p99_ms=1.214 max_ms=1.214 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.29 p99_p50_ratio=3.13 max_p50_ratio=3.13
  diag=fifo-size-49 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S30] RX Empty Non-blocking Read (FIONBIO) ===
  diag=S30 pre_section_stdout_drain_ms=2 drain_errors=0 last_errno=0
  method=open-o-nonblock status=PASS result=EAGAIN
  method=ioctl-fionbio status=PASS result=EAGAIN

=== [S31] RX Fixed Payload Witness ===
  diag=S31 pre_section_stdout_drain_ms=1 drain_errors=0 last_errno=0
  status=SKIPPED reason=BENCH_RX_FIXED_BYTES=0

=== [S41] TX CPU Work (instret: write start → final TEMT drain, 5 rounds) ===
  diag=S41 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  instret_read_overhead=241583
  s41-size=64 expected_bytes=6400 iters=100
  diag=s41-valid size=64 round=1 completed=6400 expected=6400 reset_rc=0 instret_begin=103977165119588 instret_end=103977264174598 instret_delta=99055010 instructions_per_byte=15477.35 instructions_per_write=990550 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=40
  diag=s41-valid size=64 round=2 completed=6400 expected=6400 reset_rc=0 instret_begin=103977269828092 instret_end=103977356285328 instret_delta=86457236 instructions_per_byte=13508.94 instructions_per_write=864572 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=35
  diag=s41-valid size=64 round=3 completed=6400 expected=6400 reset_rc=0 instret_begin=103977361183961 instret_end=103977443363994 instret_delta=82180033 instructions_per_byte=12840.63 instructions_per_write=813664 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=101 duration_ms=33
  diag=s41-valid size=64 round=4 completed=6400 expected=6400 reset_rc=0 instret_begin=103977450586856 instret_end=103977539853638 instret_delta=89266782 instructions_per_byte=13947.93 instructions_per_write=892668 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=36
  diag=s41-valid size=64 round=5 completed=6400 expected=6400 reset_rc=0 instret_begin=103977546670520 instret_end=103977625087678 instret_delta=78417158 instructions_per_byte=12252.68 instructions_per_write=784172 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=32
  diag=s41-summary size=64 valid_rounds=5 median_instructions_per_byte=13508.94 median_instructions_per_write=864572
  s41-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=0.0 hw_send_zero_per_kb=0.0 ring_pop_bytes_per_kb=0.0 no_progress_per_kb=0.0 bytes_per_hw_send=0.000 bytes_per_ring_pop=0.0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 ring_pop_calls=0 ring_pop_bytes=0 user_push_calls=0 user_push_acc=0
  s41-size=256 expected_bytes=25600 iters=100
  diag=s41-valid size=256 round=1 completed=25600 expected=25600 reset_rc=0 instret_begin=103977634696774 instret_end=103977960230975 instret_delta=325534201 instructions_per_byte=12716.18 instructions_per_write=3255342 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=134
  diag=s41-valid size=256 round=2 completed=25600 expected=25600 reset_rc=0 instret_begin=103977964913281 instret_end=103978286642342 instret_delta=321729061 instructions_per_byte=12567.54 instructions_per_write=3185436 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=101 duration_ms=132
  diag=s41-valid size=256 round=3 completed=25600 expected=25600 reset_rc=0 instret_begin=103978290864848 instret_end=103978624209687 instret_delta=333344839 instructions_per_byte=13021.28 instructions_per_write=3333448 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=137
  diag=s41-valid size=256 round=4 completed=25600 expected=25600 reset_rc=0 instret_begin=103978627811452 instret_end=103978968739788 instret_delta=340928336 instructions_per_byte=13317.51 instructions_per_write=3409283 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=140
  diag=s41-valid size=256 round=5 completed=25600 expected=25600 reset_rc=0 instret_begin=103978974290944 instret_end=103979330044968 instret_delta=355754024 instructions_per_byte=13896.64 instructions_per_write=3522317 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=101 duration_ms=146
  diag=s41-summary size=256 valid_rounds=5 median_instructions_per_byte=13021.28 median_instructions_per_write=3333448
  s41-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=0.0 hw_send_zero_per_kb=0.0 ring_pop_bytes_per_kb=0.0 no_progress_per_kb=0.0 bytes_per_hw_send=0.000 bytes_per_ring_pop=0.0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 ring_pop_calls=0 ring_pop_bytes=0 user_push_calls=0 user_push_acc=0
  s41-size=1024 expected_bytes=102400 iters=100
  diag=s41-valid size=1024 round=1 completed=102400 expected=102400 reset_rc=0 instret_begin=103979344097320 instret_end=103981162580138 instret_delta=1818482818 instructions_per_byte=17758.62 instructions_per_write=167079 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=10884 duration_ms=751
  diag=s41-valid size=1024 round=2 completed=102400 expected=102400 reset_rc=0 instret_begin=103981167418459 instret_end=103982902260098 instret_delta=1734841639 instructions_per_byte=16941.81 instructions_per_write=161878 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=10717 duration_ms=716
  diag=s41-valid size=1024 round=3 completed=102400 expected=102400 reset_rc=0 instret_begin=103982908710148 instret_end=103984718227435 instret_delta=1809517287 instructions_per_byte=17671.07 instructions_per_write=175647 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=10302 duration_ms=747
  diag=s41-valid size=1024 round=4 completed=102400 expected=102400 reset_rc=0 instret_begin=103984723004964 instret_end=103986479712223 instret_delta=1756707259 instructions_per_byte=17155.34 instructions_per_write=159281 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=11029 duration_ms=725
  diag=s41-valid size=1024 round=5 completed=102400 expected=102400 reset_rc=0 instret_begin=103986484489190 instret_end=103988207045625 instret_delta=1722556435 instructions_per_byte=16821.84 instructions_per_write=157139 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=10962 duration_ms=711
  diag=s41-summary size=1024 valid_rounds=5 median_instructions_per_byte=17155.34 median_instructions_per_write=161878
  s41-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=0.0 hw_send_zero_per_kb=0.0 ring_pop_bytes_per_kb=0.0 no_progress_per_kb=0.0 bytes_per_hw_send=0.000 bytes_per_ring_pop=0.0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 ring_pop_calls=0 ring_pop_bytes=0 user_push_calls=0 user_push_acc=0

=== [S42] TX Compute Overlap (64B x 100, fixed window, 5 sample rounds) ===
  diag=S42 pre_section_stdout_drain_ms=5 drain_errors=0 last_errno=0
  idle window_ms=542.535 window_ns=542534722 iters=325458 duration_ms=542.585 iters_per_sec=599829
  overlap payload=64 iters=100 warmup=1 sample_rounds=5 theoretical_line_time_ms=542.535
  diag=s42-sample round=1 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=844.4 useful_iters=318382 useful_work_per_ms=587 final_drain_ms=0.052 total_duration_ms=542.596 total_over_line_ratio=1.000 overlap_efficiency=0.9783 reset_rc=0 drain_errors=0 leftover_ns=9378
  diag=s42-sample round=2 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=805.3 useful_iters=318988 useful_work_per_ms=588 final_drain_ms=0.042 total_duration_ms=542.584 total_over_line_ratio=1.000 overlap_efficiency=0.9801 reset_rc=0 drain_errors=0 leftover_ns=7878
  diag=s42-sample round=3 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=909.3 useful_iters=319659 useful_work_per_ms=589 final_drain_ms=0.037 total_duration_ms=542.578 total_over_line_ratio=1.000 overlap_efficiency=0.9822 reset_rc=0 drain_errors=0 leftover_ns=6578
  diag=s42-sample round=4 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=676.1 useful_iters=309836 useful_work_per_ms=571 final_drain_ms=0.056 total_duration_ms=542.601 total_over_line_ratio=1.000 overlap_efficiency=0.9520 reset_rc=0 drain_errors=0 leftover_ns=9978
  diag=s42-sample round=5 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=863.9 useful_iters=319434 useful_work_per_ms=589 final_drain_ms=0.043 total_duration_ms=542.587 total_over_line_ratio=1.000 overlap_efficiency=0.9815 reset_rc=0 drain_errors=0 leftover_ns=9078
  diag=s42-summary valid_rounds=5 median_useful_iters=318988 median_total_duration_ms=542.587 median_overlap_efficiency=0.9801
  s42-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=0.0 hw_send_zero_per_kb=0.0 ring_pop_bytes_per_kb=0.0 no_progress_per_kb=0.0 bytes_per_hw_send=0.000 bytes_per_ring_pop=0.0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 ring_pop_calls=0 ring_pop_bytes=0 user_push_calls=0 user_push_acc=0

=== [S43] Timer Wakeup Overshoot (5 idle groups + 5 loaded groups) ===
  diag=S43 pre_section_stdout_drain_ms=2 drain_errors=0 last_errno=0
  s43-phase=idle groups=5 samples=50 interval_us=5000
  diag=s43-idle-group group=1 status=PASS collected=50 errors=0 valid=50 duration_ms=258 sample[0]=3257200 sample[1]=7961500 sample[2]=3091000
  s43-idle-group-summary n=50 errors=0 p50_ns=7799800 p95_ns=8261300 p99_ns=8413800 max_ns=8413800
  diag=s43-idle-group group=2 status=PASS collected=50 errors=0 valid=50 duration_ms=259 sample[0]=4650400 sample[1]=9495900 sample[2]=4531500
  s43-idle-group-summary n=50 errors=0 p50_ns=4973400 p95_ns=9954000 p99_ns=10038200 max_ns=10038200
  diag=s43-idle-group group=3 status=PASS collected=50 errors=0 valid=50 duration_ms=259 sample[0]=4710100 sample[1]=9509900 sample[2]=4557600
  s43-idle-group-summary n=50 errors=0 p50_ns=5017300 p95_ns=9863000 p99_ns=9902500 max_ns=9902500
  diag=s43-idle-group group=4 status=PASS collected=50 errors=0 valid=50 duration_ms=259 sample[0]=4535800 sample[1]=9618800 sample[2]=4647800
  s43-idle-group-summary n=50 errors=0 p50_ns=9287800 p95_ns=9813500 p99_ns=9876900 max_ns=9876900
  diag=s43-idle-group group=5 status=PASS collected=50 errors=0 valid=50 duration_ms=259 sample[0]=4544300 sample[1]=9321100 sample[2]=4350700
  s43-idle-group-summary n=50 errors=0 p50_ns=5044100 p95_ns=9721000 p99_ns=9944100 max_ns=9944100
  s43-phase=loaded groups=5 burst_bytes=4096 theoretical_line_time_ns=347222222
  diag=s43-loaded-group group=1 status=PASS reset_rc=0 burst_written=4096 expected=4096 write_dur_ns=340800 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=255 drain_ms=0 drain_errors=0 sample[0]=12075700 sample[1]=7114600 sample[2]=2124000
  s43-loaded-group-summary n=50 errors=0 p50_ns=5726900 p95_ns=6324100 p99_ns=12075700 max_ns=12075700
  s43-loaded-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=0.0 hw_send_zero_per_kb=0.0 ring_pop_bytes_per_kb=0.0 no_progress_per_kb=0.0 bytes_per_hw_send=0.000 bytes_per_ring_pop=0.0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 ring_pop_calls=0 ring_pop_bytes=0 user_push_calls=0 user_push_acc=0
  diag=s43-loaded-group group=2 status=PASS reset_rc=0 burst_written=4096 expected=4096 write_dur_ns=415000 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=255 drain_ms=0 drain_errors=0 sample[0]=9983500 sample[1]=5022100 sample[2]=30600
  s43-loaded-group-summary n=50 errors=0 p50_ns=5076500 p95_ns=5501500 p99_ns=9983500 max_ns=9983500
  s43-loaded-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=0.0 hw_send_zero_per_kb=0.0 ring_pop_bytes_per_kb=0.0 no_progress_per_kb=0.0 bytes_per_hw_send=0.000 bytes_per_ring_pop=0.0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 ring_pop_calls=0 ring_pop_bytes=0 user_push_calls=0 user_push_acc=0
  diag=s43-loaded-group group=3 status=PASS reset_rc=0 burst_written=4096 expected=4096 write_dur_ns=361900 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=254 drain_ms=0 drain_errors=0 sample[0]=10108600 sample[1]=5146500 sample[2]=155700
  s43-loaded-group-summary n=50 errors=0 p50_ns=4994700 p95_ns=10008400 p99_ns=10108600 max_ns=10108600
  s43-loaded-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=0.0 hw_send_zero_per_kb=0.0 ring_pop_bytes_per_kb=0.0 no_progress_per_kb=0.0 bytes_per_hw_send=0.000 bytes_per_ring_pop=0.0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 ring_pop_calls=0 ring_pop_bytes=0 user_push_calls=0 user_push_acc=0
  diag=s43-loaded-group group=4 status=PASS reset_rc=0 burst_written=4096 expected=4096 write_dur_ns=532500 incomplete_logical=0 syscall_calls=17 collected=50 errors=0 valid=50 sample_duration_ms=252 drain_ms=0 drain_errors=0 sample[0]=11627500 sample[1]=6670300 sample[2]=1679700
  s43-loaded-group-summary n=50 errors=0 p50_ns=6670300 p95_ns=8339800 p99_ns=11627500 max_ns=11627500
  s43-loaded-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=0.0 hw_send_zero_per_kb=0.0 ring_pop_bytes_per_kb=0.0 no_progress_per_kb=0.0 bytes_per_hw_send=0.000 bytes_per_ring_pop=0.0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 ring_pop_calls=0 ring_pop_bytes=0 user_push_calls=0 user_push_acc=0
  diag=s43-loaded-group group=5 status=PASS reset_rc=0 burst_written=4096 expected=4096 write_dur_ns=272000 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=257 drain_ms=0 drain_errors=0 sample[0]=12109400 sample[1]=7147700 sample[2]=2156900
  s43-loaded-group-summary n=50 errors=0 p50_ns=7469800 p95_ns=8085600 p99_ns=12109400 max_ns=12109400
  s43-loaded-local-counters counters=ok reset_rc=0 snapshot_rc=0 ring_pop_calls_per_kb=0.0 hw_send_zero_per_kb=0.0 ring_pop_bytes_per_kb=0.0 no_progress_per_kb=0.0 bytes_per_hw_send=0.000 bytes_per_ring_pop=0.0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 ring_pop_calls=0 ring_pop_bytes=0 user_push_calls=0 user_push_acc=0
  diag=s43-loaded-aggregate n=250 valid_groups=5 p50_ns=5052500 p95_ns=9890700 p99_ns=11627500 max_ns=12109400
  diag=s43-idle-aggregate n=250 valid_groups=5 p50_ns=4975500 p95_ns=9860500 p99_ns=9954000 max_ns=10038200

=== [S40] TX Counter Proxy Summary ===
  telemetry_available=0 ioctl_rc=0
  counter=user-push user_calls=0 user_req=0 user_acc=0
  counter=ring-pop ring_pop_calls=0 ring_pop_bytes=0
  counter=hw-send hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0
  counter=no-progress no_progress_budget=0 slow_poll_exh=0 yield_exh=0
  counter=drain-state ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=1
  proxy=derived status=not-available reason=telemetry-counters-are-zero

Done.
starry:/bin# 