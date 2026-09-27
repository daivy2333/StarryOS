# QEMU 构建与运行

## 适用范围

- QEMU riscv64-virt 平台编译、运行、回归验证
- 不适用于：真板烧录（见 `d1-build-and-flash.md`）、VisionFive2、LoongArch64/AArch64

## 前置条件

### 环境

```bash
# musl 工具链
export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH
riscv64-linux-musl-gcc --version   # 验证

# Rust 工具链 (来自 rust-toolchain.toml)
rustup show

# QEMU
qemu-system-riscv64 --version
```

### 首次 rootfs 准备

```bash
make rootfs   # 下载 rootfs-riscv64.img.xz → 解压 → 复制到 make/disk.img
```

## 操作步骤

### 构建

```bash
make build                    # 标准构建
make DWARF=n LTO=y build      # 带 LTO 发布构建 (开发默认关闭, per ADR-034)
```

### 运行

```bash
make run                      # 运行 (自动重建如有必要)
make rv                       # 别名 = make ARCH=riscv64 run
```

### benchmark

```bash
# 交叉编译 C 测试程序（新版，含 polling manifest）
make BENCH_CC=/opt/musl/riscv64-linux-musl-cross/bin/riscv64-linux-musl-gcc tests/benchmark

# 注入新 payload 到 QEMU rootfs
#   注意：make run 使用 make/disk.img，不是项目根目录的 disk.img
sudo mount -o loop make/disk.img /tmp/rootfs_mnt
sudo cp tests/benchmark /tmp/rootfs_mnt/bin/benchmark
sudo umount /tmp/rootfs_mnt
make run   # 进入 shell 后 ./benchmark
```

### 构建前验证

```bash
cargo check --features qemu --target riscv64gc-unknown-none-elf
cargo clippy --features qemu --target riscv64gc-unknown-none-elf
```

## 验证

```bash
make build   # exit 0
make run     # 看到 shell 提示符 $ 或 benchmark manifest
```

## 失败处理

| 症状 | 原因 | 处理 |
|---|---|---|
| `make build` 报 musl cc 找不到 | musl 工具链未在 PATH | `export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH` |
| `make rootfs` 下载失败 | 网络问题 | 手动下载 `rootfs-riscv64.img.xz` 到项目根并 `xz -d` |
| `make run` 报 disk.img not found | 未准备 rootfs | `make rootfs` |
| benchmark manifest 缺 polling 标签 | rootfs 中为旧 payload | 重新注入：`sudo mount -o loop make/disk.img /tmp/rootfs_mnt && sudo cp tests/benchmark /tmp/rootfs_mnt/bin/benchmark && sudo umount /tmp/rootfs_mnt` |

## 平台事实

| 项 | QEMU virt |
|---|---|
| UART base | `0x10000000` |
| UART stride | 1 (byte-MMIO) |
| kernel load | QEMU 直接加载 ELF（非 boot image） |
| BUS | mmio |
| 吞吐量可信度 | ❌ 不仿真串口线延迟，tcdrain 瞬时返回。所有吞吐量声明必须以真板数据为准 |

## 注意事项

- **QEMU 不能测绝对吞吐**：QEMU 16550 模型不仿真串口线延迟 (86.8 µs/byte)。内核态 ring buffer 速度、write() 延迟、CPU cycles/byte 可在 QEMU 上测；绝对线速必须用真板。
- **增量融合铁律**：禁止一次性 apply 多个 async-uart 优化 commit。必须按依赖排序 + 每步 cargo check + QEMU benchmark Gate。
- **禁止删除 temp 分支**：保留作为增量融合的参考基线。
- **cargo check --manifest-path 副作用**：对 workspace 内 path dependency 执行可能升级 Cargo.lock 中未锁死的依赖。检查后如有漂移用 `git restore Cargo.lock` 恢复。
