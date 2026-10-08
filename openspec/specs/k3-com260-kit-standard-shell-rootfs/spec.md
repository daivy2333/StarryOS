## Purpose

定义 K3 单 hart 在临时 RAM 启动下，从与 QEMU 同源的 ext4 rootfs 加载普通 shell 与用户程序、通过既有异步 UART/tty 交互并保持官方启动不受影响的用户可观察行为。来源 change `ms18-com260-kit-standard-shell-rootfs`（2026-10-08 用户收口裁决）：合并限于实际见证的场景——脚本/管道/重定向/退出状态保持（S3）、易失写入重载对照（S5）、空闲编辑（S7，MS17 语料已覆盖同栈 tty 契约）与 shell exit 收尾观察（S8 后半）未取得正式见证，不进入语料库；未见证记录见归档 change `tasks.md` 见证状态节。

## ADDED Requirements

### Requirement: Filesystem-backed ordinary shell

K3 普通 shell 模式 MUST 从仓库 QEMU 同源 rootfs 的 VFS 路径加载 `/bin/sh` 及所需动态链接器，并在用户态运行；MUST 支持用户执行文件系统中的外部程序，不以静态内嵌实验 shell 代替。

#### Scenario: S1 Boot to the ordinary interactive shell

- **WHEN** 合法的 rootfs 和内核已临时加载并启动该模式
- **THEN** 用户 MUST 能通过串口进入普通交互 `/bin/sh`，运行文件系统中的外部命令查看实际目录内容并回到提示符

#### Scenario: S2 Startup dependency is invalid

- **WHEN** rootfs 缺失/越界、文件系统无效、shell/解释器缺失或 ELF 无效
- **THEN** 系统 MUST 报告对应失败阶段并停止该用户启动，不静默回退内嵌 shell
- **见证边界**：range 层（缺 `initrdmem` → `MissingInitrdmem`）有板上直接见证；resolve/load/InvalidExecutable 板上变异见证按用户豁免，接受依据为 host 契约测试与 QEMU 同源 loader 路径

### Requirement: Command error reporting and foreground cancellation recovery

普通 shell 会话中，未知命令与缺失文件 MUST 得到可解释的错误报告并回到提示符；对前台等待发送 Ctrl-C MUST 结束前台任务并恢复 shell 交互。（脚本、管道、重定向与子进程退出状态保持不在本轮见证范围，见 Purpose。）

#### Scenario: S4-core A command fails or is interrupted

- **WHEN** 用户执行不存在的命令或访问缺失文件，或在 `/bin/sleep` 期间发送 Ctrl-C
- **THEN** shell MUST 显示可解释的结果并恢复输入，不能永久阻塞或退出整个会话

### Requirement: Official boot preserved across RAM-rootfs use

本模式 MUST 只使用预留 RAM 中的 rootfs 与临时文件系统承载运行；临时启动过程 MUST 保持官方固件、分区内容和持久启动环境不变，外部 RESET 后 MUST 可沿原启动路径进入官方 Linux。（易失写入的重载差异对照不在本轮见证范围，见 Purpose。）

#### Scenario: S6 External reset returns to the official system

- **WHEN** 普通 shell 正常运行或启动失败后，操作者执行外部 RESET 并按原路径启动
- **THEN** 操作者 MUST 观察官方 Linux 登录，不依赖刷写或保存新的 U-Boot 环境

### Requirement: Isolated mode and qualification scope

本能力 MUST 限定当前 K3 单 hart 验收范围，拒绝与互斥启动模式或 SMP 混用，并保持既有内嵌 shell 和受影响 QEMU 路径兼容。RAM rootfs 成功 MUST NOT 被计为真实存储、分区识别、USB 存储驱动或多 hart 验收。

#### Scenario: S9 Mode selection preserves existing boundaries

- **WHEN** 构建新模式、受影响旧模式，或尝试互斥/SMP 组合
- **THEN** 合法模式 MUST 保持各自行为，非法组合 MUST 明确拒绝；验收结论 MUST 限于实际运行的目标
