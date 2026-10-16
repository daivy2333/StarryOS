# MS13 插回后 link 不稳定（reset-2 后插回，TX 门振荡至全接口静默）

- Status: open
- Filed: 2026-10-14
- Source: MS13 change `2026-10-14-ms13-com260-kit-single-hart-network-recovery` Iteration 002 Act Response（真板第三轮判别 trace 取证）与 Plan Review（accepted，残余留档）
- Environment: CoM260 Kit（SpacemiT K3）单 hart；k3-async-probe 诊断镜像（含 trace 插桩，收口时已撤销插桩代码）；串口 115200；PC 192.168.137.1 直连

## 缺陷描述

预期：reset-2（拔线中受控 reset）后物理插回，link 应稳定收敛到 Up，TX 门保持开启，数据面持续服务。实际：插回后分两相劣化——A 相（约 11 s）RX 活跃（ri/work 每秒 +13~19）而 TX 冻结（ti 恒 121）、rx-slot 占用积压爬至 5（门关闭形态：owner 视角 link Down）；B 相（至外部 RESET）全部计数器冻结（ri/work/irq-runs/aia-traps/occupancy），GMAC 一级中断完全停止，owner 快照始终 `lifecycle=2 fault=0` 对设备侧沉默无感知，`phy60 events` 冻结 5 不再增长。涉及位置：`crates/axnet/src/async_rx.rs`（owner link/recovery 路径）、`crates/axnet/src/stack_runner.rs:569-576`（非 Active 生命周期 catch-all 回退臂）、`kernel/src/platform/k3_gmac/adapter.rs::consume_link_cause`（INSR 消费）、`kernel/src/platform/phy_probe.rs`（INTN/INSR 应答链）。

三个候选机制（**Unconfirmed**，有第三轮 trace 证据未互相排除）：

1. INTN 电平型事件通路"事件→owner 读→消费 INSR"单次失手即永久哑：INTN 停低后，后续 link 变化只再锁存 INSR、不再产生 pad 边沿，且空闲不轮询无兜底读取（与 `events=5` 冻结吻合）。
2. 末次 link 判读为 Down 后门永久关闭（无新事件即永不重读；A 相 RX 活/TX 死与此吻合，但 B 相全静默需 link 同时沉默）。
3. GMAC DMA 静默停摆（两方向完成中断全停，owner 无 fault 感知）。

## 影响

运行中"拔线中 reset→插回"后网络可用性不闭环：数据面间歇可用（操作者 UDP/TCP 各成功一次）后完全沉寂，需外部 RESET 恢复。稳态数据面与首次受控 reset（reset-1，link 正常）不受影响（三轮板上全绿）。

## 事件记录

- 2026-10-14 第三轮真板会话：reset-2 `committed queue-gen=462 fault=0` → 插回 25 s 窗内 `phy60-event observed events=3` → new-2 UDP 进入 recv-wait（trace 每秒存活，排除饥饿/死锁）→ 手动 UDP/TCP 各成功一次（逐字节回显）→ 二次 UDP 超时 → A 相（RX 活 TX 死）→ B 相（全冻结）→ 操作者外部 RESET 回官方系统（串口尾段出现官方 U-Boot `try sd...`）。根因：Unconfirmed。检测：判别 trace（每秒 wd 增量 + phy-truth INSR/BMSR 直读，该插桩构建收口时已按用户裁决撤销）。恢复：外部 RESET。

## 处置

用户收口裁决（原话"算了，这些问题都不重要，取消相关调试语句和更改，我们直接填写回复，本轮change就做这么多"）：MS13 本轮不追，残余留档。当务之急为 MS14 真板多 hart；本 Issue scheduled → 用户指令或后继 change 承接。判别入口：phy-truth 直读（INSR 锁存 + BMSR 双读）可三选一定位（insr≠0 冻结=候选 1；insr=0+bmsr=up+ti 冻结=候选 2；bmsr=down=复合；bmsr 恒 up 全冻结=候选 3）。

## 证据

- `openspec/changes/archive/2026-10-14-ms13-com260-kit-single-hart-network-recovery/iterations/002-socket-combination-final-acceptance/001-rework.md`（Act Response：三轮真板汇总、第三轮两相 trace 取证、三候选与判别表；Plan Review：accepted，A3-R 折价收口边界）
- 串口原文与 PC 输出由操作者会话留存（2026-10-14，未另存 Evidence，判定行已录入 Act Response）
