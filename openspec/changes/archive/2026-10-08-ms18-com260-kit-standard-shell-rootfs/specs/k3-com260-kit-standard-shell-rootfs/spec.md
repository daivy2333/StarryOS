## Purpose

定义 K3 单 hart 在临时 RAM 启动下，从与 QEMU 同源的 ext4 rootfs 加载普通 shell 与用户程序、通过已有异步 UART/tty 交互并安全恢复官方 Linux 的用户可观察行为。范围已获用户批准，行为尚未实施和验收。

## ADDED Requirements

### Requirement: Filesystem-backed ordinary shell

K3 普通 shell 模式 MUST 从仓库 QEMU 同源 rootfs 的 VFS 路径加载 `/bin/sh` 及所需动态链接器，并在用户态运行；MUST 支持用户执行文件系统中的外部程序，不以静态内嵌实验 shell 代替。

#### Scenario: S1 Boot to the ordinary interactive shell

- **WHEN** 合法的 rootfs 和内核已临时加载并启动该模式
- **THEN** 用户 MUST 能通过串口进入普通交互 `/bin/sh`，运行 `/bin/ls /bin` 与 `/bin/cat /etc/os-release` 并回到提示符

#### Scenario: S2 Startup dependency is invalid

- **WHEN** rootfs 缺失/越界、文件系统无效、shell/解释器缺失或 ELF 无效
- **THEN** 系统 MUST 报告对应失败阶段并停止该用户启动，不静默回退内嵌 shell

### Requirement: Ordinary command composition and process lifecycle

普通 shell MUST 支持脚本、管道、文件重定向和子进程退出等待；未知命令、缺文件和非零退出 MUST 保留后续交互能力。对前台等待发送 Ctrl-C MUST 按现有进程/tty 信号契约结束前台任务并恢复 shell 交互。

#### Scenario: S3 Scripts pipes and redirection execute

- **WHEN** 用户执行包含外部命令的脚本、管道，并将输出重定向到文件后读回
- **THEN** 用户 MUST 观察正确内容与退出状态，随后仍能执行下一条命令

#### Scenario: S4 A command fails or is interrupted

- **WHEN** 命令不存在、文件缺失、子进程非零退出，或用户在 `/bin/sleep 30` 期间发送 Ctrl-C
- **THEN** shell MUST 显示可解释的结果并恢复输入，不能永久阻塞或退出整个会话

### Requirement: Volatile rootfs and preserved official boot

本模式 MUST 只使用预留 RAM 中的 rootfs 与临时文件系统承载用户写入。临时启动过程 MUST 保持官方固件、分区内容和持久启动环境不变；重新加载原始 rootfs MUST 恢复初始文件内容，外部 RESET 后 MUST 可沿原启动路径进入官方 Linux。

#### Scenario: S5 File changes are local to the RAM rootfs

- **WHEN** 用户在 rootfs 可写目录新建文件并读回，随后重新加载原始 image 启动
- **THEN** 当次读回 MUST 正确，重新加载后新建文件 MUST 不存在，源 image 与官方存储不被写入

#### Scenario: S6 External reset returns to the official system

- **WHEN** 普通 shell 正常运行、退出或启动失败后，操作者执行外部 RESET 并按原路径启动
- **THEN** 操作者 MUST 观察官方 Linux 登录，不依赖刷写或保存新的 U-Boot 环境

### Requirement: Shared async console and bounded shutdown

本模式 MUST 复用已验收的单 hart timer/AIA、唯一 UART copier 与 tty。空闲输入 MUST 等待真实事件；输入编辑保持既有 tty 契约。shell 正常退出后 MUST 完成文件系统收尾与物理 TX drain，drain timeout MUST 有界报告并进入既有停驻状态。

#### Scenario: S7 Idle and edited input remain interactive

- **WHEN** 会话空闲后用户输入带空格的命令并使用退格编辑
- **THEN** 系统 MUST 经 UART 事件恢复并按既有 tty 语义执行完整命令，不增加第二硬件 reader

#### Scenario: S8 Shell exits and the console drains

- **WHEN** 用户执行 exit
- **THEN** 用户进程 MUST 正常结束；系统 MUST 收尾文件系统，物理 drain 成功或明确报告 timeout 后 park

### Requirement: Isolated mode and qualification scope

本能力 MUST 限定当前 K3 单 hart 验收范围，拒绝与互斥启动模式或 SMP 混用，并保持既有内嵌 shell 和受影响 QEMU 路径兼容。RAM rootfs 成功 MUST NOT 被计为真实存储、分区识别、USB 存储驱动或多 hart 验收。

#### Scenario: S9 Mode selection preserves existing boundaries

- **WHEN** 构建新模式、受影响旧模式，或尝试互斥/SMP 组合
- **THEN** 合法模式 MUST 保持各自行为，非法组合 MUST 明确拒绝；验收结论 MUST 限于实际运行的目标
