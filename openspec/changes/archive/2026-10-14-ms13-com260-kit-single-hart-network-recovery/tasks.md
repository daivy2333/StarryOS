# 单 hart 网络恢复任务与分轮

需求、A1–A4和S01–S20已于2026-10-13获批；用户随后以原话“批准”接受详细计划，Gate2 PASS，首轮执行契约ready。2026-10-13 Act 执行首轮：T1（1.1）、T2（1.2）、T3（1.3）全部完成并过 Gate3/Gate4；A3 真板会话由操作者执行、Act 审核 PASS（板测原话与判定行见 Act Response）。同日 Plan Review：accepted（发现与证据见该 Cycle Plan Review）；Iteration 001 已展开并于同日获批（Gate 2 PASS，用户原话“批准”），执行契约 ready。同日 Act 执行 T4（2.1）并完成 T5（2.2）agent 侧：RED→GREEN、axnet 三面与两个 host 包全量、5 个构建门全过（见 001 Act Response）；T5 的 QEMU 恢复回归（A3）与 K3 owner/link 板测（A4）按 R44/能力边界交操作者，会话审核 PASS 前保持未勾选。A4 板测三轮发现并修复 2 个跨缺陷（phy60 注册、移交路由），仍不收敛（ping 100% 丢、payload 卡死于首个 timer 依赖点），Act 已按 Gate 5 阻塞交接（Response → blocked）。同日 Plan Review 裁决 rework-required：移交路由缺陷归属本 Iteration（Iteration 000 按其书面 A3 范围维持 accepted、不重开）、probe session2 零 TI 为设计内行为（不开 Iteration 000 rework）、timer 不作跨 MS17 前置（sie.STIE 与 IMSIC 路径分离，历史证据显示断流边界在移交窗口）；已创建 `001-rework.md`（R1 判别插桩→R2 定点修复→R3 A4 重跑→R4 A3 回归与收尾镜像）；同日用户原话“批准”，Gate 2 PASS，执行契约 ready。2026-10-13/14 Act 执行 rework 轮：R1 判别数据推翻父轮静态归因（真正关闭点为 probe 第 2 步 quiesce，timer 全程存活），R2 修复为移交步重开路由单行改动；板测四轮（判别/修复确认/S07 有界失败/S06+S08 尝试）后 aia-traps 恢复递增、UDP/TCP echo 与 quiet/resume 全达、NotConnected 有界终态、source-60 端到端、lifecycle=2/owner=1 保持。2026-10-14 Plan Review：accepted——A3 QEMU 用户豁免（原话“qemu不重要可以不做，直接填写回复吧”）、A4 残余（down→up 上升沿与插回新会话）用户接受（原话“我们已经验证功能正常了，不纠结阻塞在这里，如果没有大问题就填写回复吧”）并移交 T7 板上矩阵；Iteration 001 完成，Iteration 002 已展开；同日用户原话“批准”，Gate 2 PASS，执行契约 ready。同日 Act 执行 002 首轮（T6 host 测试面 + T7 注入接缝/矩阵编排 + 板测）并上报 U1/U2/U3（见 002 Act Response）。2026-10-14 同日 002 Plan Review：**rework-required**——U1 定位为 host 尾相探针循环不驱动 fake TX 完成（gdb 硬证据：target=21 为恢复后新会话 FIN 票、DeviceOwned 在途，产品 flush 判定链正确）；U2 定位为 payload `wait_lifecycle` 陈旧采样缺 left-Active 护栏（确定性，非竞态；产品门行为正确），附带发现门清除晚于 lifecycle 提交的 µs 抢占窗口（sched-rr 在案）需次序加固；U3 定位为 `pad35_edge_setup` 仅武装 `PAD_EDGE_FALL`（插回上升沿结构性不可达，001 移交项不可达成），串口停摆为其落7，残余的 runner 自醒风暴+自旋锁饿死登记为范围外 Issue 候选。已创建 `001-rework.md`（T6-R1 尾相驱动→T7-R2 护栏+门次序→T7-R3 双边沿武装→T7-R4 真板矩阵重跑）；2026-10-14 用户原话“批准直接实施，翻转状态，真板操作交由我手动运行”——Gate 2 PASS，Status ready，Act 执行 R1–R3，T7-R4 板测交操作者。同日 Act 执行 rework：R1 尾相驱动修复后 flush 真结算（U1 机制生效），但测试首次执行到继承的最终断言 `fallback_poll==0`（父轮从未执行过）确定性失败——根因为 MS13 恢复状态首次流入 stack runner 既有非 Active catch-all 臂（stack_runner.rs:569-576，本 change 未触碰该文件），恢复窗口空闲轮回退计数 1；无已验收 requirement 约束该计数，断言语义裁决交 Plan（候选：Active 尾相 baseline 断言 / 裁定产品缺陷），Act Response → **blocked**。R2 双修复（wait_lifecycle left 护栏 + 门清除入 guard 先于 commit）、R3 双边沿武装均已交付：axnet 三面 486/516/519 全绿（行为保持）、host 定向面全绿、phy harness 32/32（RED→GREEN）、k3-gmac harness 37/37（phy_probe 为其编译输入，覆盖面变化后重跑）、构建门 k3-async/k3-async-probe exit 0。T6/T7 保持未勾选（GREEN 条件未全满足）。其后两项 Blocker 按用户原话"这里我们不阻塞了，按你推荐的来，我们修复后重新进行真板测试"以推荐 (a) 解除，真板三轮取得 U2 闭合、上升沿可达与插回后 new-2 真实会话交换；用户收口原话"算了，这些问题都不重要，取消相关调试语句和更改，我们直接填写回复，本轮change就做这么多"止步残余。2026-10-14 001-rework Plan Review：**accepted**（A1-R/A2-R 全量满足；A3-R 折价收口——插回会话交换实质已板上见证；豁免风险与"不得表述为已板上验证"边界记录在该 Review Acceptance Gaps）。T6/T7 勾选完成，Iteration 002 完成，Next Cycle/Next Iteration 均为 None，change 待收尾。

## 1. 停止、重建与 adapter 资源安全

- [x] 1.1 **T1**：在 `kernel/src/platform/k3_gmac/mod.rs` 建立有界runtime stop/SWR/rebuild微步，复用当前寄存器、cache、16-slot与tail用法；先在 `tests/k3-gmac-host-harness.rs` 为“reset未确认不改环、延迟/超时保留backing、同arena原地重建与失效状态拒绝提交”建立RED，再GREEN。保持同步启动路径、polling模式与ArenaGuard既有保留规则；rustc生产harness和MS11受影响测试通过后自检。
- [x] 1.2 **T2**（依赖T1）：在 `k3_gmac/adapter.rs` 接通既有 `NetRecoveryControl`，以checked QueueEpoch绑定cookie/16 owner表，准确报告32-unit OwnerSummary与独立TX ledger；仅停止已确认后结清旧carrier，失败保留。`tests/k3-async-net-host/tests/recovery_contract.rs` 与现有fake补所需设备行为，先RED再验证旧hint/旧cookie、新旧slot复用、epoch边界、重复begin、reset/reinit失败及Drop；完整既有host包保持GREEN。
- [x] 1.3 **T3**（依赖T1/T2）：在现有 `k3_async_net.rs::board::run_k3_async_net_service` 的probe分支、创建adapter和Service之前接线底层原生停止/重建见证；复用同一Gmac/同一arena的T1生产实现及现有真实ARP会话。按首轮契约运行真实流量→stop/SWR→保留内存观察→原地重建→再次真实收发；随后才把同一driver移交adapter，观察与Service绝不并行。只改现有probe行为，不新增CLI/判定器/镜像身份字段。代码接线采信T1/T2先行GREEN和未变的启动基线；构建后到真板能力边界交操作者，不在无板测时勾选完成。

## 2. 常驻恢复 owner 与真实 PHY 控制面

- [x] 2.1 **T4**（依赖首轮accepted）：在 `crates/axnet/src/{lib,async_rx,service,router}.rs` 与 `device/{mod,ethernet}.rs` 完成最小共享接缝：reset请求整组门控迁入queue-diagnostics；link Again保留pending并安排1ms one-shot retry，Fault稳定隔离；恢复后重新读取link且在up确认前不接单；关闭旧socketepoch时discard旧RX slot与Router rx/tx缓冲，并维持取消/flush/wake次序。先在axnet既有测试中RED覆盖queue-only请求可达、重复请求Busy/自然恢复吸收、Again不busy wake、link失败门控、旧buffer不交付新socket及reset/link交错，再跑受影响子集。第一轮不触碰本task。（completed 2026-10-13，RED/GREEN 与全量见 001 Act Response）
- [x] 2.2 **T5**（依赖T4）：在 `k3_gmac` 的runtime PHY状态、adapter `read_link_status` 与 `k3_async_net` 的source60接线中复用既有MFPR35/INSR/BMSR/MDIO字段和真实经验：最小GPIO handler留snapshot/hold/config提示；同一owner分步MDIO、COR先快照、page恢复、CLEAR释放后重查和状态去重。先在生产host夹具RED覆盖初始up/down、重复/回声、MDIO busy/超时、hold期间最后变化及速率/双工应用；再GREEN。真板由既有独立probe入口实际拔插、受控reset及新流量验证；本轮同时完成受影响QEMU真实恢复回归，审核PASS后才展开最后一轮。（completed 2026-10-14 经 001-rework：host 侧 + source60 全部交付；A4 板测四轮闭合（A3 QEMU 与 down→up 上升沿按用户原话豁免/移交 T7，见 001-rework Plan Review）；Iteration 001 Review accepted）

## 3. Socket 组合与真板最终验收

- [x] 3.1 **T6**（依赖第二轮accepted）：扩展 `tests/k3-async-net-host/tests/recovery_socket.rs` 与现有payload/FakeWire，先RED定义生产adapter+Service+SocketOps的旧TCP/UDP/listener与pending flush错误、多waiter、cancel/submit、queued RX隔离及新对象I/O；必要的产品修复限于T4/T5已确定接缝，不另建socket框架。宿主以显式fake事件/时钟步进触达真实公共入口，不宣称真实timer或DMA；真实计时链由T7验证。异常需要新设计时返回Plan，不能借最终测试扩范围。（completed 2026-10-14 经 001-rework：R1 尾相循环驱动设备半、flush target=21 真结算；继承断言 fallback_poll 按 Blocker 1 裁决改恢复后 Active 尾相 baseline；recovery_socket 全过 + host 全包全绿，Gate3/Gate4 过。板上真计时链归 T7。）
- [x] 3.2 **T7**（依赖T6）：在现有 `k3_async_net.rs::k3_net_diagnostics`、adapter的诊断故障注入接缝与既有网络probe模式提供有界恢复会话，执行真板reset前后旧/新socket、物理link flap、reset/link/flush交错、连续两轮成功恢复后的容量/C4和四协议；固定末尾fail-stop场景暂缓reset确认step的设备读取，沿原绝对deadline进入“停止未确认”错误，再观察不再提交、不再改环、backing保留。该注入不改MMIO原值、不伪造SWR成功、不声称硬件自然卡死；普通模式不注入，不新增控制协议/专用CLI参数。宿主delayed/stuck模型保持真实区分“请求没发出”和“发出未确认”，不能简单跳过SWR写却把原本0的读值当stuck。直接记录原生输出、PC实际字节和错误，不用专用validator。板测未完成或未审核PASS时保持未完成；最后完整diff自检后仅填写Act Response。（completed 2026-10-14 经 001-rework：agent 侧全部交付——T7-R2 双修复（left 护栏+门清除入 guard，"committed⟹门开"调度无关）、T7-R3 双边沿武装、Blocker 2 INTS/INTN ack（arm 清 INSR pending + `consume_link_cause` 有界消费，RED→GREEN 9/9）、注入接缝/矩阵编排/构建门 exit 0。板测三轮：U2 板上闭合（i=0 accepted+81 帧两级边界+flush c4 ok+auto-release=0，三轮一致）、R1/R2 全链无回归、上升沿首次可达（events 3→5）+插回后 new-2 真实逐字节会话交换（001 移交残余实质取得）。容量/C4 第 2 轮、fail-stop 末场景与矩阵行形态 new-2 按用户收口原话豁免，风险与边界见 001-rework Plan Review Acceptance Gaps；插回后 link 不稳定三候选留档 Issue 候选。001-rework Review accepted。）

## Iteration Plan

### Iteration 000：停止与原地重建底层

- Tasks：T1–T3；T2依赖T1，T3依赖T1/T2。
- Depends on：已验收单hart异步网络基线；无新前置验证。
- Stable baseline：SWR未确认不复用；成功后同18页arena恢复RX/TX，adapter的epoch和carrier账本可用于后续resident恢复。
- Verification boundary：Gmac原生harness、生产adapterhost包与MS11受影响范围；一次真板stop/保留/原地重建会话。昂贵预算：本轮末一次完整k3-async-net-host包、一次MS11包及一次真板会话；单项RED/GREEN和定向故障测试不算额外全量预算。
- Diagnostic boundary：controller stop/SWR、cache/arena、ring重建、adapter carrier/cookie；不承担PHY事件投递或socket错误整合。
- Non-goals：共享请求门控、runtime source60、socket最终验收、多hart、性能。
- Balance audit：core单独缺少trait资源闭合，adapter单独缺少物理停止结果；T1–T3共同给出可复用安全底层。PHY/runtime owner和socket具有独立故障域，后置；不把全部变更塞进首轮。
- Current Cycle：`iterations/000-initial/000-initial.md`，initial，Review accepted（2026-10-13）。

### Iteration 001：resident owner 与 link 恢复

- Tasks：T4–T5；T5依赖T4。
- Depends on：000最终Review accepted且DMA停止/复用板测审核PASS。
- Stable baseline：同一queue owner驱动实际reset，source60/MDIO控制面有界；Down/Up不更改queueepoch，reset后新link采样和软件缓冲隔离成立。
- Verification boundary：axnet受影响unit/生产host接缝、source60/MDIO host与真板拔插/受控reset；一次单hartQEMU恢复真实入口回归。
- 昂贵预算：本轮结束时axnet三feature面各一次全量`--lib`，生产K3host包一次；一次QEMU恢复会话和一次K3 owner/link板测。受影响构建在T5最终状态集中执行一次：`make k3`、`make k3-shell`、`make ARCH=riscv64 SMP=1 build`，最后`make k3-async`、`make k3-async-probe`；D1/VF2无共享构建面变化时采信既有结论，不盲目补跑失败入口。
- Diagnostic boundary：request/owner生命周期、link retry timer、GPIO/MDIO、Service缓冲/epoch/wake，不混入完整应用会话。
- Non-goals：最终socket场景矩阵、SMP、长稳、性能。
- Balance audit：T4单独只给共享接缝，T5单独不能形成常驻link/reset闭环，合并为一轮。socket用例与最终PC会话留后轮，避免同时追查PHY与应用。
- Current Cycle：`iterations/001-resident-owner-link-recovery/001-rework.md`，rework，Review accepted（2026-10-14）；Iteration 001 完成（豁免原话与残余移交见该 Cycle Plan Review）。

### Iteration 002：旧新 Socket 与组合恢复验收（当前）

- Tasks：T6–T7；T7依赖T6。
- Depends on：001最终accepted且真板/QEMU结果审核PASS。
- Stable baseline：R1–R8/S01–S20闭合；旧socket稳定错误、新socket真实流量，重复恢复容量/C4闭合，失败保留backing且新提交拒绝。
- Verification boundary：生产SocketOps整合与一次真板最终矩阵；timer/物理link/DMA结论必须来自板上，不以host补位。
- 昂贵预算：本轮末完整K3host包一次、最终真板组合会话一次。共享面未再变时采信001 axnet/QEMU结论；若T6修复改变其覆盖范围，只针对变化定向验证，不重复全量。普通与probe镜像按最终载荷各构建一次，probe最后。
- Diagnostic boundary：socket terminal/readiness/flush、旧新软件数据与板上组合恢复；已accepted的底层不重做bring-up。
- Non-goals：多hart、长稳压力、性能、用户态网络工具。
- Balance audit：T6的旧新对象语义必须与T7真实入口共同验收；单独写echo不是阶段成果。不因每项fault/test再拆一轮。
- Current Cycle：`iterations/002-socket-combination-final-acceptance/001-rework.md`，rework，**Review accepted 2026-10-14**。过程：父 Cycle `000-initial.md` rework-required（U1=host 尾相探针循环不驱动 fake TX 完成，产品判定链正确；U2=wait_lifecycle 陈旧采样缺护栏+门清除次序 µs 窗口加固；U3=上升沿不可达）；用户批准后 Act 执行 R1–R3 + 两项 Blocker 修复（均按推荐 (a)，用户裁决）；真板三轮后按用户收口原话止步残余；Plan Review accepted——A1-R/A2-R 全量满足，A3-R 折价收口（插回会话交换实质已板上见证：上升 events 3→5 + new-2 逐字节交换），豁免风险（容量/C4 第 2 轮、fail-stop 板上判定行、矩阵行形态 new-2 不得表述为已板上验证）与 link 不稳定三候选 Issue 候选记录在该 Review。注意：Map 中“采信001 axnet/QEMU结论”仅 axnet host 面成立——QEMU 已按用户豁免未执行；axnet 三面已在 R2 后重跑（486/516/519）。

## Requirements Traceability Matrix

Covered表示设计、任务、代码位置与首测已配对，不表示实现或运行已通过。没有Simplified/Missing。

| Requirement | Task | First Test | Status |
|---|---|---|---|
| R1 | T1、T4、T5、T7 | `runtime_reset_is_step_bounded`；`queue_only_reset_request_is_reachable`；`resident_recovery_keeps_one_owner` | Covered |
| R2 | T1、T2、T4、T6、T7 | `reset_does_not_rewrite_ring_before_confirmation`；`recovery_epoch_cookie_and_old_hint_are_isolated`；`closed_epoch_discards_copied_packets` | Covered |
| R3 | T4、T5、T6、T7 | `initial_and_repeated_phy_link_snapshot`；`link_again_waits_without_busy_wake`；`link_down_up_preserves_queue_epoch` | Covered |
| R4 | T1、T2、T3、T5、T7 | `runtime_reset_rebuilds_same_arena`；首轮真板stop/写回观察/重建/收发 | Covered |
| R5 | T1、T2、T4、T6、T7 | `stuck_reset_retains_arena_and_owners`；`reinit_failure_never_activates_partial_queue`；`fault_refuses_new_submit` | Covered |
| R6 | T1、T2、T4、T5、T6、T7 | `reset_deadline_is_absolute`；`cancel_waiter_does_not_cancel_resident_recovery`；`mdio_phase_timeout_preserves_owner` | Covered |
| R7 | T2、T4、T6、T7 | `old_flush_is_aborted_not_c4`；`old_socket_terminal_precedes_wake_new_socket_is_clean` | Covered |
| R8 | T1–T7 | 既有生产harness/包先行GREEN；`repeated_recovery_preserves_capacity`；真板最终矩阵与QEMU真实入口 | Covered |

## 场景与验证分配

| 场景 | 最简直接验证 |
|---|---|
| S01/S02 | T4 host请求与owner交错 + T5/T7真实入口reset；状态变化/wake/owner计数与新流量 |
| S03/S04/S05 | T1/T2生产状态/cookie/hint与边界unit；T7跨16-slot的恢复后真实交换。物理raw descriptor没有epoch，不伪造硬件迟到写来冒充安全证明 |
| S06/S07/S08 | T5 host MDIO/page/COR与native source60；真拔线、插线，PHY状态/NotConnected/新socket交换；至少一完整down→up |
| S09 | T4/T6确定性交错 + T7在reset窗口内实际拔插；queue/link/socket三层结果和owner账本分别观察 |
| S10 | T3真板已有流量→stop/SWR→保留arena下实际来包→无新增写回→同arena重建→新RX/TX；单寄存器不算通过 |
| S11/S12 | T1/T2 delayed/stuck/reinit失败生产fixture；T7末尾有界安全失败路径。物理硬件不可稳定复现的fatal由fixture覆盖，不宣称自然发生 |
| S13/S14/S15 | 既有取消/阶段deadline单测采信并补K3adapter可达路径；fakeclock观察阶段，不宿主sleep。T7真实timer有界失败 |
| S16/S17 | T6 public SocketOps多waiter/flush/readiness与T7真实TCP/UDP/listener旧错误、新字节交换 |
| S18 | T2重复同arena/capacity unit；T7固定两次成功恢复（原有typed Full→恢复和C4各观察一次）；失败不重试到PASS |
| S19 | T1/T2既有harness/package；T4/T5共享三feature与QEMU恢复会话；普通K3/shell构建 |
| S20 | 每轮原生停止结果和操作者外部RESET回官方Linux一句话；遵守既有RAM启动流程，无持久写入 |

## 验证命令与运行边界

host native命令（在所属轮使用；已通过且覆盖未变化时直接采信，不逐task重复全量）：

```sh
rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/k3-gmac-recovery-host
/tmp/k3-gmac-recovery-host --test-threads=1
cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline
cargo test --manifest-path tests/k3-polling-net-host/Cargo.toml --locked --offline
RUSTFLAGS='-C linker=scripts/cc-nopie.sh' cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib
RUSTFLAGS='-C linker=scripts/cc-nopie.sh' cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib --features queue-diagnostics
RUSTFLAGS='-C linker=scripts/cc-nopie.sh' cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib --features qemu-diagnostics
```

定向RED/GREEN在相同Cargo/rustc入口加实际测试filter；没有另建脚本。目标package默认feature保持现状，缺失网络环境变量时沿用既有测试的10.0.2.x，板上仍使用192.168.137.2/1，不把IP当验证身份。

QEMU共享回归：复用 `.agents/runbooks/ms07-qemu-single-hart-recovery-evidence.md` 的既有编译、peer、qemu-system-riscv64和guest probe入口，单hart VirtIO-MMIO真实reset与HMP off/on，各case原生输出和guest退出码直接判定。当前Plan不运行QEMU；后继轮Cycle写明实际命令和最短人工步骤。旧validator、采集封装、日志目录和旧阶段全套回归不纳入本次要求。只涉及K3的面不要求QEMU冒充硬件结果。

当前最近能力边界是首轮T3构建后的真板会话。Act获批后执行T1→T2→T3接线/构建/自检，准备具体板测步骤与镜像，再停在该边界；不能把“等待板测”列成agent可执行任务。操作者提交结果后先审核PASS，再补首轮Response；首轮最终Review accepted后才展开001。

## 阶段状态、Gate 与跳过项

| 项 | 状态 |
|---|---|
| 需求与场景审计 | completed；Gate1 PASS，用户原话见proposal |
| 现有工作与参考资源复用、生产路径调查 | completed；Current-State Evidence与新鲜度见design |
| 设计、Map平衡审计、RTM、首轮契约 | completed；结构、完整diff和契约一致性自检已完成，用户批准后首轮ready |
| Gate2 | PASS：2026-10-13 用户原话“批准”；技术自检覆盖未变化，无BLOCKED/WAIVED |
| T1（1.1）/ T2（1.2） | completed 2026-10-13：RED→GREEN，Gate3/Gate4 过（harness 36/36；recovery_contract 5、adapter_contract 12、epoch单测 1；全量包见Act Response） |
| T3（1.3） | completed 2026-10-13：接线/arm前移/构建门/全量包完成；A3真板会话操作者执行，审核PASS（stop确认、有输入前提零写回、同arena重建、重建后真实收发、移交、外部RESET回官方Linux） |
| 首轮 Plan Review | accepted 2026-10-13：Act 声明与代码逐项一致，偏差 1–3 准确；4 项非阻塞 Minor finding 记录在案，Acceptance Gaps 无 |
| T4（2.1） | completed 2026-10-13：RED→GREEN，Gate3/Gate4 过（axnet 默认 486、queue 516、qemu 519 全绿；5 个先行测试全过；2 个旧 Again 语义测试按新契约更新） |
| T5（2.2） | completed 2026-10-14（经 001-rework）：R1 判别推翻父轮静态归因（真因为第 2 步 quiesce 关路由、timer 全程存活），R2 移交步重开路由；板测四轮闭合修复确认/S07 NotConnected/S06+S08；构建门 5/5、timer harness 8/8。A3 QEMU 用户豁免（原话在 001-rework Plan Review）、A4 残余（down→up 上升沿+插回新会话）用户接受并移交 T7；001-rework Review accepted，Iteration 001 完成 |
| T6–T7 | in_progress：2026-10-14 Act 完成 agent 侧（T6 host 测试面除尾相 U1 外全过；T7 注入接缝 harness 37/37、矩阵编排、25s 窗、构建门 k3-async/probe exit 0）；同日真板会话部分直证：reset-1 旧对象 `connection-reset` ×2、flush `aborted err=BadState`、new-1 新 epoch UDP/TCP、link-down `not-connected`+source-60、reset-2 交错恢复 `fault=0`。同日 Plan Review：**rework-required**——U1/U2/U3 根因全部定位（gdb 真值与代码位置见 000-initial Plan Review）：U1=尾相探针循环不驱动设备半（票 21=新会话 FIN 在途，产品判定链正确）；U2=`wait_lifecycle` 陈旧采样（确定性）+门清除次序加固；U3=pad35 仅武装下降沿（上升沿结构性不可达）；停摆为 U3 落7，runner 自醒风暴+自旋锁饿死登记为范围外 Issue 候选（只报告不落账）。`001-rework.md` 已创建（T6-R1/T7-R2/T7-R3/T7-R4）；同日用户原话批准，Gate 2 PASS。同日 rework Act：R1 交付（flush 真结算，U1 机制生效）、R2 双修复交付（left 护栏+门清除入 guard）、R3 双边沿武装交付；验证 axnet 三面 486/516/519、host 定向面全绿、phy harness 32/32、k3-gmac 37/37、构建门双镜像 exit 0。**新阻塞（Act Response → blocked）**：继承的最终断言 `fallback_poll==0`（recovery_socket.rs:947，父轮从未执行过）确定性失败——MS13 恢复状态首次流入 stack runner 既有非 Active catch-all 臂（stack_runner.rs:569-576），恢复窗口空闲轮回退计数 1，无已验收 requirement 约束；断言语义裁决交 Plan（候选 a：恢复后 baseline 的 Active 尾相断言；候选 b：裁定产品缺陷，Act 不推荐）。T6/T7 保持未勾选；T7-R4 板测待阻塞解除后交操作者。**同日 T7-R4 第一轮真板会话（操作者执行，Act 审核）**：U2 板上闭合——reset-1 后容量第 1 轮 `i=0 accepted=true`、81 帧两级边界（i=80 rejected）、release→restored→`flush c4 ok`、auto-release=0；old/reset-1 终态/new-1/reset-2 与父轮一致无回归；new-2 有界 NotConnected 无停摆。**未取得并按 Stop-when 停**：source-60 双沿全灭（events=0、handler set/rearm 保持 0x0）——父轮可见的拔线下降沿事件亦消失（link-down waiter 退化为 pending-timeout），插回 `phy60-event timeout`，capacity-2 被 link 门拒后 `err=flush-enqueue-full stop`，fail-stop 未到达。根因假设经 arm 行修正：`mfpr=0xa430->0xa430` 写入逐位幂等（before 已含双沿 0x30+CLEAR 释放）——R3 代码与本轮静默无关，父轮 U3 归因（"仅武装下降沿"）被推翻；领先假设为 RTL8211F INTN 电平保持（INTS 读清除缺失：handler 无 MDIO、arm 只写 INER 不读 INTS，INTN 置低即死锁），一个机制同时解释父轮 events=1 与本轮 events=0（boot 时序偶然决定哪一轮先出现 link 事件）。板上旁证 fallback 计数 0→85→170 仅在恢复/门闭窗口增长，与 host blocker 候选 (a) 方向一致。Act Response 增补 Blocker 2（候选 a/b/c 与恢复条件见 001-rework Blocker Handoff）；arm 行与外部 RESET（S20 闭合）均已补齐。**同日用户原话"这里我们不阻塞了，按你推荐的来，我们修复后重新进行真板测试"——两项 Blocker 均按推荐 (a) 解除**：Blocker 1 断言改恢复后 Active 尾相 baseline；Blocker 2 实施 INTS/INTN ack（arm 时 INSR 清 pending + adapter `read_link_status` 后 `consume_link_cause` 有界消费 latch[清读→latched 则双读 BMSR 重采样→至 0]，先行测试 RED→GREEN 9/9）。验证全绿：host 全包（recovery_socket 首次全过）、phy harness 32/32、axnet 三面 486/516/519、k3-gmac 37/37（均按覆盖未变采信）、双构建门 exit 0。**T6 勾选完成；T7 agent 侧完成**。真板三轮汇总：U2 闭合（容量第 1 轮+两级边界+flush c4）、R1/R2 全链无回归、INTS/INTN ack 板上成立（拔线 not-connected+source-60 端到端、插回上升沿可达 events 3→5、new-2 真实会话交换成功）；**残余未取得**（容量/C4 第 2 轮、fail-stop、矩阵形态 new-2 判定行）与**插回后 link 不稳定**（三候选留档未定位）——**用户原话"算了，这些问题都不重要，取消相关调试语句和更改，我们直接填写回复，本轮change就做这么多"**：诊断插桩全部撤销、probe 重建 exit 0、Act Response → **reported** 收口本轮，交 Plan Review 裁决残余处置。**同日 001-rework Plan Review：accepted**——六个改动面代码逐项核对一致（尾相驱动/护栏/门次序/双沿/baseline 断言/INSR 消费，插桩零残留）；验证采信（host 全包含 recovery_socket 首次全过、phy 32/32、k3-gmac 37/37、axnet 三面 486/516/519、双构建门 exit 0）；两项 Blocker 分类 Plan 遗漏并经用户裁决 (a) 修复（Blocker 2 触及 adapter.rs 属用户授权范围内扩展）；A3-R 折价收口（插回会话交换实质已板上见证）；豁免风险与"不得表述为已板上验证"边界记录在该 Review Acceptance Gaps；link 不稳定三候选与 runner 风暴/饿死为范围外 Issue 候选（只报告）。**T6/T7 勾选完成，Iteration 002 完成，change 待收尾** | |
| 产品修改、行为验证/板测、经验落账、状态同步 | 001 rework 板测四轮完成并审核；经验候选 None；全局状态同步与脚手架最终清单按 T7 Review/收尾执行 |
| 新Plan基线重跑 | SKIPPED：实际只读diff证实覆盖未变，采信最近accepted结论；新增恢复场景无旧PASS，交Act先RED |
| Persisted Evidence | none；无Evidence目录 |

每task完成后Gate3自检；各轮全部契约验证满足才Gate4完成，失败维持任务并按Gate5处理。当前范围外新Issue候选：None。所有产品任务完成须以逐轮accepted Review闭合，不以OpenSpec文件存在或本计划Covered代替。
