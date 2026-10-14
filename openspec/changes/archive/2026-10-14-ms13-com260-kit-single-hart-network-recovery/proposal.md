# CoM260 Kit 单 hart 网络恢复语义

## Why

K3 单 hart 异步双向网络已验收，但运行时 link flap、设备 reset 与物理 DMA 停止尚无本板结论。多 hart 与组合长稳之前，需要闭合恢复时的 packet、descriptor、buffer 和 socket 生命周期，避免旧完成误归属、重复回收及设备继续访问已复用内存。

## What Changes

- 增加单 hart 网络恢复控制面，保持既有唯一 queue owner 与异步分层。
- 分别处理真实 PHY link down/up 和受控运行时 GMAC reset；link down 本身不证明 DMA 停止，也不自动等同于设备 reset。
- 以 queue epoch、link 状态和 socket 生命周期隔离新旧对象，覆盖 completion、ticketed flush 与等待者错误。
- 以目标控制器依据和真板可观察行为证明 DMA 停止；失败或无法确认时保留 backing、稳定报错并拒绝新提交。
- 增加恢复后双向交换、资源闭合及失败注入验证；不引入新的验证身份协议或专用判定层。

## Capabilities

### New Capabilities

- `k3-com260-kit-single-hart-network-recovery`：本板 link、运行时 reset、epoch 隔离、DMA 停止与恢复失败的资源保留契约。

### Modified Capabilities

- `k3-com260-kit-async-network-data-plane`：将“错误、超时与取消保留 DMA 所有权”中禁止无控制面自动reset的旧边界，与本次已批准的常驻有界恢复能力配对；初始化与取消的安全约束保持，受控恢复也必须先隔离并证明停止。此前外部RESET回原系统仍不算运行时恢复。

这是已批准恢复需求的必要语料一致性修改，不新增需求或简化验收：当前代码接通NetRecoveryControl后，既有owner会对可恢复completion/reclaim超时进入有界恢复；不可能同时保留“所有情形禁止自动reset”的旧描述。

## Impact

- 目标平台：CoM260 Kit 单 boot hart、RAM-only 临时启动；正常异步数据面、AIA 和 DMA/cache 契约沿用已验收边界。
- 已定位责任面：K3 GMAC driver/adapter、既有 queue recovery、stack/socket terminal 与板测入口；实际符号、调用边和当前行为见design，首轮实施依据在自包含契约。
- 预期外部行为：link down/reset 使旧连接终止；恢复后通过新建 socket 继续 I/O；不能确认 DMA 停止时维持稳定失败而不复用 backing。
- 不新增依赖、公共用户态 ABI、syscall或通用恢复框架；复用既有恢复trait、请求事件和socket terminal，最小共享接缝变化见design/tasks。

## 需求基线（已批准）

来源为 `.agents/docs/tasks.md` 的“目标板单 CPU/hart 恢复语义”条目。以下八项完整覆盖其范围，不裁剪验收条件。

| 编号 | 目标行为 |
|---|---|
| R1 | 唯一 owner 承载有界恢复；恢复窗口中的设备、软件和 timer 事件不造成第二 owner、旧状态复活或永久等待 |
| R2 | queue epoch 与 packet/descriptor/cookie/ticket ownership 配对；旧、重复、未知完成不污染当前对象，计数边界不复用活跃身份 |
| R3 | 真板 PHY link down/up 可重复观察；down 关闭旧 socket 并停止新提交，up 只允许新对象恢复，不自动 reset 或透明续传 |
| R4 | 运行时受控 reset 在隔离旧实例后证明 DMA 停止，再重建并重新武装；新实例有真实双向网络交换 |
| R5 | 停止、reset 或重建失败时稳定 fault；不确定 DMA 可达性时保留 backing，拒绝新提交并唤醒受影响等待者 |
| R6 | waiter 取消、未提交撤销和 device-owned 资源处置分层；各恢复阶段有有限 deadline，失败阶段与 owner 可诊断 |
| R7 | 旧 socket、listener 和 flush 得到稳定 terminal/error，先提交错误再 wake；恢复后的新 socket 不受旧错误污染 |
| R8 | host 生产路径故障注入与真板行为分别验证；重复恢复资源闭合，保留正常异步、平台隔离及安全外部恢复路径 |

## 场景缺口扫描与默认假设

以下默认假设已由用户集中确认。2026-10-13 用户原话：“批准，记得参考我们已有的工作和那些参考仓库信息经验”。批准覆盖 R1–R8、S01–S20 和 A1–A4，以及继续调查和详细规划；不构成实施授权或 Gate 2 计划批准。

- A1：不透明恢复旧 TCP/UDP/listener。link down 或 reset 关闭旧 socket，恢复后新建 socket；已经可能发送的 packet 不跨恢复自动重发。
- A2：link flap 与设备 reset 分开验收。link down 不自动触发整设备 reset；若实际硬件无法安全保持当前 queue，记录实质冲突并返回需求审计。
- A3：无法证明 DMA 停止、reset 失败或恢复不变量破坏时，采用稳定 fail-stop；本轮不承诺在该失败边界内无限自动重试或继续接单，操作者可沿既有外部复位路径恢复。
- A4：本轮只做单 hart 网络恢复，保留独立网络入口；不合并 UART/shell/rootfs，不增加 ping/wget 等用户态工具，不开展多 hart、长稳或性能优化。

已扫描 Happy、Sad、Edge，以及跨特性组合、多步转换、错误、超时、取消、单 hart IRQ/任务交错和兼容性。没有请求用户接受缺口或豁免。

## 场景草图

场景与增量规格同号；每项同时给出前置状态、动作、可观察结果和失败边界。

| 场景 | 前置状态 | 动作 | 结果 | 失败边界 |
|---|---|---|---|---|
| S01 正常 reset | Active、有旧 socket 与在途资源 | 请求受控 reset | 同一 owner 隔离、停止、重建，以新 epoch 恢复 I/O | 未证明停止不复用；任一步失败不发布 Active |
| S02 恢复事件交错 | owner 处于任一恢复阶段 | IRQ、软件事件或 timer 到达，含最后一次事件 | 按当前阶段消费或重查，不丢最后推进机会 | 不靠下一包、轮询 fallback 或第二 owner |
| S03 当前完成与 wrap | 当前 epoch 有合法在途 packet | terminal/reclaim 及 ring wrap | 单次完成、C4 和资源账本闭合 | 不把 peer delivery 当作 C4，常驻 RX 单独计 owner |
| S04 迟到旧完成 | 新 epoch 已开始且位置可能复用 | 注入旧 completion/cookie/ticket | 识别旧归属，不改新实例资源或 flush | 不能证明归属时隔离，不猜测当前完成 |
| S05 重复/未知完成及 epoch 边界 | 已完成对象或计数边界 | 再次完成、未知归属或推进 epoch | 不双重回收，不复用活跃身份；稳定诊断/错误 | 不静默 wrapping 或伪成功 |
| S06 初始与重复 link | owner 已启动，link 状态尚未提交或保持不变 | 读取真实 PHY，收到相同状态通知 | 提交初始状态；重复通知不重复关闭/重建 | PHY 读取失败不猜 link up，不忙等 |
| S07 link down | 已有 socket、软件待提交与 device-owned packet | 真实拔线并观察 down | 旧 socket 错误、新提交拒绝、未提交资源撤销 | device-owned 只经合法完成或已证明停止后处置 |
| S08 link up | down 已提交，queue 未因失败隔离 | 真实插线并观察 up | 新 socket 双向 I/O；旧 socket 仍终止 | 不自动 reset、旧连接续传或重发不确定 packet |
| S09 link/reset/completion 组合 | 在途 packet、flush 与旧 socket 存在 | link 变化、reset 请求和完成交错 | 控制面有确定顺序，旧完成只闭合正确 owner | link up 不能越过失败/尚未完成的恢复阶段 |
| S10 DMA 停止后复用 | 停止新提交且旧 queue 已隔离 | 执行控制器规定 stop/reset 并取得停止依据 | 停止成立后才释放/复用并重建 | 单纯读回、清 OWN、外部复位或 QEMU 不算本板证明 |
| S11 停止/reset 失败 | 设备可能仍持有 descriptor/buffer | 注入未停止或 reset 超时/错误 | Faulted、backing 保留、新提交拒绝、waiter 返回错误 | 不释放、复用、warning 后继续或虚报恢复 |
| S12 重建失败 | 旧 DMA 已证明停止，新实例未激活 | 队列/RX refill/通知准备失败 | 不发布半激活实例，资源归属可解释 | 可能重新被设备访问的资源仍保留 |
| S13 waiter 取消 | flush/socket/恢复请求等待中 | drop waiter 或取消等待 | 仅撤销等待，常驻 owner 继续必要资源闭合 | 不把取消当 DMA 停止或回滚设备 ownership |
| S14 pre-submit/submit 交错 | packet 正从软件队列转交 driver | 取消/关闭 epoch 与 submit 交错 | 只走未提交撤销或 device-owned 处置之一 | 同一 packet 不同时提交和回收，不跨 epoch 自动重发 |
| S15 分阶段 deadline | submit、terminal、reclaim 或恢复阶段未闭合 | 到期或 fatal | 有限返回对应阶段错误，资源保留符合可达性 | 同步 MMIO 调用不伪装可抢占，不能永久 Pending |
| S16 旧 waiter 错误一致 | 旧 TCP/UDP/listener、flush 和多个 readiness waiter 存在 | link/reset/fault 关闭生命周期 | 先发布错误再 wake；poll 与紧随 I/O 一致 | 不把取消/错误伪装 C4、EOF 成功或普通 Full |
| S17 新对象恢复 | 新 queue/link 可服务，旧对象已终止 | 新建 TCP/UDP/listener 并通信 | 新对象交换成立，旧对象仍稳定错误 | 不继承旧 registry 错误或使用旧队列状态 |
| S18 重复恢复与容量 | 前一恢复完成、有正常流量 | 再次 link flap/reset 后做 RX/TX、Full→恢复和 flush | 资源账本按角色闭合，容量不逐轮缩小 | 不以新分配掩盖泄漏，稳定 fault 不算成功恢复 |
| S19 正常兼容 | 不注入恢复故障 | 受影响 host/构建及正常异步网络验证 | 原正常路径、IRQ/quiet/C4 和平台隔离保持 | 既有失败单独报告，不把 host/QEMU 当真板结果 |
| S20 退出与外部回滚 | 板测结束、取消或 fail-stop | 停止新增操作并沿既有外部 RESET 恢复 | 有界报告阶段、保留不确定 backing，原系统可恢复 | 外部 RESET 不代替运行时 reset 或 DMA 停止验收 |

## 不做的内容

多 hart、任务迁移、组合长稳、性能优化、multiqueue/offload、第二设备 bring-up、真实存储与持久刷写，以及 UART/NIC/shell 合并入口和用户态网络工具。现有 Wireshark 永久豁免继续适用，不扩大为目标状态验证豁免。

## 来源与新鲜度

- 2026-10-13 当前会话已读取：`AGENTS.md`、`.agents/docs/SNAPSHOT.md`、`.agents/docs/tasks.md`、`.agents/memory/project-model.md`，相关参考与改进条目。
- 本阶段路线权威：`.agents/docs/tasks.md` 的目标板恢复语义条目；其前置单 hart 异步网络已于 2026-10-13 accepted 并归档。
- 正常数据面契约：`openspec/specs/k3-com260-kit-async-network-data-plane/spec.md` 全文。
- QEMU 恢复语义输入：`openspec/changes/archive/2026-09-02-ms07-qemu-single-hart-recovery-semantics/specs/qemu-single-hart-network-recovery/spec.md` 全文。只复用生命周期语义，不搬用 VirtIO reset/status 或历史证据工具。
- Git 现场：分支 `k3`，HEAD `d07285c0`；已有 `AGENTS.md` 修改与旧 Cycle 模板删除，保持原状。SNAPSHOT 中 revision/clean 记录和任务表 T22 等待文字已有滞后，不在本轮同步。
- `openspec/config.yaml` 中平台、分支和 `.claude` 描述已滞后；仅使用 schema/产物规则，当前事实以本会话权威文档与实际 Git 观察为准，不复制旧描述。
- 已完成本次生产路径、测试入口与参考资料的调查，具体Current-State Evidence见`design.md`；本次未执行产品构建、板上实验或行为测试。覆盖未变化的既有GREEN按实际只读diff核对后采信，新增恢复行为尚无运行结论。

## 阶段状态与门槛

| 阶段/步骤 | 状态 | 说明 |
|---|---|---|
| 需求提取与范围定位 | completed | R1–R8 对应现有已批准路线，不修改全局路线 |
| 场景扫描与草图 | completed | S01–S20，默认假设 A1–A4 已获确认 |
| 建立需求与增量规格 | completed | 新恢复能力及正常数据面错误边界的对应增量；需求批准见Gate1，不以产物存在代替批准 |
| Gate 1 | PASS | 2026-10-13 用户上述原话批准需求、场景、范围与默认假设；无豁免 |
| 实现调查、设计、任务分轮与首轮契约 | completed | 实际源码/参考调查、7项任务/3轮、RTM、首轮自包含契约与完整diff自检均已交付；详细计划获批后首轮转ready |
| Gate 2 | PASS | 2026-10-13 用户审计详细计划后原话“批准”；技术自检结论采信未变，无BLOCKED/WAIVED |
| 产品实施、行为测试、经验落账、全局状态同步 | SKIPPED | 本轮只授权规划，未调用 Act、Recorder 或 Maintainer |

Persisted Evidence 默认 `none`；普通命令结果保留在本提案/对话，后续如有必要按公共规则单独论证，不创建 Evidence 占位目录。

计划自检（2026-10-13）：两份delta spec、design/tasks和首轮契约生成后，`openspec validate ms13-com260-kit-single-hart-network-recovery --strict` exit0，输出 `Change 'ms13-com260-kit-single-hart-network-recovery' is valid`；`git diff --check` exit0。`openspec status --change ms13-com260-kit-single-hart-network-recovery` 显示4/4规划产物存在。完整阅读全部新增文档diff，已修正入口符号、probe前source133注册时机、旧cookie夹具、新旧语料配对与阶段状态；无需求简化、Missing或待Act决定的实质语义。结构通过不表示Gate2获批或产品验证通过。

2026-10-13 用户审计上述详细计划后原话“批准”，Gate2 PASS，首轮转ready；需求与执行契约正文未改变。下一步按用户实施指令调用Act。首轮最近能力边界为具体镜像准备完成后的真板停止/复用见证，未取得审核PASS不展开后轮。本次未调用Act、Recorder或Maintainer。
