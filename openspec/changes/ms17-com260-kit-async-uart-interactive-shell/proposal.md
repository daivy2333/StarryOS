# MS17：CoM260 Kit 单 hart 异步串口与交互 shell

## Why

K3 已完成 MS09–MS11 板级、中断与轮询网络基线，但仍缺少串口交互：当前 console 输出忙等硬件，输入返回 0，K3 构建排除了常规驱动、tty、进程和用户程序加载入口。先建立串口与最小 shell，可为后续网络开发提供已验收的 timer/任务底座和交互入口。

2026-10-06 用户原话：“同意保留当前12计划但是要做适当调整，现在请你给出ms17的规划吧，我们先做这个”。本次先规划 MS17；MS12 保留需求、设计和三轮 Map，暂停执行原首轮，待 MS17 验收后吸收实际共用底座结果。目标顺序为 MS17→MS12→MS13→MS14，最后一阶段同时验收网卡与串口多 hart 异步行为。

## What Changes

- 在独立 K3 串口交互模式接通既有 task/timer，复用 MS10 AIA claim/dispatch；不要求先初始化 GMAC、PHY 或运行网络探针。
- 将 K3 PXA UART 硬件层接入现有 UartPort、RX/TX copier、字节 rings 和 waker/poll；硬中断只完成原因处理、必要通知控制和唤醒，搬运在任务上下文。
- 验证有限容量、short write、背压恢复、注册/重臂竞态、RX timeout、TX 最后完成与真实物理 drain；无输入时等待 IRQ，不通过常驻忙轮询维持进度。
- 接通现有 tty/console、stdio 与用户进程加载，以内嵌用户程序获得最小交互 shell；命令读取和输出走异步 UART 链。
- 分层验证 UART→tty→用户态→命令交互，兼容原 K3 探针、QEMU/D1/VF2 既有边界；单 hart 结论不作为多 hart 验收。

## Capabilities

### New Capabilities

- `k3-com260-kit-async-uart-interactive-shell`：K3 单 hart 异步 UART、timer 等待唤醒、tty 与内嵌最小用户态 shell，以及安全 RAM 启动下的直接验收。

### Modified Capabilities

None。MS09/MS10/MS11 和既有 UART 行为作为基线；本次不把历史未验收的 UART SMP 项提升为完成。

## Impact

- 平台：`crates/axplat-riscv64-k3` 的 IRQ、timer 和 console 接缝；timer 目标语义消费 MS12 已调查的独立 timer handler/既有 runtime tick 方案，不另建 executor 或 timer wheel。
- 驱动与 tty：`kernel/src/drivers/uart_init.rs`、`ntty_async.rs`、`pseudofs/dev/tty/`、本地 `crates/uart_16550` 的生产路径与必要 K3 adapter。
- 用户态：K3 kernel 模块 feature 门、`entry.rs`、`mm/loader.rs` 既有 ELF 字节加载路径、stdio/进程构造及最小 shell 载体；用户态映射和平台接口的实际缺口须在 Phase 2 调查闭合。
- 构建与测试：必要 Cargo/Makefile feature、静态用户程序构建与内嵌、生产核心 host 测试和真板分层载荷；不修改参考仓库、registry 或通用网络契约。
- MS12：仅登记优先级与共用底座交接边界，不改写已交接 Plan Context，不提前删减 timer 的验收条件。实际变化发生后，由 Plan 创建新的自包含执行契约再恢复。

## Requirements List（已批准）

| ID | 目标行为 | 场景 |
|---|---|---|
| R1 | 单 hart 既有 scheduler/timer 等待真正可用，timer 与 UART/其他 external IRQ 不串路 | S1–S3 |
| R2 | PXA UART 正确硬件适配、最小 ISR、通知 ownership 与无 lost wakeup | S4–S8 |
| R3 | 唯一 RX/TX copier 和有限容量 rings，完整有效字节流、short write 与背压恢复 | S9–S12 |
| R4 | TX software accepted、copier/staged/FIFO 与物理 drain 分层，最后完成、timeout/cancel 安全 | S13–S16 |
| R5 | 现有 tty/stdio 的阻塞、nonblocking 与 readiness、规范模式输入沿异步链成立 | S17–S20 |
| R6 | 内嵌静态用户程序正确加载并 spawn，错误在用户态入口前明确终止 | S21–S22 |
| R7 | 最小 shell 支持反复命令交互、sleep 与错误反馈，空闲不忙轮询 | S23–S26 |
| R8 | RAM-only、兼容与最简直接验证，MS12 共用底座交接明确 | S27–S28 |

## 默认范围与集中决策

用户已批准 MS17 优先方向，以及以下具体默认范围和 R1–R8/S1–S28。

1. **最小用户态 shell**：内嵌一个静态 ELF，通过已有 syscall/tty 读取 stdin、输出 stdout；至少提供 `help`、`echo <text>`、`sleep <毫秒>`、`exit`。未知命令/非法参数有明确反馈，执行后可继续输入；exit 正常结束并报告进程结果。不默认承诺完整 POSIX `/bin/sh`、外部程序目录、管道、重定向或完整发行版用户面。
2. **无存储依赖**：沿 R68 的 RAM/FIT 临时启动，SMP=1，UART0 使用已核实的 115200 8N1、无硬件流控连接；不引入 MS18、rootfs、UFS/SD/eMMC、网络投递或固件/分区持久写入。最小 pseudofs/stdio 挂接属于用户程序正常运行的必要部分。
3. **驱动契约完整**：验证 RX/TX、事件窗口、容量/背压和 short write；flush/drain 以软件 ring、copier/staged bytes 和硬件 transmitter-empty 共同判定。timeout/drop 仅结束等待，不丢弃已接受 TX。RX overrun/线路错误按生产路径明确分类；不承诺无流控下任意超载零丢字节。
4. **分层与证据**：先证明 timer/UART，再证明 tty/用户态和 shell。host 验证生产路径、操作者执行最短真板操作；不新增 validator、身份协议或审计工具。正常早期/故障输出保留，不能与活动 TX copier 无所有权地并发写同一 UART。Persisted Evidence 默认 none。
5. **交付边界**：本次单 hart；UART/NIC 多 hart 和组合长稳属于 MS14，性能优化属于 I19。网络命令、GMAC 状态控制和诊断工具不预先加入 shell；后续确有网络验收需要时按其计划接入。

接受这些默认范围不表示删减 MS17 的双向异步串口、tty 或用户态需求；若需要完整 `/bin/sh`，须明确其程序来源和运行依赖后再定范围。

## Scenario Sketch

| 场景 | 前置状态 | 动作 | 直接结果 | 失败边界 |
|---|---|---|---|---|
| S1 timer 恢复 | 既有任务进入有期限等待 | deadline 到达 | timer IRQ/tick 使原任务恢复，sleep 返回 | 仅读时钟或主循环延时不计 timer future 可用 |
| S2 timer/IRQ 生命周期 | timer 与 UART external 路由已准备 | 注册、重复注册、unregister 与分派 | 注册不覆盖 owner；mask/移除有序；两路互不串路 | 不复用错误 trap 编号，不使未支持 IRQ 静默成功 |
| S3 task 等待取消 | 无 UART 工作，存在多个有期限等待 | 取消其中一个并让其余期限到达 | 取消清理自身等待，其他任务恢复 | 不移除共享 timer handler，不改变 scheduler 公共契约 |
| S4 UART 首次与重复 RX IRQ | UART 已接入，copier 正在等待 | 连续两次输入合法字节 | device cause、claim/handler、wake 和最终字节可对应，第二次仍推进 | boot 日志或一次软件 IRQ 不作为 RX 验收 |
| S5 RX timeout/最后字节 | FIFO 少量字节不足阈值 | 停止继续输入 | RX timeout/通知使最后字节到达 reader | 不依赖下一字节或周期收包轮询补救 |
| S6 注册/重臂竞态 | RX/TX owner 准备 Pending | 在 register 前、期间、arm 后和返回前产生工作 | recheck 观察工作或任务被再次调度，无丢唤醒 | 不靠下一事件补救 |
| S7 组合/spurious/线路错误 | 多 cause 或无工作、线路错误 | ISR 分类并交任务处理 | 有界处理；破坏性状态在消费前保留，错误可解释 | ISR 不 drain 字节/打印/忙等 owner 锁；不产生中断风暴 |
| S8 启动失败/重复激活 | adapter/handler/copier 未完整准备或已激活 | 初始化失败或再次激活 | 不半切换、不产生第二 owner；失败阶段明确 | 已激活驱动不退回无所有权 polling 路径 |
| S9 双向字节流与 wrap | 两方向可工作 | 发送跨 FIFO/ring 边界的确定字节流 | 正常负载下次序/内容完整、wrap 正确，两方向均推进 | 不以终端提示符代替字节流验证 |
| S10 TX Full→恢复 | TX ring 已满 | 再写入，随后 copier 释放空间 | 返回实际接受量/短写或既有稳定等待语义；唤醒后续写 | 不把未接受字节报告为成功，不覆盖未消费数据 |
| S11 RX 容量/overrun | reader 暂不消费，容量接近上限 | 继续输入并恢复消费 | 边界有界，分类 drop/overrun 可见；恢复后仍可输入 | 不宣称任意过载零丢失，不动态扩容掩盖背压 |
| S12 持续双向公平性 | RX/TX 都有 backlog | 让两个 copier 持续推进 | 单 poll 搬运有界，另一方向与 timer 不饥饿 | 无 ISR 搬运或单任务无界 drain |
| S13 空 flush | ring/copier/staged/FIFO 均空 | 请求 flush/drain | 立即完成 | 不额外等待无来源 IRQ |
| S14 staged/物理 drain | ring 已空但 copier/staged/FIFO 仍有工作 | 等待最后字节离开 transmitter | 物理 drained 后才完成，最后等待者被唤醒 | THRE/可写或 ring-empty 不能替代 TEMT/物理完成 |
| S15 drain 超时 | TX 尚未物理完成 | 外层 deadline 到期 | 有界结束等待，已接受字节和 owner 保留 | timeout 不清 ring/FIFO 或重新初始化 UART |
| S16 waiter 取消/readiness 竞争 | read/write/drain waiter 已注册 | drop/cancel 或竞争登记 | 沿已有 waiter 契约清理/报告，不误成功或永久 Pending | 不覆盖其他 waiter，不撤销已接受 TX |
| S17 tty 行输入 | 用户进程阻塞读 stdin，规范模式开启 | 输入字符、退格、换行 | echo/编辑遵循 tty 语义，完整行使 read 返回 | raw ring 有字节不等于规范行 ready |
| S18 tty nonblocking/readiness | 当前无完整输入行或 TX 无空间 | nonblocking I/O 与 poll/select 等待 | readiness 与可执行 I/O 一致；无数据/空间返回既有错误或短写 | 不把物理 drained 作为 OUT ready，不隐式启用第二 reader |
| S19 EOF/取消/信号边界 | 用户任务等待 tty | 可达 EOF、取消或既有信号事件 | 按既有 tty/进程语义结束或恢复等待，UART owner 继续有效 | 不把用户态退出误判为驱动失效 |
| S20 tty 容量边界 | 输入行或 echo 接近现有容量 | 输入到边界并继续 | 不越界、不无界分配；采用明确容量/错误策略，后续行仍可处理 | 不静默执行被截断的命令；策略由 Phase 2 闭合 |
| S21 内嵌 ELF/stdio | 无 rootfs，内嵌合法静态程序 | 映射 ELF/栈/argv，构造 stdio 与 spawn | 用户态真实执行，stdin/stdout 连接唯一 tty | kernel 内打印假提示符不算用户态 shell |
| S22 loader/进程失败 | ELF 无效或创建映射/stdio 失败 | 尝试加载与启动 | 返回明确层级错误，未启动半构造用户进程，必要资源清理 | 不启动无限重试或牵引存储/网络解决载体问题 |
| S23 shell 常规交互 | 已显示用户态提示符 | 重复 help、echo 命令 | 输出内容正确，命令后再次等待下一行 | 不每条命令都要求重启/重建镜像 |
| S24 shell sleep | 用户态可运行，timer 已验收 | 执行 sleep 后继续 echo | 用户态 sleep 经现有 syscall/timer 恢复，后续命令仍可用 | 不在 shell 里 busy-delay，不以数字增长代替唤醒 |
| S25 shell 非法输入 | shell 等待命令 | 空行、未知命令、非法/超限参数 | 无崩溃/越界，明确反馈并返回提示符 | 不隐式扩展为完整命令框架 |
| S26 quiet/exit | shell 与 copier 无立即工作 | 安静等待后输入；另执行 exit | 无无界自唤醒；新 UART IRQ 恢复交互；exit 结束用户程序并报告结果 | 不用周期读取 UART 维持活性；退出不无序销毁端口 |
| S27 RAM 启动/结束 | K3 镜像待板测 | R68 RAM 启动，结束或异常后外部复位 | 可观察目标阶段，复位回原系统；early/故障输出保留 | 不 saveenv/刷写；未执行项不计通过 |
| S28 兼容/交接 | 既有 K3/QEMU/D1/VF2 基线与 MS12 计划存在 | 在实际影响范围构建/测试并恢复后续规划 | 旧模式保留，UART 与网络验收分开；共用底座经 accepted 后才能作为 MS12 当前基线 | 不改写原 MS12 Cycle，不用旧 UART SMP 豁免完成 MS14 |

已扫描 Happy/Sad/Edge、错误、超时、取消、单 hart IRQ/task 窗口与兼容性。暂未裁剪需求或接受场景缺口。

## 调查输入与闭合位置

以下是 Phase 1 输入；Phase 2 的当前实码、原生验证、具体选择与边界已写入 design.md，任务与 RTM 在 tasks.md，当前可执行范围在 Iteration000/Cycle000。不能把需求层输入当 Act 契约。

- 当前实码：`kernel/src/entry.rs::init` 在 k3 分支进入 `run_first_byte_park`；`kernel/src/lib.rs` 的 k3 门排除 drivers/file/mm/pseudofs/syscall/task/time。平台 `console.rs::read_bytes` 返回 0，write_bytes 逐字节忙等。
- 已有复用：`ntty_async.rs::ASYNC_TTY` 以唯一 raw reader/writer接入 UART，RX PollSet 唤醒 tty-reader；`uart_16550/async_/device_ops.rs` 实现短写、可写注册/重查和 tx_completion 物理 flush。`mm/loader.rs::load_embedded_user_app` 已有 ELF 字节加载入口，目前仅 D1 userbench feature 开放。
- 板级参数：K3 `axconfig.toml` 已核实 UART0=0xd4017000、IRQ42、stride4、U32访问；clock/reset/pinctrl 保持 bootloader handoff。R69 PXA `mod.rs` 明确 64B FIFO、UUE/RTOIE、非 DMA IRQ 路径；不能套通用16550 16B FIFO/寄存器访问方式。
- 已闭合：PXA IER/cause/status 与 saved-error ownership、task-context Errata20 与1B阈值；K3 copier256B预算、wrap/drop策略、最后TEMT的原生timer等待；普通异步输出与共享guard保护的early/fault console；1024B tty行上限/EOF/error event与256B shell命令缓冲；静态ELF、stdio先于全局init提交、icache同步与退出；具体native测试/构建和R68操作。tcdrain/TCSETSW的历史TODO不等于支持，也不作为本次新接口。
- 当前UART69项、AIA16项、tty输出6项native tests已实跑GREEN；系统GCC+已有musl库已生成同类静态RISC-V ELF（原musl GCC在沙箱SIGSYS，替代命令成功）。没有本次目标镜像构建或真板结果。MS11/MS12未失效结论采信；本模式不引入axnet，其普通feature缺陷留MS12。

## Sources and Freshness

- 当前体系：AGENTS、SNAPSHOT、tasks、M42（2026-10-06）；当前 tasks 已包含上次路线补丁，MS17 顺序仍为 MS12/MS13 后，用户本次指令优先于该旧排期。本 Plan 不同步全局 tasks/M/R/I。
- R74：`.agents/analysis/k3-com260-interactive-shell-exploration.md`（2026-10-02）；其 MS10 前状态已失效，以当前已验收 MS10 为准；“内嵌最小 shell 无存储依赖”属于待本阶段实现验证的可行性判断。
- R66/R69/R71/R72：`/home/daivy/projects/serial/work/k3/` 资料路线与 tgoskits PXA/AIA 实现。Phase2已补读 PXA mod/regs/irq/rx 实际操作、当前K3 AIA/IRQ/time与kernel UART/tty/loader调用链；按M42采信适用寄存器用法，不额外发明读回前置。
- R68：`.agents/runbooks/com260-uboot-usb-starryos-ram-boot.md`，既有 RAM 引导与外部复位流程；每轮具体步骤必须写进自包含 Cycle。
- MS12：`openspec/changes/ms12-com260-kit-async-bidirectional-data-plane/`，需求、调查和 Map 保留；当前原首轮 ready/Act pending，但本次用户明确选择先 MS17，不执行原网络契约。

## Non-goals

GMAC 接入与网络测试命令、网络恢复、多 hart/IPI/任务迁移、长稳/性能、存储/rootfs、完整 POSIX shell/外部程序生态、第二 UART、UART DMA、新 driver framework、新 executor、持久化部署、Recorder 经验产物、全局状态同步或归档。

## 正式分轮

1. Iteration000 / T1–T2：timer/PXA port与UART42，独立低层异步硬件入口。
2. Iteration001 / T3–T4：复用copier，完整字节/容量/事件窗口/物理drain与输出ownership。
3. Iteration002 / T5–T6：真实tty、内存根、静态ELF、用户stdio/syscall入口。
4. Iteration003 / T7–T8：最小shell命令、错误/quiet/退出、兼容与交接。

正式依赖/稳定基线/验证与诊断边界和平衡审计见tasks.md。只创建首轮Cycle；每轮accepted后展开下一轮。本轮只交付计划，不调用Act。

## Phase and Gate Status

| Phase/Step | 状态 | 边界/原因 |
|---|---|---|
| Phase 1 目标、范围与 BDD 扫描 | completed | R1–R8/S1–S28 与五项默认范围已形成 |
| Phase 1 proposal / delta specs | completed | R1–R8/S1–S28 已批准，设计细节不裁剪需求 |
| Gate 1 | PASS | 用户明确批准上述范围和需求；原话见 User Decision |
| Phase 2 实现调查、设计、RTM 与正式分轮 | completed | 设计与8任务/4轮Map、首轮自包含Cycle已形成 |
| Gate 2 | PASS | 技术调查/设计/追踪检查完成；用户已批准具体方案，首轮Cycle ready |
| 产品实施、板测、Recorder、全局同步/归档 | SKIPPED | 当前只授权规划；不修改产品或相应角色产物 |

Persisted Evidence：none；不创建 Evidence 占位目录。需求检查结果与用户决定直接记录本文。

## User Decision

已批准：MS17 优先、保留并适当调整 MS12 计划；默认范围1–5及 R1–R8/S1–S28。2026-10-06 用户原话：“不需要你核对，简单看看然后回答我就好，批准上述范围和需求，进行探索给出当前ms的方案”。本次授权继续 Phase 2，不构成 Act 授权；未收到新的需求缺口或 Gate 豁免。

执行计划批准：2026-10-06，在四轮方案交付及RAM/FIT启动/reset恢复说明后，用户原话：“批准”。据此Gate2 PASS，Iteration000/Cycle000从draft转ready；不改需求、设计或任务契约，不使用Gate豁免。按公共规则阶段边界，计划批准完成本次交接，Act等待明确执行指令。

## Phase 1 Validation（2026-10-06）

- `openspec validate ms17-com260-kit-async-uart-interactive-shell --strict --no-interactive` → `Change 'ms17-com260-kit-async-uart-interactive-shell' is valid`，exit0。
- `openspec status --change ms17-com260-kit-async-uart-interactive-shell` → `Progress: 2/4 artifacts complete`，proposal/specs 完整，design 未生成、tasks 依赖 design，exit0；这符合 Gate 1 前的边界，不代表执行就绪。
- `openspec validate ms12-com260-kit-async-bidirectional-data-plane --strict --no-interactive` → `Change 'ms12-com260-kit-async-bidirectional-data-plane' is valid`，exit0；保留的网络计划及排期调整结构有效。
- `git diff --check` exit0；当前 `.agents` 三份文档为既有用户改动，本次未触碰。MS12/MS17 目录未跟踪；对本次修改的七份 Markdown 补用原生 `git diff --no-index --check -- /dev/null <file>`，无空白诊断，exit1 表示文件新增内容差异，不作为测试失败。
- 人工审查：8 项需求与 28 个场景在 proposal/specs 一一覆盖；默认 shell 范围清晰标为待批准。未创建 design/tasks/Iteration/Cycle/Evidence；未改写 MS12 原 Cycle，也未把未来 MS17 计划作为已通过基线。

## Phase 2 Validation（2026-10-06）

- `openspec validate ms17-com260-kit-async-uart-interactive-shell --strict --no-interactive` → `Change 'ms17-com260-kit-async-uart-interactive-shell' is valid`，exit0。
- `openspec status --change ms17-com260-kit-async-uart-interactive-shell` → `Progress: 4/4 artifacts complete` / `All planning artifacts complete!`，exit0。这是规划产物齐全，不是产品验收或Gate2批准。
- 本次真实运行的UART69/AIA16/tty输出6项测试及静态ELF替代编译命令、决定性结果与退出码见design调查基线；没有运行新目标镜像或真板验收，不声明这些完成。
- 完整性检查：R1–R8均Covered，S1–S28均有直接判定；8任务恰好归属4轮且依赖有序；只创建Iteration000/Cycle000（draft/pending/pending）；Persisted Evidence none；没有需求裁剪或后继Cycle/Evidence目录。已检查Current-State/契约/接口、实际可用native入口与初轮R68操作。
- 仍未改变产品代码、MS12原Cycle或全局文档。MS12 strict结果采信Phase1未失效结论；此前 `.agents` 用户改动保持。Gate1 PASS，Gate2仅等待具体方案审计；Act/板测/Review为pending，Recorder与全局同步SKIPPED。

## 批准交接（2026-10-06）

Gate1/Gate2 PASS；首轮ready，T1–T8仍未完成，Act/Review/板测pending。Phase2验证结论采信上节；此次只更新批准与就绪状态，产品、测试范围及执行契约未变化，不重跑产品测试。Recorder、全局同步和MS12执行继续SKIPPED：本次未授权这些阶段。

此次批准状态同步后，原生 `openspec validate ms17-com260-kit-async-uart-interactive-shell --strict --no-interactive` 返回 `Change 'ms17-com260-kit-async-uart-interactive-shell' is valid`，exit0。就绪状态迁移不改变已验证规格结构。
