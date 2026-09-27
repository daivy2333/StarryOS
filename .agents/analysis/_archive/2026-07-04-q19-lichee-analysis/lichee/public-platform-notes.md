# Lichee RV Dock 公开资料平台分析

> 日期：2026-06-28
> 目的：把公开资料中已经能确认的 Lichee RV Dock / Allwinner D1 平台事实整理出来，减少后续真板采集范围。
> 当前状态：官方 Linux 采集阶段已完成；信息足够支撑 StarryOS early console smoke test 适配。

## 资料来源

- Sipeed Lichee RV Dock 官方页面：硬件规格、外设、原理图/点位图下载入口。
- Sipeed Lichee RV 烧录页面：官方镜像、PhoenixCard 启动卡流程、启动链示例。
- Linux mainline DTS：
  - `arch/riscv/boot/dts/allwinner/sun20i-d1-lichee-rv-dock.dts`
  - `arch/riscv/boot/dts/allwinner/sun20i-d1-lichee-rv.dts`
  - `arch/riscv/boot/dts/allwinner/sun20i-d1s.dtsi`
  - `arch/riscv/boot/dts/allwinner/sunxi-d1s-t113.dtsi`
- 本地真板采集：
  - `.claude/analysis/lichee/新建 文本文档.txt`
  - `.claude/analysis/lichee/boot.txt`

## 已确认的平台事实

| 项 | 结论 | 依据 |
|----|------|------|
| 板卡 | Sipeed Lichee RV Dock | mainline `model = "Sipeed Lichee RV Dock"` |
| 核心板 | Sipeed Lichee RV | Dock DTS include Lichee RV DTS |
| SoC | Allwinner D1 / `sun20i-d1` | mainline compatible + 真板 explorer |
| CPU | T-HEAD C906 / 单 hart | mainline `thead,c906` + OpenSBI `Max HARTs = 1` |
| ISA | `rv64imafdc` + T-Head vector 扩展 | mainline DTS；vendor Linux 显示 `rv64imafdcvu` |
| MMU | Sv39 | mainline DTS + 真板 `/proc/cpuinfo` |
| RAM | `0x40000000` + 512 MiB | mainline / 真板 DT；Linux 可用从 `0x40200000` 起 |
| timebase | 24 MHz | mainline `timebase-frequency = <24000000>` + 真板 `0x016e3600` |
| PLIC | base `0x10000000`, size `0x04000000` | mainline + 真板 explorer |
| PLIC source 数 | 175 | mainline `riscv,ndev = <175>` |
| Console | UART0 / `serial0:115200n8` | mainline Lichee RV DTS + 真板 cmdline |
| 当前烧录镜像 | `LicheeRV_Tina_hdmi_8723ds.img` | 用户确认 + Sipeed Wiki 后缀说明 |

## 当前系统镜像含义

用户当前烧录的镜像名是：

```text
LicheeRV_Tina_hdmi_8723ds.img
```

根据 Sipeed 官方烧录页面的后缀说明：

- `LicheeRV`：Sipeed 专用 RISC-V D1 Linux 系列系统。
- `Tina`：Tina / OpenWrt 小 Linux 系统。
- `hdmi`：显示默认输出到 HDMI。
- `8723ds`：支持 RTL8723DS Wi-Fi / BLE 驱动。

该信息说明当前镜像与 Dock + HDMI + 8723DS 无线配置匹配。它能解释当前 Linux 中出现的 HDMI、Wi-Fi、USB、音频等外设节点，但仍不能直接说明 `/dev/mmcblk0p4` 的 boot image 封装格式；后者仍需要分析 p4 分区头部或 U-Boot env。

## UART 关键结论

主线 Linux 对 D1 UART 的描述不是普通 `ns16550a`，而是：

```dts
uart0: serial@2500000 {
    compatible = "snps,dw-apb-uart";
    reg = <0x2500000 0x400>;
    reg-io-width = <4>;
    reg-shift = <2>;
    interrupts = <SOC_PERIPHERAL_IRQ(2) IRQ_TYPE_LEVEL_HIGH>;
    clocks = <&ccu CLK_BUS_UART0>;
    resets = <&ccu RST_BUS_UART0>;
};
```

含义：

- UART0 MMIO base 是 `0x02500000`，size 是 `0x400`。
- `reg-shift = 2` 表示 UART 寄存器 offset 左移 2 位，也就是寄存器间隔 4 字节。
- `reg-io-width = 4` 表示寄存器访问宽度是 32-bit。
- `SOC_PERIPHERAL_IRQ(nr)` 在 D1 RISC-V DTS 中定义为 `nr + 16`，所以 UART0 source id 是 `2 + 16 = 18`，与真板 `/proc/interrupts` 的 UART0 IRQ 18 一致。
- UART1 base 是 `0x02500400`，IRQ 是 `3 + 16 = 19`。

对 StarryOS 的影响：

- 当前 QEMU 16550 路径使用 byte-addressed NS16550，stride 必须是 1。
- D1 UART 不能直接沿用 QEMU 的 stride=1 假设。
- 适配路径应优先考虑 `snps,dw-apb-uart` / 8250 DesignWare 风格 backend：
  - MMIO 访问宽度 32-bit
  - register stride 4
  - UART0 IRQ 18
  - 初始化时需要 clock/reset 已由 bootloader 处理，还是 StarryOS 自己处理，需要真板验证。

## Timer / PLIC 结论

主线 DTS 中 D1 timer：

```dts
timer: timer@2050000 {
    compatible = "allwinner,sun20i-d1-timer", "allwinner,sun8i-a23-timer";
    reg = <0x2050000 0xa0>;
    interrupts = <SOC_PERIPHERAL_IRQ(59) IRQ_TYPE_LEVEL_HIGH>,
                 <SOC_PERIPHERAL_IRQ(60) IRQ_TYPE_LEVEL_HIGH>;
    clocks = <&dcxo>;
};
```

含义：

- Timer base 是 `0x02050000`，size `0xa0`。
- 第一个 timer IRQ 是 `59 + 16 = 75`，与真板 `/proc/interrupts` 中 `timer@2050000` IRQ 75 一致。
- 但 StarryOS 初期不一定要直接使用 D1 SoC timer；如果 OpenSBI timer 可用，优先使用 SBI timer 路径更稳。

PLIC：

- base `0x10000000`
- size `0x04000000`
- mainline compatible 是 `allwinner,sun20i-d1-plic`, `thead,c900-plic`
- vendor Linux 输出中显示为 `SiFive PLIC` / `riscv,plic0`，但地址和 IRQ source 与 mainline 对齐。

## 启动链结论

真板 `boot.txt` 已确认：

```text
BOOT0 -> OpenSBI v0.6 -> U-Boot 2018.05 -> Linux
```

已知：

- OpenSBI firmware base 是 `0x40000400`
- OpenSBI Runtime SBI Version 是 `0.2`
- U-Boot 显示 DRAM 512 MiB
- Linux kernel 启动前显示 `Starting kernel ...`
- U-Boot image name 是 `d1-nezha`

已通过 `tests/boot_probe.c` 确认：

- U-Boot 从 `/dev/by-name/boot` 加载 kernel，即 `/dev/mmcblk0p4`。
- `boot` 分区格式是 Android boot image，magic 是 `ANDROID!`。
- U-Boot env 中 `boot_normal=sunxi_flash read 45000000 ${boot_partition};bootm 45000000`。
- 当前启动命令 `bootcmd=run setargs_nand boot_normal`；vendor U-Boot 启动时会动态更新 root 分区到 `/dev/mmcblk0p7`。

Android boot header 关键字段：

| 字段 | 值 |
|------|----|
| image name | `d1-nezha` |
| kernel_size | `9783580` bytes |
| kernel_addr | `0x40200000` |
| ramdisk_size | `12` bytes |
| ramdisk_addr | `0x41200000` |
| second_size | `0` |
| second_addr | `0x41100000` |
| tags_addr | `0x40200100` |
| page_size | `2048` |

这说明 StarryOS 复用当前 U-Boot 启动链时，最直接的候选方案是生成 Android boot image，写入 `/dev/by-name/boot`，让 U-Boot 继续 `sunxi_flash read 45000000 boot; bootm 45000000`。StarryOS 内核链接 / 加载地址优先按 `0x40200000` 设计 smoke test。

## SD 卡分区结论

真板 `img.txt` 已确认官方镜像使用 GPT 分区，`/dev/by-name` 映射如下：

| 分区 | by-name | 大小 | 说明 |
|------|---------|------|------|
| `/dev/mmcblk0p1` | `boot-resource` | 4032 KiB | vfat，挂载到 `/mnt/SDCARD`，只看到 `bootlogo.bmp` / `magic.bin` |
| `/dev/mmcblk0p2` | `env` | 252 KiB | U-Boot env 候选 |
| `/dev/mmcblk0p3` | `env-redund` | 252 KiB | U-Boot redundant env 候选 |
| `/dev/mmcblk0p4` | `boot` | 10332 KiB | Android boot image；U-Boot 读到 `0x45000000` 后 `bootm` |
| `/dev/mmcblk0p5` | `dsp0` | 504 KiB | DSP firmware 候选 |
| `/dev/mmcblk0p6` | `recovery` | 14112 KiB | recovery image 候选 |
| `/dev/mmcblk0p7` | `rootfs` | 8388608 KiB | ext4 rootfs |
| `/dev/mmcblk0p8` | `UDISK` | 22093784 KiB | 用户数据区 |

`/dev/mmcblk0p4` 只读挂载失败是正常现象，因为它不是普通文件系统，而是 Android boot image。后续不要尝试 mount 该分区；应使用 boot image 打包 / 写入工具处理。

## StarryOS 平台草案

初始 smoke test 可以先硬编码 platform 常量，不必第一步就解析 DTB。

| 常量 | 候选值 |
|------|--------|
| RAM base | `0x40000000` |
| RAM size | `0x20000000` |
| kernel load | `0x40200000`（Android boot header `kernel_addr`） |
| console UART | UART0 |
| UART0 base | `0x02500000` |
| UART0 size | `0x400` |
| UART0 register stride | 4 |
| UART0 access width | 32-bit |
| UART0 IRQ | 18 |
| PLIC base | `0x10000000` |
| PLIC size | `0x04000000` |
| timer base | `0x02050000` |
| timer IRQ | 75 / 76 |
| timebase | 24 MHz |
| preferred early timer | SBI timer（待验证） |

## 适配阶段结论

当前已经不存在阻塞 early smoke test 的信息缺口。后续工作应进入工程实现，而不是继续泛采集官方 Linux 数据。

已满足：

- 硬件地址：RAM / UART0 / PLIC / timer / timebase 已确认。
- 启动封装：`/dev/by-name/boot` 是 Android boot image。
- U-Boot 路径：`sunxi_flash read 45000000 boot; bootm 45000000` 已确认。
- kernel 目标地址：Android boot header `kernel_addr = 0x40200000` 已确认。
- UART 访问模型：mainline DTS 确认为 `snps,dw-apb-uart`，stride 4，32-bit access。
- IRQ：UART0 source id 18，timer source id 75/76。

仍需工程验证，不再视为信息采集阻塞：

- `uart_16550` 是否可通过现有 backend 支持 32-bit MMIO + stride 4；如果不行，新建 DW APB UART backend。
- StarryOS linker / entry 是否能按 `0x40200000` 正常启动。
- Android boot image 打包工具链是否与当前 U-Boot `bootm` 兼容。
- bootloader 是否已保持 UART clock/reset 可用；若 early UART 无输出，再查 clock/reset 手册。

停止条件：

- 在 early console smoke test 失败前，不再继续从官方 Linux 大范围导出数据。
- 如果失败，按失败类型定向采集：无启动日志查 Android boot image / load address；有 trap 查 linker / entry；无 UART 输出查 DW APB UART / clock reset。

## 我方可继续推进的事项

- 基于公开 DTS 和真板采集，整理 StarryOS D1 platform 常量草案。
- 检查当前 `uart_16550` 是否支持 32-bit MMIO + stride 4；如果不支持，设计 `dw-apb-uart` backend。
- 设计最小 early console smoke test 路线。
- 设计 Android boot image 打包流程，并生成只含 StarryOS kernel + 空 ramdisk 的 smoke-test boot 分区镜像。

## 失败时再补充的采集

无需继续泛采集。只有当后续 smoke test 在 bootloader 阶段失败，才建议进入 U-Boot 命令行补充：

```text
printenv
bdinfo
mmc list
mmc part
```

这些决定 StarryOS 最终镜像格式和加载地址。

boot image 格式和加载地址已由 `probe` 输出确认。U-Boot 交互采集仍有价值，但不再阻塞 Android boot image 路线。
