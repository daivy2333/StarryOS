## Why

K3 当前验收的用户态 shell 仅支持内嵌的 help/echo/sleep/exit。用户希望在推进异步网卡前，先进入与 QEMU 同源的 StarryOS 普通 `/bin/sh`，执行文件系统中的用户程序，并保留外部 RESET 回官方 Linux。

参考工程已通过 UFS rootfs 进入普通 shell；本仓具备 VFS、普通 ELF/动态链接器加载和 RAM 块设备接口，但尚未在 K3 验收这条路径。用户确认当前板没有对应 SD 卡槽，TF 路线不适用。

## What Changes

用户已批准以下 RAM rootfs 范围及完整实现计划：

- 内核继续通过既有 U-Boot RAM/FIT 路径启动；rootfs 从现有 U 盘单独加载到预留 RAM，沿用仓库 `rootfs-riscv64.img` 的 ext4 用户空间。
- 从 VFS 加载 `/bin/sh`、BusyBox 和 musl，建立普通交互 shell；支持外部程序、脚本、管道、重定向及文件读写。
- 复用已验收的单 hart timer/AIA、UART copier 和 tty；覆盖子进程等待、Ctrl-C 后恢复交互、shell 退出与物理 drain。
- 文件修改只写本次 RAM rootfs 或临时文件系统；再次从原始 image 加载后恢复初始内容。保持官方固件、分区和持久启动环境不变，并实测 RESET 回官方 Linux。

本次不接入 UFS、SD、USB mass-storage 驱动，不实现持久安装或多 hart。U-Boot 能读 U 盘只用于启动加载，不代表 StarryOS 已有 USB 存储能力。

## Capabilities

### New Capabilities

- `k3-com260-kit-standard-shell-rootfs`: K3 单 hart RAM rootfs 普通 shell、文件系统用户程序与官方系统恢复。

### Modified Capabilities

None。已有内嵌 shell 模式保持其原有验收边界。

## Impact

预计涉及 K3 RAM 预留/启动参数消费、root device 与 ext4 初始化、用户进程入口、Cargo feature 和构建/临时启动说明。普通 loader、进程和 tty 仅在目标行为测试发现缺口时修改。

原 MS18 还要求真实块设备驱动、分区识别与 rootfs 挂载。本提案仅先交付其用户环境部分；真实存储要求继续保留，不以 RAM rootfs 宣布原 MS18 完成。本次不修改全局路线或 MS12 已交接的执行契约。

## Requirement Traceability

| 用户需求 | 本次验收 | 状态 |
|---|---|---|
| 进入与 QEMU 同源的正式 StarryOS shell | `/bin/sh` 从 ext4 加载，BusyBox/musl 用户态实际执行 | 已交付（Iteration 001 板测验收） |
| 执行普通用户程序 | 外部命令、脚本、管道、重定向与 fork/exec/wait 可观察结果 | 已交付（外部命令/错误报告/Ctrl-C 前台取消板测见证；脚本/管道/重定向按用户 2026-10-08 裁决不再验证，见 tasks.md 见证状态节） |
| RESET 回官方 Linux | 不写官方存储/持久环境，外部 RESET 后见官方登录 | 已确认（2026-10-08 用户实测，原话「reset我已经测试，确实可以回到官方linux」） |
| 优先于异步网卡推进 | 本次先规划普通用户环境；不推进 MS12 实施 | 用户已授权探索与计划 |
| 原 MS18 真实存储基线 | 真实驱动、分区与持久 rootfs 保留后续 | Simplified：分阶段范围已批准；原需求未删除 |

## Scenario Sketches

| 场景 | 前置与动作 | 结果与失败边界 |
|---|---|---|
| 正常启动 | 内核和合法 rootfs 已临时加载；启动 | 普通 `/bin/sh` 提示符可交互；不是内嵌命令循环 |
| 外部程序与脚本 | shell 中运行 `/bin/ls /bin`、`/bin/cat /etc/os-release` 和脚本 | 文件来自 rootfs；子进程正确退出并回到提示符 |
| 管道与重定向 | `printf 'hello\n' \| /bin/cat`，写文件后读回 | 内容正确；后续命令继续运行 |
| 非持久写入 | 写入 rootfs 可写目录；退出/复位后重新加载原始 image | 当次可读回，重新加载后文件不存在 |
| 取消与恢复 | `/bin/sleep 30` 期间 Ctrl-C，然后执行命令 | 前台任务结束，shell 仍响应，无第二 UART reader |
| 兼容与空闲 | 空闲后输入、退格编辑；构建旧模式 | UART IRQ 恢复输入；旧模式保持行为，禁止冲突模式/SMP |
| 启动失败 | 缺 rootfs、越界参数、坏 FS、缺 shell/ldso、坏 ELF | 在相应层报错并停止用户启动；不静默回退临时 shell |
| 命令错误 | 未知命令、缺文件、脚本非零退出 | 用户态报告错误/状态；shell 回到提示符 |
| 正常退出 | 执行 exit | 进程结束，文件系统收尾，TX 物理 drain 成功或明确 timeout 后 park |
| 官方恢复 | 正常或失败状态下外部 RESET，按原启动路径启动 | 观察官方 Linux 登录；复位不被当作撤销磁盘写入 |

## Workflow Status

- 探索：已完成源码与 rootfs 只读调查；未上板验证新行为。
- 需求与场景草案：已形成。
- Gate 1：PASS，用户选择“批准，先做 RAM rootfs 正式 shell”，批准本次用户环境范围及真实存储后续保留。
- 设计、任务与当前轮执行契约：完整计划已获批准，用户原话“批准计划”。
- Gate 2：PASS，调查、设计、任务契约、验证设计与计划批准齐备；首轮执行契约 ready。
- 实施：Iteration 000（RAM rootfs）与 Iteration 001（普通 shell + RAM 布局修复，D1-r1）均已完成并通过 Plan Review（2026-10-08 accepted）；Iteration 002 按用户 2026-10-08 裁决取消——串口/shell 手动板测与 RESET 官方恢复已另行确认，S3/S5/S7/S8 后半不再正式验证。change 待收尾。
- 持久化分析/R 登记：SKIPPED，环境禁止写 `.agents` 且不允许提权；分析临时保存于 `/tmp/ms18-shell-rootfs-exploration.md`。
