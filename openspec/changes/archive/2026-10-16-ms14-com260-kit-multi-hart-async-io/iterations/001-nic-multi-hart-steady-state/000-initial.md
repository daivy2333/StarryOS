# Iteration 001 / Cycle 000: NIC 多 hart 稳态

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: T5–T6（2.1–2.2），按 T5→T6 执行。
- Acceptance gaps: None。
- Repair items: None。
- Inherited scope: R5（S15–S18）与 R3 的 NIC 角色面（S08–S09）、R8 受影响旧构建；Iteration 000 已交付的 AP/local timer/S_SOFT/固定设备路由/跨簇 IPI 底座与 `k3-smp` 构建隔离；真板操作由用户手动执行（change 级授权原话覆盖，见 000 Cycle）。
- Excluded scope: UART 专项（T7–T8）、网络恢复矩阵与 ISS02/ISS03 修复（T9–T11）、组合长稳（T12–T13）、性能优化、存储/rootfs、以 QEMU 证据替代 K3。

**Objective**

16-hart `k3-smp-net` 模式下，GMAC 沿既有 adapter/DMA/queue/stack 契约运行：owner 与 stack runner 经共享 placement 取 singleton，socket 应用载荷固定 hart 9（A100 簇）实际执行；四协议、两级 Full→恢复、wrap、C4、多 waiter/readiness、quiet→新流量与跨簇实际执行在板测成立；旧单 hart 网络入口与构建不回归。

**Investigation Facts**

- Current Baseline: 2026-10-15 Iteration 000 accepted——`k3-smp` 16 AP 启动、per-hart timer/S_SOFT、固定设备路由与跨簇 IPI 板测 A1–A4 成立；host smp/ipi/timer/aia harness 3/7/10/20 全绿，`make k3`、`make k3-smp SMP=16` exit 0（来源：000 Cycle Act Response 与 Plan Review，覆盖未变化）。MS12/MS13 单 hart NIC 异步数据面与恢复语义已验收：source133 IOC 底座、64 slot/16 descriptor 账本、C4 flush、quiet 经沉降相 2 s 逐字节冻结、arm/recheck/deferred owner 协议。
- Current-State Evidence（本轮新查，2026-10-15 工作树，未提交暂存面）：
  - 单 hart 网络入口链：entry.rs MS09 first-byte 分支（`k3` 且非 `k3-async-uart`/非 `k3-smp`）→ `kernel/src/platform/k3.rs` stage 运行器：stage1–3 console、stage4–5 APMU/MAC 身份 Gate（clk/rst/version 不过即 park）、stage6–7 pad/PHY 身份、stage8 30 s link 观察窗、stage9 AIA 注入、stage10 120 s 设备窗，然后 `k3_async_net::run_k3_async_net_service(st10)`（k3.rs:168）。design 裁决（design.md）：新模式不跑 stage8/9/10 观察矩阵，只复用真实 clock/pad/PHY/GMAC 初始化与必要失败 Gate；NIC 初始化拆出可返回的安装/启动步骤，保留旧 `-> !` 包装器；source60 在新网络生产模式也武装。
  - service 安装序（`k3_async_net.rs::run_k3_async_net_service`，约 1285–1380 行）：`arm_phy_source60`（当前仅 `k3-net-diagnostics` 构建启用）→ K3NetAdapter::new → `axnet::init_network`(唯一 NIC) → `axnet::start_rx_task_affinity(schedulable_cpu_mask())`（失败 park）→ 前台等 rx_snapshot lifecycle Active（5 s 上限）→ `owner_hart_tuple()` 观察行 → `start_stack_runner_affinity(schedulable_cpu_mask())`（失败 park）→ stack_snapshot 行；probe 构建接 `k3_net_diagnostics::run_diagnostics`，plain 构建 park。返回的 AxTaskRef handle 当前被丢弃（`Ok(_handle) => {}`）。
  - 共享 placement（`kernel/src/drivers/placement.rs`，MS08 D4，相对 5df918cd 未变）：`place_roles(schedulable: &[usize], anchor: usize) -> Option<RolePlacement>` fail-closed（空/无序/重复/越界返回 None）；角色序 UartRx=0/UartTx=1/NetOwner=2/NetRunner=3，role i → `schedulable[(anchor_pos+i)%len]`；anchor 不在集合中等价索引 0；容量唯一权威 `axconfig::plat::MAX_CPU_NUM`。16 个升序 schedulable、anchor=boot hart 0 时 net_owner=2、net_runner=3（X100 簇）；`place_role(...)` 可取单角色。
  - axnet 接缝：`start_rx_task_affinity(AxCpuMask) -> Result<AxTaskRef, StartError>`（async_rx.rs:3213）、`start_stack_runner_affinity`（stack_runner.rs:683）——既有 spawn 拒绝/重复启动 fail-closed 语义；`owner_hart_tuple()`（async_rx.rs:847）、`rx_snapshot`/`stack_snapshot`；`axtask::schedulable_cpu_mask()`（api.rs:147）为已发布 runqueue 位图，AxCpuMask 可由其构造升序 hart 列表。
  - 既有载荷（k3_async_net.rs）：`run_k3_async_net_session`（1091，四协议 echo 会话）；`k3_net_diagnostics::run_diagnostics`（容量：200 ms 显式回收 hold、complete-frame enqueue 至软件 slot Full 拒绝即两级 64/16 边界、C4、`poll_io`/`IoEvents` readiness 面）；matrix waiter `spawn_matrix_udp_waiter/spawn_matrix_listener_waiter`（2036/2060，`axtask::spawn` 无 affinity，新模式需换 affinity 变体）；quiet 判据=沉降相（claim 流静止 500 ms）后 2 s 计数逐字节冻结。
  - 构建面：Makefile `k3-async`/`k3-async-probe` SMP>1 拒绝（339–351），`k3-smp` 目标产物全隔离（TARGET_DIR=target/k3-smp、OUT_DIR=out-k3-smp）；root `k3-async-net = [k3, axplat-riscv64-k3/runtime-irq, starry-kernel/k3-async-net]`，kernel `k3-async-net = [k3, axdriver/dyn, axdriver/net, dep:axdriver_net, dep:axnet]`（无 paging）；root/kernel `k3-smp` 已存在（000 交付）。entry.rs k3-smp 分支（469）先于 MS09 轮询分支；`k3-smp-net` 蕴含 `k3-smp`，新分支必须排在其前，且 k3-smp 平台见证分支须补 `not(feature = "k3-smp-net")`。
  - host 入口：Cargo 包 `k3-async-net-host`（tests/{service_owner,adapter_contract,flush_contract,stack_socket,recovery_contract,recovery_socket}.rs；`service_owner_bounded_bidirectional_contract`、`arm_recheck_reports_pending_after_suppress_cycle`、`pool_and_descriptor_ledgers_conserve_at_16_and_refuse_17` 等）；placement 纯函数 host 入口在 `tests/k3-uart-plane-host-harness.rs`（`#[path]` include 真实 placement.rs）。
- Code and Critical Path:

| 入口/符号 | 本轮责任 |
|---|---|
| root/kernel Cargo、Makefile、`tools/starry-k3-smp-net.its`（新增） | 新 `k3-smp-net` feature 闭包、目标与 FIT；旧目标行为不变 |
| `kernel/src/entry.rs`、`kernel/src/platform/mod.rs` | k3-smp-net 分支先于 k3-smp/轮询分支；分支 cfg 排他 |
| `kernel/src/platform/k3_async_net.rs` | 初始化拆出可返回安装步骤（跳过 stage8–10 观察矩阵）、新 SMP service 入口（placement singleton、保存 handle、source60 武装、观察行）、SMP 载荷组装 |
| `kernel/src/drivers/placement.rs` | 仅消费（place_roles/place_role），不改公式与容量权威 |
| axnet `start_rx_task_affinity`/`start_stack_runner_affinity`/`owner_hart_tuple` | 仅消费，不改 axnet/axtask |
| `tests/k3-async-net-host`、`tests/k3-uart-plane-host-harness.rs` | 新接缝先行测试；既有 spawn 拒绝/重复启动/安装序/最后事件用例复核 GREEN |

**Implementation Guidance**

先在现有 host 入口把新接缝的先行测试写 RED（placement→singleton 组装、应用 hart 校验、重复启动拒绝复核），再接 Cargo/Make/entry（`make k3-smp-net SMP=16` 无目标即天然 RED），随后拆初始化步骤并组装 SMP service，最后组装载荷与 FIT。owner/runner 的 singleton mask 来自 place_roles 实际返回，不硬编码 2/3；应用载荷 hart 9 为批准的固定值，构建以 SMP=16 为前提并在入口校验 hart 9 在 schedulable 集合内（fail closed park）。板测前 agent 侧完成全部 host GREEN 与构建，停在真板能力边界。

**Task Contracts**

### T5: `k3-smp-net` 模式与 owner/runner singleton 接线

- Requirement/Scenario: R3/S08–S09（NIC 面）、R5/S15 前置、R8/S26。
- Depends on: 000 accepted。
- Targets: root/kernel Cargo 新 `k3-smp-net`；Makefile 新 `k3-smp-net`/`k3-smp-net-fit` 与 `tools/starry-k3-smp-net.its`；`kernel/src/entry.rs` 分支序；`kernel/src/platform/k3_async_net.rs` 初始化拆分与新 SMP 入口；`tests/k3-async-net-host` 与 `tests/k3-uart-plane-host-harness.rs` 先行测试。
- Current behavior: 无多 hart 网络模式；`k3-async`/`k3-async-probe` SMP>1 构建拒绝；service 将整个 schedulable mask 传给 owner/runner 且丢弃 handle；source60 arm 仅 probe 构建启用；entry.rs 无 k3-smp-net 排他。
- Required behavior: 新闭包只新增显式依赖（k3-smp 已有闭包 + kernel `k3-smp-net = [k3-smp, k3-async-net]` 一类组合，具体由 Act 按 000 闭包先例写最小闭包）；`make k3-smp-net SMP=16` 产出隔离目录与独立 FIT（与 000 同 RAM 布局：load/entry 0x180000000、FDT 0x181000000），SMP=1/>16 拒绝；`.gitignore` 以独立行补 `out-k3-smp-net/`（不得复现 000 的无换行拼接缺陷）。k3_async_net 初始化拆出可返回的安装/启动步骤：真实 clock/pad/PHY/GMAC 初始化与必要失败 Gate（APMU/MAC 身份、PHY 身份）保留，stage8/9/10 观察矩阵不进入新模式；旧 `run_k3_async_net_service/session` 与其 `-> !` stage 链逐字保留。新 SMP 入口：place_roles(升序 schedulable 列表, anchor=boot hart) 取 net_owner/net_runner 构造 singleton mask（fail-closed park），保存 `start_rx_task_affinity`/`start_stack_runner_affinity` 返回的 handle，安装序与观察行沿用单 hart 形态（Active 等待、owner_hart_tuple、stack_snapshot），`arm_phy_source60` 生产路径在新模式启用；应用载荷固定 hart 9（A100 簇）。
- Preserve: MS12/MS13 安装序与 arm/recheck/deferred owner 协议、ISR 无 MDIO、owner link 读消费 INSR latch、64/16 账本与 C4 语义；placement 公式与容量权威；旧单 hart 网络模式行为与 Makefile 拒绝；000 的 k3-smp 平台见证入口（新分支排他后不再被 k3-smp-net 触发）。
- Forbidden: 改 placement 公式、建第二 owner/runner 或第二 IPI owner、跑 stage8–10 观察矩阵、把 QEMU/Linux 结果当 K3 证据、动 UART/恢复/组合面、为证据新增身份机制。
- First test: `make k3-smp-net SMP=16` 未实现时 RED（No rule/接口缺失）；新纯接缝（如 schedulable 列表→owner/runner/app singleton 组装与校验）按 000 惯例 `#[path]` include 生产模块在 k3-async-net-host 或 uart-plane harness 先 RED；既有 `service_owner_bounded_bidirectional_contract`（spawn 拒绝/重复启动）、`arm_recheck_reports_pending_after_suppress_cycle`（安装序/最后事件）作为先行复核 GREEN。
- GREEN condition: 新接缝 host 用例 GREEN；受影响 host suite 全绿；`make k3-smp-net SMP=16` exit 0 产出 FIT；`make k3`、`make k3-async` 旧目标 exit 0 且形态不变；k3-smp-net 构建中新入口的 placement/观察行可达（实际 hart 判定归 T6 板测）。
- Verification: `cargo test -p k3-async-net-host`（轮末受影响 suite 各一次）；`rustc --test tests/k3-uart-plane-host-harness.rs` 入口若被扩展则同规则；`make k3-smp-net SMP=16`、`make k3`、`make k3-async`。已有 host GREEN 覆盖未变化时采信（公共规则 › 验证）。
- Stop when: place_roles/axnet 接缝不足以支撑安装、初始化拆分破坏旧 stage 链契约、placement 结果与板上 schedulable 事实矛盾；返回 Plan。

### T6: NIC 多 hart 板上专项

- Requirement/Scenario: R5/S15–S18、R3/S06/S09、R8/S27。
- Depends on: T5。
- Targets: `k3_async_net.rs` SMP 载荷组装（四协议会话、容量/C4、matrix waiter、quiet）；`k3-smp-net` FIT；操作者板测与 Act Response。
- Current behavior: 无 K3 多 hart 网络运行结论；单 hart 四协议/容量/C4/quiet 已在 MS12/MS13 板测成立。
- Required behavior: 在 `k3-smp-net` 入口组装专项载荷并一次板测覆盖：真实四协议（ping、UDP/TCP 全字节 echo）经 hart 9 应用与 placement owner/runner 跨簇交换；两级容量 Full→恢复与单次回收、wrap、ticketed C4；多 waiter 与 readiness（matrix waiter 沿用现有载荷，readiness 语义已有 host stack_socket GREEN 且未变化时按任务文本采信）；quiet 沉降相后 2 s 冻结→新流量恢复；所有应用类任务 affinity=hart 9 singleton。操作者会话不通过则停止；真实结果与账本通过后才填 Response。
- Preserve: payload 不判 PASS（操作者按串口裁决）；R68 RAM 启动路径与无持久写入边界；一次板测预算（首次失败按场景停止定向诊断，不盲试）。
- Forbidden: QEMU/主机结果替代板测、quiet 用周期轮询替代真实唤醒、新增判定协议/身份工具、把 configured mask 当实际执行。
- First test: 采信 T5 GREEN 与未变化的既有 host 结论作为先行观察；本 task 为集成板测，不新增镜像身份测试。
- GREEN condition: 下方 Acceptance 全部真实满足且无未执行/豁免项；Fail/超期停止不盲试到 PASS。
- Verification: 轮末 `cargo test -p k3-async-net-host` 全包一次（共享面变动时 axnet 三 feature 全量，否则采信）；`make k3-smp-net SMP=16` 生成待测 FIT。最短人工步骤（复用已验证 RAM Runbook 与 000 FIT 流程）：fatload 新 FIT → bootm → 直接观察 owner-active 行的 actual hart 与 placement 一致、hart 9 应用行、四协议/容量/C4/quiet 判定行；任何错误行停止；外部 RESET 回官方 Linux 并以一句话报告。
- Stop when: 专项首个目标失败、actual hart 与 affinity 不符、账本不闭合、外部 RESET 未回官方系统；blocked 交 Plan。

**Invariants**

1. owner/runner 唯一：`start_*_affinity` 失败即 park，无第二次启动；placement fail-closed 时 park，不虚构拓扑。
2. 安装序与最后事件语义沿用 MS12/MS13（arm/recheck/deferred owner；source60 arm 清 pending、owner link 读消费 latch、ISR 无 MDIO）。
3. owner/runner 与应用各自 singleton 在首次入队前提交，无受控或隐式迁移；跨簇唤醒不依赖目标周期 timer。
4. 旧 `run_k3_async_net_service/session`、stage 链 `-> !` 包装器、`k3-async`/`k3-async-probe` 单 hart 拒绝、`make k3` 形态保持。
5. 完成声明基于真实板测行为；configured mask/FDT/构建输出不替代实际执行。

**Non-goals**

UART 专项、恢复/reset/link-flap 矩阵（ISS02 判别）、30 min 组合长稳、性能矩阵、multiqueue/RSS、host 之外的判定工具。

**Acceptance**

| ID | 覆盖 | 直接结果 |
|---|---|---|
| A5 | R3/S08–S09/T5,T6 | owner/runner actual hart 与 place_roles 结果一致且互异；应用任务 actual hart=9（A100）；重复启动/非法 affinity 拒绝且无第二角色 |
| A6 | R5/S15/T6 | 四协议真实交换成立：ping 应答、UDP/TCP 全字节 echo 经跨簇应用↔owner/runner 路径 |
| A7 | R5/S16–S17/T6 | 两级容量 Full→恢复、单次回收、wrap、ticketed C4、多 waiter/readiness 判定行满足 |
| A8 | R5/S18、R8/S26–S27/T5,T6 | quiet 沉降相 2 s 冻结→新流量恢复；`make k3`/`k3-async` 旧构建不回归；外部 RESET 回官方 Linux |

**Verification**

| Requirement | Task | First Test | Status |
|---|---|---|---|
| R5 | T5,T6 | 新接缝 RED；`service_owner_bounded_bidirectional_contract`/`arm_recheck...` 复核 GREEN；native 四协议/容量/C4/quiet | Covered |
| R3（NIC 面） | T5,T6 | placement 单角色纯函数既有 GREEN；新 singleton 组装 RED→GREEN；板测 actual hart 观察 | Covered |
| R8 | T5,T6 | `make k3`/`k3-async` 旧目标复核；新 FIT 实际运行与外部 RESET | Covered |

（R3 的 UART 面与 R4 归 Iteration 002，R6 归 003，R7 归 004，不在本批次。）

**Gate 2 Readiness**

技术检查：本轮调查、契约、先行测试、真实入口、停止条件与 RTM 已闭合；无 TBD。Gate 2 PASS：用户 2026-10-15 批准（原话"批准"），Status 改为 `ready` 交 Act；真板操作由用户手动执行。

**Persisted Evidence**

- Mode: none。命令、退出码、板测判定行与操作者一句话报告写 Act Response足够。

**Risks and Notes**

- anchor=boot hart 0 为板上实测（000 会话）；契约以 `place_roles` 实际返回为准，不硬编码 2/3；若 boot hart 非 0，公式仍确定，观察行会暴露实际值。
- hart 9 为批准的固定应用 hart；SMP=16 构建保证其存在，入口仍校验 schedulable 成员资格后 fail closed。
- source60 在新生产模式武装是 design 裁决的扩展（当前仅 probe 构建启用）；arm 复用既有 `arm_phy_source60` 生产路径，不新写 arm 序。
- ISS02（插回不稳定）不影响本轮判定：稳态专项不含拔插/reset 交错（003 范围）；如板测中自发出现其症状，按 Stop when 停止并交 Plan，不现场盲修。
- ISS03 的纯自旋锁叙述不采信为换锁依据（change tasks 既有约束）；本轮不修。
- wall_time 平台后端在板可用（MS12 板测先例）；host 上 `wall_time()≡0` 的旧事实不适用于板测判定。

## Act Response

- Status: reported
- Date: 2026-10-15

### 实际改动

新增（全部为新增或新文件，既有单 hart 行为零删改，Preserve 达成）：

- `kernel/src/platform/k3_smp_net.rs`（新）：纯接缝 `place_smp_net_roles`/`SmpNetRoles`/`SmpNetRoleError`/`APP_HART=9`；以 `#[path]` 原位 include `drivers/placement.rs`（k3 网络形态排除 drivers 树，共享 policy 文件原位不动，design D5）。
- `kernel/src/platform/k3_async_net.rs`：
  - board 模块新增 `run_k3_smp_net_boot`（阶段 1–7 真实 clock/pad/PHY 门，跳过 stage8–10；MS12/MS13 安装序：route mask→unregister 133→arm_irq_access→arena/Gmac init→register 133→source60 生产武装→adapter→init_network 唯一 NIC）、`smp_fault`/`smp_singleton`、`SMP_NET_HANDLES`（owner/runner handle 保留）；placement→singleton `start_rx_task_affinity`/`start_stack_runner_affinity`，`owner-active`/`stack-active` 实际 hart 观察行（`owner_hart_tuple`/`runner_hart_tuple` 真实执行观察）；duplicate-start 拒绝见证；载荷 spawn（affinity=hart 9，`spawn_with_name_affinity`）与 `SMP_PAYLOAD_DONE` 监督。
  - k3_net_diagnostics 模块新增 SMP 载荷：`run_smp_net_payload`（容量/C4 复用 `run_capacity_flush_cycle` + 四协议 `smp_protocol_phase`（settle 500 ms→quiet 2 s→resume）+ `smp_waiter_phase` 双 matrix waiter（affinity 变体 `spawn_smp_udp_waiter`/`spawn_smp_listener_waiter`）+ `snapshot_line` 判定行）；`SMP_PAYLOAD_DONE`/`SMP_QUIET_NANOS=2s`。
- `kernel/src/entry.rs`：k3-smp-net 分支先于 k3-smp 分支；k3-smp 平台见证分支补 `not(feature = "k3-smp-net")`。
- `kernel/src/lib.rs`：`k3-async-net`+`smp` 互斥守卫开 `not(feature = "k3-smp-net")` 唯一例外。
- `Cargo.toml`（root）：`k3-smp-net = ["k3-smp", "k3-net-diagnostics", "starry-kernel/k3-smp-net"]`；`kernel/Cargo.toml`：`k3-smp-net = ["k3-smp", "k3-net-diagnostics"]`。
- `Makefile`：`k3-smp-net`（隔离 `target/k3-smp-net`/`.axconfig-k3-smp-net.toml`/`out-k3-smp-net`，SMP 2..=16 默认 16，IP=192.168.137.2 GW=192.168.137.1）与 `k3-smp-net-fit`。
- `tools/starry-k3-smp-net.its`（新）：与 k3-smp.its 同 load/entry/FDT 地址，payload 指向隔离 out 目录。
- `.gitignore`：补 `out-k3-smp-net/` 独立行；顺带修复 000 的无换行拼接（`out-k3-smp/` 此前实际未被忽略，拆出独立行）。
- `tests/k3-uart-plane-host-harness.rs`：新增 4 个 k3-smp-net 接缝用例（16 hart placement、anchor 内/外、app hart 9 缺失、非法集合 fail-closed）。

### 与计划的偏差

1. quiet 窗口 2 s：按 Cycle 契约/A8 新写 `SMP_QUIET_NANOS=2s`；单 hart probe 的 25 s 常量（2026-10-14 用户方向）逐字保留，两条路径互不影响。
2. FIT 产物名：design D1 写"产物名称 StarryOS_riscv64-k3-smp.fit；底座及各专项复用该形态"；Task Contract 只要求新 `.its`/`k3-smp-net-fit`。取独立产物名 `StarryOS_riscv64-k3-smp-net.fit`（沿 k3-rootfs 独立 FIT 先例），不覆盖底座镜像；FIT 形态（地址/字段）与 D1 一致，无身份字段。
3. 先行测试 RED 中 anchor 用例期望写错一次（anchor=5 实际在 16 hart 集合内，属测试错误非实现错误），修正用例后 GREEN；实现未因此改动。

### 自检发现、已修复与遗留 Minor

- 已修复：`dup_owner.err()` 移动后借用编译错（E0382），先取布尔再取 `err()`；`let mut drv` 构造块在编辑中短暂丢失，即时补回（最终 diff 零删改已复核）。
- 遗留 Minor 1：启动处 boot 任务 `app spawned hart=9`（early console）与载荷 `app begin`（ax_print）两个写者并发交错，产生一行乱码（内容可辨认，A5 事实完整）；不阻塞，留后续 Cycle 处理。
- 遗留 Minor 2：复用载荷函数保留 `[ms12]` 行前缀（来源真实性），新模式判定行用 `[ms14]` 前缀，混合前缀为有意设计。

### 验证（agent 侧，真实运行）

- RED：`make k3-smp-net SMP=16` → `No rule to make target`；harness 含新模块 → 编译失败（模块缺失）。
- `rustc --edition=2024 --test tests/k3-uart-plane-host-harness.rs`：**7/7 通过**（基线 3/3 + 新增 4/4）。
- `AX_IP=192.168.137.2 AX_GW=192.168.137.1 cargo test --manifest-path tests/k3-async-net-host/Cargo.toml`：**26/26 通过**（含 `service_owner_bounded_bidirectional_contract` spawn 拒绝/重复启动复核；首次无编译期 AX_IP/AX_GW 运行失败，与既有 Runbook 一致，非代码问题）。T6 改动未触及 host 包输入面（k3_dma/mac_probe/phy_probe/k3_gmac 未变），该结论覆盖未变化、采信。
- `make k3-smp-net SMP=16` exit 0（产出 `StarryOS_riscv64-k3-smp-net.fit`，kernel 0x180000000/FDT 0x181000000）；`make k3`、`make k3-async`、`make k3-async-probe` 全部 exit 0（旧目标形态不变）。

### 真板结果（操作者执行，两轮，2026-10-15）

判定行均来自串口原文（`StarryOS_riscv64-k3-smp-net.fit`，boot_hart=0、harts=16）：

- A5（R3/S08–S09）：`placement owner=2 runner=3 app=9 harts=16 anchor=0`；`owner-active hart=2 … placement_owner=2`、`stack-active hart=3 placement_runner=3`；`duplicate-start owner-rejected=true runner-rejected=true owner-err=Some(AlreadyStarted(Active)) runner-err=Some(AlreadyStarted)`；`app begin hart_actual=9 expect=9`（Run 1）。
- A6（R5/S15）：PC `ping -n 5 192.168.137.2` 发 5 收 5、0% 丢失（TTL=64，操作者粘贴原文）；串口 `proto udp ok bytes=8 from=192.168.137.1:52290/:58101`、`tcp ok bytes=8 closed=1`（两轮各一次，逐字节回显 + 主动关闭 FIN）。
- A7（R5/S16–S17）：`accepted=80 tx-slot-full=1 desc-inflight=16 slot-backlog=64`（64 软件槽 + 16 硬件描述符两级边界）→ `restored tx-slot=0 desc-inflight=0 buf-available=16`（单次回收）→ `flush pending` → `release-2 ok` → `flush c4 ok` → `auto-release=0`；多 waiter：Run 2 `waiter udp terminal=ok` + `listener terminal=ok`。readiness 的 poll/select/epoll 语义按既有 host `stack_socket` GREEN（未变化）采信，板上以 waiter 真实 I/O 完成见证。
- A8（R5/S18、R8/S26–S27）：Run 1 quiet 沉降相后 2 s 冻结——quiet-before/after 全部队列/栈/账本计数逐字节相同（irq-runs=164、ri=111、ti=45、task-poll=165、rounds=165、work=148），resume 后 ri 111→112（操作者新包唤醒）；`make k3`/`k3-async` 旧构建不回归（agent 侧）；外部 RESET 回官方 Linux（操作者确认）。
- 操作者裁决原话："认可，通过"。

### 真板观察项（留档，不阻塞验收）

1. Run 1 `waiter udp terminal=pending-timeout`：waiter 窗口内有 +9 cause 到达，最可能是 UDP 数据报在 `waiter begin` 行前发出（打到未绑定端口，栈回 port-unreachable，后绑定的 waiter 收不到）；Run 2 同载荷双 ok 闭环，readiness 路径成立。
2. Run 2 quiet 窗口内 ri +8（146→154）：冻结前提被对端流量污染（发包时机），计数如实记录到达 cause；Run 1 干净冻结已证明 2 s 冻结语义。
3. 启动行交错乱码一行（自检 Minor 1）。
4. `ti` 与 `abnormal` 计数逐行相等为复用载荷既有口径（本板 TBU 随 TI、良性分类），两轮一致，与单 hart 形态相同。

### Persisted Evidence

None required（Mode none）：全部判定行、PC 原文与操作者结论已完整写入本 Response，无低成本不可复现的现场。

### Experience Candidates

None：板测装载流程复用既有 Runbook `com260-uboot-usb-starryos-ram-boot`（仅文件名不同）；观察项 1–3 不构成缺陷台账准入（Run 2 闭环/操作者节奏/显示 Minor）。

### 未解决问题

无阻塞项。Iteration 002（UART 多 hart）依赖本轮 Plan Review accepted 后由 Plan 展开。

## Plan Review

- Review Result: accepted

Reviewed 2026-10-15。独立阅读 001 全部未暂存 diff（11 文件，+844/−14，其中 `k3_async_net.rs` +639/−0 纯新增）与三个新文件（`k3_smp_net.rs`、`tools/starry-k3-smp-net.its`、`.axconfig-k3-smp-net.toml`）；判定行与代码 format string 逐一对应；host 7/7、26/26 与构建 exit 0 按公共规则 › 验证采信（只读 git 检查确认工作区即 Act 结论产生后的同一代码面；host 包 `#[path]` 输入面为 k3_dma/mac_probe/phy_probe/k3_gmac 四模块，001 零改动，Act 的"覆盖未变化"采信声明成立）；真板结论按操作者串口原文与"认可，通过"裁决核对。未重复运行已采信验证。

**声明与代码一致性（独立检查通过）**

- T5：`k3_smp_net.rs` 纯接缝（`#[path]` 原位 include 共享 placement、`APP_HART=9`、四种 fail-closed 错误、角色值全部来自 `place_roles` 实际返回）；root/kernel `k3-smp-net` 最小闭包（kernel `k3-smp-net=[k3-smp, k3-net-diagnostics]`，经 `k3-net-diagnostics=[k3-async-net, axnet/queue-diagnostics]` 传递数据面）；Makefile `k3-smp-net` 与 `k3-smp` 目标同形态（SMP 2..=16 默认 16、TARGET_DIR/OUT_CONFIG/OUT_DIR 全隔离、MEM=6400M/LOG=off 一致、IP/GW 沿 k3-async 先例）且旧 `k3-async`/`k3-async-probe` SMP>1 拒绝逐字未动；entry.rs 分支序 k3-async-uart→k3-smp-net→k3-smp→first-byte，first-byte 经 `not(k3-smp)` 排他（k3-smp-net 蕴含 k3-smp）、k3-smp 平台见证分支补 `not(k3-smp-net)`；lib.rs `k3-async-net`+`smp` 互斥守卫仅开 `k3-smp-net` 例外；`.gitignore` 按 000 Finding 1 建议拆回 `tests/network_benchmark-host-asan` + `out-k3-smp/` 两行并新增 `out-k3-smp-net/` 独立行；`starry-k3-smp-net.its` 与 k3-smp.its 仅描述/载荷路径/配置名不同，load/entry 0x180000000 与 FDT 0x181000000 一致。
- T5 service：`run_k3_smp_net_boot` 只跑 stage1–7（无 stage8–10 观察矩阵）；安装序 route mask→unregister 133→arm_irq_access→arena/zero→Gmac::new_async→init_rx→enable_tx→register 133→source60 生产武装（失败仅降级 event-path 并打印 note，D6）→adapter→init_network 唯一 NIC；placement 由 `schedulable_cpu_mask()` 升序列表 + boot anchor 驱动，失败 `smp_fault` park；owner/runner singleton 启动失败即 park、handle 存入 `SMP_NET_HANDLES`；Active 等待 5 s 上限、Faulted/Unavailable 即停；`owner-active`/`stack-active` 行经 `owner_hart_tuple`/`runner_hart_tuple` 输出实际执行 hart；duplicate-start 见证双拒绝、任一被接受即 `smp_fault`；应用载荷 `spawn_with_name_affinity` singleton 提交（spawn 拒绝即 park）。
- T6：SMP waiter 与 MS13 `spawn_matrix_udp/listener_waiter` 会话体逐行等价（bind→poll_io(IN)→recv / bind→listen→accept），仅 spawn 换 affinity 变体并返回 Option；`SMP_QUIET_NANOS=2s` 新增、单 hart `QUIET_WINDOW_NANOS=25s` 逐字保留；容量/C4/echo/快照全部复用既有函数（`[ms12] probe` 前缀，Minor 2 有意设计）；`app begin hart_actual/expect` 与 hart 不符即 fault+DONE；载荷各阶段有界（PHASE_CAP 25 s），监督循环 yield 等待 `SMP_PAYLOAD_DONE`，结束 summary+park。
- A5–A8 判定行与代码输出格式一一对应（placement/owner-active/stack-active/duplicate-start/app-begin、`[ms12] probe` 容量/restored/release-2/flush c4/auto-release、`[ms14] net waiter … terminal=`、snapshot_line 计数行）；Invariants 1–5 在代码中成立；Forbidden 全部遵守（placement.rs/axnet/axtask 零改动、无 stage8–10、无第二角色、无身份机制——`SMP_PAYLOAD_DONE` 为功能监督标志，FIT crc32 为 mkimage 既有标准形态）；Evidence Mode none 被遵守（change 内无 evidence 目录）。

**Findings**

1. [Act 表面缺陷 / Minor，不阻塞] `run_k3_smp_net_boot` doc 注释尾句 "The fixed-hart payload assembly is T6; until then the entry parks after the runner observation line" 为 T5 阶段遗留，与本函数已组装 T6 载荷的现状不符（仅文档准确性，无行为影响）。
2. [新证据 / Minor，提交集卫生，用户决策] `.axconfig-k3-smp-net.toml`（本轮 OUT_CONFIG 产物）未被任何忽略规则覆盖：`/.axconfig.*` 要求 `axconfig` 后紧跟字面 `.`，仅匹配 `.axconfig.toml`；`git check-ignore` 对 `.axconfig-k3-smp.toml` 与 `.axconfig-k3-smp-net.toml` 均不命中（同时更正 000 Review Finding 4 中"忽略模式覆盖它"的表述）。本轮产物未被暂存（Act 未暂存任何文件），不污染提交集；是否补 `/.axconfig-*.toml` 类规则由用户提交前裁决。
3. [000 遗留事项确认 / 无 001 偏差] `tests/network_benchmark-host-asan`（531 KB ELF）仍在暂存集：001 的 `.gitignore` 修复已按 000 Finding 1 建议拆回独立行，但取消跟踪仍需提交前显式 `git restore --staged`（用户决策项，已在 000 留档）。
4. [Act Response 复核 / 无偏差] 观察项 1（Run 1 waiter `pending-timeout`）与 `wait_smp_waiter` 的 PHASE_CAP 有界等待语义一致（窗口内 +9 cause、UDP 最可能先于绑定到达未绑定端口，Run 2 双 ok 闭环）；Minor 1（启动行乱码）成因在代码中确认：boot 任务经 early console、载荷经 `ax_print` 两写者并发写 UART0。均如实留档，不阻塞。

**Acceptance Gaps**: None。A5–A8 全部满足且有真实板测记录与操作者裁决；上述 Minor findings 不阻塞任何 Acceptance，按公共规则不创建返工。

**Evidence**

- Diff 与新文件独立阅读（本 Review 上列检查项）；判定行格式核对到 `k3_async_net.rs` 的 format string。
- 采信来源：Act Response 验证节（uart-plane harness 7/7、k3-async-net-host 26/26、`make k3-smp-net SMP=16`/`make k3`/`make k3-async`/`make k3-async-probe` exit 0），覆盖未变化；真板两轮判定行与操作者原话"认可，通过"。

**Follow-up Decision**: 仅有非阻塞 Minor findings（过期注释、忽略规则缺口、000 暂存集遗留），A5–A8 全部真实满足，无实质问题或 Acceptance gap → `accepted`，不要求当前 Cycle 修复。Iteration Plan 不变。Findings 1–3 留提交前/后续 Cycle 处理，不构成返工条件。

**Next Cycle**: None

**Next Iteration**: `iterations/002-uart-multi-hart-user-interaction/000-initial.md`（Plan Context draft，待用户 Gate 2 批准后交 Act）
