# K3 用户态段错误的二进制地址判读

- Status: active
- Last validated: 2026-10-16
- Environment: WSL2 Ubuntu 宿主（debugfs 1.46.5、`readelf`/`riscv64-linux-gnu-objdump`/`rust-objdump` 均可用）；SpacemiT K3 CoM260 Kit 组合镜像（FIT kernel 0x180000000 / DTB 0x2ef000000）；动态 BusyBox 1.37.0 + musl ext4 RAM rootfs（仓库根 `rootfs-riscv64-min.img`）
- Source: ms14 change `ms14-com260-kit-multi-hart-async-io` Iteration 004 Cycle `000-initial` Act Response「T13 定位」节（2026-10-16 真板 LOG=info 诊断轮）

## 适用范围

用户态进程在 K3 板上段错误且串口日志给出 `segmentation fault pc=… va=… sp=… ra=…` 行时，把 pc/ra/va 判读为"哪个二进制的哪条指令/哪个函数"，并区分正常取指目标与被污染地址。不适用于：纯内核 Oops（无用户地址空间上下文）、QEMU 软件模型（地址布局不同）、静态单hart载荷以外的其他 ELF 布局未经本流程验证。

## 前置条件

- 交付 FIT 备份已留存（诊断构建会覆盖仓库根 FIT）。
- 宿主具备：`debugfs`（e2fsprogs，只读导出用，**不加 `-w`**）、`readelf`、支持 riscv64 的 objdump（`rust-objdump`（cargo-binutils）或 `riscv64-linux-gnu-objdump`；x86 宿主自带的 `objdump` 不能反汇编 riscv64）。
- 知道两个加载基址：`kernel/src/config/riscv64.rs` 的 `USER_SPACE_BASE=0x1000`（主程序）与 `USER_INTERP_BASE=0x4000000`（动态解释器）。
- LOG=off 构建看不到 fault 行，须先做诊断重建（见步骤 1）。

## 操作步骤

1. **取得 fault 行（LOG=off 构建时）**：备份仓库根 FIT → 手动按 Makefile `k3-smp-combined` 目标同参重建、仅改 `LOG=info`（不改 Makefile、不改产品代码）：

   ```sh
   cp StarryOS_riscv64-k3-smp-combined.fit /tmp/k3-t12/…-bak.fit
   make ARCH=riscv64 APP_FEATURES=k3-smp-combined MYPLAT=axplat-riscv64-k3 \
     PLAT_CONFIG=$(pwd)/crates/axplat-riscv64-k3/axconfig.toml MEM=6400M BUS=mmio DWARF=n \
     LOG=info SMP=16 IP=192.168.137.2 GW=192.168.137.1 \
     TARGET_DIR=$(pwd)/target/k3-smp-combined OUT_CONFIG=$(pwd)/.axconfig-k3-smp-combined.toml \
     OUT_DIR=$(pwd)/out-k3-smp-combined build
   make --no-print-directory k3-smp-combined-fit
   ```

   板测后从串口日志取整段 fault 行。**只改 LOG 参数、零产品代码变更为本步边界**；修复实施后须重建 LOG=off 恢复交付形态。

2. **只读导出 ELF**：

   ```sh
   mkdir -p /tmp/k3-t12
   debugfs -R 'dump /bin/busybox /tmp/k3-t12/busybox' rootfs-riscv64-min.img
   debugfs -R 'dump /lib/ld-musl-riscv64.so.1 /tmp/k3-t12/ld-musl-riscv64.so.1' rootfs-riscv64-min.img
   chmod +x /tmp/k3-t12/busybox /tmp/k3-t12/ld-musl-riscv64.so.1
   ```

   debugfs 不加 `-w` 即只读，源镜像不变。其他二进制按镜像内实际路径同样导出。

3. **认定基址与入口归属**：`readelf -h` 两个 ELF 的 Entry point；内核日志 `Enter user space: ip=…` 应等于 `USER_INTERP_BASE + ldso_entry`（本例 0x4000000+0x56900=0x4056900），证实内核入口选择正确、动态解释器先跑。

4. **判地址属于哪个二进制**：`readelf -lW` 看 LOAD 段区间，`readelf -SW` 看 `.plt`/`.text` 边界。映射地址 = 日志地址 − 基址（busybox 基址 0x1000、ldso 基址 0x4000000）；落在段内即为该二进制的链接地址。日志 pc/ra 各自独立判归属，不能假设同属一个 ELF。

5. **反汇编定位**：对链接地址用 `rust-objdump -d --start-address=<x> --stop-address=<y> <elf>`（起始地址不落在指令边界时向前多取几十字节再对齐读）。识别 sepc 处指令、ra 处调用点；直跳 `jal` 的目标与 sepc 吻合是强证据（本例 0x4e958 `jal 0x49660` 与 sepc 映射 0x4a660=0x49660+0x1000 完全一致）。stripped 二进制无符号表时 objdump 以 `<main+0x…>` 相对符号和段边界定位，`readelf --dyn-syms` 查动态符号。

6. **指针/重定位类问题**：`readelf -rW` 看重定位类别（`R_RISCV_RELATIVE`/`JUMP_SLOT`/`R_RISCV_64`）与 addend，判断坏指针属于哪类表项、目标偏移落在哪个 ELF 的范围。

7. **stval 可信度检查**：SV39 下先判 va 规范性（bit 63:39 必须全同）；取指页 fault 的 sepc 应等于 stval，**二者不一致即 stval 可疑**（本例 sepc=0x4a660 正确、stval=0xffffff000004a660 非规范 = sepc|0xffffff0000000000，判定 K3 核心对该类 fault 间歇性交付污染 stval）。

## 验证

本流程 2026-10-16 在 ms14 T13 定位中端到端执行成功：fault 行 `pc=0x4a660 va=VA:0xffffff000004a660 EXECUTE|USER sp=0x3fffffbd0 ra=0x4f95c` → 步骤 3 确认入口归属 → 步骤 4 判 pc/ra 属 busybox 正文（0x49660/0x4e95c）→ 步骤 5 反汇编 `4e958: jal 0x49660` 与 sepc 吻合 → 步骤 7 判 stval 污染。完整证据链见 Source 节 Act Response。

## 失败处理

- 宿主 `objdump` 报 "file format not recognized"/无法反汇编：换 `rust-objdump` 或 `riscv64-linux-gnu-objdump`。
- `readelf --dyn-syms` 查不到地址符号：二进制 stripped 属正常，改用段边界 + 相对符号 + 反汇编上下文定位。
- 反汇编起始地址落在指令中间输出错乱：把 start-address 前移重跑再对齐读。
- 板上无 fault 行：确认构建确为 LOG=info（`Logging is enabled.` 行）、fault 前 `info!` 是否被所在路径绕过；仍无则该路径可能未走 `info!`，改查对应退出路径的 `exit with code` 行。
- 诊断轮仍无法归因：停止盲试，保留日志与 ELF 副本交回 Plan 调查。

## 回滚

- 全部导出物在 `/tmp/k3-t12/`：`rm -rf /tmp/k3-t12` 即清空。
- 诊断构建：以 Makefile 原目标 `make k3-smp-combined SMP=16`（LOG=off）重建并重出 FIT 即恢复交付形态；或用步骤 1 的备份 FIT 直接还原。
- 源镜像自始只读，无需回滚。

## 证据

- `openspec/changes/ms14-com260-kit-multi-hart-async-io/iterations/004-final-use-and-performance/000-initial.md` Act Response「T13 定位」「T13 根因定位与修复」节（2026-10-16）：fault 行原文、readelf/objdump 判读结论、stval 污染判定与修复闭环。
- 工具输出现场：本轮会话（debugfs stat/export、`file` ELF 判型、readelf -h/-lW/-SW/-rW/--dyn-syms、rust-objdump 0x4e940–0x4e978 与 0x49640–0x49690 段）。
- 限制：地址基址常量取自当时 `kernel/src/config/riscv64.rs`；若加载器基址策略变更，步骤 3 的基址须重新核对。
