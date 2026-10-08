# k3-com260-kit-async-uart-interactive-shell Specification

## Purpose
TBD - created by archiving change ms17-com260-kit-async-uart-interactive-shell. Update Purpose after archive.

## Requirements

### Requirement: Single-hart task and timer foundation

K3 串口交互模式 MUST 使用既有 executor/scheduler/timer，在单 hart 上支持有期限等待、取消与真实任务唤醒；timer handler 的注册、重复注册、移除和分派 MUST 与 UART external IRQ 隔离，不依赖 GMAC 或周期轮询 UART。

#### Scenario: S1 Timer deadline wakes a task

- **WHEN** 任务等待一个真实 timer deadline
- **THEN** timer IRQ/tick MUST 唤醒该任务并使等待返回，而不是执行忙延时

#### Scenario: S2 Timer and UART handlers have distinct lifecycles

- **WHEN** 注册、重复注册、移除或分派 timer/UART handler
- **THEN** 系统 MUST 保持唯一 owner、mask/移除顺序与正确分派，不使两路串路

#### Scenario: S3 Cancelling one timer waiter preserves others

- **WHEN** 一个有期限等待被取消而其他等待仍存在
- **THEN** 系统 MUST 清理该等待自身状态并保留其余任务的唤醒能力

### Requirement: PXA UART interrupt and notification ownership

K3 UART MUST 使用本板 PXA 的地址、访问宽度、stride、FIFO、IER/IIR/LSR 等语义接入既有 UartPort；ISR MUST 有界处理原因、必要通知控制和唤醒，字节搬运留给任务。系统 MUST 正确处理重复 RX、少量字节 timeout、事件注册/重臂窗口、组合/spurious/错误与初始化失败。

#### Scenario: S4 Real UART receive interrupts repeat

- **WHEN** 已激活 UART 的 reader 等待期间发生连续两次真实输入
- **THEN** device cause、claim/handler、wake 与字节消费 MUST 对应，后续事件仍推进

#### Scenario: S5 The last short receive reaches the reader

- **WHEN** FIFO 中少量字节后不再有输入
- **THEN** RX timeout/设备通知 MUST 推进这些字节，不能等待下一字节或周期收包轮询

#### Scenario: S6 Arm and registration windows retain work

- **WHEN** 工作发生在 register 前、期间、arm 后或 Pending 前
- **THEN** recheck MUST 观察工作或唤醒 owner，不丢最后事件

#### Scenario: S7 Combined or error causes stay bounded

- **WHEN** UART 报告组合、spurious 或线路错误
- **THEN** ISR MUST 保留必要的破坏性状态并分类/唤醒，不搬运字节、打印、忙等锁或造成中断风暴

#### Scenario: S8 Failed or duplicate activation cannot create another owner

- **WHEN** 初始化未完整成功或再次激活已启动路径
- **THEN** 系统 MUST 明确失败阶段并避免半切换、第二 owner 或活动期无所有权 polling fallback

### Requirement: Bounded bidirectional byte-stream copiers

K3 UART MUST 复用唯一 RX/TX copier 与有限容量字节 rings，保留 SPSC 所有权、字节次序和 wrap 语义；写入 MUST 报告实际接受字节，Full 后可恢复，RX overrun/线路错误可解释，各 copier 搬运有界且不饿死其他方向或 timer。

#### Scenario: S9 Byte streams cross FIFO and ring boundaries

- **WHEN** 正常负载双向输入输出跨越 FIFO/ring 边界与 wrap
- **THEN** 接收方 MUST 观察正确内容/次序，RX/TX 均持续推进

#### Scenario: S10 Full TX reports acceptance and later recovers

- **WHEN** TX 已满后再次写入，随后 copier 释放空间
- **THEN** 写入 MUST 返回实际接受量/短写或既有等待结果，空间事件使后续写入恢复，不能覆盖未消费数据

#### Scenario: S11 RX overload has an explicit bounded outcome

- **WHEN** reader 延迟消费且输入超过可用容量
- **THEN** 系统 MUST 有界处理并报告分类 overrun/drop，恢复消费后仍可接收，不宣称任意过载零丢失

#### Scenario: S12 Sustained work does not starve another direction

- **WHEN** RX/TX 持续都有 backlog
- **THEN** copier 每次搬运 MUST 有界，另一方向与 timer 仍推进

### Requirement: Physical drain and safe waiter lifetime

flush/drain MUST 区分软件接受、ring 空、copier active、staged bytes 和硬件 transmitter-empty；成功 MUST 表示实际物理 drained。可写/THRE MUST NOT 代替该完成条件。timeout/cancel MUST 只结束等待，不撤销已接受 TX 或破坏端口所有权，竞争 waiter 沿既有契约处理。

#### Scenario: S13 An empty port drains immediately

- **WHEN** ring、copier、staged bytes 和硬件均空
- **THEN** flush/drain MUST 立即完成，不额外等待无来源中断

#### Scenario: S14 Physical completion wakes the last drain waiter

- **WHEN** ring 已空但 copier、staged bytes 或 FIFO 尚未完成
- **THEN** drain MUST 等待真实物理完成，最后一个等待者能够被唤醒，不提前成功或永久 Pending

#### Scenario: S15 A drain deadline does not discard accepted bytes

- **WHEN** drain 的外层 deadline 到期
- **THEN** 等待 MUST 有界结束且保留已接受字节与 owner，不清 ring/FIFO 或重初始化设备

#### Scenario: S16 Cancellation and waiter competition preserve ownership

- **WHEN** read/write/drain 等待被取消或出现竞争登记
- **THEN** 系统 MUST 按既有等待契约清理或返回明确结果，不覆盖其他等待者或误报完成

### Requirement: Async TTY and standard I/O

用户程序 stdin/stdout/stderr MUST 接入现有唯一 tty/UART 链；规范模式行输入（保留空格/制表符）、echo/退格、阻塞与 nonblocking I/O、readiness、EOF/取消和容量边界 MUST 与实际可执行 I/O 一致。空规范行 VEOF MUST 使 read 返回0并使输入事件ready；1024B规范行上限（含终止符）溢出 MUST 整行丢弃到终止符后报告一次错误，后续行可恢复。系统 MUST NOT 将不完整规范行或物理 drain 当作错误的 readiness 判据。

#### Scenario: S17 Canonical input produces a complete line

- **WHEN** 用户程序阻塞读 stdin，操作者输入字符、退格与换行
- **THEN** tty MUST 按现有 echo/编辑语义交付完整行并唤醒 read

#### Scenario: S18 Nonblocking and polling agree with available I/O

- **WHEN** 无完整输入行或 TX 无空间时执行 nonblocking I/O 或 readiness 等待
- **THEN** 返回/唤醒 MUST 与实际数据/空间一致，保持既有错误和 short-write 语义

#### Scenario: S19 EOF or cancellation leaves the UART valid

- **WHEN** tty 等待遇到可达 EOF、取消或既有信号事件
- **THEN** 系统 MUST 按 tty/进程契约结束或恢复该等待，并保留 UART owner 的有效性

#### Scenario: S20 Line or echo capacity remains bounded

- **WHEN** 输入行/echo 到达现有容量边界
- **THEN** 系统 MUST 无越界/无界分配，采用明确容量结果，不能静默执行截断命令，后续行仍可处理

### Requirement: Embedded user process without storage

K3 MUST 从内嵌静态 ELF 接通既有用户映射、栈/argv、stdio 和 spawn；用户态程序 MUST 真实执行，无需 rootfs/块设备。加载或构造失败 MUST 在正确层级返回错误，不能启动半构造进程或无限重试。

#### Scenario: S21 Embedded ELF executes with TTY-backed stdio

- **WHEN** 合法内嵌 ELF 被加载并 spawn
- **THEN** 用户程序 MUST 在用户态执行，通过 tty-backed stdio 输入输出；kernel 打印假提示符不能代替

#### Scenario: S22 Loader or process construction fails explicitly

- **WHEN** ELF 无效或映射/stdio/进程创建失败
- **THEN** 系统 MUST 明确失败层级并处理已构造资源，不牵引网络或存储作为隐含前置

### Requirement: Minimal interactive user shell

内嵌用户态 shell MUST 支持重复 `help`、`echo <text>`、`sleep <毫秒>` 与正常 `exit`，经现有 syscall/tty/timer 实现；未知命令/非法参数有明确反馈。空闲 MUST 等待真实事件，不无界自唤醒或周期读取 UART。该最小范围不包含完整 POSIX shell、外部程序目录、管道或重定向。

#### Scenario: S23 Commands can run repeatedly in one boot

- **WHEN** 操作者重复输入 help 与 echo
- **THEN** 用户态 shell MUST 输出正确结果并再次等待下一行，不要求每次重启或重建镜像

#### Scenario: S24 User sleep resumes through the timer

- **WHEN** shell 执行 sleep 后继续输入 echo
- **THEN** 用户态 MUST 经既有 syscall/timer 等待恢复，后续命令仍可用，不 busy-delay

#### Scenario: S25 Invalid input returns to the prompt

- **WHEN** 输入空行、未知命令或非法/超限参数
- **THEN** shell MUST 无崩溃/越界，明确处理结果并返回提示符

#### Scenario: S26 Idle input resumes and exit reports a result

- **WHEN** shell/copy tasks 空闲后发生新输入，或操作者执行 exit
- **THEN** 新 UART IRQ MUST 恢复交互，exit 正常结束用户程序并报告结果，不无序销毁端口

### Requirement: RAM-only qualification and compatible shared foundation

本能力 MUST 沿 R68 RAM/FIT 路径在 SMP=1 的 CoM260 Kit UART0 上验证，保留 early/故障输出及活动期唯一硬件所有权，结束后外部复位回原系统。验证 MUST 直接观察生产路径与原生结果，区分 host/QEMU/真板和单 hart/SMP；不建立身份型证据或判定层。既有模式在实际影响范围保持兼容，共用 timer/IRQ 成果在 accepted 后才能作为 MS12 新执行基线。

#### Scenario: S27 RAM boot and external reset remain safe

- **WHEN** 操作者执行临时 RAM 启动并在完成/异常后外部复位
- **THEN** MUST 观察到相应目标行为并回到原系统，不 saveenv/刷写，不把未执行项计为通过

#### Scenario: S28 Existing platforms and MS12 contracts retain their boundaries

- **WHEN** 检查受影响平台并恢复 MS12 规划
- **THEN** MUST 保留旧模式与设备验收边界，消费 accepted 的共用底座结果并生成适用执行契约，不改写已交接 Cycle 或以历史 UART SMP 豁免完成多 hart 验收
