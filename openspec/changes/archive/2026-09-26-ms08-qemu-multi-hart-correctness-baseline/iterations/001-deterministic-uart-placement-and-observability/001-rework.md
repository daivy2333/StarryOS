# Iteration 001 / Cycle 001: Close UART placement and snapshot witnesses

## Plan Context

- Status: ready
- Iteration: 001-deterministic-uart-placement-and-observability
- Cycle: 001-rework
- Cycle Type: rework
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 2.1–2.6
- Depends on: Iteration 000
- Stable baseline: RX/TX copier 在 secondary-ready 后各以 singleton affinity 启动一次；无效 placement fail closed；QEMU snapshot 一致报告 placement、实际执行与 UART wake 因果；SPSC、readiness 和四阶段 drain witness 闭合。
- Verification boundary: placement/lifecycle、snapshot ABI 与一致性、真实 wake/readiness/Full 模型、QEMU/D1 build和 `SMP=16` snapshot startup smoke。
- Diagnostic boundary: placement validation、UART copier/waker/ring、snapshot publication、QEMU startup observation。
- Deferred tasks: 3.1–7.3

**Cycle Scope**

- Trigger: rework-required
- Acceptance gaps: A1 无效输入；A3 snapshot 一致性与 UART remote-wake 观测；A4 wake/readiness/Full 因果；A6 `SMP=16` runtime snapshot 核对。
- Repair items: 2.1-R1、2.3-R1、2.4-R1、2.6-R1
- Inherited scope: Tasks 2.1–2.6；已实现的显式 copier futures、入队前 singleton affinity、task handle保存、旧 TXDBG ABI、early console与D1 fallback兼容要求。
- Excluded scope: network placement/V5、UART migration、正式 guest qualification protocol、完整 UART 数据面压力、组合压力、真板资格和性能优化。

**Objective**

修正现有 placement 与 snapshot 的可观察契约，以动态测试证明 UART copier 在真实 park 后由 ISR/waker 恢复、Full 后恢复容量和 readiness，并在 `SMP=16` QEMU startup 中直接读取 snapshot 核对目标与实际 placement。所有修复只关闭 Iteration 001 的既有 A1/A3/A4/A6。

**Background**

Cycle 000 已完成显式 copier future、pinned spawn、handle保存和 snapshot ioctl 表面，但独立 Review 发现：placement 信任未验证输入；snapshot 由多个 Relaxed 原子拼装且缺少 UART-specific resume 因果；新增 copier tests 在 park 前 wake，未覆盖 Full/readiness；QEMU startup 只打印目标 hart，没有读取 snapshot。Cycle 000 的 build结论仍可复用，修改覆盖范围后必须按本 Cycle 重跑相关验证。

**Investigation Facts**

- Current Baseline:
  - `start_copiers` 从 `axtask::schedulable_cpu_mask()`生成候选列表，以 `spawn_with_name_affinity` 启动两个 singleton copier并保存 handle；Cycle 000 的普通、`SMP=16`和D1 build均报告通过。
  - `UartSmpSnapshot` ioctl command为 `0x5553_4d31`，旧 `UART_TXDBG_SNAPSHOT/RESET` 未修改。
  - 新鲜 Review 命令确认两个 `smp_witness` 测试通过，但测试结构不足以证明其名称声明的因果。
  - 默认并行 `make host-test` 本次复现 Iteration 000 已知调度敏感失败；用户已批准该 harness 使用 `--test-threads=1`，不得把默认并行偶发通过写成决定性 Gate。
- Current-State Evidence:
  - `placement::place_roles` 只拒绝空 slice；没有 `MAX_CPU_NUM` 范围或重复检查。
  - `record_copier_hart` 分别写 `*_LAST` 和 `*_HART_MASK`；`snapshot` 独立读取这些原子和 pinned affinity，因此读取可跨更新边界。
  - snapshot只包含全局 IPI send/receive count；没有 RX/TX poll/resume 或 UART event count，无法单靠一帧或前后两帧把进度归因到指定 copier。
  - RX witness先 `RX_WAKER.wake()`，再首次 poll future；FakeWakerSet和RawWaker均不记录唤醒。TX witness只写入4字节到空ring。
  - `start_copiers`的启动 marker打印 placement policy目标值；Cycle 000没有调用 `uart_smp_snapshot::snapshot()` 的 QEMU runtime consumer。
- Code and Critical Path:
  - Placement：`schedulable_cpu_mask → Vec<hart> → place_roles → singleton_mask → spawn_with_name_affinity`。
  - RX wake：QEMU UART IRQ → `uart_isr_handler → RX_WAKER.wake → parked rx_copier poll → RX ring → reader/PollSet`。
  - TX capacity：TTY writer → full TX ring → parked writer waker；TX copier pop释放容量并wake writer；drain还需 copier inactive、staged zero和TEMT。
  - Observation：IRQ/copy/event hooks与scheduler IPI telemetry →一致 snapshot组装→QEMU-only ioctl或最小 startup smoke读取。

**Implementation Guidance**

先以直接负向测试固定无效 placement 与旧 witness 的缺口，再修 placement validation。随后为 snapshot 建立单一 publication boundary或可重试一致读取，并补充足以区分 RX/TX resume 的 UART 事件观测；全局 IPI计数只有在受控 UART-only窗口中才能作为因果一环。动态 wake/readiness tests完成后，再增加最小 QEMU-only startup snapshot smoke。该 smoke可以调用同一 snapshot组装函数并输出可判定 marker，但不能演变为 Iteration 004 的guest数据面协议，也不能以目标 hart日志代替实际字段。

**Behavioral Change**

- placement对重复、越界或空输入返回失败，不生成可能无效的 singleton affinity。
- snapshot读取返回一个内部一致的 placement/progress view，并提供 RX/TX resume或等价的 UART-specific事件观测，使受控窗口可区分 remote enqueue/IPI/resume。
- host witness先让 copier/readiness waiter真实进入 Pending，再由 ISR或ring容量事件唤醒并恢复。
- `SMP=16` startup smoke直接读取 snapshot，核对 affinity属于schedulable集合且实际copier poll hart与singleton affinity一致。

**Task Contracts**

### 2.1-R1: Reject invalid placement sets and complete the topology matrix

- Requirement/Scenario: A1 / D4；1/2/3/4/8/16、非零anchor、稀疏和无效集合。
- Depends on: None
- Targets: `kernel/src/drivers/placement.rs::{place_roles,place_role}`；`tests/ms04-async-rx-host-harness.rs::placement_tests`。
- Current behavior: 空集合失败；重复和 `>= MAX_CPU_NUM` 值被接受；3/8-hart没有直接行为断言。
- Required behavior: policy只接受有序、去重、范围有效的schedulable集合；空、重复、越界输入fail closed且不panic；1/2/3/4/8/16均返回输入内singleton，至少两个候选时UART RX/TX分离。
- Required changes: 在纯policy边界验证输入，补全正负矩阵；调用者仍从published mask构造实际集合。
- Preserve: anchor不在集合时的既有确定性fallback，network角色顺序，普通scheduler readiness所有权。
- Forbidden: 以configured count替代schedulable输入；硬编码hart；引入topology/hotplug。
- Test witness: 先加入重复、`MAX_CPU_NUM`、超范围和3/8-hart测试并观察旧实现失败；修改后全部GREEN。
- GREEN condition: 每个成功结果只含输入成员，失败输入返回`None`，无panic。
- Verification: 串行host harness、ordinary与`SMP=16` build。
- Stop when: 有效性需要调用者之外的新平台事实，或现有anchor语义必须改变。

### 2.3-R1: Publish a consistent UART snapshot with attributable wake progress

- Requirement/Scenario: A3 / D6；placement tuple一致、实际hart和remote wake可直接观察、旧ABI不变。
- Depends on: 2.1-R1
- Targets: `kernel/src/drivers/uart_smp_snapshot.rs`；`kernel/src/drivers/uart_init.rs`事件hooks；必要的`crates/axtask`只读telemetry；`kernel/src/syscall/fs/ctl.rs`；host ABI/model tests。
- Current behavior: 多个Relaxed原子可组成混合tuple；全局IPI值不能区分UART RX/TX resume；测试只做源码字符串检查。
- Required behavior: 同一snapshot的affinity、last-hart与hart-mask满足内部不变量；并发更新时读取要么重试，要么由同一锁/sequence保护。snapshot或可配对的只读状态必须区分RX/TX事件与resume/poll进度；在受控UART-only前后窗口中能够观察event/remote-ready IPI/target receive/copier resume链。旧TXDBG command与wire布局字节级不变。
- Required changes: 建立一致publication协议；增加最小UART-specific单调事件/恢复字段或等价可归因机制；为新wire type写实际`size_of`/offset/prefix与并发一致性测试，而非字段名source guard。
- Preserve: telemetry不驱动同步；ISR只ack/mask/wake；普通/D1不暴露QEMU ioctl；全局scheduler语义和network V1–V4不变。
- Forbidden: 从目标affinity推断实际hart；用最终流量或boot文字推断IPI；revision/run identity；为snapshot建立第二套调度协议。
- Test witness: 并发writer/reader模型先在旧独立原子实现上观察不变量破坏；缺少resume字段的ABI/因果测试先RED；修改后GREEN。
- GREEN condition: 每帧tuple一致，RX/TX恢复可区分，受控前后delta可连接UART事件、IPI和copier resume，旧TXDBG布局不变。
- Verification: host ABI/model tests、UART crate tests、ordinary/`SMP=16`/D1 build、strict OpenSpec。
- Stop when: 只有改变scheduler决策语义或引入正式guest协议才能取得因果；返回Plan。

### 2.4-R1: Replace nominal copier tests with real park/wake/readiness witnesses

- Requirement/Scenario: A4；ISR→copier、RX ring→readiness、TX Full→恢复、register→recheck、四阶段drain。
- Depends on: 2.3-R1
- Targets: `crates/uart_16550/src/async_/{driver.rs,isr.rs,ring_buffer.rs,device_ops.rs}` tests；必要的kernel PollSet/TTY host model。
- Current behavior: RX数据在首次poll前预置且wake发生在注册前；noop waker无法观察resume。TX没有Full或readiness waiter。
- Required behavior: RX copier先在无数据状态poll到Pending并注册可计数waker，再注入MMIO/port状态、执行真实ISR handler或等价ISR入口并观察wake，最后poll恢复且字节顺序完整。TX先填满ring并让writer/readiness路径Pending，再由copier pop释放容量并观察waiter恢复；drain分别证明前三阶段完成但TEMT=false仍Pending，以及TEMT=true后完成。
- Required changes: 使用状态可变fake port/MMIO和可计数RawWaker/PollSet模型；保留bounded poll，删除不能影响结果的提前wake。
- Preserve: SPSC身份、short write、spurious wake可接受、ISR不搬字节、四阶段completion语义。
- Forbidden: sleep-poll、无界循环、只做源码字符串匹配、以单次空ring排空替代Full恢复。
- Test witness: 删除或禁用提前`RX_WAKER.wake()`后旧RX测试仍通过，作为旧witness无效的RED依据；新测试必须在去掉ISR wake或capacity wake时失败。
- GREEN condition: park→wake→resume、payload、capacity/readiness和TEMT边界均由动态断言证明。
- Verification: focused tests、完整`uart_16550 --features async`、串行kernel host harness。
- Stop when: 失败来自Iteration 000 scheduler/IPI产品缺陷而非fixture；记录并返回Plan。

### 2.6-R1: Read and validate the snapshot in bounded SMP=16 startup

- Requirement/Scenario: A6；QEMU `SMP=16` fixed-placement startup，不进入正式数据面资格。
- Depends on: 2.1-R1、2.3-R1、2.4-R1
- Targets: QEMU-only kernel diagnostic/startup seam或最小one-shot smoke consumer；现有bounded QEMU harness/Makefile入口；Cycle verification记录。
- Current behavior: startup marker只报告policy目标hart，未读取snapshot或实际copier poll状态。
- Required behavior: bounded startup直接调用或通过ioctl读取同一`UartSmpSnapshot`实现，等待条件仅限两个copier至少各poll一次；核对configured=16、affinity bit属于schedulable mask、RX/TX affinity分离、actual last/mask与singleton affinity一致、无duplicate start/invalid-affinity/panic。输出包含字段值和独立退出结果；超时或字段缺失失败。
- Required changes: 增加最小QEMU-only snapshot smoke入口和严格判定；它只验证startup/placement，不发送UART数据、不定义Iteration 004 case协议。
- Preserve: console作为恢复路径；boot到shell；network startup行为；正式UART数据面资格仍在Iterations 004–005。
- Forbidden: 仅打印目标hart；用shell出现或timeout退出码单独判PASS；sleep-poll；构建revision/run身份协议。
- Test witness: 在consumer不读取snapshot或actual-hart字段错配时validator/harness先RED；正确snapshot后GREEN。
- GREEN condition: 必需字段和实际hart断言全过，startup有界完成，0 panic/page fault/第二copier。
- Verification: `make build`、`make build SMP=16`、D1 build、bounded `make justrun SMP=16 NET=n`对应smoke入口、`git diff --check`、strict OpenSpec。
- Stop when: 最小读取必须扩大为正式guest RX/TX协议或依赖network placement；返回Plan。

**Invariants**

- 每方向只有一个copier；首次enqueue前affinity已提交，task handle保持同一实例。
- hardware/ring/IRQ和benchmark先于copier；copier先于TTY依赖路径。
- snapshot是只读诊断，不能改变调度、waker或completion决定。
- ISR只ack/mask/wake；TX drain要求ring empty、copier inactive、staged zero和TEMT。
- early console、旧TXDBG ABI、D1有界fallback和network行为保持。

**Non-goals**

- Network owner/runner placement、V5或timer-disabled witness。
- UART migration、正式serial payload协议、压力/性能结论。
- 真板SMP、IRQ routing、clock或FIFO timing声明。
- 修复Iteration 000已接受的并行host夹具调度敏感问题；本Cycle使用已批准串行模式。

**Acceptance**

- A1 / 2.1-R1：完整topology/invalid矩阵证明policy只返回有效singleton或fail closed。
- A3 / 2.3-R1：真实ABI/offset和并发模型证明snapshot tuple一致；UART event/IPI/resume可在受控窗口归因；旧TXDBG不变。
- A4 / 2.4-R1：动态fixture证明真实park→ISR wake→resume、RX readiness、TX Full→capacity恢复和TEMT最终边界。
- A5（继承）：ordinary/`SMP=16`/D1 build继续通过，early console和D1 fallback不变。
- A6 / 2.6-R1：`SMP=16` bounded startup读取snapshot并核对实际copier hart、affinity和schedulable集合，无duplicate、reject或panic。

**Verification**

- 先运行每个repair item的RED witness并在Act Response记录决定性失败，再修改至GREEN。
- `cargo test --manifest-path crates/uart_16550/Cargo.toml --features async`。
- 以显式`--test-threads=1`运行`tests/ms04-async-rx-host-harness.rs`；默认`make host-test`若仍触发已知并行fixture失败，单列事实，不写成PASS。
- ordinary、`SMP=16`和D1 builds；修改对应范围后不得采信旧build。
- bounded QEMU snapshot startup smoke必须报告字段值、判定与退出结果；QEMU结论只覆盖NS16550、16个同构hart的软件并发startup。
- `git diff --check`和`openspec validate ms08-qemu-multi-hart-correctness-baseline --strict`。

**Gate 2 Readiness**

- No Missing requirements: PASS — 四个repair item映射A1/A3/A4/A6，A2/A5继承并回归。
- Simplified requirements approved: PASS — 未裁剪既有Acceptance；正式数据面资格仍按Map延期。
- Investigation complete: PASS — Review已定位无效输入、tuple撕裂、缺少因果字段、无效fixture和runtime consumer缺口。
- Design closed: PASS — fail-closed、一致publication、可归因事件、动态witness与最小startup smoke边界明确。
- Tasks executable: PASS — 每项给出目标、RED/GREEN、保持/禁止和停止条件。
- Iteration plan ordered and balanced: PASS — 只修Iteration 001；Map不变。
- Traceability complete: PASS — A1/A3/A4/A6分别映射repair item、代码面和直接witness。
- Verification sufficient: PASS — pure/model、crate、ABI、build与QEMU runtime分层观察目标。
- No identity-style evidence engineering: PASS — 禁止revision/run/session协议。
- No material TBD for Act: PASS — 一致读取可选锁或sequence等价实现；可观察语义与通过条件已确定。
- Change/Cycle consistency: PASS — 不改变spec、design、tasks或Iteration Map。
- Persisted Evidence mode valid: PASS — 可低成本重跑，Act Response足够。
- User plan approval: PASS — 用户于 2026-09-17 以“批准”明确批准本 rework Cycle。

**Persisted Evidence**

- Mode: none
- Budget: 命令、决定性输出、退出码、改动文件和符号写入Act Response；不创建Evidence目录。

**Risks and Notes**

- 全局IPI counter只有在明确隔离的UART-only窗口中才可用于归因；否则必须补充task/UART-specific telemetry，不能靠hart不同推断。
- 一致性协议若在ISR与task共享锁，必须保持IRQ-safe且不能形成同hart重入死锁；sequence/retry也必须有有界读取策略。
- QEMU startup smoke不证明UART payload、真实FIFO timing或真板行为。

## Act Response

- Status: reported

**Implemented**

Repair items 2.1-R1 / 2.3-R1 / 2.4-R1 / 2.6-R1 are implemented and verified, closing Iteration 001 acceptance gaps A1 / A3 / A4 / A6.

- **2.1-R1 (A1) invalid placement fails closed**: `placement::place_roles` now validates its schedulable input through a new pure `valid_schedulable` predicate — it rejects empty, unordered, duplicate and `>= MAX_CPU_NUM` hart sets (fail closed, `None`, no panic) while accepting ordered/deduped/in-range sets. Added `placement::MAX_CPU_NUM = 64` (matches run-queue readiness bitset/critical-section capacity). `place_role` inherits the validation via `place_roles`. Added 4 hosts witnesses covering duplicate, out-of-range (`==`/`> MAX_CPU_NUM`), unordered, and 3- and 8-hart dense sets (all still return in-set singletons; 4+ harts keep the four background roles distinct).
- **2.3-R1 (A3) consistent, attributable snapshot**: introduced pure host-compilable `kernel/src/drivers/hart_counter.rs` — `HartCounter` stores `(events, last, mask)` and `record()` folds the mask bit first then publishes last, so `read()` returns a self-consistent `(last, mask, events)` tuple (invariant `last ∈ mask`, bounded-retry 1024, self-consistent fallback on exhaustion). Replaced the six standalone Relaxed atomics in `uart_smp_snapshot.rs` with three `HartCounter`s (UART_IRQ / RX / TX) and added monotonic attributable `irq_events` / `rx_polls` / `tx_polls` fields plus a leading `magic` prefix to the wire type. Moved the wire type to pure `kernel/src/drivers/uart_snapshot_types.rs` (byte-identical single source shared by kernel and host) with `UART_SMP_SNAPSHOT_MAGIC`/`UART_SMP_SNAPSHOT_PREFIX`. Reordered `record_pinned` to *before* each copier's first enqueue in `start_copiers` so a reader can never observe a copier that already polled without a known affinity. Old `UART_TXDBG_*` command values and 16-u64 struct are byte-identical. Replaced the field-name source guards with structural `offset_of!`/`size_of` layout tests (every field offset and total size pinned) plus a torn-isolated-atomics RED fixture and a multi-writer+reader concurrency test proving `read()` never yields `last ∉ mask`.
- **2.4-R1 (A4) real park/wake/readiness witnesses**: rewrote `smp_witness_tests` in `crates/uart_16550/src/async_/driver.rs`. A `CountingWakerSet` now counts and forwards wakes; `counting_waker` observes AtomicWaker resumes; a fake 8-byte MMIO region drives the real `uart_isr_handler` entry. New tests: `prepoll_wake_without_registered_waker_is_noop` (RED witness: the Cycle 000 early `RX_WAKER.wake()` before registration is a dropped no-op); `isr_park_wake_resume_moves_bytes_with_order` (park with empty UART → register→recheck wake → real ISR ReceivedDataReady entry → resume → bytes into ring in order + RX consumer readiness wake); `tx_full_capacity_recovery_and_temt_drain_boundary` (Fill → Full, producer parks on capacity, copier pops 256, frees capacity, wakes producer, and the four-stage drain stays `!is_drained` while TEMT=false, completing once TEMT=true). Serialized via a `WITNESS_LOCK` because the destination wakers are process-global.
- **2.6-R1 (A6) bounded SMP=16 snapshot smoke**: added QEMU-only `uart_smp_snapshot::snapshot_boot_smoke` (called in `entry.rs` right after `start_copiers`). It bounded-waits (with `yield_now`) until both copiers polled, reads the *same* `snapshot()` the ioctl exposes, skips non-SMP boots, and prints per-check `ok/BAD` lines plus a single greppable `[UART-SMP-SMOKE] PASS/FAIL` marker and all field values (configured, schedulable_mask, rx/tx affinity+last+mask, irq_events, rx/tx_polls, ipi counts). Validates: magic frame, `configured==16`, each affinity in schedulable mask, RX/TX distinct, actual copier hart equal to pinned affinity with cumulative mask exactly the singleton (no second instance), both polled, no affinity rejects. On failure it prints FAIL without panicking (preserves the "0 panic" GREEN condition).

**Changed Files and Symbols**

- `kernel/src/drivers/placement.rs` (reworked): `MAX_CPU_NUM`, `valid_schedulable`; `place_roles` uses it.
- `kernel/src/drivers/hart_counter.rs` (new): `UNKNOWN_HART`, `HartCounter::{new,record,read}`.
- `kernel/src/drivers/uart_snapshot_types.rs` (new): `UART_SMP_SNAPSHOT_MAGIC`, `UART_SMP_SNAPSHOT_PREFIX`, `UartSmpSnapshot` (+`magic`, +`irq_events`, +`rx_polls`, +`tx_polls`), `is_valid_frame`.
- `kernel/src/drivers/uart_smp_snapshot.rs` (reworked): statics → `UART_IRQ`/`RX_COUNTER`/`TX_COUNTER` `HartCounter`s; `record_irq_hart`/`record_copier_hart` via counters; `pub use UartSmpSnapshot`; qemu-gated `snapshot()` + new qemu-gated `snapshot_boot_smoke`.
- `kernel/src/drivers/uart_init.rs`: `record_pinned` moved before each copier's `spawn_with_name_affinity`.
- `kernel/src/drivers/mod.rs`: `mod hart_counter; mod uart_snapshot_types;`.
- `kernel/src/entry.rs`: `#[cfg(feature="qemu")] snapshot_boot_smoke()` after `start_copiers()`.
- `crates/uart_16550/src/async_/driver.rs` (test-only): `smp_witness_tests` rewritten (3 witnesses + RED).
- `tests/ms04-async-rx-host-harness.rs`: adds `#[path]` includes for both new pure modules; placement invalid/3/8-hart tests; structural layout/offset + concurrency-consistency + torn RED tests; `AtomicU64` import.

**Deviations from Plan**

- Cargo-feature gating discovery: the product build that runs under `make justrun SMP=16 NET=n` has kernel `feature="qemu"` ON but kernel `feature="smp"` OFF (the Makefile passes root `qemu` and `axfeat/smp`, not root `smp`, so `starry-kernel/smp`/`axtask/ipi` are disabled). The Task 2.6 smoke therefore gates on `feature="qemu"` only (not `all(qemu,smp)`) and uses `axhal::cpu_num() < 2` to skip non-SMP boots at runtime. `ipi_sent/ipi_received` report 0 without the `axtask/ipi` feature; the placement/progress validation does not depend on IPI counters.
- `snapshot_boot_smoke` skips (SKIP marker) single-hart QEMU boots to avoid a misleading FAIL in ordinary `make build` runtime; it runs fully only for SMP boot.
- The consistent read uses bounded retry with a self-consistent fallback (the Plan allowed "lock OR sequence/retry"); the fallback picks the lowest set bit so the tuple always satisfies `last ∈ mask`.

**Blocker Handoff**

None

**Blocker Resolution**

None

**Self-Review**

- Plan compliance: PASS (2.1-R1, 2.3-R1, 2.4-R1, 2.6-R1 all executed; persistence mode `none`)
- Full diff reviewed: PASS (all product/test/OpenSpec files reviewed end-to-end; no identity-type evidence engineering; no second scheduling protocol; no out-of-scope edits)
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 1 — the concurrency witness (`hart_snapshot_consistent_read_never_tears_under_concurrency`) is written to never fail by construction; the torn case is proven deterministically by the sibling `hart_snapshot_isolated_atomics_are_torn` fixture, so the two tests together are not redundant.

**Verification Evidence**

| Verification item | Command / operation | Decisive output (excerpt) | Coverage | Conclusion |
|---|---|---|---|---|
| 2.1 placement RED→GREEN | `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs … && /tmp/ms04-test --test-threads=1 placement` | RED first: `duplicate_hart_fails_closed`/`out_of_range_hart_fails_closed`/`unordered_set_fails_closed` FAILED (old `place_roles` accepted them); then `13 passed; 0 failed` | duplicate/out-of-range/unordered/3/8-hart topology | PASS |
| 2.3 layout/offset ABI + concurrency + torn RED | `… rustc --test … && /tmp/ms04-test --test-threads=1` | `uart_smp_snapshot_layout_is_explicit_and_prefixed ok`, `…real_offsets_match_documented_abi ok`, `hart_snapshot_isolated_atomics_are_torn ok`, `hart_snapshot_consistent_read_never_tears_under_concurrency ok` | real repr(C) offsets/size 152, magic prefix w1, concurrent record/read never tears | PASS |
| host harness full | `/tmp/ms04-test --test-threads=1` (and `make host-test` this run) | `test result: ok. 47 passed; 0 failed` (host-test suites also green this run; serial mode used for ms04) | placement/lifecycle/ABI/snapshot/consistency/D1 guards/tcdrain | PASS |
| UART causal witnesses | `cargo test --manifest-path crates/uart_16550/Cargo.toml --features async smp_witness` | `prepoll_wake_without_registered_waker_is_noop ok`, `isr_park_wake_resume_moves_bytes_with_order ok`, `tx_full_capacity_recovery_and_temt_drain_boundary ok` (3 passed) | park→ISR→wake→resume, register→recheck, RX readiness, TX Full→capacity recovery, four-stage TEMT boundary | PASS |
| UART crate regression | `cargo test --manifest-path crates/uart_16550/Cargo.toml --features async` | `65 passed; 0 failed` + `8` doc + `10` compile-fail | full UART suite preserved | PASS |
| ordinary build | `make build` | `Finished release … exit 0` (no starry-kernel warning) | QEMU ordinary integration | PASS |
| SMP=16 build | `make build SMP=16` | `Finished release … exit 0` (no starry-kernel warning); `nm … | rg -c snapshot_boot_smoke` → `1` | smoke compiled in product | PASS |
| D1 build | `make -f Makefile lichee` | `Finished release … exit 0` | D1 adapter preserved | PASS |
| bounded SMP=16 startup smoke | `timeout 120 make justrun SMP=16 NET=n` + `tr -d '\000'` | all 10 checks `ok`, `info … configured=16 sched=0xffff rx_aff=14 rx_last=14 rx_mask=0x4000 tx_aff=15 tx_last=15 tx_mask=0x8000 rx_polls=1 tx_polls=1`, `[UART-SMP-SMOKE] PASS`, reached `starry:~#`, 0 panic | configured=16, affinity∈schedulable, RX/TX distinct, actual==affinity, singleton mask, polled, no rejects | PASS |
| whitespace / OpenSpec | `git diff --check`; `openspec validate ms08-qemu-multi-hart-correctness-baseline --strict` | clean; `Change … is valid` | change structure self-check | PASS |

**Persisted Evidence**

None required — Mode `none`; every command is reproducible and decisive outputs are captured in this Response. No `required` items, no Evidence budget exceeded, no Evidence directory created.

**Experience Candidates**

None — the causal wiring behind UART copier resume (park→ISR→wake→resume), the torn-read fix (mask-then-last + bounded retry), and the snapshot/placement validation logic are new product/test work whose exact recipes (fixtures/waker-sets/ISR byte layout) reside in this change's code and tests rather than a cross-task persistent Runbook; QEMU runtime qualification is still ahead in Iterations 004–006. The cargo-feature finding (kernel `smp` OFF under `make justrun SMP=16`) is recorded here as an OpenSpec plan-deviation; whether it warrants a persistent Issue is left to the Recorder on user request.

**Remaining Issues**

- Kernel `feature="smp"` (`axtask/ipi`) is not enabled by the standard `make … SMP=16` invocation in this tree (only `axfeat/smp`). The 2.6 smoke reports `ipi_sent/ipi_received = 0` as a result and the placement validation does not depend on it. Task 1.5 maps IPI feature propagation; whether the intended automated `SMP=16` gates need the kernel `smp` feature explicitly enabled for IPI causality is an open traceability point for Iterations 002–006, flagged (not silently asserted).
- The two concurrency witnesses rely on the `WITNESS_LOCK` / process-global waker model; correctness is deterministic by construction and the torn RED is independently proven, so no flake is expected.

**Commit or Diff Reference**

Uncommitted working tree on `mul-hart-k3` (Cycle 000 staged + this Cycle's changes in the files listed under **Changed Files and Symbols**; `hart_counter.rs` and `uart_snapshot_types.rs` are new untracked modules). No commit made; routing left to the flow.

## Plan Review

- Review Result: rework-required

**Findings**

- **Blocking — A3 / 2.3-R1：`HartCounter` 仍可返回不完整的首次记录 tuple。**
  `record` 先更新 mask，再更新 last；`read` 在看到 `last == UNKNOWN_HART` 时立即返回，没有同时要求 `mask == 0`。首次记录的两个操作之间可返回 `(UNKNOWN_HART, nonzero mask, events)`，与模块文档、Task Contract 的空状态和 snapshot tuple 一致性要求冲突。并发测试跳过 unknown last，未覆盖这个交错。
- **Blocking — A3 / 2.3-R1：新增 ioctl wire 含未定义 padding，并由 `vm_write` 整体复制给 guest。**
  `UartSmpSnapshot` 的混合 `u32/i32/u64/u8` 布局虽然总长为152字节，但包含21字节隐式padding。`starry-vm 0.3.0::VmMutPtr::vm_write` 把整个对象转换为字节slice后写入用户内存；`snapshot()`的字段初始化不保证padding已初始化。因此wire frame不确定，并可能暴露内核栈字节。现有offset/size测试固定了空洞位置，却没有证明这些字节为零。
- **Blocking — A3 / 2.3-R1、A6 / 2.6-R1：标准 `SMP=16` 资格构建未启用 remote-ready IPI。**
  项目Make规则在`SMP>1`时只添加`axfeat/smp`；root `smp`没有启用，因而`starry-kernel/smp → axtask/ipi`链路缺失。Act Response明确记录该偏差和`ipi_sent/ipi_received = 0`，但startup smoke不把它判为失败。当前结果证明copier placement和首次poll，不证明计划要求的UART event/publication→remote-ready IPI→target resume因果。该事实也说明Iteration 000接受的feature传播只有在显式选择root `smp`时成立，标准资格命令没有消费它。
- **Non-blocking — 非16的合法SMP配置会打印qualification FAIL。**
  `snapshot_boot_smoke`只对`cpu_num < 2`跳过，随后无条件要求`configured_harts == 16`。因此2/3/4/8-hart合法placement boot会打印`[UART-SMP-SMOKE] FAIL`。正式资格仍固定为16 hart，所以这不单独阻塞本Iteration，但后继repair应避免把合法非资格配置标记为产品失败。
- **Accepted evidence — 不重跑Cycle 001测试。**
  用户于2026-09-18明确指示“那些测试结果可以信任不必重跑”。因此Act Response所列placement、UART causal witness、UART regression、ordinary/`SMP=16`/D1 build、bounded QEMU startup、diff check和strict validation结果均按其原覆盖范围采信。上述阻塞项来自这些测试未覆盖的代码路径或feature graph，不否定已覆盖行为。

**Deviation Classification**

- `ACT-DEVIATION`：快照实现没有满足A3的完整tuple和定义完整的wire frame；smoke在零IPI能力下仍可PASS。
- `PLAN-OMISSION`：Cycle 001没有检查标准Make入口是否实际选择root/kernel SMP，也没有约束guest copy不得包含隐式padding。
- `NEW-EVIDENCE`：Act Response报告的kernel `feature="smp"`关闭与零IPI计数，结合Make/Cargo源码确认标准资格命令缺少`axtask/ipi`。

**Acceptance Gaps**

- A3：snapshot首次记录可返回unknown last与非零mask；wire含未初始化padding；标准`SMP=16`没有真实IPI telemetry，也没有UART remote-ready因果。
- A6：startup smoke没有拒绝缺少`axtask/ipi`的资格构建，且没有在copier park后建立受控remote wake。

**Convergence**

reduced — 父Cycle的A1无效输入、A4动态wake/readiness/Full和A6实际placement读取均已关闭；A3仍有三个阻塞缺口，其中wire padding与标准feature graph是本次独立Review新定位的具体原因。

**Evidence**

- 独立阅读本Cycle Plan Context、Act Response和全部产品/测试diff，包括两个新纯模块和staged/unstaged改动。
- `kernel/src/drivers/hart_counter.rs::{record,read}`：mask先发布，unknown last分支未要求zero mask；并发test在unknown分支不做断言。
- `kernel/src/drivers/uart_snapshot_types.rs::UartSmpSnapshot`：offset/size显示`i32/u8`字段之间存在隐式padding。
- `starry-vm-0.3.0/src/thin.rs::VmMutPtr::vm_write`与`lib.rs::vm_write_slice`：整个Rust对象的`size_of_val`字节被复制到用户内存。
- `Makefile`、`make/features.mk`、root `Cargo.toml`和`kernel/Cargo.toml`：`SMP>1`选择`axfeat/smp`，但只有root `smp`传播`starry-kernel/smp`和`axtask/ipi`。
- `kernel/src/drivers/uart_smp_snapshot.rs::{snapshot,snapshot_boot_smoke}`：kernel SMP关闭时IPI counters固定为零；smoke不要求IPI delta，也不触发park后的UART remote wake。
- 采信Cycle 001 Act Response全部已报告测试结论；按用户指令未重跑测试。Persisted Evidence仍为`none`，缺少Evidence目录不是问题。

**Follow-up Decision**

创建同一Iteration的`002-rework.md`。A3/A6目标、requirement和Iteration Map不变，但修复需要新的自包含契约：定义完整的wire serialization、标准SMP feature传播，以及受控UART TX publication→remote-ready IPI→copier resume witness。现有Cycle的targets和验证契约不足以安全指导这些修改，不能在当前Cycle内直接恢复Act。

**Iteration Plan Update**

None

**Next Cycle**

`002-rework.md`

**Next Iteration**

None
