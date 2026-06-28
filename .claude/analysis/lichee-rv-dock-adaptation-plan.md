# Lichee RV Dock 适配方案

> 生成时间：2026-06-28
> 范围：StarryOS 在 Sipeed Lichee RV Dock / Allwinner D1 上的最小启动适配方案。
> 关联文档：`docs/licheerv-dock-bringup.md`、`.claude/analysis/lichee/public-platform-notes.md`、`.claude/analysis/platform-parameter-decoupling.md`

## 1. 目标与非目标

当前目标不是一次性把完整 StarryOS 用户态、文件系统、USB 和 benchmark 都搬到 Lichee RV Dock，而是先完成一个可验证、可回滚、可逐步扩展的真板启动闭环。

第一阶段成功标准：

- 使用现有官方镜像的启动链或同等路径加载 StarryOS 测试镜像。
- StarryOS 在 UART0 串口输出固定 smoke test 字符串，例如 `[starry-d1] early boot`。
- 失败时可以通过恢复 boot 分区备份回到官方 Linux。

明确非目标：

- 不用 Lichee RV Dock 验证 Q17 SMP / 多核内存序问题。D1 / C906 是单核平台。
- 不在第一阶段适配 USB、SD/MMC、rootfs、shell、异步 benchmark。
- 不在第一阶段依赖完整 async UART 栈。先用最小 polling early console 降低 bring-up 变量。
- 不从官方 Linux 继续做无目标的数据采集。现有参数已经足够进入 early console smoke test。

## 2. 已确认硬件与启动基线

| 项 | 结论 |
|----|------|
| 板卡 | Sipeed Lichee RV Dock |
| SoC | Allwinner D1 / `sun20iw1p1` |
| CPU | T-HEAD C906，单 hart，Sv39 |
| ISA | Linux 报告 `rv64imafdcvu` |
| RAM | `0x40000000 + 0x20000000`，512 MiB |
| Linux 可用内存 | `0x40200000-0x5fffffff` |
| 启动链 | `BOOT0 -> OpenSBI v0.6 -> U-Boot 2018.05 -> Android boot image -> Linux` |
| OpenSBI | firmware base `0x40000400`，SBI v0.2 |
| boot 分区 | `/dev/by-name/boot` = `/dev/mmcblk0p4` |
| boot image | Android boot image，magic `ANDROID!` |
| U-Boot 加载命令 | `sunxi_flash read 45000000 boot; bootm 45000000` |
| kernel_addr | Android boot header `0x40200000` |
| page_size | Android boot header `2048` |
| console | `ttyS0,115200`，`stdout-path = serial0:115200n8` |
| UART0 | base `0x02500000`，size `0x400`，PLIC IRQ `18` |
| UART 类型 | DesignWare APB UART，mainline DTS 为 `snps,dw-apb-uart` |
| UART 访问模型 | `reg-shift = 2`，`reg-io-width = 4`，即 stride 4、32-bit MMIO |
| PLIC | base `0x10000000`，size `0x04000000`，source count 175 |
| Timer | D1 SoC timer base `0x02050000`，timebase 24 MHz |

这些事实来自官方 Linux 运行采集程序、boot 分区探测、启动日志和公开 DTS 对照。原始材料保存在 `.claude/analysis/lichee/`。

## 3. 对当前代码的影响

当前 StarryOS 的 UART 初始化路径主要面向 QEMU virt：

- `kernel/src/entry.rs` 在内核初始化早期调用 `uart_init::init_uart_hardware()`。
- `kernel/src/drivers/uart_init.rs` 固定使用 QEMU UART MMIO base `0x10000000`。
- `kernel/src/drivers/uart_init.rs` 当前按 NS16550 byte-addressed 模型配置，stride 为 1。
- `kernel/src/drivers/ntty_async.rs` 和 `kernel/src/pseudofs/dev/mod.rs` 将异步 TTY 注册到 `/dev/console`。
- `../uart_16550/src/backend/mmio.rs` 支持 stride 参数，但当前 volatile 读写宽度是 `u8`。

因此，Lichee RV Dock 不能只改 UART base 地址。D1 UART 虽然兼容 8250 寄存器语义，但 MMIO 访问是 32-bit、stride 4。直接复用当前 `MmioBackend` 的 8-bit volatile 访问有风险，early 阶段应绕开完整 async UART 栈。

## 4. 总体方向

推荐采用分阶段收敛路线：

1. 先确认 Android boot image 的打包与回滚流程。
2. 新增 D1 / Lichee RV 平台最小配置，完成链接地址、入口、内存边界。
3. 写一个 D1 UART0 polling early console，只做 32-bit MMIO 写字符。
4. 用 boot image 替换 boot 分区进行 smoke test。
5. smoke test 成功后，再接入 SBI timer、PLIC、UART IRQ。
6. 最后再评估是否扩展 `uart_16550` backend，接入 `/dev/console` 与 benchmark。

这个方向的核心约束是：第一阶段只保留启动链、链接地址、UART polling 三个变量。不要同时引入中断、异步调度、文件系统和 USB。

## 5. 技术方案

### 5.1 镜像与启动

现有官方镜像的 boot 分区是 Android boot image。U-Boot 环境变量显示启动流程为：

```text
boot_normal=sunxi_flash read 45000000 ${boot_partition};bootm 45000000
boot_partition=boot
```

适配 StarryOS 时优先沿用这个路径：

- 生成一个 Android boot image，内核 payload 放到 `kernel` 区。
- `kernel_addr` 使用 `0x40200000`，与官方镜像 header 保持一致。
- `page_size` 使用 `2048`。
- ramdisk 第一阶段可以为空或保留最小占位。
- 写入前必须备份 `/dev/by-name/boot`。

第一阶段不建议改 U-Boot 环境变量。保持 U-Boot 从 boot 分区读入并 `bootm`，能减少不可恢复风险。

### 5.2 链接地址与内存

D1 DRAM 从 `0x40000000` 开始，OpenSBI 位于 `0x40000400` 附近，Linux 实际从 `0x40200000` 起使用。StarryOS smoke test 应以 `0x40200000` 作为内核加载 / 链接基线，避免覆盖 OpenSBI 和早期保留区。

需要确认的工程项：

- D1 平台 linker script 或 axconfig 中设置 kernel base 为 `0x40200000`。
- 栈、bss、heap 初始区域都落在 `0x40200000-0x5fffffff` 内。
- 初期不要假设 rootfs 或块设备已经可用。

### 5.3 UART early console

第一阶段只实现 polling console：

- UART0 base：`0x02500000`
- register stride：4
- access width：32-bit
- baud：沿用 U-Boot / Linux 已配置的 `115200`
- 初期不启用 UART IRQ，不配置 FIFO 高级特性。

建议行为：

- 轮询 LSR 的 THRE 位，确认发送保持寄存器可写。
- 向 THR 写入字符。
- 输出 `\n` 时补 `\r`，方便串口工具显示。

如果无输出，优先检查：

- 镜像是否真的被 U-Boot 加载到 `0x40200000`。
- early console 是否使用了 32-bit volatile 访问。
- UART base 是否误用为 QEMU 的 `0x10000000`。
- 是否误用了 stride 1。

### 5.4 PLIC 与中断

PLIC 基线：

- base：`0x10000000`
- UART0 source：18
- source count：175

PLIC 不应进入第一阶段 smoke test 的关键路径。等 polling console 稳定后，再做 UART IRQ：

- 初始化 PLIC priority / enable / threshold。
- 只打开 UART0 IRQ 18。
- 输出中断进入计数，而不是直接接 async TTY。
- 确认中断 claim / complete 机制正确后，再接上层驱动。

### 5.5 Timer

早期优先使用 SBI timer。D1 SoC timer 虽然已经确认 base `0x02050000`，但第一阶段无需依赖它。

推荐顺序：

1. early console 成功后，打印 hart id、SBI version、timebase。
2. 使用 SBI set_timer 做一个最小 tick / delay 诊断。
3. 只有在需要绕过 SBI 或调试高精度计时时，再研究 D1 SoC timer。

### 5.6 async UART 与 `/dev/console`

完整 async UART 适配应放在 early console 之后。原因是当前 `uart_16550` MMIO backend 只有 8-bit volatile 访问，不能直接表达 D1 的 32-bit MMIO 访问模型。

后续有三种选择：

| 方案 | 描述 | 适用性 |
|------|------|--------|
| A | 在 StarryOS 平台内保留 D1 专用 polling console | 适合 early 阶段，不适合完整 TTY |
| B | 扩展 `uart_16550` backend，支持 access width 1/4 | 适合复用现有 async UART 栈 |
| C | 新增 `dw_apb_uart` 适配层或 crate | 更符合 D1 真实硬件，但工程量更大 |

推荐路线是先做 A，smoke test 通过后评估 B。只有当 D1 的 DesignWare UART 行为明显偏离 8250 时，再转向 C。

## 6. Milestone 规划

| Milestone | 目标 | 通过标准 |
|-----------|------|----------|
| M0: 基线冻结 | 整理采集数据与公开资料 | `docs/licheerv-dock-bringup.md` 和本分析文档已记录真实参数 |
| M1: boot image 工具链 | 能解析、备份、重新打包 boot image | 原始 boot header 字段可复现，备份可恢复官方 Linux |
| M2: D1 平台骨架 | 新增 Lichee / D1 构建目标 | 镜像链接到 `0x40200000`，构建产物可生成 |
| M3: early console | UART0 polling 输出 | 串口看到 `[starry-d1] early boot` |
| M4: SBI 诊断 | 打印 hart id、SBI version、timer 结果 | 串口输出稳定，定时器不破坏启动 |
| M5: PLIC/UART IRQ | UART0 中断最小验证 | IRQ 18 可 claim / complete，计数可见 |
| M6: async console | 接入 `/dev/console` 与 TTY | shell 或最小 console I/O 能工作 |
| M7: benchmark | 运行串口相关 benchmark | 结果可复制回开发机，并与 QEMU / VisionFive2 区分记录 |

## 7. 风险与缓解

| 风险 | 表现 | 缓解 |
|------|------|------|
| boot 分区写坏 | 官方 Linux 无法启动 | 写入前备份 `/dev/by-name/boot`，实验使用备用 TF 卡 |
| boot image 格式不匹配 | U-Boot `bootm` 失败 | 先解析并复现官方 Android boot header，不急于改 U-Boot env |
| 链接地址错误 | 无串口输出或早期 trap | 使用 `0x40200000`，避免覆盖 OpenSBI |
| UART 访问宽度错误 | 串口无输出 | D1 必须 stride 4、32-bit MMIO，不能直接用 QEMU stride 1 |
| 同时打开太多子系统 | 失败原因不可定位 | M3 前不启用 PLIC、rootfs、USB、async TTY |
| 单核平台误用于 SMP 验证 | Q17 结论无效 | Lichee RV Dock 只做流程和平台适配演练 |

## 8. 后续工程清单

短期应执行：

- 备份官方 boot 分区并保存到开发机。
- 选择或编写 Android boot image 打包工具链，先解析官方 header。
- 在 StarryOS 中新增 D1 / Lichee RV 平台配置草案。
- 实现 D1 UART0 polling early console。
- 生成第一版 smoke test 镜像并烧录到备用 TF 卡验证。

smoke test 通过后再执行：

- 接入 SBI timer 诊断。
- 接入 PLIC 和 UART IRQ。
- 评估 `uart_16550` backend 的 32-bit MMIO access width 扩展。
- 接回 `ASYNC_TTY` 与 `/dev/console`。
- 规划 benchmark 在 StarryOS 原生环境中的运行方式。

## 9. 当前结论

Lichee RV Dock 适配已经没有“必须继续从官方 Linux 采集数据”的阻塞项。现有信息足够支持 StarryOS 的第一阶段真板 smoke test。

真正的下一步不是继续采集，而是开始工程化拆分：先完成 boot image 工具链和 D1 early console，再逐步把 PLIC、timer、TTY 和 benchmark 加回来。
