# Iteration 000 / Cycle 001: Transactional DMA Recovery Commit

## Plan Context

- Status: ready
- Iteration: 000-bounded-virtio-recovery-substrate
- Cycle: 001-rework
- Cycle Type: rework
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 1.1、1.2、1.3
- Depends on: None
- Stable baseline: transport-neutral bounded recovery contract和VirtIO adapter可在fake transport中安全完成或隔离整设备reset，epoch ledger与link snapshot独立可测。
- Verification boundary: `virtio-drivers`、`axdriver_net`、`axdriver_virtio`全量host tests通过；reset未确认无Drop/reuse；任一部分重建或回填失败都不留下设备可访问的悬空DMA地址；stale completion不命中新epoch。
- Diagnostic boundary: VirtIO初始化提交顺序、raw queue backing holder、adapter queue/buffer ledger和recovery状态边界。
- Deferred tasks: 2.1–4.2（axnet owner/cancel/deadline、link/socket epoch、QEMU qualification）

**Cycle Scope**

- Trigger: rework-required
- Acceptance gaps: A1的DMA backing安全边界仍未被部分队列构造失败证明；A2的epoch exhaustion没有在adapter入口fail-before-touch；A3的成功提交顺序、失败quarantine和全部RX入口隔离仍未满足。
- Repair items: 1.3-R1、1.3-R2、1.2/1.3-R3、V-R1
- Inherited scope: 父Cycle的R1/R2/R4/R5/R6、Tasks 1.1–1.3、D1–D8、全部Invariants/Non-goals和已通过的link snapshot/error identity修复。
- Excluded scope: axnet lifecycle/ticket/flush、kernel IRQ、socket registry、QEMU runtime、PCI/DWMAC runtime、SMP、性能、通用VirtIO初始化重构。

**Objective**

把VirtIO恢复改为可验证的prepare/refill/commit事务：只有新queue及全部RX/TX backing准备完成后才发布`DRIVER_OK`和新epoch；任一部分构造或回填失败都由唯一Rust owner保留DMA backing，且设备不能访问已释放或被错误标为quarantine的内存。补齐所有数据面入口和epoch exhaustion的fail-closed证明。

**Background**

父Cycle第二次Act修复了前一轮五项表层缺口，五组crate/邻接回归也全部通过。但独立源码审计确认，`VirtIONetRaw::reinit`在adapter回填RX前调用`finish_init()`发布`DRIVER_OK`，而`recover_after_reset`随后才清理旧owner并执行可能失败的`refill_all()`。因此部分回填失败时设备已被允许使用部分新RX backing，`Faulted`和`owner_summary`不能把这些资源当作纯driver quarantine。

同一raw路径先构造send queue，再构造recv queue；第二步失败会Drop局部send queue，而transport已记录其DMA地址。现有测试只统计`NetBufPool`，没有观察queue DMA allocation/deallocation或transport queue地址，无法排除悬空引用。该问题需要新的恢复事务责任边界，已超出父Cycle内有限测试修补。

**Current Baseline**

- Revision仍为父Cycle记录的`9d58bd422577959f84fc5e5a59db5a94bd7eb7fc`；本Cycle基于当前未提交worktree和父Cycle第二次`reported` Act Response。
- 已实现bounded reset确认、checked `QueueEpoch`类型、epoch cookie、recovery trait accessor、generation-guarded link snapshot和绝大多数非active数据面门禁。
- 新鲜Review验证：`virtio-drivers` 41/41、`axdriver_net` 12/12、`axdriver_virtio` 26/26、axnet ordinary 371/371、qemu-diagnostics 393/393，均exit 0。
- `openspec validate ms07-qemu-single-hart-recovery-semantics`通过；Persisted Evidence仍为none。
- `cargo fmt --all -- --check`实际exit 1（包含既有smoltcp差异，且当前changed lines也有rustfmt差异）；`git diff --check`因父Cycle文件末尾空白行exit 2。Act Response把两项记录为PASS，不是可接受的验证证据。

**Current-State Evidence**

1. `dev_raw.rs::VirtIONetRaw::reinit`在确认reset后依次调用`begin_init`、构造send queue、构造recv queue、`finish_init`，最后才替换旧queue；`Transport::finish_init`直接设置`DRIVER_OK`。
2. recv queue构造失败时，新send queue是局部变量；其Drop会释放DMA backing，但`queue_set`已经把地址交给transport。fake的`fail_recv_reinit`只令第二次`queue_used`失败，未清除或持有第一次成功的queue地址。
3. `VirtIoNetDev::recover_after_reset`先调用上述`reinit`，再清空旧RX/TX owner，最后`refill_all`。所以partial-refill test的失败发生在`DRIVER_OK`之后。
4. 两个partial failure tests仅在Drop adapter后从`NetBufPool`取回`2 * QS` packet buffers；它们不统计queue DMA backing，也不证明transport地址是否仍指向存活allocation。
5. `NetDriverOps::recycle_rx_buffer`没有`data_plane_active`门禁，能在Resetting/Reinitializing/Faulted调用`inner.receive_begin`并重新把buffer交给queue。
6. `owner_summary`把所有非active状态统一报告为`device_owned=0`。Resetting且status尚未读回0时，旧queue/backing仍可能被设备访问，不能声明为纯driver quarantine。
7. `progress`在Resetting/Reinitializing用`advance().unwrap_or(current)`；`begin_recovery`没有在`QueueEpoch::MAX`时拒绝，因而能先写reset、重建和发布`DRIVER_OK`，之后才因advance失败Faulted。现有overflow测试只验证纯类型`MAX.advance()==None`。

**Relevant Code**

- `crates/virtio-drivers/src/device/net/dev_raw.rs::{VirtIONetRaw,reinit}`：raw transport和queue DMA backing owner、初始化提交点。
- `crates/virtio-drivers/src/transport/mod.rs::Transport::{begin_init,finish_init}`：VirtIO status和`DRIVER_OK`发布。
- `crates/virtio-drivers/src/queue.rs::VirtQueue`：queue DMA allocation、`queue_set`和Drop owner。
- `crates/axdriver_virtio/src/net.rs::{recover_after_reset,RecoveryState,owner_summary,recycle_rx_buffer}`：adapter恢复事务、packet buffer ledger和数据面入口。
- `crates/axdriver_net/src/lib.rs::{QueueEpoch,NetRecoveryControl,NetDriverOps}`：checked epoch和公共ownership契约。

**Critical Path**

```text
begin_recovery
  -> reject exhausted epoch before any device write
  -> begin_reset -> Resetting (old backing remains device-owned/uncertain)
  -> bounded status readback == 0
  -> prepare replacement queues while retaining every partial DMA allocation
  -> install all RX/TX packet backing while DRIVER_OK is absent
  -> commit DRIVER_OK
  -> atomically publish new QueueEpoch and Recovered

any prepare/refill failure
  -> never expose freed DMA through transport
  -> retain partial/new queue and packet backing under one fault owner
  -> keep DRIVER_OK absent (or reconfirm reset before any release)
  -> Faulted; all data-plane calls fail without queue mutation
```

**Implementation Guidance**

1. 把raw reinit拆成显式prepare和commit责任，或使用等价状态holder。prepare可协商features并构造queue，但不得设置`DRIVER_OK`；commit只在adapter完成全量RX/TX回填后设置`DRIVER_OK`。
2. queue构造任一步失败时，不得让已经`queue_set`的DMA allocation作为局部值Drop。raw恢复holder必须保留完整或部分queue，直到一次已确认reset使释放安全；不要用可能无界等待的`queue_unset`作为错误清理证明。
3. adapter在reset确认后才可关闭旧owner。新packet backing必须先进入prepared queue ledger，再commit；commit和epoch advance的顺序必须保证外部不会看到`DRIVER_OK`配旧epoch。
4. 为fake HAL增加queue DMA allocation/deallocation identity/count witness，为fake transport保留每个queue的address/ready/status写历史。失败测试需同时验证packet buffer守恒、queue backing存活和无`DRIVER_OK`。
5. `recycle_rx_buffer`的非active错误路径要明确保存唯一owner：不得调用`receive_begin`，不得重复释放或丢失传入buffer。测试以queue调用计数和pool/slot守恒共同证明。
6. 区分Resetting未确认与确认后的owner语义。若公共`OwnerSummary`不足以表达uncertain，可保守计入`device_owned`；不得在status=0前报告为driver-only quarantine。
7. `QueueEpoch::MAX`必须在`begin_recovery`入口同步失败，且fake status/queue/DMA counters证明失败前没有设备或ledger副作用。

**Behavioral Change**

- recovery从“raw reinit立即发布设备、adapter随后回填”改为“raw prepare、adapter全量回填、最后commit设备与epoch”。
- partial queue/refill failure从逻辑Faulted但可能存在设备DMA访问，改为物理上未发布且backing由稳定holder唯一保留。
- Resetting owner摘要在status=0前保守反映设备仍可能拥有旧资源；确认后才允许重新分类。
- late RX recycle和epoch exhaustion在触碰queue/status前fail closed。

**Change Surface**

| Repair | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 1.3-R1 | A1/A3；R5 reset/reinit failure | `dev_raw::{VirtIONetRaw,reinit}`、`Transport::finish_init`、adapter `recover_after_reset` | queue重建、DRIVER_OK、packet refill | prepare/refill/commit事务和partial DMA holder |
| 1.3-R2 | A2/A3；R2/R5 owner隔离 | adapter `owner_summary`、`recycle_rx_buffer`及recovery tests | 状态摘要和RX返还 | status=0前保守owner分类、全部RX入口门禁和守恒 |
| 1.2/1.3-R3 | A2/A3；R1 checked epoch | `QueueEpoch`、adapter `begin_recovery/progress` | epoch推进与reset入口 | MAX时fail-before-touch，移除silent fallback |
| V-R1 | A5 | 当前Cycle相关文件和Review evidence | 格式/diff/验证报告 | 准确记录退出码并消除本Cycle whitespace错误 |

**Task Contracts**

### 1.3-R1：Transactional queue prepare/refill/commit

- Requirement/Scenario: A1/A3；R5整设备reset成功、部分queue rebuild失败、部分RX/TX refill失败。
- Depends on: 父Cycle Tasks 1.1–1.3已实现部分。
- Targets: `crates/virtio-drivers/src/device/net/dev_raw.rs`、必要时`queue.rs`/`transport/mod.rs`、`crates/axdriver_virtio/src/net.rs`及其fake tests。
- Current behavior: raw `reinit`在packet refill前设置`DRIVER_OK`；recv queue构造失败会Drop已注册的局部send queue；现有测试只统计packet pool。
- Required behavior: reset确认后prepare replacement queues并保留任何partial backing；全量packet refill成功前status不得包含`DRIVER_OK`；失败时transport不得引用已释放DMA，holder稳定保存所有未安全释放资源；成功时只commit一次并发布完整capacity和新epoch。
- Required changes: 建立可表达empty/partial/prepared/active/fault backing的move-safe owner；分离prepare与commit；把adapter refill移到commit前；扩展DMA和transport地址/status witness。
- Preserve: reset确认前old backing不Drop/reuse；每step bounded；feature negotiation、固定QS、EVENT_IDX、normal submit/reclaim行为；不以`queue_unset` busy wait清理。
- Forbidden: `DRIVER_OK`后再执行可能失败的首次全量回填；transport指向已Drop allocation；用packet pool数量代替queue DMA安全证据；用未证明unsafe绕过holder。
- Test witness: partial send/recv construction和partial RX/TX refill分别在fake中失败；断言无`DRIVER_OK`、每个transport queue地址要么指向仍存活allocation要么处于已确认reset的不可访问状态、allocation identity/count与packet ledger守恒。成功路径断言refill完成后才出现唯一`DRIVER_OK`写入。
- GREEN condition: 所有失败注入均无UAF可能、无泄漏/重复owner、epoch不推进且Faulted稳定；成功恢复得到新epoch和完整RX/TX capacity。
- Verification: 三个focused crate全量测试exit 0；`git diff`源码审计确认`finish_init`仅位于事务commit之后。
- Stop when: 当前raw结构无法在不改变公共Transport/VirtQueue根本契约的情况下保留partial DMA owner，或安全失败需要无界queue reset，返回Plan。

### 1.3-R2：Recovery phase ownership and complete data-plane isolation

- Requirement/Scenario: A2/A3；R2 owner conservation；R5 delayed-zero、Faulted和late recycle。
- Depends on: 1.3-R1的prepared/fault holder状态。
- Targets: `crates/axdriver_virtio/src/net.rs::{owner_summary,recycle_rx_buffer,data_plane_active}`及fake tests。
- Current behavior: Resetting未确认即把committed owners报告为quarantined；`recycle_rx_buffer`不检查recovery状态并会调用`receive_begin`。
- Required behavior: status=0前仍可能被设备访问的old owners保守报告为device-owned；确认后prepared/fault owner按真实可访问性分类；所有正常数据面入口在非active状态拒绝且不触碰queue，传入buffer保持唯一owner并可在最终Drop时守恒回收。
- Required changes: 按recovery phase计算owner摘要；在重建queue调用前门禁late RX recycle；增加每阶段summary、queue call counter和buffer conservation witness。
- Preserve: Active/Recovered正常RX recycle；现有TX fault quarantine和owner totals；trait签名与上层调用语义。
- Forbidden: status=0前声称device_owned为0；错误路径调用`receive_begin`；返回错误同时泄漏、重复释放或重复登记buffer。
- Test witness: delayed reset中summary仍显示old owners为device-owned；Resetting/Reinitializing/Faulted分别调用late recycle，queue submission count不变且最终pool/slot精确守恒；Recovered recycle仍成功。
- GREEN condition: 每阶段summary与实际DMA可访问性一致，非active入口没有queue副作用，active回归不退化。
- Verification: `axdriver_virtio --features net`全量测试exit 0并审计全部`NetDriverOps`/`NetTxQueue`入口。
- Stop when: 公共summary无法保守表达reset确认前ownership且需要改变上层需求语义，返回Plan。

### 1.2/1.3-R3：Epoch exhaustion fails before device touch

- Requirement/Scenario: A2/A3；R1 checked monotonic epoch；R5 reset入口。
- Depends on: None。
- Targets: `crates/axdriver_virtio/src/net.rs::{begin_recovery,progress}`，必要时`crates/axdriver_net/src/lib.rs::QueueEpoch`。
- Current behavior: MAX epoch仍会开始reset，`progress`用`unwrap_or(current)`隐藏exhaustion，后续可能在设备重建后才Faulted。
- Required behavior: MAX epoch的`begin_recovery`返回稳定错误并在任何status write、queue mutation、DMA allocation/deallocation或ledger变化前停止；非MAX target epoch始终明确且不silent fallback。
- Required changes: 在入口checked advance并保存本次target，或使用等价显式状态；删除exhaustion fallback；增加adapter-level negative witness。
- Preserve: monotonic no-wrap；普通epoch begin/poll阶段；公共transport-neutral类型。
- Forbidden: 写reset后才发现exhaustion；把MAX当作当前target继续；仅用纯类型unit test代替adapter witness。
- Test witness: 构造MAX epoch adapter，记录status/queue/DMA/owner counters，调用begin后断言错误且所有counter/状态逐项相同。
- GREEN condition: exhaustion fail-before-touch且普通恢复仍推进恰好一次epoch。
- Verification: `axdriver_net`和`axdriver_virtio`全量tests exit 0。
- Stop when: test fixture不能安全构造MAX epoch且需要扩大公共测试接口，返回Plan说明最小替代证据。

### V-R1：Accurate local verification evidence

- Requirement/Scenario: A5；Gate 5/Review evidence准确性。
- Depends on: 1.3-R1、1.3-R2、1.2/1.3-R3。
- Targets: 本Cycle实际修改文件、Act Response verification表。
- Current behavior: 父Act把exit 1的full fmt和exit 2的diff check写成PASS；当前Cycle相关Rust lines也存在rustfmt差异。
- Required behavior: 本Cycle修改的Rust文件通过可复现的focused formatting check；`git diff --check` exit 0；full-repo fmt若仍受既有差异影响，必须记录真实nonzero及分层原因，不能写PASS。
- Required changes: 格式化仅限本Cycle产品/测试文件；清除OpenSpec文件尾部空白；verification记录命令、真实exit和支持的Acceptance。
- Preserve: 不批量格式化vendored smoltcp或无关用户改动。
- Forbidden: 非零命令标PASS；为取得full fmt绿色而改无关文件。
- Test witness: focused rustfmt/check与`git diff --check`。
- GREEN condition: focused changed-file formatting和diff check均exit 0，full fmt结果被准确分层记录。
- Verification: 上述checks加`openspec validate ms07-qemu-single-hart-recovery-semantics` exit 0。
- Stop when: 工具链无法对changed files做focused check，记录准确阻塞/限制，不伪造PASS。

**Invariants**

- 任一packet buffer、descriptor、cookie和queue DMA allocation始终只有一个Rust owner；transport可见地址必须指向存活allocation，除非已确认reset且设备不可访问。
- device status未读回0前，old queue/backing不Drop、不复用且owner摘要不得把它降级为driver-only quarantine。
- `DRIVER_OK`只能在replacement queues和全部RX/TX backing准备完成后发布；失败事务不得留下设备可消费的partial queue。
- epoch exhaustion和所有非active数据面拒绝都必须发生在设备/queue副作用前。
- recovery/config step保持bounded，无busy wait、sleep或guard跨Pending。
- 不修改MS05正常submit/reclaim、EVENT_IDX、queue budget或MS06 socket/runner行为。
- fake/model证据不扩大为QEMU、PCI、DWMAC、真板或SMP资格。

**Non-goals**

- 不实现axnet ticket outcome、cancel/deadline、socket epoch或link event task。
- 不新增per-queue reset、IRQ recovery、QEMU probe/ioctl、SMP或真实硬件验证。
- 不清理整个VirtIO初始化架构、vendored格式或既有warning。

**Acceptance**

- A1（R5/R6，Task 1.1）：除父Cycle已通过的bounded reset/link证据外，partial queue construction必须证明transport不引用已释放DMA，失败holder在确认边界后唯一拥有backing。
- A2（R1/R2/R4，Task 1.2）：MAX epoch在adapter入口fail-before-touch；phase-aware owner摘要与实际device可访问性一致；既有trait/link/error identity保持通过。
- A3（R2/R5，Task 1.3）：prepare/refill成功后才commit `DRIVER_OK`和新epoch；partial rebuild/refill均安全quarantine；全部数据面入口（含RX recycle）在非active状态无queue副作用；成功恢复资源完整。
- A4（R6，Task 1.3）：父Cyclegeneration-race/link snapshot tests保持通过且不改epoch/ledger。
- A5（兼容）：三个focused crate和两组axnet邻接回归通过；focused format与diff check真实绿色；OpenSpec validate通过。

**Verification**

1. `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --locked --offline --lib --features alloc`。
2. `cargo test --manifest-path crates/axdriver_net/Cargo.toml --locked --offline --lib`。
3. `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --locked --offline --lib --features net`。
4. 使用K44已验证link-type-aware wrapper运行axnet ordinary 371和qemu-diagnostics 393邻接回归；两项均须exit 0。
5. 对本Cycle实际修改Rust文件执行focused rustfmt check；`cargo fmt --all -- --check`如仍有既有差异，记录真实exit和首个无关路径，不作为伪PASS。
6. `git diff --check`、完整diff review和`openspec validate ms07-qemu-single-hart-recovery-semantics`。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 第二次Act源码、fake seams、queue构造/Drop、DRIVER_OK和adapter refill顺序已定位；五组新鲜tests可执行。 |
| Design | PASS | prepare/refill/commit、partial backing holder、phase-aware owner和fail-before-touch闭合DMA安全与epoch边界。 |
| Iteration Plan | PASS | 仅修复Tasks 1.1–1.3既有A1–A3；不改变Iteration Map、依赖、stable baseline或后续任务。 |
| Cycle Scope | PASS | 四个repair item映射本轮Acceptance gaps；link与邻接回归只作保持性验证。 |
| Task Contracts | PASS | 每项给出targets、当前/目标行为、禁止项、failure witness、GREEN与stop boundary。 |
| Traceability | PASS | 1.3-R1→A1/A3/R5，1.3-R2→A2/A3/R2/R5，R3→A2/A3/R1，V-R1→A5。 |
| Verification | PASS | DMA identity、status history、queue call counters和现有五组suite共同覆盖安全、状态与兼容性。 |
| User plan approval | PASS | 用户已批准原Iteration目标和范围；本Cycle只为独立Review确认的既有Acceptance缺口生成返工合同，不新增需求。 |

**Persisted Evidence**

- Mode: none

所有failure witnesses和回归命令均可低成本重跑，Act Response足以记录决定性输出；不得创建Evidence占位目录。

- Budget: 本Cycle最多5个文件（含README），整个change最多20个Evidence文件；Mode为none，不消费预算。

**Risks and Notes**

- 当前worktree含用户既有暂存文档和父Cycle产品改动；Act必须保留并只修改repair targets。
- queue address在status=0后可能仍留在transport寄存器，但只有在设备明确不可访问且对应allocation释放顺序被测试证明时才安全；不能仅靠逻辑状态名推断。
- 若prepare API导致初始化正常路径和恢复路径共享代码困难，可抽取helper，但不得复制出行为漂移的第二套feature/queue初始化。
- 父Cycle的full fmt/diff记录不可信；本Cycle必须报告实际exit，不要求清理无关全仓格式债务。

## Act Response

- Status: reported

**Implemented**

Repair items 1.3-R1、1.3-R2、1.2/1.3-R3 已完成，得到 transactional prepare/refill/commit、phase-aware ownership 与 fail-before-touch 的完整证明。

**Findings 1–4 Repair（Plan Review 返回后，覆盖完整当前状态）**

1. **Finding 1（Important，A2/A3/R2/R5）— late RX recycle 与 ledger/refill 重新整合。**
   - `recover_after_reset` 在 `refill_all` 前把 `rx_recycle_hold` 归还（`self.rx_recycle_hold = None`；其 `NetBuf::Drop` 归还 pool），因此合法交错「RX completion 返回调用者 → reset → 调用者在 Resetting/Reinitializing recycle」不会因凭空占住一个 buffer 而错误 `NoMemory`→`Faulted`。
   - `owner_summary` 把 `rx_recycle_hold` 计入 `driver_held`，在所有阶段都作为 driver-held/quarantined owner 而非 device-owned；这正是该 owner 的真实可访问性（它已被 `receive()` 从 queue 取出交给调用者）。
   - 新增测试替换原先只注入无关 refill 失败的弱测试：`recycle_during_resetting_converges_to_recovered_full_capacity`、`recycle_during_reinitializing_converges_to_recovered_full_capacity`（二者均先 `rx_buffers[0].take()` 模拟收到 buffer，再在对应阶段 recycle，最后断言收敛 `Recovered` 且 `free_tx_bufs==QS`、`rx_buffers` 满、drop 后 pool 精确 `2*QS`）、`recycle_during_faulted_conserves_pool_on_drop`（Faulted 下拒绝不触碰 queue、`rx_recycle_hold` 保留、owner_summary.quarantined 计入该 owner、drop 后 pool 精确 `2*QS`）。

2. **Finding 2（Important，A1/A5/V-R1）— `virtio-drivers` 默认命令确定性 + address-aware DMA witness。**
   - 把 dev_raw 测试的共享全局静态 `DMA_ALLOCS/DEALLOCS` 改为 `thread_local!` 每测试线程独立计数，并把活分配地址记录进线程本地 `LIVE_ADDRS` 集合；因此默认（并行）命令 43/43 确定性通过，无需 `--test-threads=1`。
   - `partial_reinit_retains_send_backing_and_does_not_driver_ok` 改为 **address-aware** witness：取 `transport.queue_addr[TRANSMIT]` 的 descriptor 地址，断言其仍在 `LIVE_ADDRS`（活分配），而非聚合 `alive >= baseline`；drop 后断言 `dma_alive_count()==0`。

3. **Finding 3（Important，A2/A3/R1）— MAX-epoch fail-before-touch 加 side-effect counter witness。**
   - `FakeDeviceState` 增加 `status_writes`（`set_status` 每次递增，含同值写）与 `queue_sets`（`queue_set` 每次递增）。
   - `begin_recovery_at_max_epoch_fails_before_device_touch` 在调用前后比较 `status_writes`、`queue_sets`、`epoch`、`target_epoch`、`recovery`、`free_tx_bufs.len()`，证明穷尽时未发生任何 status write、queue mutation 或 ledger change，而非只比较会被同值写掩盖的 status 值快照。

4. **Finding 4（Important，A5/V-R1）— 消除新增 warning 并准确记录 full-format 结果。**
   - `partial_reinit_retains_send_backing` 的 `let mut transport` 改为不可变 `let transport`，消除 `unused_mut`。
   - 三个 crate 全量构建现无任何本 Cycle 引入的 `unused` warning（仅上游既有：`virtio-drivers` PCI elided-lifetime、deprecated cargo config）。
   - Act evidence 记录真实 `cargo fmt --all -- --check` exit 1，其全部 339 处 diff 均在既有 `crates/smoltcp/`（vendored debt），本 Cycle 修改文件均被排除在 full-fmt diff 之外；focused rustfmt 与 `git diff --check` 均 exit 0。

**Findings 1–2 Repair（第三次 Plan Review 返回后）**

5. **Finding 1（Important，A3/R2/R5）— late-RX 测试改走真实 completion 边界，并补 Recovered 态。**
   - `FakeDevice` 新增有界 RX completion helper：`complete_rx(token, len)` 经共享 `complete_used(0, …)` 向 RX used ring 发布真实设备完成（与 `complete_tx` 同一机制，队列 0）。
   - 重写 `recycle_during_resetting_converges_to_recovered_full_capacity`、`recycle_during_reinitializing_converges_to_recovered_full_capacity`、`recycle_during_faulted_conserves_pool_on_drop`：三者都先 `device.complete_rx(...)` 再 `dev.receive()` 取得 caller-owned 指针（真实 queue → adapter → caller 转移），而非 `rx_buffers[0].take()` 伪造生产代码不可达状态。
   - 新增 `recycle_during_recovered_restores_rx_slot`：先完成一次 recovery 到 `Recovered`，再真实 receive + recycle，断言 `receive_begin` 被调用、RX slot 恢复（`queued_after == queued_before + 1`）、且不进 `rx_recycle_hold`。

6. **Finding 2（Important，A2/A3/R1）— MAX-epoch fail-before-touch 补齐 DMA 与完整 ledger counters。**
   - `TestHal` 增加 thread-local `HAL_DMA_ALLOCS/DEALLOCS` 计数与 `hal_dma_alive()` accessor（与 dev_raw 相同 per-test 隔离，保持默认并行确定性）。
   - `begin_recovery_at_max_epoch_fails_before_device_touch` 在调用前后除 `status_writes`/`queue_sets` 外，还快照并断言 `hal_dma_alive()`、`owner_summary()`、`tx_resource_ledger()`、RX slot 数、TX occupied slot 数均不变。

**Finding 1 Repair（第四次 Plan Review 返回后）**

7. **Finding 1（Important，A2/A3/R1）— MAX-epoch witness 独立比较 DMA alloc 与 dealloc 计数器。**
   - 第三次集成本亚 `hal_dma_alive()` 只比较净活值（alloc − dealloc）：一次 alloc + 一次 dealloc 会令净值不变，无法证明 Task Contract 要求的"两个事件都未发生"。
   - 改为在拒收调用前后分别快照 `HAL_DMA_ALLOCS.get()` 与 `HAL_DMA_DEALLOCS.get()` 并各自断言不变；删除 `hal_dma_alive()` 净值辅助（避免死代码且避免净活值掩盖）；保留既有 status_writes/queue_sets/owner_summary/资源 ledger/RX slot/TX slot 断言不变。无产品恢复逻辑改动。

8. **1.3-R1（Critical，A1/A3/R5）：Transactional queue prepare/refill/commit**
   - `VirtIoNetDev::recover_after_reset` 采用事务顺序：`reinit_prepare` → 释放旧 owner → `refill_all` → `commit_driver_ok` → 推进 `epoch`/`target_epoch`。部分 rebuild 或部分 refill 失败时 `DRIVER_OK` 永不发布，设备不会 DMA 进部分填充的队列。
   - 新增测试：`partial_reinit_retains_send_backing_and_does_not_driver_ok`（dev_raw，DMA 守恒 + `pending_send` 存活 + 无 DRIVER_OK）、`reinit_prepare_does_not_publish_driver_ok_until_commit`（dev_raw，prepare 前后 status 区分）。

9. **1.3-R2（Important，A2/A3/R2/R5）：Recovery phase ownership 与完整数据面隔离**
   - `owner_summary` 按真实 device-access 边界分类：仅 healthy 设备或尚未读回 status==0 的 `Resetting` 阶段（`reset_pending`）把 committed owners 保守计为 `device_owned`；reset 确认或 Faulted 后 device 路径已停，全部 committed + fault buffer 计为 quarantined。这消除了 status=0 前错误报告为 driver-only quarantine 的问题。
   - `recycle_rx_buffer` 以 `data_plane_active()` 门禁：非 active 阶段拒绝并**触碰 queue 前**把 buffer 保留为一个单 owner（新字段 `rx_recycle_hold`）；若已有 hold buffer 则把新到 buffer 归还 pool，避免丢失/重复。
   - 新增测试：`owner_summary_keeps_device_owned_during_unconfirmed_reset`、`owner_summary_quarantines_after_confirmed_reset`、`recycle_rx_buffer_rejects_without_queue_touch_during_recovery`。

10. **1.2/1.3-R3（Important，A2/A3/R1）：Epoch exhaustion fail-before-touch**
   - `begin_recovery` 在任何 status write/queue mutation/DMA/ledger 变化前先 `self.epoch.advance()`；穷尽时返回 `BadState`（fail-before-touch）。
   - 移除 `progress` 中 `unwrap_or` 静默回退：新增 `target_epoch` 字段在 `begin_recovery` 保存本次目标，`Reinitializing 期间 progress 显式报告该目标；`recover_after_reset` 成功后同步推进 `epoch` 与 `target_epoch`。
   - 新增测试：`begin_recovery_at_max_epoch_fails_before_device_touch`、`non_exhausted_recovery_advances_epoch_exactly_once`。

**Changed Files and Symbols**

- `crates/axdriver_virtio/src/net.rs`：`VirtIoNetDev::{run,struct}` 增加 `rx_recycle_hold`、`target_epoch` 字段；`recover_after_reset` 改为 prepare/refill/commit 事务；`data_plane_active` 保留；`owner_summary` phase-aware；`recycle_rx_buffer` 门禁 + 单 owner hold；`begin_recovery` fail-before-touch；`progress` 用显式 `target_epoch`。`RecoveryState` derive `Debug/Clone/Copy/PartialEq/Eq`。测试侧新增 `FakeDevice::complete_rx`/`complete_used`（bounded RX completion helper）、`TestHal` thread-local `HAL_DMA_ALLOCS`/`HAL_DMA_DEALLOCS` 计数、`status_writes`/`queue_sets` counters；MAX-epoch witness 独立快照两个 DMA 计数器并分别断言不变，删除 `hal_dma_alive()` 净值辅助（消除死代码并避免净活值掩盖单次 alloc+dealloc）。
- `crates/virtio-drivers/src/device/net/dev_raw.rs`：`reinit` → `reinit_prepare` + `commit_driver_ok`；新增 `pending_send` 字段；新增 `device_status()` 只读 accessor；`Drop` 逻辑不变（queue_unset 先于字段 drop）。`RecoveryTransport` 测试 seam 增加 `fail_recv_reinit`。
- 仅在两个产品文件及其测试添加 `#[cfg(test)]` seam 和无 tests 条件访问。

**Deviations from Plan**

- `recycle_rx_buffer` 的 buffer 保留策略在契约范围内细化：已有 hold 时空闲 buffer 归还 pool（`drop` 触发 `NetBuf::Drop` 归还 `NetBufPool`），而非无限累积 handle；单 owner 不变量保持不变。
- `begin_recovery` 的 fail-before-touch 检查在状态 match 前执行（对所有非 Idle/Recovered 状态一视同仁返回 `BadState`），与契约“穷尽时 fail-before-touch”一致。
- Plan 原 `reinit` 改名 `reinit_prepare`/`commit_driver_ok` 属于显式接口重构，act adapter 是唯一生产调用方，无外部破坏。
- 未修改 `axdriver_net`（`QueueEpoch`/`TxCookie` 已在父 Cycle 定型）；本 Cycle 只使用其既有 contract。

**Blocker Handoff**

None。

**Blocker Resolution**

None。

**Self-Review**

- Plan compliance: PASS — 三个 repair item 与全部四轮 Plan Review Findings 闭合，无计划外产品修改。
- Full diff reviewed: PASS — 逐项核对 `git diff`，仅 `axdriver_virtio/src/net.rs` 与 `virtio-drivers/.../dev_raw.rs` 及其测试变化；无 registry、无 executor、无 MMIO header 泄漏、无 axnet product 改动。
- Critical findings unresolved: 0
- Important findings unresolved: 0（含第四次 Review Finding 1：MAX-epoch witness 独立快照 `HAL_DMA_ALLOCS`/`HAL_DMA_DEALLOCS` 并各自断言不变，删除净值辅助，避免 alloc+dealloc 相互掩盖；及其前三轮：真实 RX completion/`receive()` 边界 witness、phase-aware owner、address-aware DMA、default 并行确定性、side-effect counters）
- Minor findings unresolved: 0
- Gate 3（test witness）：每个 repair item 与每一个 Finding 的 RED 目标由新增测试捕获；观察 RED→ 实现 → GREEN。
- Gate 4（spec → code）：每个修复先对照 Task Contract/Invariants/Non-goals 复核行为方略，再审查 diff（无 `unsafe` 新增、无计划外改动）。
- Gate 5（evidence-based）：全量命令新鲜输出与 exit code 见下方表；default 并行命令确定性已验证。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| virtio-drivers | `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --locked --offline --lib --features alloc`（默认并行） | `test result: ok. 43 passed; 0 failed` (exit 0)；连续 3 次均 exit 0 | PASS |
| axdriver_net | `cargo test --manifest-path crates/axdriver_net/Cargo.toml --locked --offline --lib` | `test result: ok. 12 passed; 0 failed` (exit 0) | PASS |
| axdriver_virtio | `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --locked --offline --lib --features net`（默认并行） | `test result: ok. 36 passed; 0 failed` (exit 0) | PASS |
| axnet ordinary | `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh" cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | `test result: ok. 371 passed; 0 failed` (exit 0, K44 wrapper) | PASS |
| axnet qemu-diagnostics | `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh" cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib --features qemu-diagnostics` | `test result: ok. 393 passed; 0 failed` (exit 0, K44 wrapper) | PASS |
| focus format | `rustfmt --edition 2021 --check crates/axdriver_virtio/src/net.rs crates/virtio-drivers/src/device/net/dev_raw.rs` | clean (exit 0)；两个修改文件均不在 full-fmt diff 中 | PASS |
| full format | `cargo fmt --all -- --check` | exit 1，全部 339 处 diff 在既有 `crates/smoltcp/`（vendored debt），本 Cycle 文件不参与 | 准确分层记录 |
| diff | `git diff --check` / `git diff --cached --check` | exit 0（无 trailing whitespace） | PASS |
| validate | `openspec validate ms07-qemu-single-hart-recovery-semantics` | `Change '...' is valid` (exit 0) | PASS |

Note：`virtio-drivers` 与 `axdriver_virtio` 现均以默认并行命令确定性通过（thread_local per-test DMA 身份已消除全局静态干扰），不再依赖 `--test-threads=1`。全部新增 warning 已消除；仅上游既有 warning（`virtio-drivers` PCI elided-lifetime、deprecated cargo config）保留。

**Persisted Evidence**

None required. `Persisted Evidence` Mode 为 `none`；所有 host 命令可低成本重跑，决定性输出均在 20 行以内。未创建 `evidence/` 目录。

**Experience Candidates**

None。全部四轮 Review 的 Findings（含第四次 MAX-epoch 独立 DMA 计数器 witness）修复与验证均可在契约内低成本重跑；`--test-threads=1` 的全局静态计数问题已通过 thread_local per-test 身份消除，属于既有 known（K44）；不构成新 Runbook/Incident 候选。

**Remaining Issues**

None。三个 repair item、A1–A3、全部四轮 Plan Review Findings 与该 Iteration 保持性（A4 link snapshot、A5 全量）均关闭；default 并行命令与 focused format/diff 均 GREEN。

**Commit or Diff Reference**

None（未提交；用户 worktree 既有暂存改动与父 Cycle product 改动均保留）。当前暂存内容含 Iteration 000（含本 Cycle）全部 product/测试及用户既有 SNAPSHOT/knowledge 改动。

## Plan Review

- Review Result: accepted

**Findings**

None。未发现阻塞 Acceptance 的产品、测试或证据问题。

**Deviation Classification**

None。Act 按最新 Review 仅加强 MAX-epoch 测试见证，没有改变产品恢复语义或 Iteration 范围。

**Acceptance Gaps**

None。A1–A5 全部满足：partial queue backing、prepare/refill/commit、phase-aware owner、late-RX 四状态、MAX-epoch fail-before-touch、link snapshot 和兼容性 Gate 均闭合。

**Convergence**

Reduced。上一版唯一剩余的 DMA counter 见证缺口已关闭。

**Evidence**

- `crates/axdriver_virtio/src/net.rs:2142-2232` 分别快照并比较 `HAL_DMA_ALLOCS` 与 `HAL_DMA_DEALLOCS`；status write、queue set、owner summary、TX ledger、RX slot 和 TX slot 断言保持不变。
- `begin_recovery` 的 checked epoch advance 仍位于任何 status/queue 触碰之前；本次没有产品代码变化。
- 新鲜 focused tests：`virtio-drivers` 43/43、`axdriver_net` 12/12、`axdriver_virtio` 36/36，均 exit 0。
- Fresh neighboring regressions: axnet ordinary 371/371 and
  qemu-diagnostics 393/393, both exit 0 with the K44 linker wrapper.
- Focused `rustfmt --check`, `git diff --check`,
  `git diff --cached --check` and OpenSpec validate all exit 0.
- `cargo fmt --all -- --check` exit 1；首个与抽样差异仍只位于既有 `crates/smoltcp`，Act 的分层记录准确。
- Persisted Evidence remains `none`; no Evidence directory is required.
- Blocker Handoff and Blocker Resolution remain `None`; the Act completed
  normally and reported no external or capability blocker.

**Follow-up Decision**

接受 Cycle 001，Iteration 000 完成。既有 Iteration Map 保持不变；按 Tasks 2.1–2.2 展开下一逻辑 Iteration，不创建 rework/replan Cycle。

**Iteration Plan Update**

None。

**Next Cycle**

None。

**Next Iteration**

`../001-queue-owner-recovery-and-cancellation/000-initial.md`。
