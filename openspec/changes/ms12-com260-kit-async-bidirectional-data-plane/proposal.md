# MS12：CoM260 Kit 目标后端异步双向数据面

## Why

MS10 已验收 AIA/PHY 中断底座，MS11 已验收 GMAC DMA/cache 与轮询双向收发；目标板仍缺少由设备 IRQ 驱动的常驻 queue owner。MS12 将这些真板能力接入既有异步 queue/stack 契约，使 RX/TX、背压与 flush 在任务休眠和再次唤醒之间持续正确推进。

## What Changes

- 将目标 GMAC completion 通知、mask/ack/rearm/recheck 适配到 transport-neutral queue contract；硬中断不推进 descriptor 或协议栈。
- 由唯一常驻 queue service 推进双向硬件队列，以独立 budget、固定容量 packet slots 与软件空间事件提供背压和公平性。
- 保持完整帧提交、单次回收和 ticketed C4 flush；区分软件接受、DMA 完成、buffer 可复用与对端交付。
- 接通既有常驻 stack runner 和 socket 接口，真板验证 ARP/ICMP/UDP/TCP，数据进度不依赖调用者主动 poll 或固定周期轮询。
- 补齐直接观察 IRQ→wake→queue、Full→恢复、budget、drop、occupancy、flush 与 quiet 的验证；错误、deadline 与 waiter cancellation 沿用所有权安全边界。

## Capabilities

### New Capabilities

- `k3-com260-kit-async-network-data-plane`：CoM260 Kit 单 hart IRQ 驱动的双向 queue/stack 适配、背压、C4 flush、错误传播及真板验证。

### Modified Capabilities

None。MS10/MS11 与 QEMU 已验收域作为基线保留；新增目标后端能力不把 QEMU 结论扩写为真板结论。

## Impact

- 预期涉及 K3 GMAC 后端、AIA 设备路由/handler、K3 网络初始化和必要的 task/timer 接通，以及既有 `axdriver_net`/`axnet` 设备接入边界；具体入口、符号、调用者和接口兼容性由 Gate 1 后的实际代码调查闭合。
- 软件 packet slots、queue service、stack runner、socket readiness 与 flush 优先复用既有实现；不承诺新增驱动框架、依赖或公共 API。必须改变公共接口时，Phase 2 明确影响与迁移契约后才能交付计划。
- 增加驱动生产路径的 host 测试和最小真板内核测试载荷。QEMU 回归范围按实际受影响代码确定；未变化且有效的基线直接采信。
- 本 change 只写规划与实施产物；全局 tasks/M/R/I、Runbook、Issue 和收尾规格由相应 skill 另行维护。

## Approved Requirements List

| ID | 需求 | 场景 |
|---|---|---|
| R1 | 最小 IRQ、可重臂通知与无 lost wakeup，最后一次 completion 可推进 | S1–S4 |
| R2 | 唯一双向 queue owner、分阶段 budget、公平性与固定容量 RX/TX slots | S5–S8 |
| R3 | 完整 TX、typed 背压、单次 reclaim、ticketed C4 flush | S7、S9–S12 |
| R4 | 既有 stack/socket 契约适配、三源 wake、无主动 poll 依赖与 quiet | S13–S15 |
| R5 | 分阶段错误/deadline/cancel、稳定 fault 与 backing retention | S4、S10–S12、S16–S18 |
| R6 | 真板双向与容量边界的直接验证、可解释 telemetry | S1、S6–S10、S13–S15、S19 |
| R7 | 单 hart RAM-only、旧平台与板级基线兼容、参考资料用法对齐 | S18–S20 |

## 默认范围与场景缺口决策（一次集中审计）

以下默认假设及 R1–R7/S1–S20 已获用户批准（2026-10-06，原话：“批准”）。

1. **交付范围**：完整覆盖 MS12/T20–T21，包括目标后端 RX/TX、既有 queue/stack 接通与四协议真实交换；只做 raw IRQ/收发不计 MS12 完成。
2. **运行底座**：包含网络异步正确性不可缺少的最小单 hart task/timer 接通（若调查证明缺失）；不包含 MS17 的 UART RX、tty、shell 或用户态加载。若需要超出这一边界，Gate 2 阻塞并返回用户决策。
3. **验证环境**：沿 R68 的 CoM260 Kit RAM FIT 启动和 PC 直连拓扑；板测由操作者执行，host/build 与适用的自动检查由 agent 执行。Wireshark 按既有用户原话“Wireshark不做，后续也不用做”永久豁免，使用 PC 原生网络输出与板上目标状态。历史 ping 数量和 reset 描述豁免不自动延续为本 change 的通过条件；最短板测步骤与新验收数量在 Phase 2 明确。
4. **故障与压力边界**：覆盖确定性 wake 竞态、Full→恢复、最后完成、wrap、预算边界、空闲与稳定 fault；host 测试用真实生产路径和已有框架内的设备/时钟模拟，真板只用安全可达的压力和直接观察。设备 fatal/DMA stop 不做危险物理注错；不要求真板 runtime reset/link-flap 恢复、SMP、soak 或性能门槛。

Gate 1 已通过；需求没有简化或新增豁免。Gate 2 已于 2026-10-06 获用户批准，见 User Decision；本次路线调整不改变 MS12 的需求、任务契约或验收。

## Scenario Sketch

| 场景 | 类型/前置状态 | 动作 | 直接结果 | 失败边界 |
|---|---|---|---|---|
| S1 真正 DMA completion IRQ | Happy；ring 已发布，目标路由已配置 | 真实 RX/TX 产生设备事件 | cause 清前快照、claim、wake 与 descriptor terminal 可分别观察，后续事件仍推进 | 软件 IRQ、PHY IRQ 或 OWN 单独变化不得冒充 MAC DMA IRQ |
| S2 注册与重臂窗口 | Edge；owner 准备休眠 | completion/slot 事件分别落在 register 前、期间、rearm 后与 Pending 前 | 工作被 recheck 观察或 owner 再调度，帧只消费一次 | 不依赖下一包或轮询 timer 补救最后事件 |
| S3 mask 与锁竞争 | Edge；通知受抑制或共享状态被占用 | 最后 completion 到达，随后释放/重臂 | 已有工作确定性推进；不丢事件、不在 ISR 忙等锁 | 不照搬 try_lock 失败后返回空事件且无后续重查的参考策略 |
| S4 spurious/组合/fatal cause | Sad；无工作或多 cause 同时置位 | handler 分类并 ack 已消费 cause | 快照保留全部已观察位，分类可解释；无工作有界休眠，fatal 稳定传播 | IRQ hint 不等于 completion；无自激风暴或 warning 后继续无界服务 |
| S5 激活与 owner 切换 | Edge；初始化未完成或重复启动 | 准备 slots/handler/queue/stack，再激活；再次调用入口 | 每层至多一个 owner；激活失败无半切换或双重访问 | 激活后 fault 不创建 polling fallback/第二 owner |
| S6 budget 与双向 burst | Happy/Edge；两方向均有 backlog | 注入各阶段 budget−1、budget、budget+1 与持续双向负载 | 独立 budget 有界、exhaustion 与 yield 可见，两个方向均推进 | 单 poll 无界 drain、饥饿或用吞吐成功替代 budget 见证 |
| S7 slot/descriptor Full→恢复 | Edge；某级容量达到上限 | 再提交/接收，随后 consumer/reclaim 释放空间 | Full/occupancy/high-water 可见；合法 owner 保留，软件事件恢复推进 | 无完整帧接受却返回成功、覆盖设备 owner、动态扩容 |
| S8 RX 校验与 wrap | Sad/Edge；合法/坏帧、非法长度和索引回绕 | reap/refill，含容量不足窗口 | 完整帧或稳定分类 drop；单次回收、界限与 refill 成立 | 无越界、重复回收或把硬件压力丢帧隐瞒为零 drop |
| S9 TX 完整帧与回收 | Happy/Edge；有空闲或非法长度 | 提交并推进到 terminal/reclaim，跨环回绕 | 有效帧完整；超长明确拒绝；已接受对象账本闭合 | 无截断、buffer 泄漏、重复 reclaim；local C4 不等于 peer delivery |
| S10 flush 目标集合 | Edge；target 同时含 slot 与 device-owned packet | 调用 flush；继续接受新 packet；以可达/模拟完成顺序回收 | 所有 target 达 C4 才成功；新 ticket 不拖延旧 flush；空集合立即完成 | 最大完成 ticket 或全队列空不能替代 target 集合 |
| S11 flush waiter 取消/竞争 | Cancel；已有 flush waiter | drop waiter，或第二 waiter 注册 | 取消仅结束等待，packet 继续推进；竞争沿既有稳定 Busy 语义 | 不覆盖既有 waker、不清 OWN、不撤销已接受 packet |
| S12 flush timeout/fault | Timeout/Error；target 尚未完成 | deadline 到期或发生稳定 fault | 有界返回稳定阶段/owner 错误；等待者被唤醒 | timeout 不等于 DMA 停止，不释放/复用可能仍被设备访问的资源 |
| S13 四协议异步交换 | Happy；唯一 queue/stack 已激活 | PC ARP/ICMP/UDP/TCP 请求，板侧异步响应并关闭 TCP | PC 内容/关闭结果与板侧协议、queue 账本一致 | caller 主动 poll 或只跑 MS11 polling session 不计通过 |
| S14 device/software/timer wake | Edge；runner 休眠 | 收包/空间恢复、socket mutation、协议 timer 分别触发 | 对应层被唤醒；无额外主动网络调用仍推进 | timer 只推进有期限协议/错误，不用于周期轮询 completion |
| S15 quiet 与新流量恢复 | Edge；无立即工作且无到期协议 timer | 安静观察后发新流量 | queue/stack 不持续 poll、自唤醒；新流量经 IRQ 恢复 | 有限 quiet 窗不掩盖此前 fault；后台 PC 噪声须与无工作窗口区分 |
| S16 activation/error 分层 | Error；初始化或队列推进失败 | 地址/cache 前置冲突、submit/reclaim/fatal 错误 | 阶段、原始状态与 owner 可解释，拒绝新提交并传播稳定错误 | 不把 fatal 伪装 Full/WouldBlock；reset timeout 不继续初始化 |
| S17 提交前后取消与超时 | Cancel/Timeout；资源分别为软件待提交或 device-owned | 取消请求/等待或 deadline 到期 | 未提交可安全撤销；已提交保持 owner 直到合法 terminal/安全回收 | 不以 timeout/cancel 清 OWN 或自动 reset |
| S18 测试结束与恢复 | Sad；测试正常结束、失败或操作者取消 | 停止新增工作，有界结束；操作者外部复位 | backing 安全保留，输出实际阶段；确认回原系统 | 不把 park/reset 说成已证明 DMA quiesce 或运行时恢复 |
| S19 可解释观测与直接判定 | Edge；容量/错误/wake 行为发生 | 读取产品侧 snapshot/原生测试输出 | submit/terminal/reclaim、drop、occupancy、budget、wake、flush 结果能判断目标行为 | 不新增身份协议、validator、audit/capture 工具或工具专用测试 |
| S20 兼容与参考使用 | Compatibility；QEMU/D1/VF2 与已验收 K3 基线 | 在真实影响范围构建/测试；对齐已采信参考用法 | 平台选择隔离，既有契约保留；pre-existing 失败单独报告 | 历史参考状态不替代当前代码；本板语义优先于冲突的通用假设 |

扫描覆盖 Happy/Sad/Edge、错误、超时、取消、单 hart IRQ/task 交错与多平台兼容。没有主动裁剪场景；精确 budget、容量接缝、deadline、测试命令和安全板测动作属于 Phase 2 调查与设计，必须在 Gate 2 前明确。

## Non-goals

真板 runtime reset/link-flap 恢复（MS13）、DMA quiesce/stop 资格、SMP/affinity 迁移、长稳（MS14）、性能优化（拟登记 I19）、offload/zero-copy/multiqueue、shell/UART RX/存储、PCI、I17 独立库抽离、I18 通用驱动框架、第二 executor、持久固件/分区写入。link 丢失后的有界错误属于本轮安全边界，自动恢复不属于本轮。

## Sources and Freshness

- 当前描述与路线：`.agents/docs/SNAPSHOT.md`、`.agents/docs/tasks.md`（2026-10-06）；M36/M41/M42 与 I06/I17/I18 已读取，按公共规则信息路由引用，不复制约束正文。
- 已验收基线：`openspec/specs/k3-com260-kit-device-irq-baseline/spec.md`、`openspec/specs/k3-com260-kit-polling-network-baseline/spec.md`；MS11 最终 `iterations/002-protocol-exchange/000-initial.md` 的 Act Response/accepted Review（2026-10-06）。已知 TX tail 排他 next-slot 与 RX doorbell/OWN 用法延续，禁止用通用模型重置这些本板裁决。
- 复用目标契约：`openspec/specs/qemu-async-rx-queue-baseline/spec.md`、`qemu-bounded-bidirectional-device-data-plane/spec.md`、`qemu-application-visible-async-network-stack/spec.md`。其中历史 Evidence/identity/validator 条款与当前公共规则冲突的部分不沿用；只复用目标行为。
- R66：`.agents/analysis/k3-reference-repository-information-value.md`（2026-09-29），提供 MS12–MS14 资料路线；其中旧项目状态由当前 SNAPSHOT/MS10/MS11 替代。
- R61/R72：`/home/daivy/projects/serial/work/k3/docs/index.md`、`docs/network/k3-gmac-dma-irq.md`（2026-09 观察）与 `openspec/specs/k3-com260-gmac-network-baseline/spec.md` 已读。IRQ try_lock 的未闭合窗口是设计反例，不宣称本仓已发生相同故障。
- R71：`/home/daivy/projects/serial/work/k3/.claude/analysis/rt-async-amp-k3-drivers.md`（2026-09-08）已读，提供 GMAC queue/IRQ、AIA 和 runtime 资料入口；旧 StarryOS 分支与符号只作检索线索。
- R68：`.agents/runbooks/com260-uboot-usb-starryos-ram-boot.md`（2026-10-06 实跑）作为 RAM 启动与回滚操作入口；新的异步板测步骤须在 Cycle 自包含，不能要求 Act 沿引用链重建。
- Gate 1 后已直接读取 R69 的 `others/Rt-Async-AMP/tgoskits/drivers/ax-driver/src/net/k3_gmac/` IRQ/queue/regs/desc、AIA 接缝，以及本仓 runtime/traits/queue/stack。资料实际使用与当前基线见 design；具体寄存器用法按 M42 默认采信并留来源。
- `openspec/config.yaml` 的 `net-k3`/`.claude/docs/SNAPSHOT.md` 是过时上下文；本 change 以已读当前权威文档为准，不在本任务中修订配置。

## Phase and Gate Status

| Phase/Step | 状态 | 依据/下一边界 |
|---|---|---|
| Phase 1 需求提取与 BDD 缺口扫描 | completed | R1–R7、S1–S20 和四项默认假设已形成 |
| Phase 1 proposal / delta specs | completed | 需求与默认范围已批准，未裁剪需求 |
| Gate 1 | PASS | 2026-10-06 用户原话：“批准”；批准所提交的默认范围与 R1–R7/S1–S20 |
| Phase 2 实际代码调查/基线/设计 | completed | design记录实码接缝、参考用法与普通axnet编译失败；必要修复已规划 |
| Phase 2 RTM / Task Contracts / Iteration / Cycle | completed | 7项任务分3轮，首轮自包含 Cycle 已获批并为 ready；后轮只保留Map |
| Phase 2 完整性与结构审查 | completed | R1–R7均Covered，无简化/遗漏；strict validation与4/4产物检查通过 |
| Gate 2 | PASS | 2026-10-06 用户原话“批准当前的工作”；设计、三轮 Map 与首轮 Cycle 获批，执行契约未改写 |
| 本次路线/I/全局状态同步 | BLOCKED | 用户已授权；.agents 写入被环境自动审批拒绝，待应用 `/tmp/starryos-ms14-roadmap-update.patch` |
| 产品代码实施、Recorder、归档 | SKIPPED | 当前指令要求修改规划文档，未执行产品代码或板测；不自动进入 Act |

Persisted Evidence：本次不创建 Evidence；后续默认 `none`，除非具体 Acceptance 满足公共规则白名单和必要性。

## User Decision

### 当前执行顺序（2026-10-06）

用户最新原话：“同意保留当前12计划但是要做适当调整，现在请你给出ms17的规划吧，我们先做这个”。MS12 的 R1–R7/S1–S20、design 与三轮 Map 保留；当前原首轮暂停排期，不按原 ready 契约直接开始实施。先由 `ms17-com260-kit-async-uart-interactive-shell` 规划并验收共用 timer/IRQ/task 底座。

MS17 尚未实施，不提前把其计划当作当前基线，也不提前删除 T1 测试/验收。恢复 MS12 前由 Plan 阅读实际代码和 MS17 accepted 结果：未变化范围直接复用；timer/feature/IRQ 的目标位置、当前行为或测试见证变化时，按 replan 规则生成同一 Iteration 的后继自包含契约，保留原 Cycle 不改写。网络 adapter、queue、flush、stack/socket 和最终四协议要求保持完整。当前调整是执行顺序和恢复条件，不是既有 Acceptance 简化。

### 前次批准记录

2026-10-06 用户原话：“批准”。Gate 1 PASS，进入 Phase 2；该次批准仅覆盖需求。

2026-10-06 用户原话：“批准当前的工作，另外调整一下tasks里面的ms规划，我们把ms14当作最后的ms，优先把12、13、17做完，然后进入ms14，这样k3板子上的多hart网卡和串口的异步化就做完了，性能优化退化到I文档，开始这个更改吧给你豁免”。Gate 2 PASS：接受现有 MS12 设计、三轮 Map 和首轮 Cycle；仅更新批准状态，Task Contracts、Acceptance 和 Verification 不变。另授权路线重排、MS14 纳入 UART 多 hart 验收、性能优化退出执行路线并登记 I。该豁免不代替测试或硬件证据；本次执行范围为文档调整，不实施产品代码。

## Phase 1 Validation（2026-10-06，需求批准前的历史检查）

- `openspec validate ms12-com260-kit-async-bidirectional-data-plane --strict --no-interactive`：`Change 'ms12-com260-kit-async-bidirectional-data-plane' is valid`，exit 0。只证明当前 proposal/specs 结构有效，不代表需求获批或执行就绪。
- `openspec status --change ms12-com260-kit-async-bidirectional-data-plane`：`Progress: 2/4 artifacts complete`；proposal/specs 已生成，design 未生成，tasks 依赖 design；exit 0。符合 Gate 1 前的边界，尚无 Iteration/Cycle、Task Contract 或执行 RTM。
- `git status --short`：只列新 change 目录为未跟踪，exit 0；未修改产品代码、全局状态或参考仓库。
- 人工检查：R1–R7 与 S1–S20 均在 delta specs 对应行为中覆盖；默认假设保持待批准，未接受任何新缺口。所有当前阶段步骤与等待原因见 Phase and Gate Status。

## Plan Validation（2026-10-06）

- `openspec validate ms12-com260-kit-async-bidirectional-data-plane --strict --no-interactive`：`Change 'ms12-com260-kit-async-bidirectional-data-plane' is valid`，exit0。
- `openspec status --change ms12-com260-kit-async-bidirectional-data-plane`：`Progress: 4/4 artifacts complete`，proposal/specs/design/tasks均存在，exit0。结构完成不等于Gate2用户批准或实施完成。
- 当前真实host结果：普通axnet E0425/exit101；diagnostics事件窗口3项通过/exit0。定位、命令与必要修复契约在design与首轮Cycle，未修改产品代码。
- 结构审查：7项未开始任务各归一轮，依赖有序；只有 `iterations/000-initial/000-initial.md`，Plan Context=draft、Act=pending、Review=pending；Evidence=none。RTM全部Covered，没有TBD/TODO、未批准简化或第四轮预创建。
- `git diff --check` exit0；工作区仅本规划目录未跟踪。未修改全局状态、参考仓库或工程经验产物。
- 未跟踪文档补用原生 `git diff --no-index --check -- /dev/null <file>`逐一检查六个Markdown文件：均无whitespace诊断，exit1为no-index检测到新增内容，不作为测试失败；`.openspec.yaml`为CLI生成的spec-driven元数据。

## Plan Approval and Roadmap Update（2026-10-06）

- completed / Gate 2 PASS：用户已批准现有计划，首轮 Cycle 为 ready；7 项实施任务仍未开始，Act Response 与 Plan Review 均 pending，没有后继 Cycle 或 Evidence 目录。
- completed：按用户指令准备 MS12→MS13→MS17→MS14 路线补丁，MS14 依赖 MS13+MS17；UART 专项与组合多 hart 验收纳入最终阶段。MS15/T25 退出执行路线，优化正文转为拟登记 I19；MS18 独立存储规划保留在本次交付之外。最大现有 I 编号为 I18，未复用编号；I19 与 I16 的设施补全范围通过引用区分。
- completed：审阅路线补丁完整 diff；依赖无环，历史已完成阶段保持，MS12 需求、Task Contracts、Acceptance 与 Verification 未改变。MS14 保留设备专项故障边界，可在同一最终阶段内分 change/Iteration 验证；没有把性能门槛或未验证 UART 能力写为完成。
- BLOCKED / 全局写入：编辑工具两次拒绝 `.agents/docs/tasks.md` 的写入，原生错误为 `patch rejected: writing outside of the project; rejected by user approval settings`；未尝试绕过权限。恢复条件为具有写权限的操作者应用 `/tmp/starryos-ms14-roadmap-update.patch`，或环境解除 `.agents` 写入限制后明确要求继续。补丁包括 tasks、improvements、SNAPSHOT 三份文档，仓库原文件未变。
- SKIPPED：产品实施、板测、Recorder、归档；本次只执行获批的文档调整，不增加实施授权或测试豁免。
- 验证：`openspec validate ms12-com260-kit-async-bidirectional-data-plane --strict --no-interactive` → `Change 'ms12-com260-kit-async-bidirectional-data-plane' is valid`，exit0；`openspec status --change ms12-com260-kit-async-bidirectional-data-plane` → `Progress: 4/4 artifacts complete`，exit0。只证明规划结构，不代表产品已实施。
- 补丁预检查：`git apply --check --whitespace=error /tmp/starryos-ms14-roadmap-update.patch`，无诊断，exit0；证明补丁匹配当前文件且无空白错误，不表示已应用。
