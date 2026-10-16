# axnet runner 自醒风暴 + 自旋锁饿死（门关闭 + 持续入流下任务停摆）

- Status: open
- Filed: 2026-10-14
- Source: MS13 change `2026-10-14-ms13-com260-kit-single-hart-network-recovery` Iteration 002 `000-initial.md` Plan Review Finding 5（范围外，只报告）
- Environment: CoM260 Kit 单 hart；k3-async-probe 镜像；axnet stack runner + axsync::Mutex（纯自旋）+ axfeat `sched-rr`（kernel/Cargo.toml:115，axtask preempt）

## 缺陷描述

RX/TX 门持续关闭且有持续入流时，stack runner 的自醒条件（`crates/axnet/src/stack_runner.rs:540-561`：`socket_changed`/`rx_ready` 等不满足即重臂）驱动 runner 每包反复持 `SOCKET_SET` 锁；boot 任务在 `axsync::Mutex`（纯自旋，无排队）上等待，`sched-rr` 时间片抢占下形成优先级反转/饿死——低优先级 runner 得不到调度释放锁，高优先级等待任务连"有界失败并打印"的设计承诺都无法履行。指控点：`crates/axnet/src/stack_runner.rs`（自醒条件）、`axsync::Mutex` 纯自旋语义与 `sched-rr` 的组合（axnet/axsync 既有结构性属性，非 MS13 引入）。

## 影响

门关闭窗口（link down、恢复中）+ 操作者/局域网持续流量时，等待锁的任务可无限期停摆，破坏 payload 的有界失败契约；不影响门正常开合时的数据面。

## 事件记录

- 2026-10-14 MS13 Iteration 002 首轮真板会话：`new-session-2` 指令行后串口完全停摆（连 terminal-first 的错误行都没有），payload 卡在 `UdpSocket::new()`/bind/首次 poll 对 `SOCKET_SET` 的自旋等待；操作者 ping 与 Windows 后台 ARP/ND 持续入流驱动 runner 反复持锁。根因：Inferred（症状与代码路径吻合，未做调度级取证）。检测：串口静默；恢复：外部 RESET。后续 INTS/INTN ack 修复使插回即重开门、入流正常交付、自醒风暴平息，该场景消失——但"长时间门关闭 + 持续入流"的组合条件仍可复现。

## 处置

MS13 父 Review Finding 5 定为范围外既有结构属性，不修；本 Issue scheduled → 用户指令。候选方向（未评估）：runner 自醒节流、axsync 排队锁或等待任务优先级提升；属 MS14 多 hart 调度面的输入事实。

## 证据

- `openspec/changes/archive/2026-10-14-ms13-com260-kit-single-hart-network-recovery/iterations/002-socket-combination-final-acceptance/000-initial.md`（Plan Review Finding 4/5：U3 停摆定位与残余风险声明）
- 同目录 `001-rework.md` Act Response（第三轮排除饥饿的对照证据：门正常开合时 boot 任务每秒存活）
