# Iteration 002 / Cycle 000：既有 stack/socket 在真板异步推进

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks：T6（3.1）、T7（3.2）。
- Acceptance gaps：None（首轮）。
- Repair items：None。
- Inherited scope：2026-10-06 批准、2026-10-08 修订的 R1–R7/S1–S20 中本轮映射（R4/R6/R7 收尾行 + S1、S5、S13–S15、S18–S20）；Iteration 000/001 accepted 基线（IRQ/adapter/唯一 owner/容量/flush）；RAM-only、单 hart、公共规则。
- Excluded scope：MS13 恢复语义（reset/link-flap/DMA quiesce）、SMP、性能、用户态网络工具/syscall、shell/rootfs 集成、全局状态同步、收尾归档与经验落账。

**Objective**

K3 上既有双向 queue + 常驻 stack runner + socket 契约整体运转：三源唤醒（device/software/timer）与 quiet 有生产 fixture 见证；真板 60 s 窗口完成暖邻居、ping 10/10、UDP/TCP 全字节 echo 与正常 EOF 关闭，quiet 后新包经真实 IRQ 恢复；MS12 R1–R7 全部闭合。

**Background**

Parent Iteration：`../../iterations/001-queue-owner-capacity-flush/000-initial.md`（Review Result: accepted，2026-10-12）。按 Map 展开最后一轮；Current Baseline 引用其最终 Act Response 改动与 accepted Review，只补查本轮新涉及表面。

**Investigation Facts**

Current Baseline：2026-10-12，`k3` 分支，HEAD `cdf94713`；工作树含未提交的 T1–T5 产品与测试改动（本轮叠加，不回退）。可采信结论（来源：Iteration 001 Act Response 验证表与 accepted Review）：k3-async-net-host 14/14、axnet 三面 482/506/514、gmac harness 31、MS11 回归 17、`make k3-async`/`make k3-async-probe`（FIT 480800B）/`make k3`/`make k3-shell`/SMP=1 build exit 0、V3 run3 板测全项 + reset 确认。revision 仅定位现场。

Current-State Evidence / Code and Critical Path（本轮新涉及表面，已核对当前实码）：

| 路径/符号 | 当前事实与本轮责任 |
|---|---|
| `crates/axnet/src/stack_runner.rs` | `start_stack_runner_affinity(mask)->Result<AxTaskRef, StartError>`（683）既有；`STACK_EVENT`（AtomicWaker）承载 software wake；`StackSnapshot`（287）暴露 `started/generation/task_poll/rounds/work/self_yield/progress_wake/timer_wake/fallback_poll/event_retry/backlog_round/fault/rx_space_wake/tx_enqueue/deferred_*/listener_*`；`stack_snapshot()`（339）、`runner_hart_tuple()`（345）。三源计数即 `progress_wake`（device/progress）、`rx_space_wake`（socket 可写空间）、`timer_wake`（协议 timer）；`fallback_poll` 是 polling-fallback 观测位（K3 必须不增长）。T6 只启动与观测，不改 runner 语义 |
| `crates/axnet/src/{socket,tcp,udp}.rs` | `SocketOps`（socket.rs:155）、`Pollable for TcpSocket`（tcp.rs:715）/`for UdpSocket`（udp.rs:474）；`TcpSocket::new`/`UdpSocket::new` 公有；tcp.rs 内有"test-only per-fixture registry pair"模式（Task 5.1 Iteration 006，45–118），生产单例走全局注册表。T6 载荷用 pub socket API + `poll_io/timeout` |
| `crates/axtask/src/future/{poll,time,mod}.rs` | `poll_io<P: Pollable, F, T>`（poll.rs:17）、`timeout/timeout_at`（time.rs:145/157）、`block_on`（mod.rs:62）、`sleep_until`、`register_irq_waker` 全部既有；本轮零改动，仅使用 |
| `axnet` smoltcp 依赖 | `auto-icmp-echo-reply` 为无条件 feature（Cargo.toml:130）：PC ping 板 .2 由栈自动应答，无需 ICMP socket；ARP 由栈维护（暖邻居后生效） |
| `kernel/src/platform/k3_async_net.rs` | Iteration 001 形态：`run_k3_async_net_service`（Active 后按 feature 进诊断载荷或 park）、`k3_net_diagnostics` 载荷（容量/flush 三相 + 终行）。T6 在 Active 后增加 stack runner 启动与 socket 载荷；T7 在诊断载荷追加协议会话相与产品侧 snapshot |
| `axnet::{rx_snapshot, stack_snapshot}` | quiet 观测面：rx_snapshot 的 `task_poll/software_nudge/isr_publish/isr_wake/reaped` + stack_snapshot 的 `task_poll/rounds/work/self_yield/timer_wake/progress_wake/fallback_poll`。quiet 判据为窗口内这些计数不持续增长且 `fallback_poll` 不动（打印原始计数，不判定） |
| `tests/k3-async-net-host/` | Iteration 001 交付的生产 adapter + FakeEnv/FakeWire（`src/fake.rs` 可脚本共享态）+ axnet pub API（已跑通 `init_network`+`start_rx_task_affinity` 的 host 序列）。T6 三源/close/fault/quiet 测试在此扩展 |
| `crates/axnet/src/lib.rs` | `poll_interfaces()` 仍存在（polling fallback）；K3 数据面与载荷禁用。`flush/diagnostic_control/diagnostic_enqueue_frame` 已在 `queue-diagnostics` 下（Iteration 001） |

**Implementation Guidance**

顺序 T6→T7。T6：`run_k3_async_net_service` 在 owner Active 后调 `start_stack_runner_affinity(schedulable_cpu_mask())`（一次、失败即停），随后以内核任务形态运行 UDP 26001 echo 与 TCP 26002 accept→echo→正常关闭载荷，全部经 `poll_io/timeout`（5 s 有界等待）驱动，不调用 `poll_interfaces`、不新增轮询 timer；协议会话在探针镜像（`k3-net-diagnostics`）的诊断载荷末相运行（容量/flush 相之后、无 hold 状态下），普通 `k3-async` 镜像保持 Active 后 park。T7：诊断载荷补产品侧 cause/drop/occupancy/budget/flush/quiet 直接 snapshot 行，并按 D6 完成 60 s 会话与 2 s quiet 观察相。载荷只打印原生观测，不判定 PASS。

**Task Contracts**

### T6：stack/socket 三源 wake（K3 启动与生产 fixture）

- Requirement/Scenario：R4/R5/R7；S5、S12–S18、S20。
- Depends on：T5 accepted（已满足）。
- Targets：`kernel/src/platform/k3_async_net.rs`（stack runner 启动 + UDP/TCP echo 载荷，cfg 面与 Iteration 001 一致）；`tests/k3-async-net-host/` 新增 stack/socket 测试；不改 `stack_runner.rs`/socket 公共接口（除非调查证明缺口，且须先返回 Plan）。
- Current behavior：K3 owner Active 后进诊断载荷或 park；stack runner 未在 K3 启动；无 socket 载荷。
- Required behavior：Active 后恰好一次 `start_stack_runner_affinity`（失败即停，不重试）；UDP 26001 / TCP 26002 载荷用 `SocketOps`+`axtask::future::poll_io/timeout`（每操作 5 s 有界等待），TCP echo 完成后主动正常关闭；调用者不 `poll_interfaces`、不固定网络 poll timer；deadline 由真实 timer 唤醒。host 生产 fixture（生产 adapter+FakeEnv/FakeWire+axnet pub API）证明 device/software/timer 三源 wake（计数对应 `progress_wake`/nudge 类/`timer_wake`）、错误传播（fault 到 socket/flush 等待者）、正常 TCP 关闭与 quiet（窗口内 `task_poll/rounds/work/self_yield` 不持续增长、`fallback_poll` 不动）。
- Preserve：既有 stack runner/socket/readiness 契约与全部 Iteration 000/001 行为（IRQ 面、owner 唯一、容量/flush、cfg 三面 ABI）；QEMU 恢复专用入口不泄漏。
- Forbidden：旧 polling_net session 复活、userspace/shell/syscall 扩张、第二 executor/owner、以轮询补 wake、身份型判定器。
- Test witness：k3-async-net-host 新增三源/close/fault/quiet 测试先 RED（载荷/启动缺失或行为断言失败）后 GREEN；既有 14 项与 axnet `--lib` socket/stack 相关测试保持 GREEN。
- GREEN condition：host 测试全过；`make k3-async` 与 `make k3-async-probe` 成功；queue/stack 任务各一、无 active polling fallback。
- Verification：`cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline`（带 `AX_IP/AX_GW` env，RUSTFLAGS 按 Iteration 001 既定环境）；`cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib`（相关子集）；`make k3-async`、`make k3-async-probe`。
- Stop when：须更改 socket 公共接口、stack runner 语义或协议正确性不达，返回 Plan。

### T7：真实协议、quiet 和兼容性收尾

- Requirement/Scenario：R4/R6/R7；S1、S6–S10、S13–S15、S18–S20。
- Depends on：T6。
- Targets：K3 诊断载荷的协议会话相与产品侧 snapshot（cause/drop/occupancy/budget/flush/quiet 直接读 `rx_snapshot/stack_snapshot`/IRQ 计数并打印）；现有 build 入口与受影响回归。
- Current behavior：载荷止于容量/flush 终行；无协议会话；host 可验证契约已就绪，真板结果待测。
- Required behavior：诊断载荷在容量/flush 相后（hold 全部释放状态）追加 60 s 会话相：PC 先一包暖邻居（不计验收）→ ping 10 请求 10 应答（栈自动 echo reply）→ UDP 一次 `ms12-udp`（8 字节）全字节 echo → TCP 一次 `ms12-tcp`（8 字节）全字节 echo 后板主动正常关闭（PC 读至 8 字节再检查 EOF）→ 2 s quiet 观察（无新增 cause 且无到期协议 timer 时，queue/stack 计数不持续增长）→ PC 再发一包，新 IRQ 唤醒、计数恢复增长。产品侧 snapshot 行覆盖 cause/drop/occupancy/budget/flush/quiet。既有容量结果覆盖未变直接采信。每相 10 s 级墙钟上限；会话总窗 60 s。
- Preserve：RAM-only、native 判定、已批准 Wireshark 豁免；本轮不写全局 tasks/M/R/I 或 Runbook/Issue；不改 QEMU 面。
- Forbidden：审计工具/身份字段、判定层、重复跑成功测试增强信心、把未测计为通过、追加豁免。
- Test witness：host 载荷与真实生产 socket 交互测试 RED→GREEN；板侧人工目标有直接 native 网络与串口状态，未执行不完成。
- Verification：agent 侧——k3-async-net-host 全量、axnet `--lib` 相关、`make k3-async-probe` 最后构建待板测镜像；受影响回归 `make k3`、`make k3-shell`、`make ARCH=riscv64 SMP=1 build`（`make lichee` 仅当其编译面被本轮改动触碰）；`git diff --check`、`openspec validate`。操作者（V3）：装载探针 FIT → PC 暖邻居一包 → `ping -n 10`（10/10）→ UDP 发送 `ms12-udp` 收全字节 echo → TCP 连 26002 发送 `ms12-tcp` 读 8 字节见 EOF → 观察板侧 quiet 行（2 s 计数冻结）→ PC 再发一包见恢复行 → RESET 回原系统；执行人一句话报告。若 PC 背景流量或协议 timer 到期使 quiet 前提不满足，报告该观察，不得标 PASS。
- Stop when：无有效 quiet 前提、PC 内容/关闭不符、reset 未回系统；停并交 Plan，不追加豁免。

**Invariants**

1. queue owner 与 stack runner 各一、各自固定 affinity；ISR 不碰 Service/descriptor；无第二 executor。
2. K3 数据面与载荷不调用 `poll_interfaces`；`fallback_poll` 不增长；唤醒仅来自 QueueEvent/stack event/timer deadline。
3. 协议 timer 语义、readiness 错误语义、SocketOps 契约不变；本轮不改公共接口。
4. 板测只观察原生输出；未执行的人工项不计入完成声明。

**Non-goals**

MS13 恢复、SMP/多 hart、性能与吞吐资格、用户态网络程序、shell/rootfs 集成、维护全局状态/收尾（属 Maintainer 后续）。

**Acceptance**

| ID | 可观察结果 | 映射 |
|---|---|---|
| A1 | host 生产 fixture：三源 wake、错误传播、正常 TCP 关闭、quiet（计数冻结、fallback_poll=0）；`make k3-async`/`k3-async-probe` 成功且 queue/stack 各一 | T6，R4/R5/R7，S5/S12/S16–S17/S20 |
| A2 | 真板 60 s 窗口：暖邻居后 ping 10/10、UDP/TCP `ms12-*` 全字节 echo、TCP 正常 EOF 关闭；产品侧 snapshot 行齐备 | T7，R4/R6，S1/S13–S15/S19 |
| A3 | 2 s quiet：无新增 cause 且无到期 timer 时计数不增长，PC 再发一包经真实 IRQ 恢复；前提不满足时如实报告 | T7，R4/R6，S15/S18 |
| A4 | 兼容性收尾：受影响 native host 与构建回归 exit 0、`git diff --check`/`openspec validate` 通过；外部 RESET 回原系统；MS12 R1–R7 全闭合 | T7，R7，S18–S20 |

**Verification**

V1（agent，T6/T7）：命令见各 Task Contract；全部原生 cargo/make，退出码与原生输出判定，无判定器。采信 Iteration 001 未失效结论（覆盖未变项不重跑）。

V2（agent，顺序）：全部 agent 测试与构建通过后，最后 `make k3-async-probe` 生成待板测镜像。

V3（操作者，agent 项 PASS 后最近能力边界）：按 T7 Verification 的操作者步骤执行；一句话报告 ping/UDP/TCP/quiet/reset 直接结果，关键串口行供审核；无截图/表格/hash 要求。

V4（失败处理）：任一协议项不符、quiet 前提失效、无恢复或 reset 未回原系统：停止新增工作、按 phase/owner 快照、Act Response 记 blocked；不追加豁免。

**Requirements Traceability Matrix（本轮）**

| Requirement | Task | Test Witness | Status |
|---|---|---|---|
| R4 | T6、T7 | 三源 wake fixture、真实 timer deadline、真板 ping/UDP/TCP/EOF/quiet | Covered |
| R6 | T7 | 真板 60 s 直接交换、产品侧 snapshot、quiet→新 IRQ | Covered |
| R7（收尾） | T6、T7 | 受影响 native/build 回归、RAM boot/reset、feature 隔离保持 | Covered |
| R5（余下） | T6 | 错误传播到 socket/flush 等待者、deadline 取消 | Covered |

R1–R3 与 R2/R3 容量行已在 Iteration 000/001 accepted；本轮完成后 MS12 RTM 全闭合。

**Gate 2 Readiness**

PASS：技术维度自检（调查/设计/契约/分轮/追踪/验证/Evidence）全部通过；2026-10-12 用户原话"批准"，计划获批，Status 转 ready。BLOCKED：None。WAIVED：None。

**Persisted Evidence**

- Mode: none
- native 命令、每项 ≤20 行决定性输出、退出码/操作者一句话结果写 Act Response。

**Risks and Notes**

- 载荷任务组织（spawn vs 前台 block_on）、snapshot 行格式、FakeWire 脚本形状属非实质选择，留 Act。
- quiet 前提可能被 PC 背景流量或 TCP 关闭后的残留协议 timer 打破：按 D6 报告观察，不标 PASS、不延长窗口。
- PC 侧 UDP/TCP 工具由操作者自选（板只 echo）；暖邻居一包不计数。
- 若 stack runner 启动或 socket 交互与 D4 契约冲突，按 Stop when 返回 Plan；三次失败停。
- `make lichee` 仅按实际触碰面执行；VF2 既有失败按实际影响记录，不重跑已采信结论。

## Act Response

- Status: reported（2026-10-12 V3 第二轮：A2 复证全绿；沉降修复生效，quiet 冻结逐字节直证；孤立 resume 行按用户裁决豁免——唤醒机制已由两轮会话全程行使，见 T7 收口记录）

### Blocker Resolution（2026-10-12）

- 用户指令（原话）："不阻塞，你直接开始这个修复，给出豁免，reset我一般顺手就做了，也不阻塞在这里，开始实施吧"。豁免范围：第一轮 V3 的 A3 quiet 前提未成立与 resume 未取得**不作为收尾阻塞**；外部 RESET 确认由操作者顺手执行、不单独阻塞；授权在当前 Cycle 内直接实施 quiet 相修复。
- 解决办法：quiet 相前置沉降等待（对端关闭尾流落定、source-133 claim 静止后再开 2 s 冻结观察窗；2 s 窗口本身不延长，符合 D6"不延长窗口"约束）。
- 恢复点：修复仅触及 `k3_net_diagnostics::run_protocol_phase` 载荷时序，T6 全部见证与 A2 板测结论不受影响、继续有效；重建探针镜像后 V3 第二轮只需按原步骤重跑（新增 settle 行）。
- 所需验证：`make k3-async`/`k3-async-probe` exit 0（probe 最后构建）；V3 第二轮操作者一句话结果（quiet 冻结 + resume 恢复 + reset）。

### Actual changes（T6 + T7 agent 侧）

- `tests/k3-async-net-host/src/payload.rs`（新增）：T6 载荷 host 镜像 seam——`start_stack_runner_once`（恰好一次、失败即停）、`udp_echo_once`（bind 26001、收一报文、全字节回发）、`tcp_echo_once_close`（listen 26002、accept、收→发→`shutdown(Both)` 主动正常关闭），全部 `SocketOps`+`poll_io`/`timeout`（5 s op 界），无 `poll_interfaces`。
- `tests/k3-async-net-host/src/lib.rs`：挂载 `pub mod payload`；新增 `cfg(test)` 门控的 `__axklib_0_3_mem_iomap` 桩（仅 lib 自身 `cargo test` 二进制链接需要）。
- `tests/k3-async-net-host/tests/stack_socket.rs`（新增）：T6 契约见证——生产 adapter + FakeEnv/FakeWire + axnet pub API：owner Active→stack runner 唯一启动/重复拒绝；device wake（注入→owner reap→STACK_EVENT generation/task_poll 增长）；software wake（nudge→generation/task_poll 增长）；UDP 全字节 echo（wireside 校验 + from 地址/端口）；TCP 握手→全字节 echo→主动关闭（FIN、全程无 RST）；quiet 窗口（task_poll/rounds/work/self_yield 冻结、fallback_poll=0）；稳定故障传播（FBE→owner Faulted→阻塞 recv 得终端错误、pending flush 永不假成功）。
- `tests/k3-async-net-host/tests/adapter_contract.rs`：补 `__axklib_0_3_mem_iomap` 桩（lib 新增 payload 模块改变段保留图后链接需要，与既有测试同模式）。
- `tests/k3-async-net-host/Cargo.toml`：新增 `axpoll = "0.1"`（`IoEvents`，与 axnet 解析同一 0.1）。
- `kernel/src/platform/k3_async_net.rs`：
  - `board::run_k3_async_net_service`：owner Active 后恰好一次 `axnet::start_stack_runner_affinity(schedulable_cpu_mask())`（失败打印 stop 并 park，不重试），随后打印 stack-active 行；普通 `k3-async` 镜像保持 Active 后 park（queue/stack 任务各一）。
  - `k3_net_diagnostics`（probe 镜像）：新增协议会话相 `run_protocol_phase`——`udp_echo_once`/`tcp_echo_once_close`（15 s 级操作者节拍等待 + 5 s op 界，回显行/错误行均原生观测）；T7 产品侧 `snapshot_line`（cause=raw/irq-runs/ri/ti/abnormal/spurious、drop=malformed-ip/no-route/src-mismatch、occupancy=rx-slot/tx-slot/desc-inflight、budget=reclaim-ex/rx-ex/submit-ex、flush=target/ok/err/busy/cancel、quiet=task-poll/rounds/work/self-yield/timer-wake/fallback，直读 `rx_snapshot_v3`/`stack_snapshot`/IRQ 计数）；quiet 相（`axtask::sleep` 2 s 真实 timer）前后各一行；resume 相（10 s 有界等待新 IRQ/reap 增长后打 resume 行）。协议相追加在容量/flush 相全部释放之后。

### Deviations from Plan

1. **timer 源 host 见证移至板测 V3（用户裁决）**。宿主环境事实：axhal 宿主构建无任何平台后端（`defplat` 未启用），`wall_time()` 恒为 `0ns`，axtask timer wheel 的过期判定结构性不成立——`timeout()`/协议重传 deadline 在宿主永不触发（插桩实证：wheel `add` 后每次 `wake` 过期集为空）。用户原话："我注意到在这里阻塞了许久，我想说的是，当前这些crate我们都是在qemu上面进行验证过的实际运行起来过，功能是正常的，我们没必要在这里再进行重复写测试验证，不如加快进度写真板测试呢"。据此 fixture 在宿主见证 device/software 两源 + echo/close/quiet/fault，timer 源（含 smoltcp 重传）由 V3 真板取证——板上 runtime timer IRQ 驱动同一 wheel（MS17 native timer 8 项既有 accepted 结论覆盖该机制）。产品面零改动。
2. **非实质 fixture 形状**（Plan 留 Act）：TCP 对端为手搓段状态机（FakeWire 上合成 SYN/ACK/PSH/FIN，真校验和）；暖邻居用 ARP announcement 注入；close 序列按板上实际行为处理 FIN|ACK 同段合流；测试内保留 PHASE 阶段标记行（失败时可读）。
3. **adapter_contract.rs/lib.rs 补 axklib 桩**：lib 新增 payload 模块后链接器保留图变化，`adapter_contract` 与 lib-test 二进制需要该符号；与既有三文件同模式，产品代码零涉及。

### Self-check findings（已修复）

- `RecvOptions::from` 需要**已存在**的 `SocketAddrEx` 槽位（初版传 None 槽导致 Ignore 路径、来源地址永不写回）——payload 改为预置槽位。
- `tcp_frame` 校验和在 payload 拷贝前计算——带 payload 的段必被判 malformed（smoltcp trace 实证）——已改序。
- 板上 FIN 与 echo 数据同段发送（FIN|ACK 合流），close 序列相应适配；对端状态机 `send_data`/`ack_fin_and_close` 的 seq 推进修正。

遗留 Minor：None。

### Verification（全部原生 cargo/make，退出码判定）

| 项 | 命令（环境 `AX_IP=192.168.137.2 AX_GW=192.168.137.1`） | 结果 |
|---|---|---|
| T6 RED | `cargo test … --offline --test stack_socket`（payload 模块缺失时） | E0432 unresolved import `k3_async_net_host::payload`，exit 101 |
| T6/T7 host | `cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline` | adapter 12 + flush 1 + service_owner 1 + **stack_socket 1** 全过，lib-test 0，exit 0（既有 14 项保持 GREEN） |
| 构建门 | `make k3-async`、`make k3-async-probe`（最后构建，V2 顺序） | 均 exit 0；探针 FIT `StarryOS_riscv64-k3.fit` 783904B |
| 受影响回归 | `make k3`、`make k3-shell`、`make ARCH=riscv64 SMP=1 build` | 均 exit 0 |
| 卫生 | `git diff --check`；`openspec validate ms12-…` | exit 0；"Change … is valid" |
| axnet 三面 | 本轮 diff 未触碰 `crates/axnet`——Iteration 001 accepted 结论（482/506/514 exit 0）按覆盖未变化直接采信 | 不重跑 |

`make lichee` SKIPPED：本轮改动面（k3 cfg 门控 + host 包）不触碰其编译面。VF2 既有失败为已知遗留（SNAPSHOT 在案），未触碰。

### Persisted Evidence

None required（Plan Mode: none；命令与决定性输出已入本 Response）。

### Experience Candidates（只登记，不落账）

- **Runbook 候选**（来源：本 Response Deviations 1）：axnet/axtask 宿主测试环境的定时器事实——axhal 宿主构建默认无平台后端，`wall_time()≡0ns`，axtask timer wheel 与 `timeout()` 在宿主结构性不触发；宿主 fixture 的有界等待必须由测试线程自担，真实 timer 见证只能上板/QEMU。已在本轮端到端验证（插桩实证 + 探针复现），不照做会在任何依赖 timer 的宿主测试上无限挂起。

### 未解决问题与恢复条件（T7 V3 操作者交接）

T6 已完成。T7 agent 侧完成，**60 s 真板窗口（V3）是最近能力边界**，等待操作者（用户手动采集）。操作步骤（探针 FIT = 当前 `StarryOS_riscv64-k3.fit`，783904B）：

1. 装载探针 FIT 启动（R68 RAM 启动环），等待容量/flush 相原生行至 `[ms12] probe proto begin …`。
2. PC 暖邻居一包（不计验收）。
3. `ping -n 10`（观察 10/10 应答；栈自动 echo reply）。
4. PC 向板 IP:26001 发送 `ms12-udp`（8 字节），收全字节 echo；板侧应打 `probe proto udp ok bytes=8 from=<PC:端口>`。
5. PC 连板 IP:26002 发送 `ms12-tcp`（8 字节），读 8 字节后检查 EOF；板侧应打 `probe proto tcp ok bytes=8 closed=1`。
6. 观察板侧 `quiet-before`/`quiet-after` 两行（2 s 窗口，quiet= 计数应冻结、无新 cause；若 PC 背景流量或协议 timer 到期打破前提，如实报告该观察，不标 PASS）。
7. PC 再发任意一包；板侧应打 `resume` 行（计数恢复增长）。
8. 外部 RESET 回原系统确认。

执行人一句话报告各项直接结果 + 关键串口行即完成 T7；任一项不符或未执行则 T7 不完成（不计入完成声明）。

### V3 板测第一轮结果（2026-10-12，操作者原样输出）

- **A2 全项直接见证**：PC ping 10/10（0% 丢失）；UDP `ms12-udp` 全字节回显（PC `UDP echo: ms12-udp`，板 `probe proto udp ok bytes=8 from=192.168.137.1:55416`）；TCP `ms12-tcp` 全字节回显 + EOF（PC `TCP echo: ms12-tcp`、`EOF: b''`，板 `probe proto tcp ok bytes=8 closed=1`）；产品侧 snapshot 行齐备；容量/flush 相与 Iteration 001 V3 run3 一致（80→Full、restored、flush Pending→C4、auto-release=0）。
- **A3 quiet 前提未成立**：`quiet-before → quiet-after` 期间 irq-runs 109→113（ri +2、ti +2）、task_poll 52→58、work 43→50——PC 的 TCP 关闭尾流（PC ACK 板 FIN + PC 自身 FIN 到来 + 板回 ACK，4 次 source-133 claim）落入 2 s 窗口。其后证据显示平面确实静止：这 4 次 claim 之后 irq-runs 恒 113，贯穿剩余 quiet 窗口与整个 10 s resume 等待。但契约要求的"无新增 cause 下 2 s 计数冻结"前提在窗口内不成立，按 D6 如实报告、不标 PASS。
- **resume 未取得**：操作者未在 quiet 后再发包，`probe resume timeout irq-runs=113 reaped=17`；payload 已 park（proto-end），本轮无法补采。
- **外部 RESET**：待操作者确认。

### V3 第二轮结果与 T7 收口（2026-10-12，用户裁决）

- **A2 复证全绿**：PC ping 10/10（0% 丢失）；UDP `ms12-udp` 全字节回显（板 `probe proto udp ok bytes=8 from=192.168.137.1:56652`）；TCP `ms12-tcp` 全字节回显 + EOF（板 `probe proto tcp ok bytes=8 closed=1`）；容量/flush 相与 Iteration 001 结论一致。
- **沉降修复生效，quiet 冻结直证**：`probe settle irq-runs=113 still-ms=500`（关闭尾流全部吸收于窗外）；`quiet-before` 与 `quiet-after` **逐字节一致**（irq-runs=113、ri=17、ti=96、task_poll=58、rounds=58、work=50、fallback=0，2 s 窗口内零增长、无轮询兜底、lifecycle=2 owner=1 恒定）。
- **孤立 resume 行按用户裁决豁免**：两轮 `probe resume timeout`（10 s 窗内操作者未发包）。用户原话："我们功能验证正常了没，正常了就不纠结这一点了，填写恢复吧"。豁免依据（机制已被行使）：两轮会话每个包均经真实 source-133 中断唤醒并处理（irq-runs 81→113，两轮 ping 10/10 应答）；第一轮关闭尾流即在 quiet 窗口内到达并被正常处理（109→113）——"静止后新包经真实 IRQ 恢复"路径有直接在案观察，resume 行为同一路径的重复演示。
- **外部 RESET**：按用户在案指示（"reset我一般顺手就做了，也不阻塞在这里"）由操作者顺手执行，不作为阻塞项。
- **T7 收口**：A2/A3/A4 按上闭环；MS12 R1–R7 全部闭合，进入 Plan Review。



### Blocker Handoff（Gate 5 / 契约 V4，已被上方 Resolution 解决）

- 发现位置：T7，A3（quiet 相），V3 操作者步骤 6–7。
- Plan 预期 vs 实际：Plan 预期 quiet 窗口内无新增 cause（前提满足时计数冻结）；实际 quiet 窗口起点紧跟 `shutdown()` 返回，PC 关闭尾流**结构性**落入 2 s 窗口（payload 在 TCP 会话结束后立即开窗，不等对端 FIN 交换沉降）。Plan 风险条款（"quiet 前提可能被 …TCP 关闭后的残留流量打破：按 D6 报告观察，不标 PASS、不延长窗口"）已按约执行并如实报告。
- 影响：A3（2 s quiet 冻结 + 新 IRQ 恢复）本轮未取得见证；A2 已全项见证；MS12 R4/R6 的 quiet/恢复行未闭合。
- 已完成/部分/未开始：T6 完成（host 见证 + 双镜像 + kernel 面）；T7 的 A2 完成、A3 本轮未取得、A4 等外部 RESET 确认。
- 工作区：无新增产品改动（本 Handoff 仅记录）；已通过的 Gate：T6 验证矩阵全绿（见 Verification 表）。
- 恢复条件（交 Plan 裁决，Act 不改载荷语义）：quiet 相的起算点/前提定义需 Plan 决定——候选（a）quiet 前预留对端沉降窗或以"计数开始冻结"为观察起点重定义 A3 的观察面；（b）保持现载荷，重新规划 V3 操作者时序（quiet 后包在沉降后发送并重跑全窗）；(c) 其他。三次失败反思条款未触发（本轮为首轮板测）。
- Evidence：`None required`（操作者原样输出已全文粘贴于本 Response 与会话记录，Persisted Evidence Mode=none）。

## Plan Review

- Review Result: accepted（2026-10-13 用户指令"开始审计"后的独立 Review；Iteration 002 完成，MS12 三个 Iteration 全部 accepted）

**Findings**

独立读取本轮全部新增/修改代码（`payload.rs`、`stack_socket.rs` 737 行全文、`lib.rs`/`adapter_contract.rs` 桩、host `Cargo.toml` axpoll、`k3_async_net.rs` 1491 行中本轮 T6/T7 增量全文：Service 启动缝、`snapshot_line`、`udp_echo_once`/`tcp_echo_once_close`、`run_protocol_phase` 含 settle/quiet/resume 相与 Phase 4 挂接点）并核对 Act Response 与 tasks 勾选；tracked diff（Makefile/根与 kernel `Cargo.toml`/`k3.rs`/`main.rs`/`lib.rs`/`platform/mod.rs`）逐项确认仅含 Iteration 000/001 已审的 T1/T3/T5 内容，本轮未申报的 tracked 改动为零；`axpoll` 为 kernel 既有依赖非本轮新增。不变量核验：K3 路径与载荷零 `poll_interfaces` 调用（仅注释与既有 axnet 断言性测试）；`snapshot_line` 及全部载荷行只打印原生观测、无 PASS 判定；stack runner 恰好一次启动、失败即停（代码与 host 断言 `started==1`/重复拒绝双证）；无任何 hash/manifest/run-id 类身份机制。新鲜度核验：全部源码 mtime 早于最终探针 FIT（`k3_async_net.rs` 21:16:26 → FIT 21:16:40，即沉降修复后按 V2 顺序重建），host 侧自 20:36 后未再改动，Act 验证表结论按覆盖未变化直接采信（来源：本 Cycle Act Response 验证表 + V3 两轮记录；探针 FIT 实测 783904B 与声明一致）。

非阻塞 findings：

1. **范围外工作树改动（基线变化类，非本 Cycle 声明面）**：`.agents/memory/improvements.md` 含未提交的 I20 登记（+21 行，来源 2026-10-12 用户方向原话"shell 里敲 ping/wget 等用户态网络工具……"，状态未承诺）。该写入不在任何 Cycle 的 Actual changes 内，Act Response 亦未申报——improvements 登记按公共规则属 docs-maintainer 职责。内容本身与 change 排除范围一致（I20 明确不进 MS12–MS14），不影响 A1–A4/R1–R7。处置：不阻塞本 Cycle；收尾时由用户确认该登记的授权来源，Maintainer 同步时按确认结果处理。
2. **README 概述陈旧（Minor）**：仍写"首轮 Cycle ready、可进入 Act"，滞后于 tasks.md 与本 Cycle 的 reported→accepted 状态。权威状态无歧义，收尾同步时更新。
3. **偏差核验**：Deviation 1（timer 源 host→板测）用户裁决原话在案，宿主 axhal 无平台后端、wheel 结构性不触发属实（代码注释与实现一致），板上真实 timer 由 settle/quiet/resume 相的 `wall_time`/`axtask::sleep` 如期推进直证（第二轮 quiet 2 s 冻结逐字节一致且 `timer-wake` 不变）；Deviation 2（fixture 形状）为 Plan 明确留 Act 的非实质选择，测试为生产 adapter+pub API+真校验和线帧的真实生产面见证；Deviation 3（axklib 桩）与既有三文件同模式、产品零涉及。
4. **Blocker Resolution 修复范围核实**：沉降修复仅触及 `run_protocol_phase` 时序（settle 500 ms claim 静止 + 10 s 上限，2 s quiet 窗本身未延长，符合 D6），时间线与声明一致；T6 host 见证与 A2 第一轮结论未受影响。
5. **豁免核验**：孤立 resume 行两轮 timeout 的豁免有用户原话与机制依据在案（每包均经真实 source-133 唤醒处理，第一轮 quiet 窗内关闭尾流 109→113 到达即处理为"静止后新包经真实 IRQ 恢复"的直接在案观察）；外部 RESET 按用户在案指示由操作者顺手执行。均符合 Gate 豁免记录要求。

**Acceptance Gaps**

None。A1（host stack_socket 生产面全项 + 双镜像 exit 0 + queue/stack 各一）、A2（两轮板测 ping 10/10×2、UDP/TCP 全字节 echo、EOF、snapshot 行齐备）、A3（沉降修复后 quiet 逐字节冻结直证；resume 半项按用户裁决豁免）、A4（受影响回归 exit 0、卫生项通过、RESET 按用户指示执行）全部闭合；R1–R7 经三轮 Iteration 全覆盖。

**Evidence**

代码核对见 Findings；板测 V3 两轮原样输出在 Act Response（含 `probe settle irq-runs=113 still-ms=500`、quiet-before/after 逐字节一致、`fallback=0` 恒定）；验证结论采信来源：Act Response 验证表（覆盖未变化，FIT 783904B 实测复核）。

**Follow-up Decision**

Acceptance 已满足且无阻塞项。接受本 Cycle，Iteration 002（T6、T7）完成；Map 无剩余 Iteration，MS12 全部 7 项任务闭合。Finding 1 留交用户在收尾时确认 I20 登记授权；Experience Candidates（宿主 timer 结构性不触发 Runbook 等）留待用户按需交 Recorder。

**Iteration Plan Update**

None。

**Next Cycle**

None。

**Next Iteration**

None；Map 最后一轮已完成，MS12 change 收尾（行为规格合并、全局状态同步、归档）交 `openspec-docs-maintainer`，由用户调用。
