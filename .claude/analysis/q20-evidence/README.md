# Q20 Benchmark Gap Closure — Evidence

## Build Commands

### QEMU rootfs benchmark
```bash
export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH
make tests/benchmark              # 编译 benchmark 二进制
/sbin/debugfs -w disk.img -R "write tests/benchmark /bin/benchmark"  # 注入 rootfs
cp disk.img make/disk.img
```

### D1 command-entry benchmark
```bash
export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH
make benchmark-fullbench-elf          # 编译（含 BENCH_D1_DIAG + telemetry）
make lichee-fullbench-command         # 打包 Android boot image

# 烧录（在 D1 官方 Linux 中执行）
dd if=/dev/by-name/boot of=/mnt/exUDISK/boot-official-backup.img bs=1M  # 先备份
sync
dd if=/mnt/exUDISK/starry-lichee-fullbench-command-boot.img of=/dev/by-name/boot bs=1M conv=fsync
sync
reboot -f
# 串口 115200 8N1；fullbench command-entry 会自动构造 /bin/benchmark 并等待退出

# 恢复官方 boot
dd if=/mnt/exUDISK/boot-official-backup.img of=/dev/by-name/boot bs=1M conv=fsync
sync
reboot -f
```

## Run Commands

### QEMU
```bash
make justrun   # QEMU 启动后，shell 中执行 /bin/benchmark
```

### D1
```bash
# 在 D1 官方 Linux 中烧录（详见上方 Build Commands）
# 重启后 StarryOS fullbench command-entry 自动运行 /bin/benchmark
# 串口保存完整启动和 benchmark 输出
```

## Expected Benchmark Sections

| Section | Description | Q20 Changes |
|---------|-------------|-------------|
| S00 | Manifest | None |
| S10 | TX Throughput (write + tcdrain) | jitter diag now includes `p99_p50_ratio`, `max_p50_ratio` |
| S11 | TX Enqueue (no-drain) | txdbg output now always available (was BENCH_D1_DIAG only) |
| S12 | TX Batch Drain | None |
| S13 | TX writev | None |
| S14 | TX Small Packet Break-even | jitter diag now includes `p99_p50_ratio`, `max_p50_ratio` |
| S20 | TX Latency (1B) | **Changed**: now uses `print_tx_latency_diag()` with diag fields |
| S21 | TX Latency FIFO Matrix | **Changed**: now uses `print_tx_latency_diag()` per size |
| S30 | RX Non-blocking | None |
| S31 | RX Fixed Payload | Intentionally skipped (RX excluded from Q20) |
| **S40** | **TX Counter Proxy Summary** | **NEW**: raw counters + derived proxy fields |

## S40 Output Format

### Raw counters
- `counter=user-push`: user_push_calls, user_req, user_acc
- `counter=ring-pop`: ring_pop_calls, ring_pop_bytes
- `counter=hw-send`: hw_send_calls, hw_send_bytes, hw_send_zero, hw_send_max_chunk
- `counter=no-progress`: no_progress_budget, slow_poll_exh, yield_exh
- `counter=drain-state`: ring_empty, copier_active, staged_bytes, transmitter_empty

### Derived proxy (when counters are available)
- `bytes_per_user_call`, `bytes_per_ring_pop`, `bytes_per_hw_send`
- `zero_per_kb`, `no_progress_per_kb`

On QEMU without effective TX debug counters: `proxy=derived status=not-available reason=telemetry-counters-are-zero`.
This is accepted for Q20; QEMU still proves the output shape and jitter fields, while D1 provides the effective counter proxy.

## Evidence Status

| Evidence | File | Status |
|----------|------|--------|
| QEMU rootfs raw log | qemu-rootfs.log | Completed from `docs/out.md` |
| D1 serial raw log | d1-fullbench-command.log | Pending recapture |
| RX fixed payload | — | Intentionally excluded (design D1) |

## Notes
- Q20 does NOT claim SMP correctness
- Q20 does NOT modify UART driver semantics (only benchmark output)
- Counter proxy is labeled as proxy evidence, not precise CPU utilization
- QEMU throughput is path/relative-behavior evidence only, not physical line-rate
