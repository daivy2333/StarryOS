# D1 真板构建与烧录

## 适用范围

- Lichee RV Dock / Allwinner D1 真板编译、Android boot image 打包、烧录、恢复
- 不适用于：QEMU（见 `qemu-build.md`）、VisionFive2（Q24 待定）

## 前置条件

### 环境

```bash
# musl 工具链
export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH
riscv64-linux-musl-gcc --version   # 验证

# Rust 工具链 (来自 rust-toolchain.toml)
rustup show
```

### 平台事实

| 项 | 值 |
|---|---|
| SoC | Allwinner D1 / `sun20iw1p1` |
| CPU | T-HEAD C906，单核，Sv39 |
| RAM | `0x40000000 + 0x20000000`，512 MiB |
| UART0 | base `0x02500000`，IRQ 18，stride 4，32-bit MMIO |
| boot 分区 | `/dev/by-name/boot`（`/dev/mmcblk0p4`）|
| boot image | Android boot image，magic `ANDROID!`，name `d1-nezha`，page_size `2048` |
| kernel_addr | `0x40200000` |
| linker base | `0xffffffc040200000` |
| 启动链 | BOOT0 → OpenSBI v0.6 → U-Boot 2018.05 → Android boot image → kernel |
| U-Boot 启动命令 | `sunxi_flash read 45000000 boot; bootm 45000000` |

## 操作步骤

### 构建

| 命令 | 产物 | 说明 |
|---|---|---|
| `make lichee` | `starry-lichee-boot.img` | smoke test 基础构建 |
| `make lichee-kbench` | `starry-lichee-kbench-boot.img` | 内核态 benchmark |
| `make lichee-userbench` | `starry-lichee-userbench-boot.img` | 用户态 benchmark (embedded ELF) |
| `make lichee-fullbench-mem` | `starry-lichee-fullbench-mem-boot.img` | 全量 benchmark (memory-root) |
| `make lichee-fullbench-command` | `starry-lichee-fullbench-command-boot.img` | 全量 benchmark (command-entry) |

每个 target 自动执行：`cargo build` → `python3 tools/android_boot_image.py pack` → `inspect` 验证 header。

### 构建前验证 (必做)

```bash
# QEMU 模式 (确保改动未破坏 QEMU 构建)
cargo check --features qemu --target riscv64gc-unknown-none-elf
cargo clippy --features qemu --target riscv64gc-unknown-none-elf

# D1 smoke 模式
cargo check --features lichee-d1 --target riscv64gc-unknown-none-elf
cargo clippy --features lichee-d1 --target riscv64gc-unknown-none-elf

# D1 kbench 模式
cargo check --features lichee-d1-kbench --target riscv64gc-unknown-none-elf
cargo clippy --features lichee-d1-kbench --target riscv64gc-unknown-none-elf

# 三模式全部通过后才能声明构建就绪
```

### 烧录

```bash
# ① host 构建
make lichee-fullbench-command

# ② 将产物 .img 拷贝到 TF 卡 exUDISK 分区 (在 PC 上 mount 后 cp)

# ③ 在 D1 官方 Linux 中：
#    备份 (首次必做，只做一次)
dd if=/dev/by-name/boot of=/mnt/exUDISK/boot-official-backup.img bs=1M

#    烧录
dd if=/mnt/exUDISK/starry-lichee-fullbench-command-boot.img of=/dev/by-name/boot bs=1M conv=fsync

#    重启
sync && reboot -f
```

**注意**：使用 `/dev/by-name/boot` 而非 `/dev/mmcblk0p4`，by-name 路径是稳定接口。

### 恢复官方 Linux

```bash
dd if=/mnt/exUDISK/boot-official-backup.img of=/dev/by-name/boot bs=1M conv=fsync
sync && reboot -f
```

## 验证

### 构建 Gate

```bash
# 确认 link symbols 来自 D1 axplat (非 QEMU)
objdump -t StarryOS_riscv64-lichee-d1.bin | grep _start

# 确认 boot header 正确
python3 tools/android_boot_image.py inspect starry-lichee-*.img
# 期望: magic: ANDROID!, kernel_addr: 0x40200000, page_size: 2048

# 确认尺寸安全
ls -lh starry-lichee-*.img   # 必须 << 10MB
```

### 烧录验证

- 接串口 (115200 8n1)，reboot 后看到 `[starry-d1]` 输出
- benchmark 模式：进程 `exit 0`，S10 输出 64B ≥93% 线速，S11 short_writes=0

## 失败处理

| 症状 | 原因 | 处理 |
|---|---|---|
| 构建产物 >10MB | `DWARF=n` 未生效 | Makefile lichee target 已强制，不要手动覆盖 |
| 构建报 PCI/axdriver 符号缺失 | 未传 `BUS=mmio` | Makefile 已处理；手动构建务必加 `BUS=mmio` |
| 构建报 undefined `__IrqIf_*` | D1 axplat 缺 IRQ 接口 | kernel 中 `irq-if = ["axplat/irq"]` + irq_stub 已是标准配置 |
| `android_boot_image.py inspect` 报 bad magic | pack 失败或 .img 损坏 | 重新 `make lichee*` |
| axplat 版本不匹配 | cargo registry 拉到了最新版而非本地版本 | 以 `make build` 输出中实际编译的版本为准，不用 registry 最新版 |
| 烧录后无 StarryOS 输出 | PTE 属性缺失或 linker base 错误 | 检查 `xuantie-c9xx` feature，`objdump` 确认 entry `0xffffffc040200000` |
| 启动时 Store/AMO access fault | C9xx PTE 属性未在 final page table 启用 | `page_table_entry/xuantie-c9xx` 必须同时覆盖 early 和 final page table |
| UART 无输出但其他正常 | 误用 QEMU stride=1 配置 | D1 必须 stride 4 + 32-bit MMIO |
| 烧录后 virtio MMIO fault (VA `0xffffffc000000000`) | `virtio-mmio-ranges` 写成 `[[0,0]]` | D1 无 virtio-mmio，必须写成空数组 `[]` |

## 回滚

```bash
dd if=/mnt/exUDISK/boot-official-backup.img of=/dev/by-name/boot bs=1M conv=fsync
sync && reboot -f
```

**不可回滚场景**：首次烧录前未备份 `/dev/by-name/boot` → 需从其他 D1 板或官方镜像恢复 boot 分区。

## 注意事项

- **`DWARF=n` 铁律**：未传 `DWARF=n` 时 raw binary 可达 ~25MB，超过 boot 分区 ~10MB 容量。`rust-objcopy --strip-debug` 对此无效。Makefile 的 lichee target 已强制，不要手动移除。
- **`BUS=mmio` 必需**：D1 无 PCI，缺少此参数时 `axdriver` 编译 PCI bus 代码并查找不存在的 `PCI_RANGES`/`PCI_BUS_END`。
- **D1 泛采集已足够**：不要再回到官方 Linux 做无目标的数据采集（见 K19/L213），现有事实已覆盖 bring-up 所需。
- **Lichee smoke feature gate**：D1 smoke 验证只启用 boot + early console，必须隔离 fs/net/display/axdriver/PCI/task-ext；QEMU 完整路径通过 `starry-kernel/qemu` feature 保持。
- **增量融合铁律**：禁止一次性 apply 多个 async-uart 优化 commit。必须按依赖排序 + 每步 cargo check + benchmark Gate。
