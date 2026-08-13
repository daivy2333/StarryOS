# Iteration 004: Bidirectional Queue Service Cutover

## Plan Context

- Status: ready
- Round: 004
- Parent: Iteration 003

**Objective**

先关闭 Iteration 003 Review 确认的 bounded handoff、fault ownership、MTU 与 dormant RX
transaction 缺口，再把现有 RX-only task 原子演进为 RX/TX 唯一 queue owner。完成后，一个
长驻 task 每轮按 reclaim 32 → RX 32 → submit 32 推进同一个 NIC；stack 只访问 fixed slots，
ISR 只发布通用 queue event，queue-owner 与 stack-progress waker 不互相覆盖。

**Background**

Iterations 000-002 已建立 transport-neutral bidirectional queue control、opaque TX cookie、
真实 adapter ownership/error tests 与 EVENT_IDX old/new window。Iteration 003 增加 fixed frame
storage、typed Device handoff 和 Router preflight/commit，但 Plan Review 发现 activation 前必须
关闭五个实现缺口：Router/Ethernet/ARP 每包分配、commit drift 后双重逻辑所有权、L3 MTU
边界错误、dormant RX ARP Full 场景缺失，以及已发送 ARP 状态的假 TX backpressure。

用户批准把 Review 问题直接并入原下一轮。为保持一个可验证结果，本轮把 Task 2.4 设为
activation 前置 Gate，再执行原 Tasks 3.1-3.3；不得在 handoff RED/GREEN 未闭合时发布
bidirectional `Active`。

**Current Baseline**

- Revision: `1a2bc99f657986d554d21f496579476569de6368`，branch `net-k3`；Iterations 000-003
  尚在工作树中。
- `FixedFrameQueue` 预分配固定 byte/length/meta/ticket backing；Ethernet 拥有 64 RX、64 TX
  slots 与 128-live `TicketTracker`，但当前只有 cfg(test) seam 切换 TX，RX slots 无 consumer。
- `Router::dispatch` 已实现 peek→全目标 preflight→commit，但借用规避依靠 packet/target
  `Vec`；commit drift 设置 `tx_fault` 后保留队首。
- `EthernetDevice` 产品默认 polling；dormant frame emission 与 pending flush 仍临时分配。
  payload oversize check 使用 1514-byte L2 frame 上界，而正确 L3 MTU 是 1500。
- `RxNotify` 当前只有一个 `AtomicWaker`、generation 和 Router-space waiting bit；
  `RxRxFuture` 只 preflight/suppress RX，并以单一 `RX_BUDGET=32` 推进 `rx_one_step_target()`。
- `Service` 保存唯一 target device index；`Router`/`Device` 已能借此访问同一个 NIC，无需第二
  handle。当前 queue control 已支持 RX/TX direction mask、completion query、suppress 和
  arm-and-check；VirtIO adapter 已有单步 `submit_tx`/`reclaim_tx` 与 opaque `TxCookie`。
- kernel ISR 读取/ACK MMIO cause 后调用 `axnet::publish_rx_event()`；它不持 Service 锁、不碰
  descriptor。V1/V2 snapshot ABI 与 MS03/MS04 host harness 必须保持原样。
- Fresh Review Gates：axnet 150/150、axdriver_net 7/7、axdriver_virtio 11/11、virtio-drivers
  36/36、kernel QEMU check、strict validation、diff check PASS；targeted rustfmt FAIL 两处；
  lichee-d1 仍有 change 外 25 个 `axfs`/`axtask` baseline errors。
- 用户明确排除 `make LOG=info build`；不要运行或报告它。用户提供的 `make run` 正常仅作为
  环境上下文，本轮不需要手工 QEMU runtime claim。

**Current-State Evidence**

- `router.rs::dispatch/plan_packet` 分别执行 `packet.to_vec()`、`vec!`/`collect::<Vec<_>>()`；
  commit 非 Accepted 分支没有 `tx_buffer.dequeue()`。
- `ethernet.rs::emit_frame_dormant` 以 `vec![0; frame_len]` 构造 frame；
  `process_arp` 以 `buf.to_vec()` 规避借用；`preflight_send` 与 `send` 使用
  `MAX_FRAME_SIZE=1514` 检查 IP payload。
- `device/tests.rs::dispatch_ready_commit_drift_enters_stable_fault` 断言 fault 后仍 pending；
  `arp_reply_tx_full_keeps_neighbor_unresolved_and_rx_consumed` 只见证 polling raw RX 消费；
  oversize test 从 1515 开始，未覆盖 1500/1501 payload。
- `async_rx.rs::{RxNotify,RxRxFuture,RxLifecycle}` 分别是单 waker、RX-only round 与既有数值
  lifecycle；`Active/Faulted` 已映射 AsyncOwned，`Unavailable` 保留 polling owner。
- `service.rs::poll` 仍让 stack Router poll target（由 owner view 屏蔽 RX）并同步 dispatch TX；
  `rx_one_step_target` 是现有唯一 target-index queue-service seam。
- `virtio_net_irq.rs::net_irq_handler` 的顺序是 status→telemetry→ACK→RX publish；
  `virtio_net_irq_logic.rs::should_publish_rx` 只按 used-ring bit 判定，ISR 无法区分 RX/TX。

**Relevant Code**

| File / Symbol | Current Responsibility | Iteration Use |
|---|---|---|
| `axnet/device/fixed_queue.rs` | fixed frame/ticket backing | add allocation-free mutable fill and frame/meta/ticket peek→commit seam |
| `axnet/device/ethernet.rs` | polling TX, dormant TX, raw RX, ARP | close Review gaps; atomic slot-mode switch; RX/TX copier endpoints |
| `axnet/router.rs` | stack buffers, routing, device dispatch | allocation-free plan/commit; invariant dequeue; target queue-control/data seams |
| `axnet/async_rx.rs` | RX lifecycle, one waker, RX future | generic event, two waker roles, bidirectional future and budgets |
| `axnet/service.rs`, `lib.rs` | stack polling and global Service | activation transaction, queue rounds, stack-progress registration |
| `axdriver_net`, `axdriver_virtio` | neutral control and one-step TX | existing submit/reclaim/control contract consumed by Service |
| `kernel/.../virtio_net_irq*.rs` | cause/ACK and RX event publish | publish generic queue event without data-plane access |
| `tests/ms03-*`, `tests/ms04-*` | ISR, ABI and source guards | preserve V1/V2 and extend generic-event guard |

**Critical Path**

```text
activation under one Service guard
  → validate target + RX/TX queue control
  → suppress BOTH directions
  → switch Ethernet RX/TX access to slots
  → publish existing lifecycle code Active once

stack TX: Router head → allocation-free plan/preflight/commit → Ethernet TX slot(ticket)
queue task round: reclaim ≤32 → RX copy/refill ≤32 → TX slot submit ≤32
stack RX: Ethernet RX slot → Router/smoltcp

used-ring IRQ → ACK → QueueEvent generation Release → queue-owner wake
slot RX-ready / TX-space / fatal → stack-progress wake → smoltcp re-evaluates readiness
```

**Implementation Guidance**

1. Start with Task 2.4 RED tests. Split Router field borrows and perform route/fanout as two deterministic
   passes so neither packet bytes nor target list is copied. Do not replace per-packet `Vec` with a
   preallocated growable container whose capacity can change in the data path.
2. Let `FixedFrameQueue` expose a closure-based vacant-frame fill or equivalent fixed-backing commit, plus
   head data/meta/ticket observation required by the copier. The producer writes directly into reserved
   slot storage and publishes length/ticket only after frame emission succeeds.
3. On any Ready→non-Accepted commit drift, set the stable Router fault and remove the logical Router head
   before return. Add a two-target test where target 0 accepts and target 1 drifts; the packet must exist
   only in target 0 after fault. Preflight `Fault` occurs before any commit and may retain the head.
4. Check Ethernet L3 payload against `STANDARD_MTU`; fixed queue checks the complete L2 frame against
   1514. For `neighbor=None`, preflight only pending capacity; only unknown/expired state requires both ARP
   request TX capacity and pending capacity.
5. Evolve the test seam into a bidirectional slot seam used by product activation. In slot RX mode,
   `recv` peeks the fixed RX head and pops only after `handle_frame` completes transactionally. An ARP reply
   Full/fault keeps that head. The queue task, not stack code, is the only raw RX/TX driver copier.
6. Generalize notification to one wrapping generation with separate `AtomicWaker`s for queue-owner and
   stack-progress. All publication follows state commit. Register→arm BOTH→recheck BOTH slots/completions
   and generation before `Pending`; use Acquire/Release only for control state and Relaxed for counters.
7. Activation is all-or-nothing under one guard. Before `Active`, polling owns both raw directions; after
   `Active`, async owns both even on `Faulted`. `Unavailable` may be published only before the switch.
   Never create a second NIC handle or expose an intermediate RX-active/TX-polling state.
8. Implement independent budgets in fixed order: TX reclaim 32, RX completion/refill 32, TX submit 32.
   Exhausting one stage cannot skip later stages. `Again` stops only submit and retains slot; RX slot Full
   does not reap; fatal records stage, wakes both roles as applicable, and never falls back to raw polling.
9. Rename public/internal RX-specific task/event entries only with compatibility shims required by current
   kernel callers. The ISR remains cause/ACK/counter/wake only. A used-ring event is direction-agnostic;
   completion masks are resolved by the task under Service, never by the ISR.

**Behavioral Change**

- Router/Ethernet/ARP handoff becomes allocation-free after initialization; 1501-byte IP payload is a
  stable `FrameTooLarge` drop, not a commit fault.
- Stable commit drift removes the Router head, preventing dual ownership and later duplicate delivery.
- Successful activation switches both raw directions at once. Stack TX accepts into TX slots and stack RX
  consumes RX slots; the queue task alone submits/reclaims/refills hardware buffers.
- A generic queue event can wake queue-owner and stack-progress roles independently. Stack progress remains
  a hint; smoltcp/socket state still decides actual read/write readiness.
- Active queue faults retain async ownership and stop further raw operations; there is no automatic polling
  fallback.

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 2.4 | R1/R2/R5; bounded handoff and fault ownership | fixed queue, Ethernet, Router, tests | partial fixed handoff | remove packet allocations; correct MTU/fault/RX transaction |
| 3.1 | R8/R10; atomic cutover and lost wakeup | async_rx, Service, lib | RX-only lifecycle/event | bidirectional lifecycle and dual-role QueueEvent |
| 3.2 | R1/R3/R12/R13; bounded copier | Service, Router, Ethernet, future | RX one-step only | reclaim/RX/submit independent 32 budgets |
| 3.3 | R9/R10/R14; ISR and stack progress | kernel IRQ logic/source, exports, harness | RX event publish | generic event and stack-progress wiring |

**Task Contracts**

### Task 2.4 — Close Iteration 003 handoff findings

- Depends on: Tasks 2.1-2.3; blocks activation in Task 3.1.
- RED: allocation counter observes Router unicast/fanout, dormant emission or pending flush allocation;
  second fanout commit drift retains Router head; 1501 payload preflights Ready; dormant RX ARP Full cannot
  retain/retry; `neighbor=None` returns Full solely because raw TX is full; rustfmt reports diff.
- GREEN: the actual handoff paths allocate zero after initialization; drift removes the Router head while
  retaining stable fault; 1500/1501 boundary is Accepted/Dropped; dormant RX Full retains the exact bytes
  and retry commits once; requested-neighbor pending enqueue depends only on pending capacity; format passes.
- Must modify: fixed queue, Ethernet, Router and tests; async/service only for shared seam preparation.
- Must not modify: route policy, socket API, lifecycle numeric ABI, descriptor algorithms or public test API.
- Stop: if allocation-free fanout needs an unbounded target set or fault recovery needs a persistent partial
  delivery bitmap; return to Plan rather than retaining/retrying a partially committed head.

### Task 3.1 — Publish atomic bidirectional lifecycle and QueueEvent

- Depends on: Task 2.4.
- RED: deterministic local models for event before/during register, queue/stack waker independence, event
  during one-direction arm, slot Full→space, generation wrap, spurious poll, duplicate start and every
  preflight failure; existing RX-only activation must fail the new BOTH-direction assertions.
- GREEN: lifecycle codes/V2 interpretation remain stable; target/control/suppress/mode switch/Active occur
  under one guard as an all-or-nothing transaction; queue and stack wakers never overwrite each other;
  Active/Faulted are bidirectional AsyncOwned and Unavailable is pre-switch only.
- Must modify: async_rx lifecycle/event/future, Service activation seams, lib exports, Ethernet mode switch.
- Must not modify: V1/V2 layout, add a second device handle, hold a guard across `Pending`, or expose a half
  owner state.
- Stop: if failure rollback cannot prove raw owner uniqueness or AtomicWaker use changes IRQ restore state.

### Task 3.2 — Run the bounded bidirectional queue service

- Depends on: Tasks 2.4 and 3.1.
- RED: fake driver/slot matrices for each stage at 31/32/33, earlier-stage exhaustion plus later-stage
  progress, TX Again retention, RX Full no-reap, submit/reclaim/refill fatal, two-direction multi-round
  growth, ticket/cookie round-trip, no-work nudge and raw-owner-at-Pending inspection.
- GREEN: every poll performs independent reclaim/RX/submit budgets; successful RX copies then refills in the
  same action; successful submit alone pops a TX slot; every reclaim releases the matching live ticket;
  visible backlog self-wakes/yields once; no work sleeps after bounded recheck.
- Must modify: Service/Router/Ethernet/fixed queue/future and model tests; consume existing NetTxQueue and
  NetQueueControl contracts rather than downcast raw drivers.
- Must not modify: ARP/IP/smoltcp from queue task, leak raw token/buffer across Pending, rely on 10ms fallback,
  implement flush waiters or V3 telemetry.
- Stop: if a stage cannot preserve unique buffer/slot owner on error, or one budget necessarily starves a
  later stage; report exact owner ledger before further changes.

### Task 3.3 — Wire the generic ISR event and stack-progress role

- Depends on: Tasks 3.1-3.2.
- RED: mutate source/logic harness to reject RX-specific publish, Service lock/descriptor access in ISR,
  publish before ACK, config/unknown/zero publish, overwritten waker, or stack hint treated as fd readiness.
- GREEN: used-ring/combined cause publishes one generic event after ACK; task queries BOTH directions;
  RX-slot ready, TX-slot space and fatal wake stack-progress; MS03 cause/ACK/PLIC, MS04 critical-section,
  V1/V2 ABI and UART AtomicWaker regressions all pass.
- Must modify: kernel IRQ source/logic, axnet exports and host harness; Service/socket registration only at
  the existing progress-waker boundary.
- Must not modify: MMIO cause policy, ISR descriptor/slot state, old ioctl sizes/commands, or claim exact
  fd readiness.
- Stop: if ISR must acquire Service state to choose a direction, or generic wake re-enables IRQs before PLIC
  completion.

**Invariants**

- Initialization after construction performs no dynamic packet/target/frame allocation in Router, slots or
  ARP pending handoff.
- Exactly one logical owner exists for every Router packet, slot frame, driver buffer, descriptor and ticket,
  including fault paths.
- 64 RX and 64 TX complete-frame slots remain transport-neutral; raw buffers/tokens never cross `Pending`.
- Preflight Full performs zero commit; Ready→non-Accepted is fatal and cannot retain a possibly delivered
  Router head.
- Polling owns both raw directions before activation; async owns both in Active/Faulted; no half-state and no
  automatic fallback.
- Queue work order and maxima are reclaim 32 → RX 32 → submit 32, with independent budgets.
- Events publish after state commit; queue-owner and stack-progress wakers are distinct; generation uses
  Release publish/Acquire observe.
- V1/V2 ABI, socket semantics, TCP short writes, UDP datagram atomicity, MS03 cause/ACK/PLIC behavior and
  UART IRQ restore behavior remain unchanged.

**Non-goals**

- 不实现 C4 flush waiter、V3 snapshot、QEMU pressure controls、probe 或 Evidence。
- 不运行手工 QEMU runtime，不声明性能、SMP、DWMAC 或真板结论。
- 不把 stack-progress hint 映射为精确 `POLLOUT`/fd readiness，不修改 public socket API。
- 不修复 change 外的 lichee-d1 `axfs`/`axtask` feature baseline。
- 不运行或报告 `make LOG=info build`。

**Acceptance**

| Requirement | Scenario | Design | Task | Code/Test | Status |
|---|---|---|---|---|---|
| R1/R5 | zero-allocation slots and exact MTU | D2/D3 | 2.4 | allocation + 1500/1501 + dormant RX tests | Covered |
| R2 | fanout drift unique ownership | D3 | 2.4 | two-target Accepted→drift matrix | Covered |
| R8 | all-or-none bidirectional owner | D4 | 3.1 | activation failure/interleaving matrix | Covered |
| R10 | generation and two waker roles | D5 | 3.1/3.3 | register/arm/recheck + overwrite guards | Covered |
| R3/R12/R13 | bounded raw↔slot copier | D6 | 3.2 | 31/32/33 and bidirectional model | Covered |
| R9/R14 | minimal generic ISR, compatibility | D5/D10 | 3.3 | MS03/MS04/UART/source/ABI regressions | Covered |

No requirement is simplified. Flush, V3 and runtime controls remain assigned to Iteration 005.

**Verification**

Record RED and GREEN commands, key output, exit codes and full-diff Review in the Act Response. Final Gates:

```text
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib device:: -- --nocapture
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib async_rx -- --nocapture
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib service -- --nocapture
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture
repeat the deterministic axnet interleaving/race filter 100 times with zero failures
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib -- --nocapture
make host-test
cargo check --offline -p starry-kernel --features qemu
cargo check --offline -p starry-kernel --features lichee-d1
openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict
git diff --check -- crates/axnet crates/axdriver_net crates/axdriver_virtio crates/virtio-drivers kernel tests openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane
```

Also run targeted `rustfmt --check --edition 2024` on every changed Rust file. Add source/allocation guards
proving no post-init packet `Vec` in Router/Ethernet/ARP handoff, no raw buffer/token in slots or future,
no ISR Service lock/descriptor access, product activation changes both directions together, and V1/V2 field
count/order/commands remain unchanged.

The lichee-d1 command is a compatibility observation: the known 25-error baseline must not gain a new error
or an error in this iteration's change surface. It is not a PASS unless it exits 0. Do not substitute
`make LOG=info build`; do not use the user's normal `make run` observation as fresh implementation evidence.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | Review traced allocation, ownership, MTU, dormant RX, lifecycle, Service, adapter and ISR paths |
| Design | PASS | D2-D6 already fix allocation, fault dequeue, atomic owner, event ordering and stage budgets |
| Task Contracts | PASS | 2.4/3.1/3.2/3.3 have dependencies, RED/GREEN, boundaries, commands and stop conditions |
| Traceability | PASS | R1/R2/R3/R5/R8/R9/R10/R12/R13/R14 map to D2-D6, code and tests |
| Verification | PASS | allocation/host/race/driver/kernel/format/source/strict/diff Gates are explicit |

**Persisted Evidence**

- Mode: none

Unit/model/build evidence belongs in the Act Response. Do not create an Evidence directory, Runbook,
Incident or global M/D/K/R/I entry.

**Risks and Notes**

- This iteration is larger than the original 004 because the user approved merging Review repairs. The
  balance remains coherent only if Task 2.4 closes before activation; if it blocks, stop without partial
  owner publication.
- Borrow splitting is expected to eliminate Router/ARP copies. Unsafe pointer aliasing or hidden per-packet
  allocation is not an acceptable workaround.
- An `AtomicWaker` is one registration role, not a list. Queue-owner and stack-progress require distinct
  instances even though they share one generation.
- Used-ring IRQ is direction-ambiguous by design. Query direction-specific completion under Service after
  wake; do not inspect queues in the ISR.
- `Faulted` retaining async ownership is intentional: safety takes precedence over availability until a
  later reset/recovery design exists.
- The staged Runbook/R52 remains outside this iteration and is not accepted Evidence.

**Deferred Tasks**

- Iteration 005: Tasks 4.1-4.3, ticketed C4 flush, V3 and QEMU-only bounded controls.
- Iteration 006: Tasks 5.1-5.2, probe and automatic product Gates.
- Iteration 007: Tasks 6.1-6.3, environment rerun, QEMU runtime and final Review.

## Act Response

- Status: pending

## Plan Review

- Status: pending
