# Lichee RV Dock Bring-up 流程笔记

> 目标：在星光 2 真板到手前，先用手头的 Lichee RV Dock 打通 RISC-V 真板开发流程，包括烧录、串口、启动日志、板级信息采集、用户态测试部署和后续 StarryOS 平台适配准备。
>
> 当前结论：Lichee RV Dock 适合作为真板流程演练板，但不适合作为 Q17 SMP / 多核内存序验证板。它使用 Allwinner D1 / XuanTie C906，属于单核 RISC-V 平台。

## 1. 定位与边界

Lichee RV Dock 的主要价值不是直接验证当前 StarryOS 的 Q17 SMP 修复，而是提前打通以下流程：

- TF 卡烧录与启动链观察
- USB-TTL 串口连接与日志采集
- 官方 Linux 下的板级信息采集
- 测试程序交叉编译、上传、运行、结果回收
- UART / PLIC / timer / RAM / DTB 信息整理
- 为后续星光 2 bring-up 建立可复用检查清单

当前 StarryOS 仓库默认面向 QEMU virt，并已有 `vf2` 构建入口。异步 UART 初始化仍使用 QEMU virt 的 UART MMIO 地址 `0x10000000`。Lichee RV Dock 是 Allwinner D1 平台，UART、PLIC、timer、内存布局、启动协议均不同，因此不能直接把当前 StarryOS 镜像烧进 TF 卡运行。

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

因此 Lichee RV Dock bring-up 前不能只改烧录命令。需要先确认 D1 的 UART 是否兼容 16550、寄存器 stride、IRQ 编号和时钟初始化要求，再决定是否复用当前 `uart_16550` 异步栈。

Q17 之前，Lichee RV Dock 可以用于单核真板流程和基础串口实验；Q17 的 SMP / 内存序验证仍需要 QEMU SMP 或多核真板。
