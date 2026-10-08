# Iteration 001 / Cycle 000: 完整 UART 异步数据面与物理 drain

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: T3、T4（Iteration 001 既有任务）。
- Acceptance gaps: None。
- Repair items: None。
- Inherited scope: R2/R3/R4 已批准场景（S4–S16、S26–S28）与全局契约；design 裁决（K09 唯一 `embassy-sync::AtomicWaker`、K26 不复制 UART 字节 ring、Q19C.8e D1 slow-poll 有据保留、D5 显式 copier future + 预入队 affinity）；Iteration 000 accepted 基线（timer 底座 B2、AIA route42 交付链、`K3UartCore` 激活/等待协议/ISR 纪律、raw 双向资格真实 IRQ 推进、CR/LF 行接受、512 上限）；板上裁决事实（EIE 掩码窗口 MSI 丢弃、IIR 对掩码 cause 照报、SETIP 对 LevelHigh 无效、rx_wakes 计数口径失真已知）。
- Excluded scope: T5–T8（tty/用户态/shell）、Iteration 000 raw 载荷行为、SMP（MS14）、性能优化、受控迁移、MS12 网络恢复、全局文档与经验产物。

**Objective**

K3 上唯一 RX/TX copier + 64KiB ring 的完整异步 UART 数据面：copier 事件窗口无丢失唤醒、wrap/容量边界有界且 drop 可分类、TX 末字节物理 drain 有 native timer 见证、K3 路径无 D1 slow-poll/TEMT spin；板上有限双向负载、Full 恢复、尾部 drain、quiet 后新输入全部经真实 IRQ 推进。

**Background**

Iteration 000 accepted 建立了"硬件可异步等待"基座（timer + PXA port 真实 IRQ 双向资格）。本 Iteration 把 `uart_16550` crate 的通用 copier 修到 K3 契约（事件窗口、wrap 余量、drop 分类、drain 策略），再把 K3 port 接入 `uart_init` 唯一 owner 启动架构，形成完整数据面。

**Investigation Facts**

Current Baseline：Iteration 000 最终 Act Response（Cycle 002）+ accepted Review。native uart 24 GREEN、`make k3-uart-port` exit0、板测两轮全过（A1″–A4″）；timer 8 项、AIA 17/18、`make k3`/`make build`/`make lichee` 采信（覆盖未变）。工作区 = MS17 Cycle 000–002 全部改动（未提交）。

Current-State Evidence 与关键路径（T3/T4 新表面，本 Plan 本次实读）：

1. `crates/uart_16550/src/async_/driver.rs`
   - `rx_copier_loop`（535–580 行）：receive(check)→`push_batch`→NAPI(阈值 16/批 64)→`update_ier(DATA_READY)`(arm)→`RX_WAKER.register`→Pending，**register 后无 recheck**。arm→register 间到达的 IRQ 其 wake 是 dropped no-op（crate 自证用例 `prepoll_wake_without_registered_waker_is_noop`）；K3 raw 路径已用 check→register→arm→recheck 纪律（Iteration 000 交付）。
   - 550 行 `self.rx.push_batch(&read_buf[..total]);` **返回值忽略**：ring 近满或空闲段 wrap 时余量字节被静默丢弃、无 drop 分类。
   - `tx_copier_loop`（583–870 行）：fast retry 32→register `TX_WAKER`+arm THRE+final recheck→**D1 slow-poll（4096×256 spins ≈1–2 ms busy）**→self-wake yield×4→纯 ISR 等；TEMT corner（848–856 行）**256 次 spin 轮询**。slow-poll 注释明确 L266 裁决"不得以 FAST=0+drain 侧 TX_WAKER 替代"——D1 行为保留，K3 以 port 策略豁免。
   - TX 空环 park（Task 3.6 replan 已闭合）：register ring waker + `TX_WAKER`→one-shot THRE（`thre_idle_armed`）→register 后 recheck；`transmitter_empty` 时 `DRAIN_WAKER.wake()`（666–668、860–861 行）——K3 drain 完成见证可经 TX copier park 路径，不依赖 ISR DRAIN 分支。
   - `TxCompletion` 四条件（`ring_empty && !copier_active && staged_bytes==0 && transmitter_empty`）已在 crate；`flush()`（device_ops.rs 323–347 行）已实现四条件 + register→recheck。
2. `crates/uart_16550/src/async_/ring_buffer.rs`：push/pop/push_batch/pop_batch 均单段连续拷贝（embassy writer/reader 闭包只暴露连续段），wrap 时返回部分接纳数——调用方必须处理余量；`occupied_len`/`vacant_len` 已跨段计数（readiness 无缺口）；`RingBufRx::pop` 为 pub(crate)，仅经 `AsyncUartReader` 暴露。
3. `crates/uart_16550/src/async_/isr.rs`：`uart_isr_handler` 为 QEMU/D1 路径；K3 用 `K3UartCore::isr` 有界分类后 wake RX/TX_WAKER（`kernel/src/drivers/k3_uart.rs:101–109`）。
4. 既有测试基线 **69 项** = lib 3 + spec 42 + ring_buffer 17 + driver 7（`smp_witness_tests` 6 + prepoll no-op 1）；命令 `env RUSTFLAGS='-C linker=/home/daivy/projects/serial/work/StarryOS/scripts/cc-nopie.sh' cargo test --manifest-path crates/uart_16550/Cargo.toml --locked --offline --features async,telemetry --lib -- --test-threads=1`。
5. `kernel/src/drivers/uart_init.rs`（563 行）：QEMU/D1 双路径；64KiB×2 静态 ring、`DRIVER: Once<Arc>`、`start_copiers`（`placement::place_roles` + singleton affinity 先于首次入队 + fail-closed 重复守卫 `RX/TX_COPIER_STARTED` + 句柄保存）；`place_roles`（placement.rs:103）对单元素 schedulable 集合自然退化为两 copier 同落唯一 hart，无需特判。`ArceOsWriter = SerializedWriter<RawArceOsWriter>` 实现 TtyWrite/TtyWriteReady。k3-async-uart 模式下 `uart_init`/`os_arceos`/`serialized_writer`/`placement` 被 `drivers/mod.rs` 整体排除。
6. `kernel/src/drivers/k3_uart.rs:148–164`：`K3UartPort` 已实现 `UartPort` trait（receive/send/transmitter_empty/update_ier 直通 `K3UartCore`）——copier 侧接缝已在 Iteration 000 预置。
7. feature 链：root `k3-uart-port-test=[k3-async-uart,…]`、`k3-async-uart=[k3, axplat-riscv64-k3/runtime-irq, …/uart-irq, starry-kernel/k3-async-uart]`；Makefile `k3-uart-port`（SMP=1 LOG=off）。`make k3-uart` 与对应 feature 尚不存在。
8. `kernel/src/entry.rs`：k3-async-uart 分支直接 `run_uart_port_bringup()`（park 结束）；QEMU 分支 init→bench→start_copiers 的启动序可参考。

**Implementation Guidance**

先 crate 内修 RX copier 窗口/wrap 余量/drop 分类（native RED 先行），再以 port 策略 seam 豁免 K3 的 slow-poll/TEMT spin（如 `UartPort` 关联常量或等价形态），最后接 `uart_init` K3 路径、资格载荷与 `make k3-uart`。策略 seam 的具体形状为非实质选择；D1/QEMU 默认策略不变是契约。

**Task Contracts**

### T3：copier 事件窗口、wrap/容量与 drop 分类（crate 层）

- Requirement/Scenario：R2/R3/R4；S6/S9–S16。
- Depends on：None。
- Targets：`crates/uart_16550/src/async_/driver.rs`（`rx_copier_loop`/`tx_copier_loop`、策略 seam）；`ring_buffer.rs`（`push_batch` 余量语义）；`device_ops.rs`（drop 可观测，如需）；既有 native test modules（`smp_witness_tests`/`readiness_tests`）。
- Current behavior：RX copier arm→register 无 recheck（窗口丢失唤醒）；`push_batch` 返回值忽略（wrap/近满余量静默丢弃）；TX 侧 D1 slow-poll/TEMT spin 对所有 port 生效。
- Required behavior：①RX copier 改 check→register→arm→recheck（与 K3 raw 路径同纪律，register 后 recheck 关闭 arm 前到达的丢失窗口）；②RX copier 处理 `push_batch` 部分接纳：ring 无空间且硬件仍有数据时按既有预算有界推进，无法接纳的字节计入显式 drop 分类（ring-full 类），不静默、不无界等待；③K3 策略：TX copier 无 slow-poll、无 TEMT spin，传输未物理完成时以 native 1 ms timer hook 等待代替（D1/QEMU 默认策略不变）；④K3 策略下每 poll 256 B / 每硬件调用 64 B 预算成立（`K3UartCore::HW_CALL_LIMIT=64` 已有，copier 侧预算对齐）。
- Preserve：D1 slow-poll 常量与行为（L266 裁决）、NAPI 常量、`TxCompletion` 四条件与 `flush` 语义、69 项既有测试语义（新测试加入同一 test modules）。
- Forbidden：删除/绕过 D1 slow-poll、复制第二套 copier/ring/waker 框架、周期 RX 轮询、常驻 TEMT polling、无界重试或无界 ring。
- Test witness：crate native 新用例——arm→register 窗口（register 后、park 前注入数据/IRQ，断言无丢失唤醒）；push_batch wrap 余量（构造 wrap 环，断言部分接纳显式返回）；drop 分类（近满环 + 超预算输入，断言 drop 计数且已接纳数据完整）；K3 策略分支（策略关闭 slow-poll/TEMT spin 时 copier 不进入 spin 路径）。RED = 新用例缺失编译失败或断言失败，GREEN 后 69 项不回归。
- GREEN condition：新增用例 GREEN 且 69+新 全 GREEN。
- Verification：`env RUSTFLAGS='-C linker=/home/daivy/projects/serial/work/StarryOS/scripts/cc-nopie.sh' cargo test --manifest-path crates/uart_16550/Cargo.toml --locked --offline --features async,telemetry --lib -- --test-threads=1`（69 基线变更前 GREEN 采信；新用例 RED→GREEN；全量复跑一次）。
- Stop when：事件窗口修复需要改 executor/waker 框架语义，或 drop 分类需要改变 accepted flush/字节契约。

### T4：K3 唯一 owner 接入与 UART 数据面资格载荷

- Requirement/Scenario：R2/R3/R4/R8；S4–S16/S26–S28。
- Depends on：T3。
- Targets：`kernel/src/drivers/uart_init.rs`（K3 第三路径）；`drivers/mod.rs`、`os_arceos.rs`、`serialized_writer.rs` 门；K3 数据面资格载荷（`platform/k3.rs` 或新文件）；root/kernel `Cargo.toml` feature；`Makefile` `k3-uart`；`kernel/src/entry.rs`。
- Current behavior：k3-async-uart 模式排除 `uart_init`/`os_arceos`/`serialized_writer`/`placement`；唯一入口为 raw bringup（park 结束）；无 `make k3-uart`。
- Required behavior：①K3 数据面模式（root feature `k3-uart` + `make k3-uart`，SMP=1）：`K3UartCore::activate` 序列激活 → ready 后恰好一次 `start_copiers`（`place_roles` 单 hart 退化 + singleton affinity 先于首次入队 + fail-closed 重复守卫复用）；②普通输出与 echo 共用 `SerializedWriter` 单 producer（`AsyncUartWriter` 唯一写者）；early/fault console 仍走共享 guard 的同步路径；③资格载荷（一次性、所见即判定）：有限双向流（输入行经 ring→reader、输出经 writer→ring→copier→TX）、长输入（≥256 B，跨 64 B 硬件预算多轮）、TX ring Full 压入→copier 排空恢复、尾部 drain（最后字节后 `TxCompletion` 四条件完成行）、quiet 10 s 后新输入立即恢复、错误分类可见；④owner 语义：重复 activate 显式失败、copier 重复 start fail-closed、reader/writer lifetime 内不出现第二消费者。
- Preserve：QEMU/D1 构建与行为零回归（`make build`/`make lichee`）、Iteration 000 raw 载荷不动（`k3-uart-port-test` 保留）、AIA/timer 底座与共享 guard、`k3-uart-port` 交付判据。
- Forbidden：网络模块进入 K3 数据面构建、第二 RX/TX copier 或直接写 THR 的常规输出路径、wire 协议/身份字段/validator/专用 host CLI、tty canonical 语义。
- Test witness：uart_init K3 路径中可纯化的启动/守卫逻辑 native 测试（重复 start fail-closed、单 hart placement 退化）；数据面行为以板上直接观察为主。
- GREEN condition：`make k3-uart` exit0、受影响共享门不回归、板上资格全过。
- Verification：agent——`make k3-uart` exit0；`make build`；`make lichee`（T3 触及 driver.rs 共享表面，必须重跑）；`make host-test`（共享 UART/placement 表面受改时重跑；已知一项 pre-existing 失败按 SNAPSHOT 遗留归属，不计新失败）；`git diff --check`。操作者（B5，按 R68 流程）——双向行回显（经 ring+copier，非 raw 直写）、≥256 B 长输入完整、TX Full 后恢复、尾部 drain 完成行、quiet 10 s 后新输入恢复、B4 reset 回原系统；一句话结果记入 Act Response。
- Stop when：需要 SMP、存储、tty canonical 或 D1 硬件行为变更才能通过；日志存在未归属直接写；第二消费者无法以类型/运行时守卫拒绝；板上资格失败按 Iteration 000 判读表模式（快照 + 错误分类）停并交 Plan。

**Invariants**

1. Iteration 000 全部不变量与板上裁决事实继续有效（EIE 掩码窗口 MSI 丢弃→掩码只作止损；IIR 对掩码 cause 照报→rx_wakes 口径失真已知；真实输入 IRQ 仍是 S4 唯一验收）。
2. K09/K26：只用 `embassy-sync::AtomicWaker` 与既有 `axpoll::PollSet`；不引入第二套 executor/ring 语义。
3. Q19C.8e：D1 slow-poll 是有据 workaround，保留于 D1/QEMU 路径；K3 豁免必须以 port 策略 seam 表达，不得改全局常量。
4. copier 唯一性：每方向恰好一个 copier，affinity 先于首次入队，重复 start fail-closed，句柄保存。
5. QEMU/D1/VF2 既有构建与 69 项测试语义不回归。
6. raw 资格载荷（`k3-uart-port-test`）行为不变。

**Non-goals**

tty/ldisc/用户态（Iteration 002）、SMP（MS14）、性能调参、受控迁移、MS12 网络、完整终端 API、ISR 8-cause 预算优化。

**Acceptance**

| ID | 可观察结果 | 映射 |
|---|---|---|
| A1 | crate native：新增事件窗口/wrap/drop/策略用例 RED→GREEN；69+新 全 GREEN | T3 / R3 / S9–S16 |
| A2 | K3 无 slow-poll/TEMT spin：策略用例见证 copier 不进入 spin 分支；D1 默认路径用例不回归 | T3 / R2 |
| A3 | `make k3-uart` exit0；`make build`/`make lichee` 不回归；host-test 无新失败 | T4 / R8 / S27 |
| A4 | 板上数据面资格：双向流经 ring+copier、长输入完整、Full 恢复、尾部 drain、quiet 恢复、B4 reset | T4 / R2/R3/R4 / S4–S16/S26–S28 |

RTM（本 Cycle）：R2→T3/T4 Covered；R3→T3/T4 Covered；R4→T3/T4 Covered；R8→T4 Covered。R1/R5–R7 属后续 Iteration。

**Verification**

V1（agent，native）：crate cargo test 全量（69 基线变更前 GREEN 采信 → 新用例 RED → 全量 GREEN exit 0）。
V2（agent，build）：`make k3-uart` exit0；`make build`；`make lichee`；受改共享表面时 `make host-test`（pre-existing 失败按归属不计）；`git diff --check`。
V3（操作者，B5）：按 T4 Verification 步骤与判读；无结果则 A4 pending，不展开 Iteration 002。
V4（失败处理）：板上任一阶段失败→快照 + 错误分类，停并交 Plan；三次失败规则适用。

**Gate 2 Readiness**

PASS — 2026-10-07 用户计划审计原话"批准"。调查闭合（Iteration 000 accepted 基线 + 本 Plan 实读的 crate/内核接入面：69 项测试基线、RX copier 窗口/wrap 缺口、D1 slow-poll 裁决、`place_roles` 单 hart 退化）、契约自包含、追踪完整、验证为最简直接判定；无 BLOCKED/WAIVED、无 Missing/Simplified、无待 Act 决定的实质未知项（策略 seam 形状与载荷文件归属为非实质选择，留在 Act）。Plan Context 离开 draft 改为 ready；下一边界为 Act 指令。

**Persisted Evidence**

- Mode: none

命令、≤20 行决定性输出、exit 与操作者一句话结果足以判定。

**Risks and Notes**

- `make host-test` 一项 pre-existing 失败（ms04 placement 来源）与 vf2 入口失败为本仓已知遗留（SNAPSHOT 记录），不因本 Iteration 阻塞；host-test 重跑只要求"无新失败"。
- 板测 B5 的错误注入边界（如拔线）不预先承诺：Stop when 已限定失败即停交 Plan。
- TX ring Full 压入需要输出量 > 64 KiB 或消费侧停顿配合；载荷以"copier 暂停窗口内压满 + 恢复后排空"最简实现，具体脚本属非实质选择。
- telemetry 计数（drop/预算耗尽）在 K3 数据面构建中的开关跟随既有 feature 组织，不新增构建矩阵。

## Act Response

- Status: reported（2026-10-07 修复完成后覆盖为完整当前状态：原实施 + Review F2 有限修复 + 最终树复验，见「当前 Cycle 修复记录」）
- Date: 2026-10-07

### 实际改动

**T3（crate 层）**

- `crates/uart_16550/src/async_/ring_buffer.rs`：`RingBufRx::push_batch` 升级为有限双段接纳（至多两次 `Writer::push`），wrap 余量不再只收连续尾段；返回值供调用方做 drop 分类。
- `crates/uart_16550/src/async_/telemetry.rs`：`Telemetry` 新增 `rx_dropped_bytes: AtomicU64`（ring-full 类 drop 计数），接入 `new`/`reset`/`Debug`。
- `crates/uart_16550/src/async_/driver.rs`：
  - `rx_copier_loop` 重写为 check → register(`RX_WAKER`) → arm(`DATA_READY`) → recheck 纪律；park 边注册先于武装，arm 后 recheck 收口；NAPI 阶梯（阈值/批长/退出版）语义保留；`push_batch` 返回值参与 drop 分类（telemetry 门控）。
  - `UartPort` trait 新增策略 seam：`const SLOW_POLL_LIMIT`（默认=原 `TX_SLOW_POLL_LIMIT`）、`const TEMT_POLL_LIMIT`（默认=原 `TX_TEMT_POLL_LIMIT`）、`fn tx_stall_wait()`（默认立即就绪 future）。
  - `tx_copier_loop` 引入 `TxPollOutcome{Normal,StallWait}`：`SLOW_POLL_LIMIT==0` 时预算耗尽路径（TX_WAKER 已注册、THRE 已武装、final recheck 之后）返回 StallWait，不走 slow-poll/yield 阶梯；`TEMT_POLL_LIMIT==0` 时 TEMT 角不走 spin，register+武装 THRE 后返回 StallWait；外层循环在 StallWait 上 `await uart.tx_stall_wait()`。D1/QEMU 默认路径控制流等价（spin 常量原值启用）。

**T4（K3 接入）**

- `kernel/src/drivers/copier_start_guard.rs`（新增）：`CopierStartGuard{claim,rollback,claimed}`，fail-closed 一次性启动守卫。
- `kernel/src/drivers/k3_uart_core.rs`：新增纯函数 `map_copier_ier(set,clear)`——抽象 `DATA_READY` ⇔ 全 RX 组（RDI|RLSI|RTOIE）映射；ISR 每次 wake 掩掉整组，只回武装 RDI 会让 RTOIE 保持掩码、短行（<8 字节触发）失唤醒。
- `kernel/src/drivers/k3_uart.rs`：`K3UartPort` 的 `UartPort` impl 设 `SLOW_POLL_LIMIT=0`/`TEMT_POLL_LIMIT=0`；`tx_stall_wait` = `axtask::future::sleep(1ms)`（`K3_TX_TIMER_WAIT_MS`）；`update_ier` 经 `map_copier_ier`。
- `kernel/src/drivers/uart_init.rs`：新增 K3 第三路径（`k3-uart` cfg 臂）——类型别名（ArceOsDriver/ArceOsReader/RawArceOsWriter over `K3UartPort`）、`init_uart_hardware` K3 臂（`activate_port` 显式阶段结果，失败 panic 指名阶段）、QEMU 路径 cfg 收窄为 `not(any(lichee-d1-async-uart, k3-uart))`；`TtyWrite`/`TtyWriteReady` impl 及其 import 在 k3-uart 下门出（本形态无 pseudofs/tty）；`ArceOsWriter::push` 固有方法（载荷单生产者输出）；`start_copiers` 守卫换为 `CopierStartGuard`（claim/rollback，panic 语义不变）。
- `kernel/src/drivers/mod.rs`：门控重排——数据面树（uart_init/os_arceos/placement/serialized_writer/uart_smp_snapshot/hart_counter/uart_snapshot_types/copier_start_guard）改为 `not(all(k3-async-uart, not(k3-uart)))`；k3 排除模块（bench/net_*/ntty_async/ASYNC_TTY/uart_migration_logic）原门不动。
- `kernel/src/entry.rs`：新增 `k3-uart` 数据面分支（init_uart_hardware → start_copiers → `run_uart_data_plane`）；raw 分支加 `not(feature="k3-uart")`。
- `kernel/src/platform/k3.rs`：`run_uart_data_plane` 资格载荷——唯一 reader/writer、重复 activate 显式拒绝、ready 横幅、2× 行回显、≥256B 长行（含 line-too-short 提示）、72 KiB NUL TX 突发（Full 见证=short_writes，20ms 退避）、driver 侧四条件 drain 等待（`wait_tx_drain`）、quiet 10s + post-quiet 回显、错误分类计数行；`emit` 做 LF→CRLF；故障路径复用 `fault_snapshot`+park。
- `Cargo.toml`（根）/`kernel/Cargo.toml`：新增 `k3-uart` feature（根：`k3-async-uart` + `starry-kernel/k3-uart`；kernel：`k3-async-uart` + `uart_16550/telemetry`）。
- `Makefile`：新增 `k3-uart` 目标（SMP=1 前置检查、同一裁决 MEM/PLAT 窗口、k3-fit 打包）及 `.PHONY`。

### 与计划的偏差

1. `uart_init` 的 TtyWrite/TtyWriteReady impl 在 k3-uart 下门出（k3 形态无 pseudofs/tty，计划未预见）；载荷经新增固有 `ArceOsWriter::push` 输出——单生产者契约不变。
2. K3 `update_ier` 将抽象 DATA_READY 映射为全 RX 组（经纯函数 `map_copier_ier`，host 可测）；计划隐含直通。短行唤醒（PXA 8 字节触发 + RTOIE）所需，属接缝适配。
3. `start_copiers` 守卫由裸 AtomicBool swap 提取为 `CopierStartGuard`（契约要求该守卫逻辑 native 测试，提取为可测模块是最小路径）；claim/rollback 语义等价，QEMU/D1 panic 行为不变。
4. TX Full 突发以 NUL(0x00) 输出（用户 2026-10-07 指示，对齐 QEMU bench `vec![0u8;1024]` 形态）；Full 见证由内核账本 requested/accepted/short_writes 承担。
5. ms04 harness 两处过期见证经用户指示修复：(a) `uart_start_copiers_uses_pre_enqueue_affinity_and_saves_handle` 守卫拼写 swap(true)→claim()（语义等价）；(b) `net_migration_stimulus_is_blocked_gated_in_source` 门控包含性检查由"gate 与 wake 间无 `}`"改为括号深度检查——MS08 在分支内加入的 `#[cfg(feature="qemu")] { wakes += 1; }` 平衡遥测块致原断言误报，生产语义（Blocked 门控在前、wake 在分支内、三个终止标记、`parked=` 诊断）逐项核实完好。此项即 SNAPSHOT 登记的 pre-existing 失败，本轮关闭。
6. 载荷 drain 以 driver 侧 `wait_tx_drain` 实现（与 device_ops::flush 同四条件、同 register→recheck 顺序）——`ArceOsWriter` 内层为 `SpinNoPreempt` 包裹的 crate writer，锁不可跨 await。

### 自检发现与修复

- 实现过程中自身测试逻辑错误 3 处（pop 清空后断言 occupied、staged 误断言 ring-empty、DRAIN wake 轮询顺序），均为测试侧修正，实现未动，重跑全绿。
- 板测驱动修复 2 处：(1) 载荷输出缺 CR 致终端阶梯显示——加 `emit` LF→CRLF；(2) `read_line` 被误包无限 `loop`，future 永不完成、60s 超时（板测 fault-snap：acc_len=178 多行累积、RX 全链路证据完好）——改为单 `poll_fn`（与 `raw_rx_stage_into` 同型）。两修复均经后续板测验证。
- 遗留 Minor：无阻塞项。`rx_wakes` 计数口径失真为 Iteration 000 已登记事实；ms04 `cross_hart_second_acquire_waits_for_first_release` 在并行测试线程下有一次偶发失败（重跑通过，机制=并行用例共享进程级 `NEST_DEPTH`/`GLOBAL_LOCK`），已记 Experience 候选。

### 验证（命令与决定性输出）

- T3 crate native（RED 两阶段先观察：断言 RED `left: 2 right: 4` / `left: 0 right: 3`；编译 RED：trait 常量/`tx_stall_wait`/`rx_dropped_bytes` 缺失）：
  `env RUSTFLAGS='-C linker=scripts/cc-nopie.sh' cargo test --manifest-path crates/uart_16550/Cargo.toml --locked --offline --features async,telemetry --lib -- --test-threads=1`
  → `test result: ok. 74 passed; 0 failed`，exit 0（69 基线 + 5 新增）。
- T4 native：`rustc --edition=2024 --test tests/k3-uart-plane-host-harness.rs -o /tmp/ms17-k3-uart-plane-test && /tmp/ms17-k3-uart-plane-test` → `3 passed` exit 0；`tests/k3-uart-host-harness.rs` → `25 passed` exit 0；`tests/ms04-async-rx-host-harness.rs` → `105 passed` exit 0。
- 构建：`make k3-uart` exit 0（FIT 321056 B；`strings` 验证含 `mode=k3-uart-dataplane begin` 与 `pattern=nul-0x00`）；`make k3-uart-port` exit 0；`make build` exit 0；`make lichee` exit 0；`git diff --check` exit 0（**最终树复验结果**，见文末修复记录——首轮声明的"干净"为过期结论，被 Plan Review F2 指出后重跑）；`openspec validate ms17-com260-kit-async-uart-interactive-shell` → valid。
- host-test 全套由操作者运行：通过（`cross_hart_second_acquire_waits_for_first_release` 一次偶发失败后重跑通过，见上）。
- V3 板测（操作者 2026-10-07，B5，`make k3-uart` 镜像）：`duplicate-activate result=rejected`；`ready ring-output=live`；echo1 len=3、echo2 len=192（跨多轮 64B 硬件预算回显完整）、`long received_len=356 echo=done`；`txfull drained requested=73728 accepted=73728 short_writes=83`（终端零刷屏，NUL 形态）；`drain done (TxCompletion four conditions)`；quiet 10s 后 post-quiet 立即回显（len=18 含 quiet 窗内排队字节，"旧数据先读"契约行为）；`errors rx_dropped_bytes=0 tx_no_progress=74562 tx_hw_bytes=74919 irq=2849 rx_wakes=4584 tx_wakes=2276 unknown=0 oe=0 pe=0 fe=0 bi=0 fifoerr=0 closed=false`；`qualification complete park`；操作者确认复位（B4）回原系统。

### Acceptance 对照

A1 ✓（74/74 含新用例）；A2 ✓（策略用例见证 K3 不进 spin 路径，D1 默认路径用例不回归）；A3 ✓（构建/门全过、host-test 无新失败、V2 于最终树复验成立）；A4 ✓（操作者转录如上）。RTM：R2/R3/R4/R8 Covered；R1/R5–R7 属 Iteration 002/003。

### 当前 Cycle 修复记录（2026-10-07，按 Plan Review Follow-up 执行）

- **修复对象（Review F2）**：首轮 Act Response 声称"git diff --check 干净"为过期结论——16:48 板测驱动修复（`emit` 批量替换在行尾留下空格）后未复跑该项，最终树上 `git diff --check` 实为 exit 2（`kernel/src/platform/k3.rs` 586/632/717/732 共 4 处尾随空白）。
- **修复内容**：删除 4 处尾随空白（纯格式，无可观察行为变化——按 Review 裁定，板测证据对行为等价镜像继续有效，无需重上板）。
- **修复后复验（最终代码树，2026-10-07 17:04）**：`git diff --check` → exit 0；`make k3-uart` → exit 0（FIT 321056 B 重新打包，`strings` 验证含 `mode=k3-uart-dataplane begin`）；`openspec validate` → valid。其余验证项（crate 74/74、harness 3/25/105、make build/lichee/k3-uart-port、板测转录）覆盖未变，按 Review 采信结论沿用。

### Persisted Evidence

- None required（Plan 模式为 none；命令、≤20 行输出、exit 与操作者转录已入本 Response）。

### Experience Candidates

- Issue 候选（本 change 范围外，测试基建）：`tests/ms04-async-rx-host-harness.rs` 的 `cross_hart_second_acquire_waits_for_first_release`（及同组直驱真实 `acquire`/`release` 的用例）在默认并行测试线程下共享进程级 `NEST_DEPTH`/`GLOBAL_LOCK`，存在用例间串扰（本会话观察：用户两轮运行一败一过；机制分析：他例并发占用同 hart 深度致外层 acquire `prev!=0` 而跳过 GLOBAL_LOCK）。候选处置：以 harness 级 Mutex 串行化直驱策略状态的用例。证据：本 Response「自检发现」+ 用户 2026-10-07 两次 host-test 转录。
- Runbook 候选（R68 增补）：`make k3-uart` 与 `make k3-uart-port` 产出同名 `StarryOS_riscv64-k3.fit`，交叉验证构建会互相覆盖，上板前以镜像内 `mode=` 行或构建时间判别（本会话实际发生一次误烧旧模式镜像）。证据：本 Response「自检发现」。

### 未解决问题

- 无阻塞项。A1–A4 全部成立，等待 Plan Review 审计；Iteration 002（T5/T6）待本 Iteration accepted 后展开。

## Plan Review

- Review Result: accepted（2026-10-07 复审；Iteration 001 完成。首轮 Review 的有限修复已闭合，见 F2 与 Follow-up）

**Findings**

1. 代码/diff 独立核对通过，与契约逐项一致：
   - T3 ①`ring_buffer.rs` `push_batch` 有限双段接纳（至多两次 push，wrap 余量显式返回），新用例 `rx_push_batch_accepts_across_wrap` 见证跨段接纳与字节序；②`driver.rs` `rx_copier_loop` 重写为 check→register→arm→recheck（register 先于 arm，arm 后 recheck 收孤立字节窗口），NAPI 退出语义等价（空批即退出版+park）；drop 分类 `telemetry.rx_dropped_bytes` 门控接入 push 边（两处），用例 `rx_park_poll_rechecks_data_injected_at_arm`（arm 步注入 3 字节同 poll 收口）与 `rx_copier_classifies_ring_full_drops`（300B/256 环 → 恰 44 drop，接纳前缀完整有序，无界重试不存在）；③策略 seam 以 `UartPort` 关联常量 `SLOW_POLL_LIMIT`/`TEMT_POLL_LIMIT`（默认=原常量）+ `tx_stall_wait()`（默认立即就绪）实现，K3 置 0 后 `TxPollOutcome::StallWait` 在预算耗尽与 TEMT 角两处短路 spin/yield 阶梯，外层 `await tx_stall_wait()`；D1/QEMU 默认路径控制流等价（spin 常量原值）。用例 `tx_k3_policy_stall_parks_on_timer_hook_without_slow_poll`（`slow_poll_exhausted==0`、`yield_retries_exhausted==0`、THRE 武装、payload 保持 staged、恢复后同一循环排空）与 `tx_k3_policy_temt_corner_uses_timer_hook_and_completes_drain`（TEMT 角 timer hook、DRAIN_WAKER 在 TEMT=true 后唤醒、四条件完成）。
   - T4：`copier_start_guard.rs` claim/rollback/claimed 与原 swap/store 语义等价；`map_copier_ier`（RDI⇔RX_GROUP 含 RTOIE）论证与 `K3UartCore::isr` 掩组行为吻合（ISR 每次 wake 掩整组，只回武装 RDI 会使 RTOIE 滞留掩码、短行失唤醒——接缝必要，host 用例覆盖 THRI 1:1 与清位映射）；`uart_init` K3 臂（activate 显式阶段 panic、QEMU 路径 `not(k3-uart)` 收窄、driver over `k3_uart::port()`）、`ArceOsWriter::push` 单生产者路径、`mod.rs` 数据面树门控 `not(all(k3-async-uart, not(k3-uart)))`、entry 分支与 raw 分支收窄、`k3-uart` feature 链（kernel 含 `uart_16550/telemetry`，与载荷读 `rx_dropped_bytes` 一致）、`make k3-uart`（SMP 守卫+同裁决窗+.PHONY）全部落码。
   - 载荷 `run_uart_data_plane`：唯一 reader（`Box::leak`）/writer、重复 activate 拒绝、ready 横幅、2× 行回显、≥256B 长行（line-too-short 提示）、72 KiB NUL 突发（requested/accepted/short_writes 账本 + 20ms 退避）、`wait_tx_drain` 四条件 register→recheck、quiet 10s+post-quiet、errors 全字段行、完成前 drain。`read_line` 单 `poll_fn`（板测修复后形态）。
2. **（首轮阻塞，复审已闭合）Act 验证声明与最终代码树矛盾**：`git diff --check` 首轮复跑 **exit 2**——`kernel/src/platform/k3.rs` 586/632/717/732 行共 4 处尾随空白（`emit(&writer, ` 后）。Act 原声称"git diff --check 干净"是 16:48 板测驱动修复之前的过期结论。Act 按 Follow-up 完成有限修复并覆盖 Act Response（含成因归因与最终树复验）；复审独立证实：`git diff --check` exit 0、4 行尾随空白消除且全文件无同型残留（`emit(&writer, $` 计 0）、修复后唯一变动文件为 k3.rs（其余表面 mtime 均在原实施窗口）、FIT 重建（17:04:50）晚于修复编辑（17:04:46）。纯格式修复无可观察行为变化，板测证据继续有效。
3. 偏差分类（Act 已记录，本 Review 裁定）：deviation 1（TtyWrite/TtyWriteReady 在 k3-uart 门出 + `ArceOsWriter::push` 固有方法）成立——k3 形态无 pseudofs，单生产者契约不变；deviation 2（`map_copier_ier`）成立且必要（见 F1）；deviation 3（`CopierStartGuard` 提取）成立——契约要求守卫 native 测试，提取为可测模块是最小路径，ms04 拼写见证随实现必要更新；deviation 4（NUL 突发形态）按用户原话记录；deviation 5（ms04 两处见证修复）按用户指示记录——(a) 随 T4 守卫改动的必要跟随，(b) 括号深度检查替代过强断言，Blocked 门控在前、wake 在分支内、终止标记等生产语义仍逐项见证；该修复同时关闭 SNAPSHOT 登记的 pre-existing 失败（全局文档同步属 docs-maintainer 收尾职责，本轮不动）；deviation 6（driver 侧 `wait_tx_drain`）成立——`SpinNoPreempt` 锁不可跨 await，四条件与顺序与 `flush` 相同。
4. 采信（覆盖未变，本 Review 核对代码一致）：crate native 74/74（计数独立复核：3+42+11+18=74，较基线 +5 即新用例数）、plane harness 3、uart harness 25（24+1 map_copier_ier）、ms04 105、`make k3-uart`/`k3-uart-port`/`build`/`lichee` exit0、V3 板测转录（echo2 len=192、long=356≥256、txfull requested=accepted=73728 且 short_writes=83 即 Full 见证、drain 完成、quiet 后回显 len=18 含窗内排队字节为旧数据先读契约行为、errors 全 0 错误计数、rx_wakes/irq 比值与已知 IIR 口径失真一致、B4 复位回原系统）。FIT（16:48:16）晚于最后源码编辑（16:48:13），板测证据附着于当前形态镜像。
5. 非阻塞 Minor（记录）：`tx_no_progress=74562` 量级反映 K3 策略下无 spin 的预算耗尽路径高频进入——1ms timer hook 语义使然，有界且被 short_writes/账本覆盖；非缺陷。
6. Experience 候选两项（ms04 并行串扰 Issue 候选、k3 FIT 同名覆盖 Runbook 候选）按规则只报告不落账，Recorder 未授权。

**Acceptance Gaps**

无。A1（crate 74/74 含 5 新用例）、A2（策略用例见证 K3 不进 spin 路径，D1 默认不回归）、A3（`make k3-uart`/`k3-uart-port`/`build`/`lichee` exit0、host-test 无新失败且 pre-existing 项关闭、最终树 `git diff --check` exit 0）、A4（板测 B5 全阶段 + B4 复位）全部成立。Iteration 001 验收面（T3 copier 契约 + T4 K3 数据面资格）全部有直接证据。

**Evidence**

- 首轮 Review 只读检查：T3/T4 全部 diff 与新文件独立阅读（F1）、测试计数 grep 复核（3+42+11+18=74；plane 3、uart 25、ms04 105）、`git diff --check` 复跑（exit 2 定位 4 处尾随空白）、mtime 周期边界。
- 复审只读检查：`git diff --check` exit 0；4 行 `cat -A` 证实空白消除；同型残留 grep 计 0；修复面与 mtime 排序（k3.rs 17:04:46 → FIT 17:04:50）。
- 采信 Act Response（含修复记录）：全部 native/build/板测命令与输出（覆盖未变或晚于最后源码编辑；修复为纯格式）。

**Follow-up Decision**

accepted。首轮唯一 Acceptance gap（V2 的 diff-check 项）已按 Follow-up 有限修复并在最终树上独立证实；其余 finding 均非阻塞且已记录。本 Cycle 为 Iteration 001 最终 Cycle，Iteration 001 完成，T3/T4 勾选生效；按 Map 展开 Iteration002（T5/T6），新 Cycle 以 draft 交付并已并入 R74/R62 参考仓库经验核对，待用户计划审计批准后转 ready。

**Iteration Plan Update**

None。

**Next Cycle**

None。

**Next Iteration**

`iterations/002-tty-userland-stdio/000-initial.md`（draft，待用户批准后转 ready）。

