# Iteration 007 / Cycle 006: Zero Rebuilt Virtqueue DMA Before Exposure

## Plan Context

- Status: ready
- Iteration: 007-single-hart-qemu-qualification
- Cycle: 006-rework
- Cycle Type: rework
- Parent cycle: `005-rework.md`

**Iteration Scope**

- Change tasks: 4.2
- Depends on: Iteration 006 accepted；Cycle 005 的 bounded recovery progress 实现与自动 Gate 保持
- Stable baseline: reset 确认后，重建的 VirtIO queue 从全零 owned DMA ring 开始；恢复提交新 epoch、
  恢复 64/64/0 owner ledger，并完成单 hart QEMU 六 case 与兼容回归。
- Verification boundary: 脏页 test HAL 先证明当前缺口，再验证 DMA 全区清零、queue 初始无伪 completion、
  邻接 crate/host/build；最后由用户手工 QEMU、validator 与四组回归作 runtime 终态。
- Diagnostic boundary: DMA allocation postcondition、modern/legacy virtqueue layout、used-ring 初值、
  reinit queue ownership、QEMU raw serial 首个失败层。
- Deferred tasks: None

**Cycle Scope**

- Trigger: rework-required
- Acceptance gaps: A2 最终 run；A4 post-reset/post-link-up peer；A5 epoch 与 64/64/0 owner；A6
  old/new socket 和 validator；A7 MS01/MS04/MS05/MS06 回归。
- Repair items: T4.2-R3、T4.2-R4
- Inherited scope: Task 4.2；R1/R2/R4–R8；D2/D3/D5–D8；V4 ABI、六 case、absolute deadline、
  terminal-before-wake、single-hart VirtIO-MMIO 与手工 QEMU/HMP 边界。
- Excluded scope: 修改恢复状态机、deadline、epoch/socket ABI、validator grammar、allocator策略、
  receive token 容错；vendoring `axdriver`；SMP、PCI/DWMAC、真板和性能结论。

**Objective**

在本地 `virtio-drivers::Dma::new` 把新分配的 owned DMA region 全部清零后才交给 virtqueue/device，
阻止 reset/reinit 读取复用页中的陈旧 used-ring 状态；随后完成剩余单 hart QEMU 资格与兼容回归。

**Background**

Cycle 005 已使 owner 在 reset/reinitialize deadline 前按 10 ms one-shot cadence 执行 bounded driver
step。真实 QEMU 因此首次到达 Reinitializing，但新 RX queue 立刻把 token 28526 当作 completion，导致
64 项 `rx_buffers` 越界。该值不在设备合法 token 域内。

本地 `Hal::dma_alloc` 的 unsafe implementation contract 要求返回页已清零；registry
`axdriver::VirtIoHalImpl` 仅调用不清零的 `global_allocator().alloc_pages`。`Dma::new` 的公开说明同样承诺
“The pages will be zeroed”，但当前只保存地址。workspace 已通过 `[patch.crates-io]` 使用本地
`crates/virtio-drivers`，可在最小本地边界兑现该后置条件，无需复制整个 `axdriver` crate。

**Current Baseline**

- HEAD `b83e800a` 仅作现场定位；Cycle 004 staged 改动和 Cycle 005 unstaged T4.2-R1 改动必须保留，
  revision/hash 不作为验证判据。
- Cycle 005 自动 Gate 全绿；手工 QEMU 的 preflight、pre_reset_traffic、reset_request PASS，随后在
  `old_socket_terminal` 的 Reinitializing 后 panic。
- `Dma::new` 调用 `H::dma_alloc`、检查 `paddr != 0` 后直接返回；没有清零。
- modern queue 分别为 descriptor/avail 与 used ring 调用 `Dma::new`；`VirtQueue::new` 把可信
  `last_used_idx` 初始化为 0，并依赖 DMA ring 的 `used.idx` 同样为 0。
- legacy queue 也通过单个 `Dma::new` 承载全部 ring；同一修复必须覆盖两种 layout。
- FakeHal 使用 `alloc_zeroed`。新鲜 virtio-drivers alloc suite 为 43/43 PASS，但没有脏页见证。
- 全局 `git diff --check` 会被 Cycle 003/004 已 staged raw serial 的 CRLF 报告阻塞；这些历史 Evidence
  不得为本 Cycle 重写。Act 对本 Cycle 产品/测试/文档路径执行 scoped diff check，并在 full diff review
  中把既有 raw-log 例外与新 whitespace 问题分开。

**Current-State Evidence**

1. `qemu-serial.log` 中先出现健康 `avail=64 dev=64 quar=0`，再出现
   `lifecycle=7 avail=64 dev=0 quar=64`，随后 `net.rs:638` 以 token 28526 panic。
2. `VirtQueue::can_pop()` 比较 `last_used_idx` 与 DMA used-ring `idx`；新建 queue 的可信值为 0，脏
   `used.idx` 会在设备合法写入前制造伪 completion。
3. `axalloc::GlobalAllocator::alloc_pages` 的 level-1/page allocator 分支都只分配并记账，不调用
   `write_bytes`、`zeroize` 或等价清零。
4. registry `axdriver::VirtIoHalImpl::dma_alloc` 直接返回上述页；它是当前 MMIO VirtIO net 的 HAL。
5. `receive()` 直接索引的前提是 virtqueue 只返回合法 token。给此处加 bounds check 无法修复损坏的
   ring/owner ledger，且会掩盖根因。

**Relevant Code**

- `crates/virtio-drivers/src/hal.rs::{Dma::new,Dma::raw_slice,Hal::dma_alloc}`：零页契约与修复点。
- `crates/virtio-drivers/src/hal/fake.rs::FakeHal`：现有测试总返回零页，不能作 RED。
- `crates/virtio-drivers/src/queue.rs::{VirtQueue::new,VirtQueueLayout::allocate_legacy,
  VirtQueueLayout::allocate_flexible,can_pop}`：queue 对零 ring 的消费者。
- `crates/axdriver_virtio/src/net.rs::{reinit_prepare,receive}`：重建调用方与 panic 观察点，只读验证。
- registry `axdriver-0.3.0-preview.2/src/virtio.rs::VirtIoHalImpl::dma_alloc` 与
  `axalloc-0.3.0-preview.2/src/default_impl.rs::alloc_pages`：只读根因依据，不修改。

**Critical Path**

```text
reset confirmed
  -> reinit_prepare
  -> VirtQueue::new
  -> Dma::new
       Hal allocates exclusive writable pages
       driver zeroes pages * PAGE_SIZE before exposure
  -> used.idx == 0 and no completion is visible
  -> refill valid RX tokens 0..63
  -> reinit_commit / epoch publication / Active I/O
```

**Implementation Guidance**

1. 在 `hal.rs` tests 增加私有 DirtyHal：分配对齐页后填充确定的非零 pattern，并以相同 layout 释放。
   先断言当前 `Dma::new(1, direction).raw_slice()` 仍含 pattern，形成真实 RED。
2. 在 `Dma::new` 验证分配成功后、构造 `Dma` 前，用明确的 unsafe safety comment 对
   `pages * PAGE_SIZE` 全区清零。不要改变 `Hal` trait 签名、direction 或 deallocation identity。
3. GREEN 至少覆盖 DriverToDevice、DeviceToDriver、Both；再用 DirtyHal 构造 modern 与 legacy queue，
   证明刚创建时 `can_pop()==false`。若 queue test 需要最小 fake transport seam，可局部实现，不把
   DirtyHal 提升为生产或公共 API。
4. 保持 `paddr==0 -> DmaError` 的既有失败语义；不得在失败地址上写内存。若 `pages==0` 暴露既有独立
   问题，停止返回 Plan，不在本 Cycle 扩大 allocator API。
5. 完成 focused GREEN 后运行所有邻接 Gate，再由用户按既有 Runbook 重跑完整 MS07；旧 BLOCKED log
   只作根因基线，不可复用为新实现的 PASS Evidence。

**Behavioral Change**

- 每次成功 `Dma::new` 在返回前将其独占的完整 DMA region 置零，即使具体 HAL 未兑现相同契约。
- 新建或重建的 modern/legacy split virtqueue 不继承旧 `used.idx/ring[].id`，设备写入前无 completion。
- 分配失败、物理/虚拟地址、direction、drop/dealloc、queue token 与 recovery/epoch 语义不变。

**Change Surface**

| Repair | Requirement/Scenario | File/Symbol | Planned Change |
|---|---|---|---|
| T4.2-R3 | R2/R5/R8：重建 queue 不读取陈旧 completion | `virtio-drivers::hal::Dma::new`及 tests | 清零完整 DMA region；DirtyHal RED/GREEN |
| T4.2-R3 | R2/R5：modern/legacy queue 初值 | `virtio-drivers::queue` tests | 脏 allocator 下新 queue 无伪 `can_pop` |
| T4.2-R4 | R6–R8：reset/link/socket/兼容资格 | 既有 probe、peer、validator、QEMU入口 | 重跑六 case、validator 与四组回归 |

**Requirements Traceability Matrix**

| Requirement / Scenario | Design | Repair | Witness |
|---|---|---|---|
| R2 current/unknown completion；R5 reset确认后重建 | D3 | T4.2-R3 | DirtyHal Dma 全零；modern/legacy queue 初始 empty |
| R1/R4/R5 bounded owner recovery | D2/D3/D5 | T4.2-R3/R4 | Cycle 005 timer tests；QEMU reset后恢复，无 panic |
| R6/R7 link与socket epoch | D6/D7 | T4.2-R4 | MS07 old/new socket、HMP down/up、V4 ledger |
| R8 fault matrix与single-hart资格 | D8 | T4.2-R3/R4 | crate/host/build Gate；raw serial；validator；四组回归 |

**Task Contracts**

### T4.2-R3: Enforce zeroed DMA postcondition before virtqueue exposure

- Requirement/Scenario: R2 当前/未知 completion；R5 reset 确认后安全重建；R8 host/model fault matrix。
- Depends on: Cycle 005 T4.2-R1 自动 GREEN 与 runtime panic 现场。
- Targets: `crates/virtio-drivers/src/hal.rs::Dma::new`及 tests；`queue.rs`仅新增脏页 queue witness。
- Current behavior: production HAL 可返回复用脏页，`Dma::new` 不清零；新 queue 把陈旧 used-ring 当完成。
- Required behavior: 成功分配的全部 `pages * PAGE_SIZE` 在任何 queue/device 可见前为零；modern/legacy
  queue 初始 empty；所有 direction 一致。
- Required changes: DirtyHal RED；`Dma::new` 全区清零；Dma 与 queue focused GREEN。
- Preserve: `Hal` API/unsafe contract、paddr/vaddr、direction、Drop/dealloc、queue layout/size、token域、
  recovery transaction与错误映射。
- Forbidden: 修改 registry crate；vendoring `axdriver`；仅清 used.idx 或单一 queue 字段；在
  `receive/can_receive/poll_receive` 加 bounds-check 来吞掉损坏；假造 device completion；放宽 panic Gate。
- Test witness: 修复前 DirtyHal 的 region 非零或 queue `can_pop()` 为 true；修复后三方向全区为零，
  modern/legacy queue 初始 `can_pop()==false`，drop/deallocation正常，既有43项不退化。
- GREEN condition: focused RED→GREEN；virtio-drivers、axdriver_virtio、axdriver_net、axnet与host/build
  Gate全绿；diff只有计划内零页与测试改动，加上继承的Cycle 005改动。
- Verification: focused tests；virtio-drivers alloc；axdriver_virtio net；axdriver_net；两套axnet；
  `make host-test`；RISC-V build；rustfmt/diff/OpenSpec。
- Stop when: 清零要求改变DMA cache/coherency或IOMMU协议、无法在device exposure前安全写入、需要修改
  allocator/registry `axdriver`、或测试发现非零初值不是panic来源；保存证据并返回Plan。

### T4.2-R4: Re-run single-hart QEMU qualification and affected regressions

- Requirement/Scenario: R6 initial/HMP link；R7 old/new socket；R8真实reset与兼容回归。
- Depends on: T4.2-R3全部自动GREEN，kernel/probe重新构建。
- Targets: 既有手工 QEMU/HMP/probe/peer/validator 和 MS01/MS04/MS05/MS06 入口；默认不再改产品。
- Current behavior: 前三段 PASS；old_socket_terminal 在 reinit 后因脏 ring panic，其后未运行。
- Required behavior: LOG=warn 的完整新 run 通过六 case；reset后epoch与64/64/0恢复，旧socket稳定终止、
  新socket双向成功，HMP flap不推进QueueEpoch；validator和四组回归明确PASS。
- Required changes: 仅执行既有协议并采集本 Cycle 最小证据。
- Preserve: single hart VirtIO-MMIO user-net；手工guest/HMP；P8四边界；2 s absolute deadline；V4 ABI；
  terminal-before-wake；raw serial事实源。
- Forbidden: 复用Cycle 005失败log判PASS；以INFO诊断/pcap/host模型替代guest syscall；缺marker/exit
  判PASS；runtime失败后在本Task临时改产品继续跑。
- Test witness: 完整新raw serial、validator exit 0、peer三phase与四组回归终态。
- GREEN condition: A2/A4–A7全部成立，无panic/trap/fatal owner drift/permanent Pending。
- Verification: 用户按既有Runbook手工完成MS07和四组回归，Plan随后审计原始证据。
- Stop when: 任一case、validator或回归失败，artifact在自动Gate后变化，或用户尚未提供手工结果；保存
  本Cycle最小BLOCKED/PASS Evidence并停止。

**Invariants**

- DMA 清零发生在成功分配后、queue address 交给 transport/device 前，且覆盖完整 owned region。
- reset 未确认不释放或复用 backing；成功提交前不发布新 epoch 或 Active。
- queue completion token 仍必须来自合法 descriptor id；不以边界检查替代完整性。
- Cycle 005 的 one-shot cadence、absolute deadline、唯一 owner、Faulted backing retention 保持。
- QEMU 结论限于 single-hart VirtIO-MMIO，不外推 SMP、真板或非一致性 DMA。

**Non-goals**

- 不改变通用 allocator 是否默认清零，不修改或复制 registry `axdriver`。
- 不设计 DMA cache maintenance/IOMMU API，不宣称物理板卡 coherent 行为。
- 不修改 recovery、socket、poll/ppoll、probe或validator语义。
- 不更新Runbook、Incident、SNAPSHOT、tasks全局状态或提交Git。

**Acceptance**

- A2：新 LOG=warn runtime 无未解释 user fault、kernel trap 或 panic。
- A4：pre-reset、post-reset、post-link-up 三个 peer phase 均双向成功。
- A5：脏页测试证明 Dma 全区清零和 queue 初始 empty；runtime reset 后 QueueEpoch/SocketEpoch 按规则
  推进并恢复 64/64/0；HMP flap 不推进 QueueEpoch。
- A6：旧 socket 返回稳定 terminal，新 socket 成功；validator exit 0；无 owner drift/permanent Pending。
- A7：MS01 14/14、MS04 四 mode、MS05 六 mode、MS06 12-case 全部明确 PASS。

**Verification**

1. RED/GREEN：DirtyHal 的 Dma 三 direction 全区零；modern/legacy queue 初始无 completion。
2. 邻接：virtio-drivers alloc、axdriver_virtio net、axdriver_net、axnet ordinary/qemu-diagnostics 串行全量。
3. 集成：`make host-test`、静态 probe、`make ARCH=riscv64 build`、rustfmt；对本 Cycle 产品/测试/文档
   路径执行 scoped diff check，并单独记录历史 raw-log CRLF 例外。
4. runtime：用户手工新建 single-hart QEMU session，运行 MS07 六 case、validator及四组兼容回归。
5. Review：完整 diff、unsafe safety comment、无 token fallback、Evidence 预算和 strict OpenSpec。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | raw serial、panic token域、reinit调用链、Hal契约、allocator实现与现有测试缺口已独立核对 |
| Design | PASS | 在本地Dma所有权边界清零完整region；不改registry、allocator策略或上层token语义 |
| Iteration Plan | PASS | 修复仅关闭Task 4.2既有A2/A4–A7；Iteration Map不变 |
| Cycle Scope | PASS | R3修DMA/queue postcondition，R4恢复既有资格；无新成果或平台声明 |
| Task Contracts | PASS | 两项repair均含位置、RED/GREEN、保持/禁止项、验证和stop条件 |
| Traceability | PASS | R1/R2/R4–R8→D2/D3/D5–D8→R3/R4→unit/host/QEMU见证，无Missing |
| Verification | PASS | contract unit→queue→邻接crate→host/build→QEMU→compat递增；历史raw-log diff例外已分层 |
| Evidence | PASS | 本Cycle最多README/raw serial/regressions/按需pcap四文件；全change最多15文件 |
| Authorization | PASS | 用户明确要求“阅读回复，进行审计，解决问题，给出下一轮cyc”；批准Plan完成审计并给出可执行后继Cycle |

**Persisted Evidence**

- Mode: required
- Path: `evidence/007-single-hart-qemu-qualification/006-rework/`
- `README.md`：支持A2/A4–A7；记录自动Gate、环境、命令、exit、artifact变化边界与最终判定。
- `qemu-serial.log`：支持A2/A4–A6；保存新实现的完整MS07 marker/exit，或BLOCKED首个决定性失败段。
- `regressions.txt`：支持A7；进入四组回归后保存各组终态和exit。
- `usernet.pcap`：仅serial不足以判断A4/A6 packet交付时创建；否则README写`not needed`。
- Budget: 本Cycle最多4文件；当前change已有11文件，完成后最多15，低于全局20。

**Risks and Notes**

- `Dma::new` 防御性兑现自身公开后置条件，会让已合规 HAL 发生一次重复清零；本 Cycle 优先保证
  queue 初始化正确性，不把性能改进纳入 Acceptance。
- 清零在当前 QEMU coherent DMA 模型下可直接验证；对非一致性 DMA 的 cache maintenance 不作推断。
- Cycle 005 的 panic log只用于根因与新Cycle基线，必须用修复后的新runtime关闭Acceptance。
- Cycle 003/004 raw serial 的 CRLF 是既有 Evidence 属性；不得借 whitespace Gate 修改原始现场。

## Act Response

- Status: reported

**Implemented**

Task 4.2 两项 repair item 均已实现并经运行时闭合：`T4.2-R3`（DMA 零化后置条件，自动 Gate 全绿）与 `T4.2-R4`（single-hart QEMU 六 case + 四组兼容回归，用户手工执行后全过）。

1. `Dma::new` 在成功分配后、构造 `Self` 前，用 `core::ptr::write_bytes` 把整段 `pages * PAGE_SIZE` 独占 DMA region 清零。未改动 `Hal` trait 签名、`paddr`/`vaddr`/`direction`、`Drop`/dealloc identity 或 queue token 域；`paddr == 0` 的 `DmaError` 失败语义保留，失败地址不写内存。
2. 新增测试专用 `DirtyHal`（`crates/virtio-drivers/src/hal/dirty.rs`，`#[cfg(test)] pub(crate)`，非生产/公共 API）：`alloc` 后以确定性 `0xA5` pattern 填充全页，使 `Dma::new` 的零化后置条件在宿主测试中可观测。
3. 三向 RED witness：`dma_zeroes_the_entire_region_for_every_direction`（DriverToDevice / DeviceToDriver / Both，2 页覆盖跨页边界）。修复前 RED（`byte at offset 0 = 0x90` 一类非零读回），修复后 GREEN。
4. modern / legacy queue 初始 empty witness：`modern_queue_with_dirty_hal_starts_empty`（经 MmioTransport + DirtyHal）、`legacy_queue_with_dirty_hal_starts_empty`（经最小 `LegacyLayoutTransport` seam 包装 `FakeTransport`，仅覆盖 `requires_legacy_layout()==true`，绕过真实 legacy MMIO u32-PFN 无法表示宿主堆“物理地址”的约束）。两者修复前 RED（`can_pop()==true` 暴露陈旧 used-ring 作为伪 completion），修复后 GREEN。

**Changed Files and Symbols**

- `crates/virtio-drivers/src/hal.rs::Dma::new`：分配成功、`paddr != 0` 校验后对 `vaddr` 全 region 执行 `write_bytes(0, pages * PAGE_SIZE)`；`#[cfg(test)] mod tests`，`dirty` 模块声明顺序调整。
- `crates/virtio-drivers/src/hal/dirty.rs`：新增 `#[cfg(test)] pub(crate) mod dirty`；`DirtyHal` + `dma_alloc/dma_dealloc/mmio_phys_to_virt`，`DIRTY_PATTERN = 0xA5`。
- `crates/virtio-drivers/src/queue.rs`：`#[cfg(test)] mod tests` 新增 `LegacyLayoutTransport` seam、`legacy_transport()` helper 与 `modern_queue_with_dirty_hal_starts_empty` / `legacy_queue_with_dirty_hal_starts_empty`；补 `DeviceStatus, Transport`，`hal::{dirty::DirtyHal, fake::FakeHal}` 导入。

**Deviations from Plan**

无 Acceptance 偏差。非实质记录：
- modern queue witness 复用既有 MmioTransport + `DirtyHal`；legacy witness 因真实 legacy MMIO 的 `legacy_queue_pfn` 为 32 位段号，宿主堆地址（约 1.25e14 ≈ 125 TB，远超 2^44 上限）无法写回，故采用 Plan 明确允许的「最小 fake transport seam」`LegacyLayoutTransport`（局部测试型，不提升为公共 API），仅翻转 `requires_legacy_layout()`。
- witness 附带生命周期修正：早期把 `Dma::new(...).raw_slice().as_mut()` 写成临时借用，`Dma` 在语句末 `drop` 会 dealloc region，形成悬挂 slice——单独运行时恰好读到未被复用的清零内存而误通过，全量运行时被 allocator 复用为 0x90。改为先绑定 `let dma = ...` 再取 `raw_slice` 后，全量套件稳定 GREEN。
- rustfmt：`cargo fmt --manifest-path crates/virtio-drivers/Cargo.toml -- --check` 在本 crate 基线即失败（约 20 个本次未触碰文件存在格式差异，属既有状态）；本次对 `dirty.rs`（新文件）用 rustfmt 格式化，并对 `hal.rs`/`queue.rs` 中新增区域按 rustfmt 风格手动对齐，`git diff --check`（scoped 到本 Cycle 产品/测试路径）exit 0。
- 运行时判据偏差：MS07 `ms07-qemu-validate.py` 对 hmp_link_down 报 marker 计数 exit 1——`MS07_HMP_OBSERVED: link=off` 被 QEMU monitor `(qemu) ` 提示符前缀污染（共享 `-nographic` 控制台竞态）。设备状态与数据面 marker 均真实存在，判为采集伪影；用户明确豁免，MS07 计入通过。
- Evidence 预算偏差：Cycle 原计划 4 文件；按用户明确指示将四组回归序列日志一并收入 `006-rework/`，本目录 7 文件，change 级 Evidence 总数 18（低于全局上限 20）。

**Blocker Handoff**

`T4.2-R4`（重跑 single-hart QEMU 六 case、validator 与四组兼容回归）由 Plan 与 Runbook（`ms07-qemu-single-hart-recovery-evidence.md`）明确指定为**用户手工执行**：需要 Terminal A peer、Terminal B QEMU、Terminal C HTTP、guest 内 probe，以及 HMP link flap（`Ctrl-A c` + `set_link net0 off/on`）——后者按 R44 一律手工、不可自动驱动，属能力边界而非缺陷。`T4.2-R3` 的 kernel/probe 构建已为 R4 准备好 `StarryOS_riscv64-qemu-virt.bin`；用户完成 R4 后按 Plan 要求保存 `evidence/007-single-hart-qemu-qualification/006-rework/` 下 `README.md`、`qemu-serial.log`、`regressions.txt`，才能关闭 A2/A4–A7。

**Blocker Resolution**

用户按 Runbook 手工完成了 single-hart QEMU 资格，解决了 T4.2-R4 阻塞（`blocked → pending → 继续 → reported`）：

- MS07 六 case（pre_reset_traffic / reset_request / old_socket_terminal / new_epoch_traffic / hmp_link_down / hmp_link_up）唯一顺序完成，`MS07_HARNESS_EXIT: 0`。`old_socket_terminal` 在 Reinitializing 后恢复 Active（`lifecycle=2 q=1 s=1 dev=64`），不复现 token 28526 panic；HMP off/on 中 QueueEpoch 不推进、LinkGeneration/SocketEpoch 推进。
- 采集伪影（用户明确豁免）：`MS07_HMP_OBSERVED: link=off` 一行被 QEMU monitor 提示符 `(qemu) ` 前缀污染，`ms07-qemu-validate.py` 对 hmp_link_down 报 marker 计数、exit 1。该行与两侧数据面 marker 真实存在，判为采集/控制台竞态而非产品/探针缺陷；用户明确指示「没必要因为采集因素再做一遍，豁免了」，MS07 计入通过。
- 四组兼容回归全过：MS01 14/14、MS04 四 mode、MS05 六 mode、MS06 12-case + `ms06-qemu-validate.py` exit 0。
- 据此关闭 T4.2-R4 与 A2/A4–A7。

**Self-Review**

- Spec compliance：PASS。对照 T4.2-R3 契约逐项核对——Requirement R2/R5/R8 零化后置条件由 DirtyHal witness 覆盖；`Hal` API/unsafe contract、paddr/vaddr/direction、Drop/dealloc、queue layout/size、token 域、recovery transaction 与错误映射均未改动；Forbidden 全部未触碰（未改 registry crate、未 vendor `axdriver`、未只清 `used.idx`、未在 `receive()` 加 bounds-check、未伪造 completion、未放宽 panic Gate）。
- Code quality：PASS。diff 无计划外修改；unsafe `write_bytes` 有 SAFETY 理由；错误/边界/生命周期正确（先 `paddr==0` 返回、再清零、后构造，paddr 与 pages 不突变）；没有新增 warning、死代码或身份型证据机制。
- RED/GREEN：真实 GREEN 前先反向禁用修复观察到 `43 passed; 3 failed`（三个新 witness 全 RED），恢复修复后 `46 passed; 0 failed`（GREEN 确系修复所致，非测试自身恒真）。
- 运行时审计：MS07 六 case/`MS07_HARNESS_EXIT: 0`、MS01 14/14、MS04 四 mode、MS05 六 mode、MS06 12-case + `ms06-qemu-validate.py` exit 0，均经 Evidence `006-rework/` 下 raw serial 与 validator 复核；hmp_link_down 采集伪影按用户豁免计入通过。
- Minor findings：`cargo fmt --check` 在本 crate 基线即失败（既有、与本次无关，已 scoped 处理）；`Dma::new` 的防御性清零会给已合规 HAL 造成一次重复清零（Plan Risky/Notes 已接受，非 Acceptance 项）。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| Gate 3 RED(去除修复) | `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --locked --offline --lib --features alloc -- --test-threads=1` | `test result: FAILED. 43 passed; 3 failed`（三 witness RED） | PASS（预期 RED） |
| 全量 GREEN | 同命令（修复在产物内） | `test result: ok. 46 passed; 0 failed` | PASS |
| 邻接 crate | `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --locked --offline --features net -- --test-threads=1` | `test result: ok. 36 passed; 0 failed` | PASS |
| 邻接 crate | `cargo test --manifest-path crates/axdriver_net/Cargo.toml --locked --offline -- --test-threads=1` | `test result: ok. 12 passed; 0 failed` | PASS |
| axnet ordinary 全量 | `RUSTFLAGS="-C linker=/home/daivy/projects/serial/work/StarryOS/scripts/cc-nopie.sh" cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --test-threads=1` | `test result: ok. 474 passed; 0 failed` | PASS |
| axnet qemu-diagnostics 全量 | 同上 + `--features qemu-diagnostics` | `test result: ok. 506 passed; 0 failed` | PASS |
| 集成 host-test | `make host-test` | `exit=0`（early-console/memtrack/MS03/MS04/MS05/MS06/MS07 harness、C probe、validator case/schema diff 全过） | PASS |
| kernel build | `make ARCH=riscv64 build` | `StarryOS_riscv64-qemu-virt.bin` `.bin` 生成，`build_exit=0` | PASS |
| whitespace | `git diff --check -- hal.rs dirty.rs queue.rs` | `exit=0` | PASS |
| rustfmt | `rustfmt --edition 2021 src/hal/dirty.rs`；新增区域手动对齐 | exit 0；crate 级 `-- --check` 基线既有失败，已 scoped（见 Deviations） | PASS（scoped） |
| OpenSpec | `openspec validate ms07-qemu-single-hart-recovery-semantics --strict` | `Change ... is valid`，exit 0 | PASS |
| 运行时 MS07 | 用户手工 QEMU（`$EV/qemu-serial.log`） | 六 case 唯一顺序 + `MS07_HARNESS_EXIT: 0`；`old_socket_terminal` Active 恢复，无 token 28526 panic；link flap 不推 QueueEpoch | PASS |
| validator MS07 | `python3 scripts/ms07-qemu-validate.py --expect-environment "qemu-virt-riscv64-single-hart-virtio-mmio-user-net" qemu-serial.log` | hmp_link_down marker 计数 exit 1 —— 采集伪影，用户明确豁免 | PASS（用户豁免） |
| 回归 MS01 | `$EV/ms01-qemu-serial.log` | `PASS` 14/14 + exit 0 | PASS |
| 回归 MS04 | `$EV/ms04-qemu-serial.log` | snapshot/idle/nudge/burst 四 mode + burst 96/96/96 + fault=0 | PASS |
| 回归 MS05 | `$EV/ms05-qemu-serial.log` | 六 mode + Full→recovery 闭合 + flush_ok=1 | PASS |
| 回归 MS06 | `$EV/ms06-qemu-serial.log` + `ms06-qemu-validate.py --expect-environment qemu-virt-riscv64-single-hart` | 12/12 `PASS` + `MS06_HARNESS_EXIT: 0` + validator exit 0 | PASS |
| 回归终态 | `grep FAIL\|panic\|trap\|fatal\|illegal\|page fault` 于四组 serial | 无可疑字符串 | PASS |

**Persisted Evidence**

- Mode: required。已采集于 `evidence/007-single-hart-qemu-qualification/006-rework/`：
  - `README.md`：结论、根因/修复、自动 Gate、运行时、兼容回归、证据预算与适用限制。
  - `qemu-serial.log`：MS07 六 case raw serial（含采集伪影豁免记录）。
  - `ms01-qemu-serial.log` / `ms04-qemu-serial.log` / `ms05-qemu-serial.log` / `ms06-qemu-serial.log`：四组兼容回归 raw serial。
  - `regressions.txt`：四组回归终态汇总。
- 证据预算偏差：Cycle 原计划 4 文件，按用户明确指示将四组回归序列日志一并收入，本目录 7 文件；change 级 Evidence 总数 18，低于全局上限 20。

**Experience Candidates**

None. `Dma::new` 清零修复是普通可复现代码修复，host 测试所需 nopie linker wrapper 为既有环境前提；无端到端可重复风险操作路径。若用户 R4 中再现/排除 reset 后脏 ring 场景，可作为 Runbook 更新候选，但当前无证据支撑，不声明。

**Remaining Issues**

- 无阻塞项。T4.2-R3/R4 与 A2/A4–A7 全部闭合；阻塞经用户手工运行时解除，采集中 hmp_link_down 的 `(qemu)` 前缀伪影由用户明确豁免。
- 遗留 Minor：`Dma::new` 防御性清零造成一次重复 memset（Plan 已接受）；virtio-drivers crate 级 rustfmt 基线失败（既有）。

**恢复条件**

已满足。用户手工 QEMU 资格完成，`qemu-serial.log`、四组回归 `ms01/ms04/ms05/ms06-*-serial.log` 与 `regressions.txt` 已写入 `evidence/007-single-hart-qemu-qualification/006-rework/`；本 Cycle 已恢复并经 `blocked → pending → reported`，T4.2-R4 关闭。

**Commit or Diff Reference**

Diff reference: `git diff`（工作树，未提交）——T4.2-R3 变更跨 `crates/virtio-drivers/src/hal.rs`、`hal/dirty.rs`（新）、`queue.rs` 三路径。commit 未建（未获提交授权）。`T4.2-R4` 未改动产品代码。

## Plan Review

- Review Result: pending

**Findings**

None yet.

**Deviation Classification**

None.

**Acceptance Gaps**

None yet.

**Convergence**

Not reviewed.

**Evidence**

None yet.

**Follow-up Decision**

None.

**Iteration Plan Update**

None.

**Next Cycle**

None.

**Next Iteration**

None.
