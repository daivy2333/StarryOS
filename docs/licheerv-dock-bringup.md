# Lichee RV Dock Bring-up 流程笔记

> 目标：在星光 2 真板到手前，先用手头的 Lichee RV Dock 打通 RISC-V 真板开发流程，包括烧录、串口、启动日志、板级信息采集、用户态测试部署和后续 StarryOS 平台适配准备。
>
> 当前结论：Lichee RV Dock 适合作为真板流程演练板，但不适合作为 Q17 SMP / 多核内存序验证板。它使用 Allwinner D1 / XuanTie C906，属于单核 RISC-V 平台。

> 2026-06-28 更新：官方 Linux 采集阶段已完成。当前信息量已经足够进入 StarryOS Lichee RV Dock early console smoke test 适配；后续不再需要继续从官方 Linux 泛采集数据。除非 smoke test 失败并指向 bootloader / clock / UART 细节，否则新的官方 Linux 采集不是阻塞项。

## 1. 定位与边界

Lichee RV Dock 的主要价值不是直接验证当前 StarryOS 的 Q17 SMP 修复，而是提前打通以下流程：

- TF 卡烧录与启动链观察
- USB-TTL 串口连接与日志采集
- 官方 Linux 下的板级信息采集
- 测试程序交叉编译、上传、运行、结果回收
- UART / PLIC / timer / RAM / DTB 信息整理
- 为后续星光 2 bring-up 建立可复用检查清单

当前 StarryOS 仓库默认面向 QEMU virt，并已有 `vf2` 构建入口。异步 UART 初始化仍使用 QEMU virt 的 UART MMIO 地址 `0x10000000`。Lichee RV Dock 是 Allwinner D1 平台，UART、PLIC、timer、内存布局、启动协议均不同，因此不能直接把当前 StarryOS 镜像烧进 TF 卡运行。

当前适配入口已经清晰：先做 D1 platform + Android boot image + UART0 early console，只要求串口输出一行 smoke test 日志；rootfs、USB、SD/MMC、Shell、async benchmark 都放到后续阶段。

### Roadmap 对齐（2026-06-28）

根据 `.claude/analysis/platform-parameter-decoupling.md` 和 `.claude/analysis/lichee-rv-dock-adaptation-plan.md`，Lichee RV Dock 不再作为孤立实验推进，而是纳入 StarryOS 后续 milestone：

| Milestone | 与 Lichee RV Dock 的关系 | Gate |
|-----------|--------------------------|------|
| Q17 | SMP / 内存序修复，Lichee 不用于验证多核，但应先消除通用 async UART 风险 | QEMU benchmark 无退化 |
| Q18 | 平台参数解耦 / early console 基础，为 Lichee 和 VisionFive2 共享前置 | QEMU 行为保持，driver 不再新增板级硬编码 |
| Q19 | Lichee RV Dock early smoke test 主阶段 | 串口输出 `[starry-d1] early boot` |
| Q20 | VisionFive2 UART 验证，复用 Q18/Q19 形成的平台边界和 bring-up 经验 | VisionFive2 真板基线落档 |

因此，Lichee 的下一步工程入口不是继续采集官方 Linux 信息，而是在 Q18 后进入 Q19：Android boot image 工具链、D1 platform descriptor、D1 UART0 32-bit polling early console。

### 分支策略

Lichee RV Dock 的测试分支不应直接基于主开发分支创建。正确顺序是：

1. `feat/uart-16550-async` 继续作为主开发分支。
2. `feat/uart-16550-bench` 作为测试分支，先同步主开发分支的最新实现。
3. 在 `feat/uart-16550-bench` 上维护 benchmark / 测试兼容代码，并确认 QEMU benchmark 正常。
4. 从已经验证过的 `feat/uart-16550-bench` 派生 `uart-16550-lichee`，只做 Lichee RV Dock 相关测试与适配探索。

推荐命令：

```bash
git switch feat/uart-16550-bench
git merge feat/uart-16550-async
# 解决冲突后，先完成 QEMU benchmark 验证

git switch -C uart-16550-lichee feat/uart-16550-bench
```

这样可以保证 Lichee RV Dock 分支继承测试分支里的 benchmark 代码，而不是直接从开发分支开始重复补测试兼容。

## 2. 推荐阶段

### 阶段 A：跑通官方 Linux

先使用官方镜像验证板子、TF 卡、串口和启动链全部正常。

准备：

- Lichee RV Dock + Lichee RV 核心板
- TF 卡，建议至少两张：一张官方 Linux，一张后续实验
- USB-TTL 串口模块，电平使用 3.3V TTL
- 串口线：GND 接 GND，板子 TX 接 USB-TTL RX，板子 RX 接 USB-TTL TX
- 串口工具：`picocom`、`minicom`、`screen` 均可
- 烧录工具：优先按官方文档使用 PhoenixCard；如果镜像明确是 raw image，再考虑 Linux 下 `dd`

串口建议参数：

```bash
picocom -b 115200 /dev/ttyUSB0
```

如果看不到输出，优先检查：

- 串口设备名是否正确：`ls /dev/ttyUSB*`
- TX/RX 是否接反
- 是否共地
- 是否使用 3.3V TTL，不能使用 RS232 电平
- 波特率是否为 `115200`
- TF 卡镜像是否烧录为启动卡

官方资料入口：

- Lichee RV 烧录系统：<https://wiki.sipeed.com/hardware/zh/lichee/RV/flash.html>
- Lichee RV 基础上手：<https://wiki.sipeed.com/hardware/zh/lichee/RV/user.html>
- Lichee RV Dock 硬件介绍：<https://wiki.sipeed.com/hardware/zh/lichee/RV/Dock.html>

阶段 A 的完成标准：

- 串口能看到 BOOT0 / OpenSBI / U-Boot / Linux 启动日志
- 能登录官方系统
- 能确认网络或串口文件传输方式可用
- 能保存完整启动日志

### 阶段 B：采集板级信息

进入官方 Linux 后，先采集平台基础信息。后续做 StarryOS 平台适配时，这些数据比口头参数更可靠。

建议建立目录：

```bash
mkdir -p ~/bringup-licheerv
cd ~/bringup-licheerv
```

采集命令：

```bash
uname -a | tee uname.txt
cat /proc/cpuinfo | tee cpuinfo.txt
cat /proc/meminfo | tee meminfo.txt
cat /proc/iomem | tee iomem.txt
cat /proc/interrupts | tee interrupts.txt
dmesg | tee dmesg.txt
```

设备树采集：

```bash
find /sys/firmware/devicetree/base -maxdepth 4 -type f | sort > dt-files.txt
tar -czf devicetree.tar.gz -C /sys/firmware/devicetree base
```

重点提取字段：

| 信息 | 需要记录 |
|------|----------|
| CPU | SoC 型号、核心型号、ISA 字符串、hart 数量 |
| RAM | 物理起始地址、可用容量、保留区域 |
| UART | console UART 节点、MMIO base、IRQ、baud、compatible |
| 中断控制器 | PLIC base、interrupt context、UART IRQ number |
| timer | CLINT / ACLINT / SBI timer 路径 |
| 启动链 | BOOT0、OpenSBI、U-Boot、Linux 的完整日志 |
| DTB | boot 分区或 `/sys/firmware/devicetree/base` 中的设备树 |

如果系统带 `dtc`，可以导出 DTS：

```bash
dtc -I fs -O dts /sys/firmware/devicetree/base > licheerv.dts
```

如果没有 `dtc`，先保存 `devicetree.tar.gz`，回开发机后再分析。

### 已采集参数（2026-06-28）

以下数据来自官方 Linux 上运行 `tests/explorer.c` 的输出，原始记录保存在 `.claude/analysis/lichee/新建 文本文档.txt`。

| 项 | 当前值 |
|----|--------|
| SoC / model | `allwinner,d1` / `sun20iw1p1` |
| 当前烧录镜像 | `LicheeRV_Tina_hdmi_8723ds.img` |
| Linux | `5.4.61`，节点名 `MaixLinux` |
| CPU | 单核 `hart 0`，ISA `rv64imafdcvu`，MMU `sv39` |
| RAM (DT) | base `0x40000000`，size `0x20000000` |
| RAM (Linux iomem) | `0x40200000-0x5fffffff` 可用系统内存，内核占用 `0x40200000` 起 |
| Console | `ttyS0,115200`，`stdout-path = serial0:115200n8` |
| UART0 | base `0x02500000`，size `0x400`，`status = okay`，Linux `ttyS0` |
| UART1 | base `0x02500400`，size `0x400`，`status = okay`，Linux `ttyS1` |
| UART compatible | vendor DT: `allwinner,sun20i-uart`；mainline DT: `snps,dw-apb-uart` |
| UART0 IRQ | `/proc/interrupts` 显示 PLIC IRQ `18` |
| PLIC | base `0x10000000`，size `0x04000000`，compatible `riscv,plic0` |
| Timer | base `0x02050000`，size `0xa0`，compatible `allwinner,sun4i-a10-timer` |
| Timer IRQ | `/proc/interrupts` 显示 PLIC IRQ `75` |
| timebase | `0x016e3600` = 24 MHz |
| rootfs | `/dev/mmcblk0p7` ext4 |
| boot/resource | `/dev/mmcblk0p1` 挂载到 `/mnt/SDCARD` |
| boot image | `/dev/mmcblk0p4` (`/dev/by-name/boot`)，Android boot image |
| U-Boot load | `sunxi_flash read 45000000 boot; bootm 45000000` |
| kernel_addr | Android boot header: `0x40200000` |
| USB 外接盘 | `/dev/sda` 挂载到 `/mnt/exUDISK` |

镜像名含义按 Sipeed 官方后缀约定理解：`Tina` 表示 Tina / OpenWrt 小 Linux，`hdmi` 表示默认 HDMI 输出，`8723ds` 表示 RTL8723DS Wi-Fi / BLE 驱动配置。该镜像名能确认当前系统类型和外设配置，但不能直接推出 `/dev/mmcblk0p4` 的 boot image 格式。

`tests/boot_probe.c` 已确认 `/dev/mmcblk0p4` 是 Android boot image，header 关键字段：

| 字段 | 值 |
|------|----|
| name | `d1-nezha` |
| kernel_size | `9783580` bytes |
| kernel_addr | `0x40200000` |
| ramdisk_size | `12` bytes |
| ramdisk_addr | `0x41200000` |
| second_addr | `0x41100000` |
| tags_addr | `0x40200100` |
| page_size | `2048` |

### 当前不再阻塞的信息

以下问题已经有足够答案，不需要继续从官方 Linux 采集：

- 启动链：`BOOT0 -> OpenSBI v0.6 -> U-Boot 2018.05 -> Android boot image`
- boot 分区：`/dev/by-name/boot` = `/dev/mmcblk0p4`
- U-Boot 加载命令：`sunxi_flash read 45000000 boot; bootm 45000000`
- kernel 加载地址：Android boot header `kernel_addr = 0x40200000`
- 串口 console：UART0，base `0x02500000`，IRQ `18`，baud `115200`
- UART 访问模型：DesignWare APB UART，stride 4，32-bit MMIO
- PLIC：base `0x10000000`
- RAM：`0x40000000 + 512 MiB`
- 初期 timer 路线：优先使用 OpenSBI timer，不先适配 D1 SoC timer

后续若要补充信息，优先从官方手册 / 主线 DTS / 适配失败日志中查证；不要再无目标地从官方 Linux 导出大块数据。

### 已完成采集记录

本轮 bring-up 已完成 boot 分区、启动链和板级参数采集。以下命令曾用于采集，不再是下一步阻塞项：

```bash
ls -la /mnt/SDCARD
find /mnt/SDCARD -maxdepth 2 -type f -print
cp /sys/firmware/fdt /mnt/exUDISK/lichee.dtb
```

如果板子上有 `dtc`：

```bash
dtc -I fs -O dts /sys/firmware/devicetree/base > /mnt/exUDISK/lichee.dts
```

如果没有 `dtc`，保留 `lichee.dtb` 回开发机反编译。

实际结论：

- `/mnt/SDCARD` 是 `boot-resource` 分区，只包含 `bootlogo.bmp`、`magic.bin` 等资源。
- `/dev/by-name/boot` (`/dev/mmcblk0p4`) 是真正 boot 分区，格式为 Android boot image。
- `/sys/firmware/fdt` 在当前系统中拷出为空；后续如果需要完整 DTS，优先使用 mainline DTS + explorer 输出对照，不阻塞 early smoke test。

### 阶段 C：跑用户态测试程序

在官方 Linux 上先跑用户态测试，验证交叉编译、部署、执行、结果采集流程。

可优先测试：

- `clock_gettime()` 延迟
- `rdcycle` / `rdtime` 读取
- pipe / poll / epoll 行为
- tty 阻塞与非阻塞读写
- `/dev/console` 或当前登录 tty 的吞吐与延迟

推荐流程：

1. 在开发机交叉编译 RISC-V Linux 用户态程序。
2. 通过 `scp`、U 盘、串口传输或 TF 卡复制到板子。
3. 在板子上运行测试并保存输出。
4. 将结果回收到 `docs/` 或 `.claude/analysis/` 中。

注意：Linux 用户态测试结果不能直接等价于 StarryOS 内核态性能，但可以验证硬件、串口、调度和测试工具链是否可用。

### 阶段 D：评估 StarryOS 平台适配

只有在阶段 A-C 稳定后，再开始考虑 StarryOS 裸机 / 内核启动。

需要补齐的平台适配项：

- 新增或接入 Allwinner D1 / Lichee RV 对应 platform
- 配置 RAM 起始地址、内核加载地址、linker script
- 配置 early console 或 SBI console
- 配置 UART MMIO base、IRQ、stride、clock
- 配置 PLIC
- 配置 timer
- 明确启动方式：U-Boot 加载 kernel image，或 OpenSBI payload
- 最小 smoke test：只打印 early boot 日志
- 再逐步打开中断、任务调度、文件系统、TTY、异步 UART

不要一开始就直接启用完整 async UART benchmark。推荐顺序：

1. early console 输出一行日志
2. 读取 hart id / SBI 信息
3. 打印内存布局
4. 初始化 PLIC / timer
5. 注册 UART IRQ
6. 跑 shell
7. 跑 `/dev/console` 读写测试
8. 跑 async UART benchmark

### 适配前置待办

在真正修改 StarryOS 平台代码前，先完成以下确认：

- 查 D1 / sun20iw1p1 官方手册 UART 章节，确认 `allwinner,sun20i-uart` 是否兼容 NS16550：
  - 寄存器 offset 与当前 `uart_16550` 是否一致
  - mainline DT 显示 `reg-shift = 2`、`reg-io-width = 4`，即 register stride 4 + 32-bit access；确认当前 `uart_16550` 是否能支持，或需要 `dw-apb-uart` backend
  - FIFO 深度与触发阈值
  - IER / IIR / LSR / FCR 位定义
  - baud rate divisor / clock 输入关系
- 查 PLIC / 中断章节，确认 Linux 显示的 UART0 IRQ `18`、timer IRQ `75` 与裸机初始化需要的 source id 一致。
- 查 timer / clock 章节，确认 `0x02050000` timer 能否用于 StarryOS，或是否应优先使用 SBI timer。
- 查启动链资料，明确 U-Boot 加载 StarryOS 的镜像格式、加载地址、DTB 传递方式。
- 当前 boot 路线已确定为 Android boot image：优先设计 StarryOS kernel 打包到 Android boot image，并写入 `/dev/by-name/boot` 的 smoke-test 流程。
- 整理一个 Allwinner D1 platform 草案：
  - RAM base / size
  - kernel load address (`0x40200000` 优先)
  - UART0 base / IRQ / stride
  - PLIC base / context
  - timer source
  - early console 路径
- 先实现 early console smoke test，再考虑异步 UART 栈复用。

### 下一步工程目标

最小目标：`StarryOS` 通过当前 U-Boot 启动链在 UART0 输出一行 early log。

成功标准：

```text
[starry-d1] early boot
```

建议拆分：

1. 新增 D1 / Lichee RV platform 常量：RAM、UART0、PLIC、SBI timer。
2. 准备 UART0 early console：DesignWare APB UART，stride 4，32-bit MMIO。
3. 设置 kernel link/load address，优先对齐 `0x40200000`。
4. 生成 Android boot image：page size 2048，kernel addr `0x40200000`，ramdisk 可为空。
5. 写入测试 SD 卡的 `/dev/by-name/boot` 前先备份原 boot 分区。
6. 串口观察 early log。

## 3. 和星光 2 的复用关系

Lichee RV Dock 和星光 2 的 SoC、外设地址和多核能力不同，但以下经验可以复用：

- TF 卡烧录和分区检查流程
- 串口接线、串口工具、启动日志保存方式
- 官方 Linux 下采集 `/proc/iomem`、`/proc/interrupts`、DTB 的流程
- 从启动日志确认 OpenSBI / U-Boot / kernel 加载地址的方法
- 用户态测试程序的交叉编译和部署流程
- bring-up checklist 的记录格式

不能直接复用的内容：

- UART MMIO 地址
- PLIC 地址和 IRQ 编号
- timer / clock 配置
- RAM layout
- DTB
- 多核行为
- 性能数据绝对值

## 4. 建议保存的产物

建议后续把以下文件保存到单独目录，例如 `.claude/analysis/licheerv-dock/` 或外部实验记录目录：

```text
bootlog.txt
uname.txt
cpuinfo.txt
meminfo.txt
iomem.txt
interrupts.txt
dmesg.txt
dt-files.txt
devicetree.tar.gz
licheerv.dts
test-results.txt
```

如需纳入仓库，优先保存精简后的分析文档，不建议提交完整二进制镜像或过大的原始日志。

## 5. 当前 StarryOS 的注意事项

当前代码中异步 UART 初始化仍包含 QEMU virt 假设：

- UART MMIO base: `0x10000000`
- NS16550 stride: `1`
- QEMU virt 中断和设备布局

因此 Lichee RV Dock bring-up 前不能只改烧录命令。当前已确认 D1 UART 是 DW APB UART 风格：UART0 base `0x02500000`、IRQ `18`、register stride 4、32-bit MMIO。后续必须先适配 D1 platform 和 early console，再考虑复用或扩展 `uart_16550` 异步栈。

Q17 之前，Lichee RV Dock 可以用于单核真板流程和基础串口实验；Q17 的 SMP / 内存序验证仍需要 QEMU SMP 或多核真板。
