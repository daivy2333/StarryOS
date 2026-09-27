# Iteration 001 / Cycle 002: Close UART snapshot safety and remote-wake causality

## Plan Context

- Status: ready
- Iteration: 001-deterministic-uart-placement-and-observability
- Cycle: 002-rework
- Cycle Type: rework
- Parent cycle: `001-rework.md`

**Iteration Scope**

- Change tasks: 2.1–2.6
- Depends on: Iteration 000
- Stable baseline: RX/TX copier 在 secondary-ready 后各以 singleton affinity 启动一次；QEMU snapshot 安全且一致地报告 placement、实际执行和 UART remote-wake 因果；SPSC、readiness 与四阶段 drain witness 闭合。
- Verification boundary: snapshot tuple 与 wire 安全、标准 `SMP=16` feature graph、受控 UART remote-wake 因果、ordinary/`SMP=16`/D1 build 和 QEMU startup smoke。
- Diagnostic boundary: snapshot publication/serialization、Cargo/Make feature 传播、S_SOFT 所有权、UART TX ring→waker→remote-ready IPI→copier resume。
- Deferred tasks: 3.1–7.3

**Cycle Scope**

- Trigger: rework-required
- Acceptance gaps: A3 snapshot 初始 tuple 与 wire frame 不安全；标准 `make ... SMP=16` 未启用 `axtask/ipi`，因此没有 UART remote-wake IPI 因果；A6 smoke 未拒绝缺失 IPI 的资格构建。
- Repair items: 2.3-R2、2.3-R3、2.6-R2
- Inherited scope: Tasks 2.1–2.6；Cycle 001 已通过的 placement validation、唯一 copier、实际 park/wake/readiness witness、旧 TXDBG ABI、early console 与 D1 fallback 要求。
- Excluded scope: network placement/V5、copier migration、正式 guest qualification protocol、UART 压力或性能、真板资格，以及 Iterations 002–006 的成果。

**Objective**

使 UART SMP snapshot 在任意读取时都返回定义完整、不会泄露 padding 的 wire frame，并让标准 `SMP=16 NET=n` 资格构建实际启用 axtask-owned remote-ready IPI。通过启动阶段的受控 UART TX publication，直接观察远端 copier 的 blocked→ready、IPI send/receive 和 resume 因果，不依赖 timer 或后续 guest 协议。

**Background**

Cycle 001 修复了 placement 输入、copier 动态 witness 和 startup snapshot consumer，但独立 Review 发现三个剩余缺口。`HartCounter::record` 在首次记录时先置 mask，`read` 却允许 `last == UNKNOWN_HART` 时直接返回，因此可产生 `(UNKNOWN_HART, nonzero mask)`。`UartSmpSnapshot` 混合 `u32/i32/u64/u8` 后含隐式 padding，`starry_vm::VmMutPtr::vm_write` 会复制整个对象字节，字段赋值不能保证 padding 已初始化。最后，项目 Make 规则在 `SMP>1` 时只添加 `axfeat/smp`；根 crate 的 `smp → starry-kernel/smp → axtask/ipi` 没有进入标准资格构建，现有 smoke 的 IPI 字段固定为零。

用户于 2026-09-18 明确允许 Review 采信 Cycle 001 Act Response 中的测试结果而不重跑。该授权避免重复执行，不豁免代码审计发现的 Acceptance gap。

**Investigation Facts**

- Current Baseline:
  - Cycle 001 Act Response 的 placement、UART causal witness、UART regression、ordinary/`SMP=16`/D1 build、bounded QEMU startup、diff check 与 strict validation 结果按用户指令采信。
  - `snapshot_boot_smoke` 已证明 `SMP=16` 下 RX/TX 目标和实际 singleton hart 一致；该运行的 `ipi_sent/ipi_received` 都是 0，不能证明 remote-ready 因果。
  - A1、A2、A4、A5 已满足；本 Cycle 只修 A3，并让 A6 拒绝没有 IPI 能力的伪资格结果。
- Current-State Evidence:
  - `HartCounter::read` 在 `last == UNKNOWN_HART` 时不检查 `mask == 0`；首次 `mask.fetch_or` 与 `last.store` 之间可以返回不符合其文档的不完整 tuple。
  - `UartSmpSnapshot` 的 152 字节布局含 21 字节隐式 padding；`starry-vm 0.3.0::vm_write` 经 `slice::from_raw_parts` 把整个结构体对象复制给 guest。
  - 根 `Cargo.toml` 的 `smp` 才传播 `starry-kernel/smp`，kernel `smp` 才传播 `axtask/ipi`；`make/features.mk` 对 `SMP>1` 只生成 `axfeat/smp`，项目 `Makefile` 的 `APP_FEATURES` 仍只有 `qemu`。
  - `snapshot()` 在 kernel `smp` 关闭时显式返回 `(0, 0)` IPI counters；`snapshot_boot_smoke` 不把该状态判为失败，也没有触发 copier 已 park 后的远端 UART wake。
  - `AsyncUartDriver::bench_tx_push` 可在 TTY writer 建立前作为唯一 TX producer 向 ring 发布有界字节，`RingBufTx::push` 会唤醒已注册 copier；它可作为 startup 内部 witness，不需要 guest ABI 或正式 qualification protocol。
- Code and Critical Path:
  - Tuple：`HartCounter::record(mask → last → events) → HartCounter::read → uart_smp_snapshot::snapshot`。
  - Wire：`snapshot() → sys_ioctl(UART_SMP_SNAPSHOT) → VmMutPtr::vm_write → guest memory`。
  - Feature：`SMP=16 → make/features.mk / APP_FEATURES → root smp → starry-kernel/smp → axtask/ipi → S_SOFT handler`。
  - Causality：startup boot hart → TX ring publication → parked TX copier waker → remote run queue blocked→ready → S_SOFT IPI → target hart resume → `tx_polls`/actual hart update。

**Implementation Guidance**

先修纯 tuple 与 wire 表示，使 host witness 能直接检查无隐式 padding、reserved 字节为零以及首次记录交错。随后修项目级 SMP feature 传播，保持 `axtask` 是唯一 S_SOFT owner。最后在 `NET=n`、user/TTY producer 尚未创建的 startup 窗口发布一个有界 TX diagnostic byte；用同一 snapshot 的前后 delta 判定 IPI 与 copier resume。窗口外的全局 IPI 值不能用于归因，timer tick 或 shell 到达不能替代该因果链。

**Behavioral Change**

- Snapshot 的所有 152 个 wire 字节都有定义值；任何 reserved 字节固定为零，不复制隐式 padding。
- `HartCounter::read` 只返回空 tuple，或 `last` 已属于非空 cumulative mask 的 tuple。
- 标准 `make build/justrun SMP=16` 启用 root/kernel SMP 与 `axtask/ipi`，不再把零 IPI build 当作 MS08 资格构建。
- `SMP=16 NET=n` startup 在 copier 首次 park 后执行一次内部 TX remote-wake witness，观察 publication、IPI send/receive 和目标 copier resume；其他 CPU 数量只做合法 placement，不打印虚假 qualification FAIL。

**Task Contracts**

### 2.3-R2: Make the snapshot tuple and wire representation fully defined

- Requirement/Scenario: A3 / D6；一致 snapshot、稳定 ABI、guest copy 不泄露未初始化字节。
- Depends on: None
- Targets: `kernel/src/drivers/hart_counter.rs`；`kernel/src/drivers/uart_snapshot_types.rs`；`kernel/src/drivers/uart_smp_snapshot.rs::snapshot`；`kernel/src/syscall/fs/ctl.rs`；`tests/ms04-async-rx-host-harness.rs`。
- Current behavior: 首次 mask publication 与 last publication 之间可返回 `(UNKNOWN_HART, nonzero mask)`；wire 类型含 21 字节隐式 padding并由 `vm_write` 整体复制。
- Required behavior: `HartCounter::read` 的空状态必须同时满足 unknown last、zero mask 和 zero events；非空状态必须满足 `last < 64` 且 `last ∈ mask`。wire 的每个字节必须由字段或显式 zero reserved bytes 定义，保持 command、字段语义和既有字段 offset/总长，旧 TXDBG 不变。
- Required changes: 修正有界读取/回退协议；把所有 ABI 空洞改为显式 reserved 字段并在 `snapshot()` 中置零，或使用等价的显式零化序列化表示。host test 必须按实际 `vm_write` 字节范围证明没有隐式 padding。
- Preserve: telemetry 不参与调度；ISR 路径无阻塞；QEMU-only ioctl；已有字段含义、152 字节总长和旧 TXDBG ABI。
- Forbidden: 依赖 struct literal 自动清零 padding；对含隐式 padding 的 Rust 对象做整对象 guest copy；在 ISR 中取得可被 task 持有的锁；revision/run identity。
- Test witness: 加入首次 `mask-before-last` 交错的 RED 模型和 wire `size == explicit-field-bytes`/reserved-zero witness；旧实现先失败，修复后 GREEN。
- GREEN condition: 所有可返回 tuple 满足空或 `last ∈ mask`；wire 无隐式 padding且 reserved bytes 为零；结构 offset、size、magic 和旧 TXDBG ABI 通过。
- Verification: 串行 host harness、UART crate regression、ordinary/`SMP=16`/D1 build、strict OpenSpec。
- Stop when: 安全 wire 需要改变既有 command、字段语义或总长；返回 Plan。

### 2.3-R3: Activate and attribute remote-ready IPI in the standard SMP build

- Requirement/Scenario: A3 / remote UART wake；标准资格命令必须包含实际 S_SOFT owner。
- Depends on: 2.3-R2
- Targets: project `Makefile` / `make/features.mk` 中项目级 feature 传播；root/kernel `Cargo.toml` feature graph；`crates/axtask/src/ipi.rs` 只读 telemetry；`kernel/src/drivers/uart_smp_snapshot.rs`；必要的 source/feature guards。
- Current behavior: `SMP=16` 只向 cargo 传递 `axfeat/smp`，kernel `feature="smp"` 关闭，`axtask/ipi` 未编译，snapshot IPI counters 固定为零。
- Required behavior: 标准项目命令在 `SMP>1` 时启用 root `smp`，从而唯一传播 `starry-kernel/smp → axtask/ipi`；单 hart不启用 IPI；`axruntime/ipi`/`axipi` 仍关闭；缺少该链时资格 smoke 必须 fail closed。
- Required changes: 在项目层补齐 root SMP feature 选择并增加解析后的 feature-graph witness；保留通用 make 层对非 StarryOS app 的兼容性。snapshot 在正式 `SMP=16` build 中必须读取真实 axtask counters，不得走零值 fallback。
- Preserve: axtask 唯一 S_SOFT handler、axfeat SMP、单 hart行为、D1 build和非 QEMU feature集合。
- Forbidden: 同时启用 `axruntime/ipi`；注册第二个 S_SOFT handler；用源码字符串 alone 代替解析后的 feature graph；把 IPI counter作为调度条件。
- Test witness: 标准 `make ... SMP=16` 的解析 feature graph在旧配置下先显示 kernel SMP/axtask IPI缺失，修复后显示 root/kernel SMP与唯一axtask IPI；单 hart图保持无 IPI。
- GREEN condition: 标准资格 build包含且仅包含axtask-owned IPI，snapshot不使用zero fallback，S_SOFT注册无冲突。
- Verification: feature graph、ordinary与`SMP=16` build、bounded QEMU startup；D1 compile regression。
- Stop when: 根 `smp` 会引入第二IPI owner或改变既有platform selection；返回Plan。

### 2.6-R2: Prove a bounded UART remote-wake chain in SMP=16 startup

- Requirement/Scenario: A3、A6；UART publication→remote-ready IPI→target copier resume。
- Depends on: 2.3-R3
- Targets: `kernel/src/drivers/uart_smp_snapshot.rs::snapshot_boot_smoke`；`kernel/src/drivers/uart_init.rs` 的唯一-producer startup seam；host/source guard和 bounded QEMU判定。
- Current behavior: smoke只等待首次poll和核对placement；没有在copier park后触发UART wake，IPI值为零也可PASS；2/4/8-hart合法boot会因`configured==16`打印qualification FAIL。
- Required behavior: 在`SMP=16 NET=n`、TTY writer尚未创建且TX copier已park的隔离窗口，以唯一startup producer发布一个有界diagnostic byte；前后snapshot必须显示TX poll/resume增加、IPI sent与received增加、actual hart仍等于TX singleton affinity、ring/staged最终收敛。超时、零IPI、timer-only进度或第二copier失败。非16的合法SMP配置只标记SKIP，不声称资格失败。
- Required changes: 增加有界park确认、单次TX publication和前后delta判定；输出决定性字段与PASS/FAIL/SKIP。若现有global IPI counter无法在该窗口唯一归因，改用task/UART-specific只读telemetry，不得靠hart不同推断。
- Preserve: early console恢复路径、SPSC唯一producer、正式guest RX/TX协议仍留给Iterations 004–005、无sleep-poll、无panic作为成功判据。
- Forbidden: 启动第二copier/producer；依赖timer tick；发送无界payload；把shell到达或最终counter非零当作因果证明；建立run identity协议。
- Test witness: validator/model对零IPI、无resume、错误hart、未收敛ring和非16合法配置先RED；正确前后delta后GREEN。
- GREEN condition: `SMP=16 NET=n`在deadline内输出完整因果delta并PASS，0 panic/page fault/duplicate；1/2/3/4/8配置不产生虚假qualification FAIL。
- Verification: focused host/model、ordinary/`SMP=16`/D1 build、bounded `make justrun SMP=16 NET=n`、diff check、strict OpenSpec。
- Stop when: witness必须引入正式guest协议、破坏console输出或无法隔离全局IPI来源；返回Plan。

**Invariants**

- 每方向只有一个copier；startup diagnostic publication发生在TTY writer建立前并保持TX SPSC。
- snapshot与counter只读观测，不驱动调度或completion。
- axtask是唯一S_SOFT owner；remote-ready仍只对真实`Blocked → Ready`发送一次IPI。
- old TXDBG、early console、D1 bounded fallback、network行为和Iteration Map保持不变。

**Non-goals**

- 正式UART payload/readiness/quiet资格协议、压力或性能结论。
- Network placement/V5、timer-disabled network witness、migration或recovery。
- 真板IRQ、FIFO timing、clock/reset或SMP资格。

**Acceptance**

- A3 / 2.3-R2：任意snapshot tuple满足空或`last ∈ mask`，152字节wire无隐式padding且reserved bytes为零，旧TXDBG不变。
- A3 / 2.3-R3：标准`SMP=16`命令解析到唯一axtask IPI owner；snapshot读取真实IPI counters。
- A3+A6 / 2.6-R2：`SMP=16 NET=n`受控窗口直接证明TX publication、remote-ready IPI send/receive和目标copier resume/收敛；合法非16配置不产生虚假FAIL。
- 继承A1/A2/A4/A5：Cycle 001已通过结论在覆盖范围未变时继续采信；修改相应范围后只重跑受影响Gate。

**Verification**

- 先建立tuple初始交错、wire padding、feature graph缺失和zero-IPI smoke的RED witness，再修改至GREEN。
- 直接目标：serial host harness、相关axtask feature/telemetry tests和UART crate tests。
- 边界：ordinary、标准`SMP=16`和D1 build；确认唯一S_SOFT owner。
- Runtime：bounded `make justrun SMP=16 NET=n`输出before/after字段、IPI delta、resume delta、目标hart和最终completion；非16配置只验证SKIP判定模型。
- `git diff --check`与`openspec validate ms08-qemu-multi-hart-correctness-baseline --strict`。

**Gate 2 Readiness**

- No Missing requirements: PASS — 三个repair item覆盖A3/A6剩余缺口。
- Simplified requirements approved: PASS — 未裁剪remote-wake、wire安全或placement要求。
- Investigation complete: PASS — 已定位tuple交错、guest copy路径、feature传播和可用startup TX publication seam。
- Design closed: PASS — 显式wire、唯一IPI owner和隔离startup因果的行为边界明确。
- Tasks executable: PASS — 每项含targets、RED/GREEN、保持/禁止和停止条件。
- Iteration plan ordered and balanced: PASS — 只关闭Iteration 001既有Acceptance；Map不变。
- Traceability complete: PASS — A3/A6映射到snapshot、feature graph、startup seam与直接witness。
- Verification sufficient: PASS — pure/ABI、feature graph、build和QEMU runtime分层观察目标行为。
- No identity-style evidence engineering: PASS — 禁止revision/run/session协议。
- No material TBD for Act: PASS — 等价局部实现可选，但wire字节、feature结果和runtime因果已确定。
- Change/Cycle consistency: PASS — 不改变spec、design、tasks或Iteration Map。
- Persisted Evidence mode valid: PASS — 所有结果可低成本重跑，由Act Response承载。
- User plan approval: PASS — 用户于2026-09-18以“批准”明确批准本rework Cycle。

**Persisted Evidence**

- Mode: none
- Budget: 命令、决定性输出、退出码、改动文件和符号写入Act Response；不创建Evidence目录。

**Risks and Notes**

- 当前Cycle的测试采信授权只适用于审计，不自动豁免Act修改覆盖范围后的重跑要求。
- QEMU witness只证明16个同构虚拟hart、NS16550和S_SOFT模型，不外推到真板memory ordering、IRQ routing或UART timing。
- 若startup窗口存在其他remote-ready来源，global IPI delta不足以归因，必须使用task/UART-specific只读telemetry或返回Plan。

## Act Response

- Status: blocked

**Implemented**

- **2.3-R2 (wire/tuple safety): COMPLETE.** `HartCounter::read` now returns only a strictly-empty `(UNKNOWN_HART,0,0)` tuple or `last ∈ mask`; the mask-before-last first-record interleave is retried (bounded) and never exposes a torn `(UNKNOWN, mask≠0)`; a fallback forces a self-consistent tuple. `UartSmpSnapshot` replaces the 21 bytes of implicit `#[repr(C)]` padding with explicit `_r0.._r4` reserved arrays (struct byte-count == field-byte sum, no implicit/trailing padding, total stays 152), adds `zeroed()`/`wire_bytes()`/`WIRE_SIZE`, and the QEMU ioctl writes the fully-defined `[u8; WIRE_SIZE]` frame via `wire_bytes()` instead of a whole-struct `vm_write` (no uninitialized bytes leak to guest). Old `UART_TXDBG_*` ABI unchanged.
- **2.3-R3 (standard SMP feature graph): COMPLETE.** Project `Makefile` now appends the root crate `smp` feature under an `SMP>1` gate, which propagates `starry-kernel/smp → axtask/ipi` in the standard `make ... SMP=16` build (verified via linked `axtask::ipi::{IPI_SENT,IPI_RECEIVED,ipi_sent_count,ipi_received_count}` + `__IrqIf_send_ipi` symbols, `snapshot_boot_smoke` present). Single-hart does not enable it; `axruntime/ipi`/`axipi` stay off (single S_SOFT owner preserved).
- **2.6-R2 (startup remote-wake witness): PARTIAL / blocked.** The smoke now correctly: waits for the TX copier to be *parked* (`copier_active == false`, i.e. waker registered — the `tx_polls>=1` baseline races the copier's first poll and strands the wake byte), SKIPs rather than FAILs on non-16 SMP boots, publishes one bounded diagnostic byte as the sole pre-TTY TX producer, waits for `tx_polls` to advance, and fails closed when `ipi_sent`/`ipi_received` do not increase. With the park-wait fixed, the TX copier **does** resume after the push (`tx_resumed_by_wake: ok`, tx_polls 1→2), but `ipi_sent`/`ipi_received` stay 0 — the remote-ready IPI is never actually delivered.
- **Root cause (material, blocked):** `axtask`'s `block_on` waker (`AxWaker::wake_by_ref`) calls `unblock_task(task, false)` (`resched=false`), so the remote-ready IPI branch (`resched=true` → `send_reschedule_ipi`, Iteration 000's `axtask/ipi`) is never reached for a `block_on`-parked copier. The Iteration-000 `ipi` feature is compiled and its counters/symbols are linked, but it is **not wired into the block_on wake path**, so a cross-hart woken copier does not receive a prompt IPI (it is made ready and eventually scheduled by its own hart, with no IPI). Forcing `resched=true` globally was tried and caused an `axnet-stack-runner` full-mask load-balancing IPI/migration storm across all 16 harts — a proven-harmful naive fix.

**Changed Files and Symbols**

- `kernel/src/drivers/hart_counter.rs` (reworked): strict-empty `read()` contract; `#[cfg(test)] record_partial_mask` seam.
- `kernel/src/drivers/uart_snapshot_types.rs` (reworked): `_r0.._r4` explicit reserved arrays; `WIRE_SIZE`; `zeroed()`; `wire_bytes()`; `is_valid_frame`.
- `kernel/src/drivers/uart_smp_snapshot.rs` (reworked): `snapshot()` builds via `zeroed()`; `snapshot_boot_smoke` park-wait + remote-wake/ipi witness + non-16 SKIP; `REMOTE_WAKE_BYTE`.
- `kernel/src/syscall/fs/ctl.rs`: `UART_SMP_SNAPSHOT` ioctl writes `wire_bytes()` `[u8; WIRE_SIZE]` (not the padded struct).
- `Makefile`: `SMP>1` → `APP_FEATURES += smp` (root `smp` → `starry-kernel/smp → axtask/ipi`).
- `tests/ms04-async-rx-host-harness.rs`: new pure RED→GREEN witnesses (first-record torn tuple, concurrent unknown-last→empty, wire no-implicit-padding + reserved-zero, standard-SMP feature graph, smoke remote-wake/non16-skip source-guard); layout test updated to `s.magic=...`/`wire_bytes` forms.
- `crates/axtask/src/{future/mod.rs,run_queue.rs}`: **no net change** (experiments reverted to baseline `resched=false`).

**Deviations from Plan**

- 2.6-R2's park detection uses the driver's existing `tx_completion().copier_active` (set false only *after* `ring.register_waker`) because `tx_polls>=1` is incremented at the start of the first poll, before the waker registers; without this, publishing the byte loses the wake and strands it with the parked-on-empty copier. This closes a real race in the witness and is kept.
- The remote-ready-IPI causal chain (2.6-R2's GREEN) is **not satisfied**: no `resched=true` wake path exists for `block_on` suspensions, and enabling it globally is destructive. See Blocker Handoff.

**Blocker Handoff**

- **Gate:** Gate 6 (实质问题 — 任务契约无法安全达到既有 Acceptance；修复需要新的调度器/唤醒设计决定).
- **Task/step:** repair item 2.6-R2, Phase 3/4 of `snapshot_boot_smoke` (`ipi_sent_causal`/`ipi_received_causal`).
- **Plan expected:** after publishing one UART TX byte from the sole pre-TTY producer, the before/after snapshot shows `tx_polls` increase **and** `ipi_sent`/`ipi_received` increase (remote-ready IPI → target copier resume).
- **Actual:** `tx_polls` increases (copier resumes and drains; `tx_resumed_by_wake ok`, `tx_ring_converged` only off-by-one on vacancy timing, non-blocking) but `ipi_sent`/`ipi_received` stay 0.
- **Root cause evidence:**
  - `crates/axtask/src/future/mod.rs::AxWaker::wake_by_ref` calls `rq.unblock_task(task, false)`; `run_queue.rs::unblock_task` only sends the reschedule IPI inside `if resched`, so `resched=false` never notifies a remote hart.
  - The Iteration-000 `axtask/ipi` mechanism is compiled into the SMP=16 build (`nm` shows `axtask::ipi::{IPI_SENT,IPI_RECEIVED,...}` and `__IrqIf_send_ipi`) but is unreachable from the `block_on` wake path.
  - Bounded QEMU `SMP=16 NET=n LOG=info` with `resched` forced to `true` produced a continuous `axnet-stack-runner` full-mask migration/IPI storm across harts 0–15 (`unblock_task task=axnet-stack-runner cpu=4 this=2 … cpu=5 this=4 …`), `ipi_sent` climbing 8→17+ within the boot window — a naive global `resched=true` is unsafe.
  - With `resched=false`, `ipi_sent=0 ipi_received=0` throughout, so no remote ready-IPI is emitted for the cross-hart copier wake.
- **Impact:** 2.6-R2's remote-ready-IPI causality witness cannot pass. The axtask/ipi feature (Iteration 000's delivered remote-task wake) is not operationally wired for `block_on`-suspended tasks; making it work requires a scheduler wake-path design decision (e.g. IPI only for pinned/singleton-affinity awakenings, or a dedicated wake-with-resched path that avoids full-mask load-balancing migration storms, plus preempt semantics) — a material expansion beyond this Cycle's UART snapshot/feature/wire + startup-smoke targets.
- **Started/Task status:** 2.3-R2 complete; 2.3-R3 complete; 2.6-R2 witness written and correctly RED on the missing IPI; the IPI causal fix is NOT made.
- **Others:** ordinary/SMP=16/D1 builds PASS; UART crate and MS04 host harness GREEN (52 tests); boot reaches `starry:~#` with `[UART-SMP-SMOKE] FAIL` (non-panicking) — the failing marker is the witness, not a crash.
- **Recovery condition:** Plan decides the correct remote-ready-wake design for `block_on`-parked cross-hart tasks (and any preempt/migration guard), then returns a rework/replan Cycle with a concrete wake-path contract; Act then re-runs the smoke to flip `ipi_sent_causal`/`ipi_received_causal` to GREEN.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PARTIAL (2.3-R2 / 2.3-R3 complete; 2.6-R2 witness built and RED; causal fix returned as blocker to Plan).
- Full diff reviewed: PASS for the shipped safe changes (wire, HartCounter, Makefile, smoke park-wait/non-16-skip, harness witnesses). No identity-type evidence engineering; no second scheduling protocol left in product.
- axtask experiment reverted to baseline (`git diff` for future/mod.rs, run_queue.rs is empty).
- Critical findings unresolved: 1 — the remote-ready IPI not delivered for `block_on` wakes (material, in Handoff).
- Important findings unresolved: 0.
- Minor findings unresolved: 1 — `tx_ring_converged` off-by-one on vacancy timing in the smoke (non-blocking; the resume and IPI results are decisive).

**Verification Evidence**

| Verification item | Command / operation | Decisive output (excerpt) | Coverage | Conclusion |
|---|---|---|---|---|
| hart first-record torn + empty | `rustc --test tests/ms04-async-rx-host-harness.rs … && /tmp/… --test-threads=1` | `hart_snapshot_first_record_never_returns_torn_empty ok`, `hart_snapshot_concurrent_read_unknown_last_implies_empty_tuple ok` | A3 tuple contract | PASS |
| wire no-padding + reserved-zero | same | `uart_smp_snapshot_wire_has_no_implicit_padding_and_reserved_zero ok` (size==field-sum==152, reserved gaps zero, deterministic) | A3 wire safety | PASS |
| standard SMP feature graph | same | `standard_smp_build_enables_single_axtask_ipi_owner ok` | A3 / root smp→axtask/ipi | PASS |
| smoke causality source-guard | same | `uart_smp_smoke_enforces_remote_wake_causality_and_non16_skip ok` | A3+A6 smoke contract | PASS |
| host harness full | `make host-test` | `test result: ok. 52 passed; 0 failed` | placement/lifecycle/ABI/snapshot/consistency/smoke | PASS |
| UART crate regression | `cargo test --manifest-path crates/uart_16550/Cargo.toml --features async` | `65 passed; 0 failed` + 8 doc + 10 compile-fail | wake/readiness/drain preserved | PASS |
| ordinary / SMP=16 builds | `make build`, `make build SMP=16` | `Finished release … exit 0` both | wire/fmt/ioctl + feature graph compile | PASS |
| SMP=16 IPI wiring (resolved) | `nm StarryOS_riscv64-qemu-virt.elf` | `axtask::ipi::{IPI_SENT,IPI_RECEIVED,ipi_sent_count,ipi_received_count}`, `__IrqIf_send_ipi`, `snapshot_boot_smoke` | 2.3-R3 resolved feature graph | PASS |
| D1 build | `make -f Makefile lichee` | `Finished … exit 0` | D1 adapter/workaround preserved | PASS |
| bounded SMP=16 startup smoke | `timeout 25 make justrun SMP=16 NET=n` | `ok tx_resumed_by_wake`, `BAD ipi_sent_causal`, `BAD ipi_received_causal`, `info … ipi_sent=0 ipi_received=0` , `tx_polls=2`, reached `starry:~#`, 0 panic | 2.6-R2 remote-wake witness RED on missing IPI | RED (blocker) |
| axtask reverted clean | `git diff --stat crates/axtask/src/{future,run_queue}` | (empty) | no ship of experiment | PASS |

**Persisted Evidence**

None required — Mode `none`; all commands reproducible and decisive outputs captured in this Response. No Evidence directory created; Blocker diagnostics recorded here (a root-cause log proof is reproducible via the bounded `SMP=16 NET=n LOG=info` run).

**Experience Candidates**

- **Issue candidate (out-of-scope, evidence-backed):** `axtask::future::AxWaker::wake_by_ref` uses `unblock_task(task, false)`, so `block_on`-parked tasks cross-hart are never notified with the Iteration-000 `axtask/ipi` remote-ready IPI; forcing `resched=true` creates an `axnet-stack-runner` migration/IPI storm. This is a material scheduler/wake-path defect for MS08 multi-hart remote wake (tracked as the Blocker Handoff; whether to persist as a Runbook note/Issue is left to the Recorder on user instruction).

**Remaining Issues**

- 2.6-R2 remote-ready IPI causality is not satisfied (`ipi_sent`/`ipi_received` stay 0) — the material blocker returned to Plan.
- `tx_ring_converged` off-by-one (minor, timing) should be tightened once the IPI chain is fixed.
- Pre-confirmed: kernel `smp`/`axtask/ipi` is now active in the standard `SMP=16` build (2.3-R3 fixed); that is the correct graph, but it exposed that the IPI is not delivered for `block_on` wakes.

**Commit or Diff Reference**

Uncommitted working tree on `mul-hart-k3` (Iteration 000 committed state + this Cycle's safe changes listed under **Changed Files and Symbols**; axtask untouched). No commit made; routing left to the flow.

## Plan Review

- Review Result: rework-required

**Findings**

- **Blocking — A3/A6 / 2.6-R2：`block_on` wake path没有接入remote-ready IPI。**
  独立代码审计确认`AxWaker::wake_by_ref`调用通用`select_run_queue(task)`后执行`unblock_task(task, false)`；`unblock_task`只有在`resched=true`且`Blocked → Ready`成功时才设置本地preempt pending或向远端发送IPI。因此Cycle 002的UART publication虽然令TX copier poll从1增至2，`ipi_sent/ipi_received`仍为0，不能满足受控remote-wake因果。
- **Blocking — 全局改成`resched=true`不是可接受修复。**
  SMP通用`select_run_queue_index`对`task affinity ∩ schedulable`使用全局round-robin。Act的回退实验显示，直接让所有`AxWaker`以现有目标规则请求reschedule，会使full-mask `axnet-stack-runner`在16个hart间连续迁移并形成IPI storm。该实验没有留在产品diff；后继契约必须把blocked-task wake目标选择与普通spawn负载分配分开。
- **Non-blocking — smoke的completion取样仍有时序偏差。**
  `tx_polls`在copier poll入口增长，早于ring/staged drain完成。当前smoke只等待poll delta就读取snapshot，因此`tx_ring_converged`可能出现vacancy off-by-one。它不改变“没有IPI”这一阻塞结论，但后继Cycle必须用独立有界阶段等待四阶段completion终态。
- **Accepted — 2.3-R2 snapshot tuple与wire安全。**
  `HartCounter::read`只返回严格空tuple或`last ∈ mask`，有界回退也保持该不变量；wire类型以显式reserved填满原21字节空洞，`wire_bytes()`从零数组逐字段编码，ioctl复制`[u8; 152]`而非Rust结构对象。现有offset、总长、magic和旧TXDBG ABI保持。
- **Accepted — 2.3-R3标准SMP feature传播。**
  项目`Makefile`只在`SMP>1`时追加root `smp`，对应`starry-kernel/smp → axtask/ipi`；单hart不启用，且没有引入`axruntime/ipi`/`axipi`第二owner。Act报告的linked symbols与build结果支持该结论。
- **Accepted evidence — 不重跑未矛盾的Act测试。**
  按用户明确授权，采信Cycle 002 Act Response中的52/52 host harness、65/65 UART tests、ordinary/`SMP=16`/D1 builds、feature symbols及bounded QEMU输出。独立审计未发现与这些覆盖结论冲突的代码；失败的UART smoke本身作为阻塞证据保留。

**Deviation Classification**

- `PLAN-INVALID`：Cycle 002假定已有remote-ready IPI机制能被`block_on` copier wake消费，但实际waker固定传`resched=false`；把它改为`true`又会与通用round-robin wake目标组合成迁移/IPI storm。
- `BASELINE-CHANGED`：Iteration 000接受的remote successful unblock IPI只覆盖决策seam和已编译机制，没有覆盖真实`block_on`调用路径；Cycle 002 runtime首次证明该基线不可操作。
- `NEW-EVIDENCE`：park后单字节publication、poll 1→2但IPI 0→0，以及`resched=true`回退实验共同定位了wake target/notification耦合问题。

**Acceptance Gaps**

- A3：标准SMP已有真实IPI owner与telemetry，但pinned UART copier的成功remote ready transition仍未发送IPI。
- A6：startup smoke仍为FAIL；尚未证明publication→IPI send/receive→目标copier resume→四阶段completion的完整有界链。

**Convergence**

reduced — 父Cycle的首次tuple撕裂、wire padding、标准SMP feature缺失、park前publication竞态和非16虚假FAIL均已关闭；剩余缺口从“无IPI能力/无受控触发”缩小为一个已定位的blocked-task wake placement/notification契约及其completion取样。

**Evidence**

- 独立阅读Cycle 002 Plan Context、完整Act Response及产品/测试diff；`crates/axtask/src/{future/mod.rs,run_queue.rs}`无净diff，危险实验已回退。
- `crates/axtask/src/future/mod.rs::AxWaker::wake_by_ref`：通用queue选择后调用`unblock_task(task, false)`。
- `crates/axtask/src/run_queue.rs::{select_run_queue_index,unblock_task}`：前者对有效集合全局round-robin；后者仅在成功transition且`resched=true`时做local pending或remote IPI。
- `crates/axtask/src/task.rs::cpu_id`与run queue state表明blocked task保有上次queue信息，但后继设计无需依赖它；当前唤醒hart若在affinity与schedulable交集中可安全优先本地，否则选择合法远端。
- `kernel/src/drivers/{hart_counter.rs,uart_snapshot_types.rs,uart_smp_snapshot.rs}`与`kernel/src/syscall/fs/ctl.rs`：tuple/wire修复符合2.3-R2；smoke明确拒绝零IPI，但在poll入口后立即取completion snapshot。
- `Makefile`及root/kernel feature定义：`SMP>1`传播到唯一axtask IPI owner，符合2.3-R3。
- 采信Act Response记录的全部未失效测试结果，未重复运行产品测试；Persisted Evidence mode为`none`，无Evidence目录不构成缺口。

**Follow-up Decision**

创建同一Iteration的`003-rework.md`。Requirement、Acceptance和Iteration Map不变；后继Cycle明确区分普通spawn round-robin与blocked-task wake locality，要求当前唤醒hart合法时优先本地，否则投递合法远端，并只在成功remote transition后发送一次IPI。同时修正UART completion等待，并以`NET=y`邻接启动拒绝迁移/IPI storm。该设计问题不能在已冻结的Cycle 002内恢复Act。

**Iteration Plan Update**

None

**Next Cycle**

`003-rework.md`

**Next Iteration**

None
