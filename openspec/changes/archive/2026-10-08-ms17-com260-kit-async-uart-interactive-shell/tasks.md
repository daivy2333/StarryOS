# MS17 任务与 Iteration Map

R1–R8/S1–S28、默认范围、设计和本 Map 已获批准。Iteration000：Cycle000–002 后 2026-10-07 Review accepted（T1/T2 闭合）。Iteration001：Cycle000 实施 + 首轮 Review 要求有限修复（k3.rs 尾随空白 + diff-check 复跑）→ 修复完成后 2026-10-07 复审 accepted（T3/T4 闭合；crate 74/74、板测 B5 数据面资格全过）。Iteration002：Cycle000 实施后 2026-10-07 Review accepted（T5/T6 闭合；native tty 11/11 + host 9/9、板测 B6 probe 全阶段通过）。Iteration003：Cycle000 实施后 2026-10-07 Review accepted（T7/T8 闭合；native 20/20、板测 B7 两轮全场景 + 操作者裁决）。全部 8 项任务闭合，无待执行 Cycle；change 可交 docs-maintainer 收尾。任务勾选以 Iteration accepted 为准。

## Phase / Gate

| 项目 | 状态 | 结论/边界 |
|---|---|---|
| Phase1 需求、BDD、范围 | completed | 用户明确批准 |
| Gate1 | PASS | proposal User Decision 保留原话 |
| Phase2 实码调查/设计/RTM/分轮 | completed | design、本文与首轮 Cycle |
| Gate2 | PASS | 用户在方案交付及RAM启动说明后明确“批准”；首轮ready |
| Act/Gate3–5 | completed | Iteration000 三 Cycle accepted（T1/T2）；Iteration001 Cycle000 复审 accepted（T3/T4）；Iteration002 Cycle000 accepted（T5/T6）；Iteration003 Cycle000 accepted（T7/T8，2026-10-07） |
| 板测 | completed | Iteration000 B2/B3″/B4 两轮；Iteration001 B5 数据面资格全过；Iteration002 B6 probe 全阶段；Iteration003 B7 两轮交互全场景（含 SIGINT 对照、R68 复位）；豁免与 Minor 见各 Cycle Review |
| 后续 Cycle 展开 | completed | 无剩余 Iteration；Next Iteration: None（2026-10-07 Iteration003 Review accepted） |
| Recorder/Maintainer/归档 | SKIPPED | 未授权；不改 `.agents` |

## 全部任务

- [x] T1：K3 UART 模式与真实 timer 任务底座。（Iteration 000 accepted，2026-10-07）
- [x] T2：PXA port、UART42 IRQ 与低层双向资格载荷。（Iteration 000 accepted，2026-10-07）
- [x] T3：复用 copier，修复事件窗口、wrap/容量与物理 drain。（Iteration 001 accepted，2026-10-07）
- [x] T4：唯一读写 owner、异步输出和 UART 数据面资格载荷。（Iteration 001 accepted，2026-10-07）
- [x] T5：修复实际 tty 行输入、EOF、容量与 readiness 接缝。（Iteration 002 accepted，2026-10-07）
- [x] T6：K3 内存根/ELF/stdio 用户态入口及真实 syscall 资格载荷。（Iteration 002 accepted，2026-10-07）
- [x] T7：内嵌最小 shell 命令与异常输入。（Iteration 003 accepted，2026-10-07）
- [x] T8：交互、quiet/退出、受影响兼容和 MS12 交接检查。（Iteration 003 accepted，2026-10-07）

## Iteration Plan

### Iteration000：timer 与 PXA 硬件可异步等待

- Tasks：T1、T2；Depends on：None。
- Stable baseline：独立 UART 模式能 RAM 启动，既有 task/timer 恢复，UART42 两次真实 IRQ 与低层双向传输成立；不依赖网线/GMAC/PHY。
- Verification boundary：生产 timer/AIA/PXA native tests、旧模式受影响回归、新 port FIT构建；操作者完成首次/重复/孤立字节与 timer/复位操作。
- Diagnostic boundary：feature/bootstrap/timer/CSR、AIA42、PXA U32/IER/cause、最小任务等待；不涉及64KiB/tty/ELF。
- Non-goals：generic copier策略/物理 drain修复、tty和用户态、SMP。
- 状态：completed — 2026-10-07 Cycle002 Review accepted（最终 Cycle；Cycle000/001 见各自 Plan Review）。

### Iteration001：完整 UART 异步数据面与物理 drain

- Tasks：T3、T4；Depends on：Iteration000 accepted。
- Stable baseline：唯一copiers、64KiB rings、无lost wake、容量/short write、有限搬运、最后TEMT completion、等待取消与空闲成立。
- Verification boundary：新增生产copier/ring/flush native tests + 69项既有UART回归；原生 UART FIT、真板有限双向负载/timer/尾部drain/quiet。
- Diagnostic boundary：copier/ring/waker/PXA通知/输出owner；下层路由采信前轮，不掺用户态问题。
- Non-goals：tty与ELF、性能调参、完整终端API、SMP。
- 状态：completed — 2026-10-07 Cycle000 复审 accepted（首轮有限修复后；native 74/74 + 板测 B5 数据面资格全过）。

### Iteration002：tty 与真实内嵌用户态标准I/O

- Tasks：T5、T6；Depends on：Iteration001 accepted。
- Stable baseline：内存根下真实用户程序运行，stdin/stdout唯一tty，canonical/EOF/容量、nonblocking/poll/nanosleep与exit已验收。
- Verification boundary：生产ldisc与loader测试、静态ELF原生readelf、stdio镜像构建、操作者真实read/write/ppoll/sleep/EOF/signal场景。
- Diagnostic boundary：line discipline/FD与syscall、paging/loader/process；UART/timer采用accepted结论。
- Non-goals：shell完整命令、tcdrain/TCSETSW扩展、rootfs/块设备、fork/exec生态。
- 状态：completed — 2026-10-07 Cycle000 Review accepted（tty native 11/11 + host 契约 9/9 + 板测 B6 probe 全阶段）。

### Iteration003：最小交互shell交付

- Tasks：T7、T8；Depends on：Iteration002 accepted。
- Stable baseline：内嵌shell重复help/echo/sleep/exit，错误与长行恢复、空闲IRQ恢复、正常退出/EOF成立；可交接MS12。
- Verification boundary：生产C命令处理native tests、shell ELF/build、真实交互与RAM复位；受影响QEMU/D1构建及共有UART tests。
- Diagnostic boundary：用户输入/命令语义与最终集成；不把下层回归失败归给shell。
- Non-goals：网络命令、完整POSIX shell、K3多hart与组合长稳、性能优化。
- 状态：completed — 2026-10-07 Cycle000 Review accepted（native 20/20、板测 B7 两轮全场景、MS12 交接指针写入；FIT strings 判定为 shell 镜像）。

### 平衡审计

000的timer与PXA共同形成可等待的硬件入口，单拆timer不会形成串口基线；001围绕完整传输/完成契约，与硬件路由分开便于诊断；002将tty与无存储用户态接入聚合为真实stdio结果，避免只验收内核假提示符；003以命令交互和结束形成产品交付。四轮均含实现与自己的行为验证，不单独划“测试轮”；没有首轮承载全部change。每项任务只分配一次，依赖单向。

## Task Contracts（全局契约；当前执行以当前Cycle自包含契约为准）

### T1：UART模式与timer底座

- Requirement/Scenario：R1/R8，S1–S3/S27–S28；Depends on：None。
- Targets：root/kernel Cargo、Makefile、kernel lib/entry gates；K3 irq/time/init，必要生产timer可测核心；`tests/k3-aia-host-harness.rs`及新timer harness。
- Current behavior：K3只external bit9、timer未注册/未启动、默认entry跑网络。
- Required behavior：独立runtime-irq；完整bit5独立handler槽/CAS/mask-before-remove；set_timer(0)触发首次tick，复用runtime；新模式SMP1隔离并拒绝互斥平台/资格载荷组合；原k3不变。native tests覆盖duplicate、timer/external分离、取消不撤共享handler；最小入口真实任务sleep并继续。
- Preserve/Forbidden：24MHz数学、MS10 claim、scheduler/timerwheel、原平台模式；不修axnet、不改registry、不加SMP启动。
- Test witness：新timer生产核心native RED；受改gates变更前GREEN采用design AIA16/现有构建。命令为native rustc --test/test binary，构建make k3-uart-port。
- GREEN/Verification：timer tests与legacy AIA通过，新FIT构建，T2板测中多个sleep/取消后恢复；仅host不宣称真板timer已验收。
- Stop when：需要改runtime初始化或timerwheel、第一次事件未进入独立handler、功能必须依赖GMAC/SMP。

### T2：PXA port与低层IRQ资格

- Requirement/Scenario：R2/R8，S4–S5/S7–S8/S27–S28；Depends on：T1。
- Targets：K3 AIA route42可选门、console共享IRQ-save guard；`kernel/src/drivers/k3_uart{,_core}.rs`、uart_init/entry；native PXA测试与port资格载荷。
- Current behavior：无K3 adapter；盲目开放drivers会落QEMU U8/用户态依赖。
- Required behavior：1152008N1、U32stride4/64BFIFO、UUE/IER通知ownership、有限ISR只status/mask/wake；shared destructive LSR latch、Errata20 task顺序、参考startup三步FCR+运行态PXAR8(0x41)+激活前TX TEMT排空与陈旧RBR排空（Cycle001裁决，取代原1B阈值）；source42 LevelHigh只opt-in。activate先检查once再注册，失败回滚并报阶段；两次/孤立输入均由真实IRQ推进。raw资格载荷只占port，不启动copiers/tty。
- Preserve/Forbidden：bootloader clock/pins/divisor、原四route、早期/故障console；无ISR RBR/THR/打印/分配/忙等owner、不先清活动FIFO。
- Test witness：production core fake U32/IER/IIR/LSR/错误/重复与failedregister RED；legacyroute GREEN。`rustc --edition=2024 --test tests/k3-uart-host-harness.rs -o /tmp/ms17-k3-uart-test`及nativebinary；route42配置同一AIA harness rustc --cfg 'feature="uart-irq"'。
- GREEN/Verification：新tests与老AIA均通过、make k3-uart-port exit0；板测步骤与停止边界见当前Cycle。
- Stop when：必须重写AIA、未知clock/reset、Errata20无法在copier上下文成立或末字节需周期RX轮询。

### T3：copier、wrap、容量和drain

- Requirement/Scenario：R2/R3/R4，S6/S9–S16；Depends on：T2。
- Targets：UART driver.rs/ring_buffer.rs/device_ops.rs/telemetry.rs及既有nativetests；K3 port task侧completion hook。
- Current behavior：RX arm/register窗口与wrap尾部缺口、可能无界poll；D1 slow-poll默认复用于所有port；单槽drain通知/无独立TEMT IRQ。
- Required behavior：check-register-arm-recheck；K3每poll256B/每硬件调用64B、无进展IRQ等待；有限双段wrap、真实drop分类；K3无slow-poll/TEMT spin，只有传输未物理完成时native1ms timer hook；四条件flush、tx.poll广播多waiter、timeout/drop保留accepted数据；空flush立返。
- Preserve/Forbidden：唯一RX/TX copier、64KiB SPSC、active/staged原子、D1默认策略；不造driver/executor/waiter框架、periodic RX或常驻TEMT polling。
- Test witness：在现有driver/ring native test modules加入真实copier窗口、budget/wrap/drop恢复和flush多waiter/timeout测试，观察RED；既有69项GREEN直接采信变更前基线。
- GREEN/Verification：`env RUSTFLAGS='-C linker=/home/daivy/projects/serial/work/StarryOS/scripts/cc-nopie.sh' cargo test --manifest-path crates/uart_16550/Cargo.toml --locked --offline --features async,telemetry --lib -- --test-threads=1`；新增tests均过，native计数与字节数组直接断言，无判定器。board由T4补真实物理完成。
- Stop when：需要改变accepted字节/flush错误契约或取消copier、native timer不能由固定TX任务管理、需要同时改D1硬件行为。

### T4：唯一owner与UART资格

- Requirement/Scenario：R2/R3/R4/R8，S4–S16/S26–S28；Depends on：T3。
- Targets：uart_init、os_arceos、SerializedWriter、K3 entry/资格载荷与Make k3-uart。
- Current behavior：once copier/placement可复用，现有init日志直写pollingconsole，D1failedregister仅记录不阻止copier。
- Required behavior：K3初始化有明确Result/阶段，ready之后仅一次start_copiers；复用placement/handles，普通输出与echo共用serialized ring producer；仅early/fault走受guard保护的同步console。qualification reader/writer lifetime结束后才交唯一tty，不能第二消费者。有限双向流、Full恢复、尾部drain、quiet后新IRQ有效，错误明确分类。
- Preserve/Forbidden：原QEMU/D1 public启动接口、单一producer/consumer；不增加wire协议、身份字段、validator或专用host CLI。
- Test witness：原生集成测试调用实际startup/owner状态，重复activate/失败/接管RED；nativeDriver tests采信T3 GREEN。
- GREEN/Verification：make k3-uart；板上两次输入确定文本、同时输出并运行timer，最后字节后drain完成；quiet10s后新输入恢复；R68复位。256B/FIFO真板流与64KiB/wrap host精确测试分别支持各自结论。
- Stop when：日志存在未归属直接写、第二rawreader/writer、硬件通知不推进或进入driverdeadline。

### T5：实际tty缺口

- Requirement/Scenario：R5，S17–S20；Depends on：T4。
- Targets：tty terminal/ldisc.rs、mod.rs及生产canonical.rs；ntty_async、现有tty_write_logic与kernel/tests/tty_input_logic.rs。
- Current behavior：空格不保留、空VEOF无event、line Vec无上限、tty-reader无限while可能不公平；FIONBIO已正确转换，不是新缺陷。
- Required behavior：空格/tab/编辑、1024B行界限、overflow整行discard→一次InvalidInput、EOF0read event、旧queued数据先读、后续行恢复；readiness包含error/EOF且与真实read一致；每poll256输入字节。单consumer，非阻塞无数据/空间保持WouldBlock，echo best-effort，正文blocking/ONLCR不变；signal/取消不销毁UART。
- Preserve/Forbidden：job control、sys_ioctl bool转换、已有ONLCR/PTY语义；不新增用户tcdrain/TCSETSW drain实现、完整终端或第二tty-reader。
- Test witness：production input/queue/readiness tests先RED，canonical.rs由实际ldisc调用，fake只提供字节/queue边界，不能复制ldisc作stand-in。`rustc --edition=2024 --test kernel/tests/tty_input_logic.rs -o /tmp/ms17-tty-input-test`后运行该原生test binary；现tty_write_logic六项GREEN复用，改ONLCR表面才重跑其native命令。
- GREEN/Verification：空格/EOF/overflow/wrap分段/回压/取消事件测试GREEN；T6用户probe实际输入行/EOF和ppoll返回。未运行人工项不计完成。
- Stop when：须扩展termios/API或修改多reader语义、overlong前缀仍可执行、只能靠无界缓冲保活。

### T6：无存储用户态stdio

- Requirement/Scenario：R1/R5/R6/R8，S17–S22/S27–S28；Depends on：T5。
- Targets：Cargo/lib/entry用户态gates、pseudofs内存根、loader/AddrSpace与生产mm/embedded_loader.rs、file/stdio.rs、stdio/process构造、tests/k3-embedded-user-host、`tests/k3_stdio_probe.c`及其ELF构建。
- Current behavior：内嵌入口仅D1；k3排除模块；loader segment slice可被min(len)截短，缺少本次明确的拒绝契约。
- Required behavior：k3-shell/k3-stdio选择真实ET_EXEC载荷，内存根/stdio完成后才spawn，kernel/MMIO与USER段/栈权限正确、BSS零、argv和icache同步；ELF格式/越界/entry/动态载荷拒绝。ELF/mapping/open这些可恢复失败在Process::new_init之前，失败对象释放、FD表不发布；new_init是一次提交点，tty绑定不变量失败明确终止到reset，不能假设INIT_PROC可回滚或重试。probe真实read/write/FIONBIO/ppoll/nanosleep/exit，最后kernel物理drain。
- Preserve/Forbidden：现有parser/ProcessData/syscall，网络k3排除；不rootfs/axruntime fs probe、registry fork、新ELF加载框架或移植SMP迁移ioctl。
- Test witness：提取既有eager函数的实际校验/映射顺序到生产embedded_loader，native backend仅替换map/populate/write；stdio实际组装函数的fake仅替换open。tests/k3-embedded-user-host加入无效/截短/entry/mapping/open失败RED，断言实际生产路径的顺序/返回/未发布与drop；`env RUSTFLAGS='-C linker=/home/daivy/projects/serial/work/StarryOS/scripts/cc-nopie.sh' cargo test --manifest-path tests/k3-embedded-user-host/Cargo.toml --offline -- --test-threads=1`。合法loader重用D1行为先GREEN再放宽feature。
- GREEN/Verification：原生readelf -h -l -r、静态compile exit0、make k3-stdio；板测probe打印实际argv→EAGAIN→输入完整行ppoll/read返回→sleep恢复→EOF/退出实际join结果。错误fixture只在native测试；不另造板测错误镜像矩阵。
- Stop when：必须支持动态链接/重定位、存储、扩大用户内核地址范围或更新依赖才能运行；paging/trap异常先停在本层。

### T7：最小shell

- Requirement/Scenario：R7，S23–S25；Depends on：T6。
- Targets：`kernel/resources/k3-shell.c`、必要ELF Make规则、entry嵌入选择、production command native C tests。
- Current behavior：无K3 shell。
- Required behavior：help、echo文本、sleep0..60000ms、exit0；空行/未知命令/参数错误/整数溢出/超长行反馈后继续；256B命令buffer，完整读行后执行，不执行截断前缀。read/write EINTR/shortwrite与持久错误退出，sleep走nanosleep。
- Preserve/Forbidden：既有user/syscall/tty，轻量静态C程序；不fork/exec、脚本/管道/重定向、网络命令/工具框架。
- Test witness：新C native tests include/call production parse/dispatch函数，以fake I/O/timer只替换系统边界；cc -std=c11 -Wall -Wextra -Werror 编译tests/k3_shell_test.c并执行。先RED后GREEN。
- GREEN/Verification：参数/多命令/文本空格/长行/shortwrite/EINTR testsGREEN；静态ELF readelf有效；make k3-shell；board T8对应用户交互。
- Stop when：parser需要新libc/依赖或只能通过裁剪命令/用户態要求实现。

### T8：最终交互与兼容交接

- Requirement/Scenario：R7/R8，S23–S28；Depends on：T7。
- Targets：shell入口/退出/有限等待，受影响build与tests；MS12 README/design/tasks中仅准确交接指针（不改原Cycle）。
- Current behavior：目标镜像未运行；MS12原首轮ready暂停。
- Required behavior：重复help/echo、sleep1000后echo、非法参数与longline后恢复；quiet10s后新输入；exit/EOF/SIGINT分别按已验收流程结束user而owner保持；join报告实际结果、结束输出native1s drain，timeout明确失败不清数据；外部reset回原系统。MS12恢复需accepted底座和必要replan，不直接跳过网络要求。
- Preserve/Forbidden：原make k3、QEMU/D1构建与共享driver行为；不改冻结MS12Cycle、全局tasks/M/R/I或Recorder产物。
- Test witness：退出/错误/等待已有T6/T7生产tests，新增变化有先行RED或既有GREEN；本任务不另建证据工具测试。
- GREEN/Verification：受改共享UART suite、原make k3、新make k3-shell；受改QEMU门时make build，D1门/共享loader改时make lichee与make lichee-userbench。VF2既有失败与全量host已知失败报告原归属，不重复全量失败追求全绿。已有成功且覆盖未变直接采信。最后构建实际待测shell FIT，避免同名产物被旧target覆盖。board按上述直接输入/结果；无额外日志协议。
- Stop when：需要SMP/网络/存储以通过shell验收，退出丢数据却报成功，原平台产生本次回归或board未提供结果。

## Requirements Traceability Matrix

| Requirement | Task | Test Witness | Status |
|---|---|---|---|
| R1 | T1,T6 | 生产timer生命周期/取消测试、真板sleep、用户nanosleep | Covered |
| R2 | T2,T3,T4 | PXA真实core fake、copier窗口、真板UART42两次/孤立字节 | Covered |
| R3 | T3,T4 | production wrap/Full/drop恢复/budget、板上双向流 | Covered |
| R4 | T3,T4,T6 | production四条件/multiwaiter/drop/timeout、板上末字节drain | Covered |
| R5 | T5,T6 | production ldisc/ONLCR、用户EAGAIN/ppoll/canonical/EOF/signal | Covered |
| R6 | T6 | production ELF/失败未入队与释放、真实user argv/syscall/exit | Covered |
| R7 | T7,T8 | production C命令测试、真板help/echo/sleep/quiet/exit | Covered |
| R8 | T1,T2,T4,T6,T8 | feature/build、R68 RAM/reset、受影响回归、MS12冻结检查 | Covered |

场景与最简直接判定见 design 验证矩阵；每个Task对应场景已列于其契约。无Simplified/Missing，无接受的场景缺口。Persisted Evidence none；Act Response保存命令/≤20行输出/exit或人工一句结果。全部任务验收前保持未勾选，当前Cycle不存在后继产物。
