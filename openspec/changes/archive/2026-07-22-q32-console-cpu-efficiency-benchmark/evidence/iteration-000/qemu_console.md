Boot at 2026-07-22 04:58:07.005458700 UTC

[  0.503577 0 axnet_ng:139]   No vsock device found!
[  0.504218 0 axdisplay:26]   No display device found!
Welcome to Starry OS!
SHLVL=1
HOME=/root
PWD=/

Use apk to install packages.

starry:~# cd /bin
starry:/bin# ls ./
arch           dumpkmap       kill           mv             setserial
ash            echo           link           netstat        sh
base64         egrep          linux32        nice           sleep
bbconfig       false          linux64        pidof          stat
benchmark      fatattr        ln             ping           stty
busybox        fdflush        login          ping6          su
cat            fgrep          ls             pipe_progress  sync
chattr         fsync          lsattr         printenv       tar
chgrp          getopt         lzop           ps             touch
chmod          grep           makemime       pwd            true
chown          gunzip         mkdir          reformime      umount
cp             gzip           mknod          rev            uname
date           hostname       mktemp         rm             usleep
dd             ionice         more           rmdir          watch
df             iostat         mount          run-parts      zcat
dmesg          ipcalc         mountpoint     sed
dnsdomainname  kbd_mode       mpstat         setpriv
starry:/bin# ^C

starry:/bin# ./benchmark
UART Async Benchmark
====================

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

  instret_read_overhead=1245668
[ 16.330982 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 3
=== [S10] TX Throughput Baseline (write + tcdrain each iteration) ===
  diag=S10 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=30 line_time_ms=542.5 kbps=201.79 line_rate_pct=1751.7
  diag=drain-each-size-64 n=100 avg_ms=0.306 p50_ms=0.310 p95_ms=0.557 p99_ms=0.732 max_ms=0.732 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.13 p99_p50_ratio=2.36 max_p50_ratio=2.36
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=139 line_time_ms=2170.1 kbps=178.82 line_rate_pct=1552.3
  diag=drain-each-size-256 n=100 avg_ms=1.394 p50_ms=1.397 p95_ms=1.992 p99_ms=2.314 max_ms=2.314 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.11 p99_p50_ratio=1.66 max_p50_ratio=1.66
  policy=drain-each size=1024 iters=100 bytes=102400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=576 line_time_ms=8680.6 kbps=173.47 line_rate_pct=1505.8
  diag=drain-each-size-1024 n=100 avg_ms=5.759 p50_ms=5.728 p95_ms=7.396 p99_ms=8.639 max_ms=8.639 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.10 p99_p50_ratio=1.51 max_p50_ratio=1.51

=== [S11] Blocking Transmit (write loop, final drain outside timing) ===
  diag=S11 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
[ 17.089911 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[ 17.126604 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
[ 17.127180 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  policy=no-drain size=64 iters=100 bytes=6400 short_writes=0 enqueue_ms=35 final_drain_ms=0 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=542.5 enqueue_kbps=174.68
  diag=s11-txdbg-reset size=64 ioctl_rc=-1
  diag=s11-txdbg phase=enqueue size=64 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=64 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-derived size=64 submit_fraction=0.9834 producer_available=0.0166 total_time_ms=36 enqueue_time_ms=35
[ 17.133660 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[ 17.283486 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
[ 17.284524 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  policy=no-drain size=256 iters=100 bytes=25600 short_writes=0 enqueue_ms=149 final_drain_ms=1 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=2170.1 enqueue_kbps=167.15
  diag=s11-txdbg-reset size=256 ioctl_rc=-1
  diag=s11-txdbg phase=enqueue size=256 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=256 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-derived size=256 submit_fraction=0.9931 producer_available=0.0069 total_time_ms=150 enqueue_time_ms=149
[ 17.290263 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[ 17.897157 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
[ 17.898378 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  policy=no-drain size=1024 iters=100 bytes=102400 short_writes=0 enqueue_ms=606 final_drain_ms=1 final_drain_rc=0 final_drain_errno=0 drain_calls=1 drain_errors=0 last_drain_errno=0 line_time_ms=8680.6 enqueue_kbps=164.95
  diag=s11-txdbg-reset size=1024 ioctl_rc=-1
  diag=s11-txdbg phase=enqueue size=1024 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-txdbg phase=final-drain size=1024 ioctl_rc=-1 user_calls=0 user_req=0 user_acc=0 ring_pop_calls=0 ring_pop_bytes=0 hw_send_calls=0 hw_send_bytes=0 hw_send_zero=0 hw_send_max_chunk=0 no_progress_budget=0 slow_poll_exh=0 yield_exh=0 ring_empty=0 copier_active=0 staged_bytes=0 transmitter_empty=0
  diag=s11-derived size=1024 submit_fraction=0.9980 producer_available=0.0020 total_time_ms=607 enqueue_time_ms=606

=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===
  diag=S12 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=batch-drain size=64 iters=100 batch=8 drains=13 bytes=6400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=33 line_time_ms=542.5 kbps=184.81 line_rate_pct=1604.2
  policy=batch-drain size=256 iters=100 batch=8 drains=13 bytes=25600 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=149 line_time_ms=2170.1 kbps=166.94 line_rate_pct=1449.1
  policy=batch-drain size=1024 iters=100 batch=8 drains=13 bytes=102400 drain_calls=13 drain_errors=0 last_drain_errno=0 elapsed_ms=587 line_time_ms=8680.6 kbps=170.25 line_rate_pct=1477.9

=== [S13] TX writev Fragments (fragment aggregation witness) ===
  diag=S13 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=writev-drain-each fragments=4 fragment_size=64 total_size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=149 line_time_ms=2170.1 kbps=166.78 line_rate_pct=1447.7

=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===
  diag=S14 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  policy=drain-each size=64 iters=100 bytes=6400 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=37 line_time_ms=542.5 kbps=166.31 line_rate_pct=1443.6
  diag=break-even-size-64 n=100 avg_ms=0.371 p50_ms=0.373 p95_ms=0.642 p99_ms=0.852 max_ms=0.852 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.16 p99_p50_ratio=2.28 max_p50_ratio=2.28
  policy=drain-each size=128 iters=100 bytes=12800 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=77 line_time_ms=1085.1 kbps=160.40 line_rate_pct=1392.3
  diag=break-even-size-128 n=100 avg_ms=0.775 p50_ms=0.761 p95_ms=1.363 p99_ms=1.801 max_ms=1.801 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.17 p99_p50_ratio=2.37 max_p50_ratio=2.37
  policy=drain-each size=256 iters=100 bytes=25600 short_writes=0 drain_calls=100 drain_errors=0 last_drain_errno=0 elapsed_ms=132 line_time_ms=2170.1 kbps=188.79 line_rate_pct=1638.8
  diag=break-even-size-256 n=100 avg_ms=1.319 p50_ms=1.410 p95_ms=2.419 p99_ms=4.301 max_ms=4.301 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.20 p99_p50_ratio=3.05 max_p50_ratio=3.05

=== [S20] TX Latency (single byte + tcdrain) ===
  diag=S20 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  diag=s20-single-byte n=100 avg_ms=0.042 p50_ms=0.043 p95_ms=0.049 p99_ms=0.212 max_ms=0.212 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=2.50 p99_p50_ratio=4.88 max_p50_ratio=4.88
  diag=S20 drain_calls=100 drain_errors=0 last_drain_errno=0

=== [S21] TX Latency FIFO Boundary Matrix ===
  diag=S21 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  diag=s21-fifo-size-1 n=100 avg_ms=0.032 p50_ms=0.036 p95_ms=0.043 p99_ms=0.099 max_ms=0.099 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=1.16 p99_p50_ratio=2.75 max_p50_ratio=2.75
  diag=fifo-size-1 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-15 n=100 avg_ms=0.108 p50_ms=0.107 p95_ms=0.213 p99_ms=0.416 max_ms=0.416 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.33 p99_p50_ratio=3.88 max_p50_ratio=3.88
  diag=fifo-size-15 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-16 n=100 avg_ms=0.122 p50_ms=0.118 p95_ms=0.212 p99_ms=0.283 max_ms=0.283 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.21 p99_p50_ratio=2.39 max_p50_ratio=2.39
  diag=fifo-size-16 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-17 n=100 avg_ms=0.108 p50_ms=0.110 p95_ms=0.204 p99_ms=0.245 max_ms=0.245 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.17 p99_p50_ratio=2.24 max_p50_ratio=2.24
  diag=fifo-size-17 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-31 n=100 avg_ms=0.189 p50_ms=0.195 p95_ms=0.333 p99_ms=0.388 max_ms=0.388 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.15 p99_p50_ratio=1.99 max_p50_ratio=1.99
  diag=fifo-size-31 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-32 n=100 avg_ms=0.206 p50_ms=0.185 p95_ms=0.377 p99_ms=1.159 max_ms=1.159 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.43 p99_p50_ratio=6.25 max_p50_ratio=6.25
  diag=fifo-size-32 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-33 n=100 avg_ms=0.207 p50_ms=0.198 p95_ms=0.388 p99_ms=0.464 max_ms=0.464 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.17 p99_p50_ratio=2.34 max_p50_ratio=2.34
  diag=fifo-size-33 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-48 n=100 avg_ms=0.280 p50_ms=0.288 p95_ms=0.484 p99_ms=0.695 max_ms=0.695 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.17 p99_p50_ratio=2.41 max_p50_ratio=2.41
  diag=fifo-size-48 drain_calls=100 drain_errors=0 last_drain_errno=0
  diag=s21-fifo-size-49 n=100 avg_ms=0.278 p50_ms=0.278 p95_ms=0.483 p99_ms=0.585 max_ms=0.585 slow_gt10ms=0 slow_over_line_plus10ms=0 max_line_ratio=0.14 p99_p50_ratio=2.11 max_p50_ratio=2.11
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
  instret_read_overhead=194196
  s41-size=64 expected_bytes=6400 iters=100
[ 19.276158 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=64 round=1 completed=6400 expected=6400 reset_rc=-1 instret_begin=7531622988623 instret_end=7531710325034 instret_delta=87336411 instructions_per_byte=13646.31 instructions_per_write=873364 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=35
[ 19.315521 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=64 round=2 completed=6400 expected=6400 reset_rc=-1 instret_begin=7531718019568 instret_end=7531806476217 instret_delta=88456649 instructions_per_byte=13821.35 instructions_per_write=884566 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=36
[ 19.354684 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=64 round=3 completed=6400 expected=6400 reset_rc=-1 instret_begin=7531812941167 instret_end=7531905621333 instret_delta=92680166 instructions_per_byte=14481.28 instructions_per_write=926802 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=38
[ 19.395915 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=64 round=4 completed=6400 expected=6400 reset_rc=-1 instret_begin=7531912526875 instret_end=7532009398822 instret_delta=96871947 instructions_per_byte=15136.24 instructions_per_write=968719 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=39
[ 19.438640 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=64 round=5 completed=6400 expected=6400 reset_rc=-1 instret_begin=7532015262807 instret_end=7532107160079 instret_delta=91897272 instructions_per_byte=14358.95 instructions_per_write=918973 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=37
  diag=s41-summary size=64 valid_rounds=5 median_instructions_per_byte=14358.95 median_instructions_per_write=918973
[ 19.480592 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s41-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
  s41-size=256 expected_bytes=25600 iters=100
[ 19.482771 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=256 round=1 completed=25600 expected=25600 reset_rc=-1 instret_begin=7532122373266 instret_end=7532481958917 instret_delta=359585651 instructions_per_byte=14046.31 instructions_per_write=3595857 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=148
[ 19.633645 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=256 round=2 completed=25600 expected=25600 reset_rc=-1 instret_begin=7532487048061 instret_end=7532832752709 instret_delta=345704648 instructions_per_byte=13504.09 instructions_per_write=3457046 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=142
[ 19.779442 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=256 round=3 completed=25600 expected=25600 reset_rc=-1 instret_begin=7532840612626 instret_end=7533193023654 instret_delta=352411028 instructions_per_byte=13766.06 instructions_per_write=3524110 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=145
[ 19.928180 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=256 round=4 completed=25600 expected=25600 reset_rc=-1 instret_begin=7533198709448 instret_end=7533541075120 instret_delta=342365672 instructions_per_byte=13373.66 instructions_per_write=3423657 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=141
[ 20.072118 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=256 round=5 completed=25600 expected=25600 reset_rc=-1 instret_begin=7533547707080 instret_end=7533890929338 instret_delta=343222258 instructions_per_byte=13407.12 instructions_per_write=3432223 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=100 duration_ms=141
  diag=s41-summary size=256 valid_rounds=5 median_instructions_per_byte=13504.09 median_instructions_per_write=3457046
[ 20.217428 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s41-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
  s41-size=1024 expected_bytes=102400 iters=100
[ 20.219071 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=1024 round=1 completed=102400 expected=102400 reset_rc=-1 instret_begin=7533904385038 instret_end=7535335835021 instret_delta=1431449983 instructions_per_byte=13979.00 instructions_per_write=3578625 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=400 duration_ms=591
[ 20.814311 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=1024 round=2 completed=102400 expected=102400 reset_rc=-1 instret_begin=7535343441330 instret_end=7536754925819 instret_delta=1411484489 instructions_per_byte=13784.03 instructions_per_write=3528711 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=400 duration_ms=583
[ 21.400892 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=1024 round=3 completed=102400 expected=102400 reset_rc=-1 instret_begin=7536762293118 instret_end=7538179883988 instret_delta=1417590870 instructions_per_byte=13843.66 instructions_per_write=3543977 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=400 duration_ms=585
[ 21.989672 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=1024 round=4 completed=102400 expected=102400 reset_rc=-1 instret_begin=7538186548413 instret_end=7539557739492 instret_delta=1371191079 instructions_per_byte=13390.54 instructions_per_write=3427978 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=400 duration_ms=566
[ 22.559054 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s41-valid size=1024 round=5 completed=102400 expected=102400 reset_rc=-1 instret_begin=7539564800861 instret_end=7540989217363 instret_delta=1424416502 instructions_per_byte=13910.32 instructions_per_write=3561041 begin_reason=ok end_reason=ok drain_rc=0 drain_errors=0 logical_writes=100 syscall_writes=400 duration_ms=588
  diag=s41-summary size=1024 valid_rounds=5 median_instructions_per_byte=13843.66 median_instructions_per_write=3543977
[ 23.151010 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s41-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1

=== [S42] TX Compute Overlap (64B x 100, fixed window, 5 sample rounds) ===
  diag=S42 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  idle window_ms=542.535 window_ns=542534722 iters=311639 duration_ms=542.576 iters_per_sec=574369
  overlap payload=64 iters=100 warmup=1 sample_rounds=5 theoretical_line_time_ms=542.535
[ 23.697896 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
[ 24.240970 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s42-sample round=1 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=27103.6 useful_iters=310386 useful_work_per_ms=572 final_drain_ms=0.026 total_duration_ms=542.564 total_over_line_ratio=1.000 overlap_efficiency=0.9960 reset_rc=-1 drain_errors=0 leftover_ns=3078
[ 24.784742 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s42-sample round=2 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=28215.2 useful_iters=311550 useful_work_per_ms=574 final_drain_ms=0.018 total_duration_ms=542.556 total_over_line_ratio=1.000 overlap_efficiency=0.9997 reset_rc=-1 drain_errors=0 leftover_ns=2278
[ 25.328996 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s42-sample round=3 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=27047.1 useful_iters=311198 useful_work_per_ms=574 final_drain_ms=0.027 total_duration_ms=542.565 total_over_line_ratio=1.000 overlap_efficiency=0.9986 reset_rc=-1 drain_errors=0 leftover_ns=2978
[ 25.873014 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s42-sample round=4 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=32725.6 useful_iters=303544 useful_work_per_ms=559 final_drain_ms=0.027 total_duration_ms=542.565 total_over_line_ratio=1.000 overlap_efficiency=0.9740 reset_rc=-1 drain_errors=0 leftover_ns=2878
[ 26.417192 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s42-sample round=5 completion=PASS byte_ok=1 drain_ok=1 completed=6400 expected=6400 write_return_us=32151.2 useful_iters=303956 useful_work_per_ms=560 final_drain_ms=0.017 total_duration_ms=542.554 total_over_line_ratio=1.000 overlap_efficiency=0.9753 reset_rc=-1 drain_errors=0 leftover_ns=2178
  diag=s42-summary valid_rounds=5 median_useful_iters=310386 median_total_duration_ms=542.564 median_overlap_efficiency=0.9960
[ 26.961893 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s42-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1

=== [S43] Timer Wakeup Overshoot (5 idle groups + 5 loaded groups) ===
  diag=S43 pre_section_stdout_drain_ms=0 drain_errors=0 last_errno=0
  s43-phase=idle groups=5 samples=50 interval_us=5000
  diag=s43-idle-group group=1 status=PASS collected=50 errors=0 valid=50 duration_ms=251 sample[0]=6841500 sample[1]=1984600 sample[2]=6334800
  s43-idle-group-summary n=50 errors=0 p50_ns=6077200 p95_ns=6608400 p99_ns=6841500 max_ns=6841500
  diag=s43-idle-group group=2 status=PASS collected=50 errors=0 valid=50 duration_ms=259 sample[0]=4327600 sample[1]=9482500 sample[2]=4522700
  s43-idle-group-summary n=50 errors=0 p50_ns=5067400 p95_ns=9898000 p99_ns=9982600 max_ns=9982600
  diag=s43-idle-group group=3 status=PASS collected=50 errors=0 valid=50 duration_ms=258 sample[0]=3487400 sample[1]=8228500 sample[2]=3274100
  s43-idle-group-summary n=50 errors=0 p50_ns=8228500 p95_ns=8782000 p99_ns=8822300 max_ns=8822300
  diag=s43-idle-group group=4 status=PASS collected=50 errors=0 valid=50 duration_ms=250 sample[0]=4406400 sample[1]=9497100 sample[2]=4535900
  s43-idle-group-summary n=50 errors=0 p50_ns=5013300 p95_ns=10001700 p99_ns=10131200 max_ns=10131200
  diag=s43-idle-group group=5 status=PASS collected=50 errors=0 valid=50 duration_ms=257 sample[0]=3284500 sample[1]=8177900 sample[2]=3224800
  s43-idle-group-summary n=50 errors=0 p50_ns=7837500 p95_ns=8451800 p99_ns=8561800 max_ns=8561800
  s43-phase=loaded groups=5 burst_bytes=4096 theoretical_line_time_ns=347222222
[ 28.246775 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s43-loaded-group group=1 status=PASS reset_rc=-1 burst_written=4096 expected=4096 write_dur_ns=13863700 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=244 drain_ms=0 drain_errors=0 sample[0]=8914400 sample[1]=3923900 sample[2]=3230600
  s43-loaded-group-summary n=50 errors=0 p50_ns=7890600 p95_ns=8421900 p99_ns=8914400 max_ns=8914400
[ 28.508967 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s43-loaded-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
[ 28.511297 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s43-loaded-group group=2 status=PASS reset_rc=-1 burst_written=4096 expected=4096 write_dur_ns=16822800 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=237 drain_ms=0 drain_errors=0 sample[0]=11859000 sample[1]=6870000 sample[2]=1878100
  s43-loaded-group-summary n=50 errors=0 p50_ns=6870000 p95_ns=9034500 p99_ns=11859000 max_ns=11859000
[ 28.769390 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s43-loaded-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
[ 28.770903 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s43-loaded-group group=3 status=PASS reset_rc=-1 burst_written=4096 expected=4096 write_dur_ns=18509700 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=235 drain_ms=0 drain_errors=0 sample[0]=13538800 sample[1]=8547900 sample[2]=3555200
  s43-loaded-group-summary n=50 errors=0 p50_ns=8275200 p95_ns=8966900 p99_ns=13538800 max_ns=13538800
[ 29.028224 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s43-loaded-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
[ 29.030646 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s43-loaded-group group=4 status=PASS reset_rc=-1 burst_written=4096 expected=4096 write_dur_ns=16852100 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=236 drain_ms=0 drain_errors=0 sample[0]=11875300 sample[1]=6883300 sample[2]=1889800
  s43-loaded-group-summary n=50 errors=0 p50_ns=6883300 p95_ns=8518000 p99_ns=11875300 max_ns=11875300
[ 29.286620 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s43-loaded-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
[ 29.288113 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070770 for fd: 4
  diag=s43-loaded-group group=5 status=PASS reset_rc=-1 burst_written=4096 expected=4096 write_dur_ns=19435600 incomplete_logical=0 syscall_calls=16 collected=50 errors=0 valid=50 sample_duration_ms=237 drain_ms=0 drain_errors=0 sample[0]=14456900 sample[1]=9465300 sample[2]=4472300
  s43-loaded-group-summary n=50 errors=0 p50_ns=6727900 p95_ns=7431700 p99_ns=14456900 max_ns=14456900
[ 29.546504 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 4
  s43-loaded-local-counters counters=not-available reason=ioctl-failed errno=25 snapshot_rc=-1 reset_rc=-1
  diag=s43-loaded-aggregate n=250 valid_groups=5 p50_ns=6727900 p95_ns=8960300 p99_ns=11875300 max_ns=14456900
  diag=s43-idle-aggregate n=250 valid_groups=5 p50_ns=5013300 p95_ns=9789700 p99_ns=10001700 max_ns=10131200

=== [S40] TX Counter Proxy Summary ===
[ 29.548206 0:10 starry_kernel::syscall::fs::ctl:59] Unsupported ioctl command: 1415070769 for fd: 3
  status=UNSUPPORTED reason=backend-polling-console-no-telemetry
  proxy=not-available

Done.
starry:/bin# 