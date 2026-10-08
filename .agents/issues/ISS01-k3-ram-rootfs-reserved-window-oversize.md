# K3 RAM-rootfs 预留窗口固定 1 GiB：实际镜像 128 MiB 时 896 MiB RAM 预留未用

- Status: open
- Filed: 2026-10-08
- Source: MS18 Iteration 000 实施与压缩分析（Act Response「镜像压缩裁决」；用户 2026-10-08 指示登记）
- Environment: 分支 k3（MS18 Iteration 000 实施）；`crates/axplat-riscv64-k3/src/ram_layout.rs::RAM_ROOTFS_RESERVED`；CoM260 Kit（6.4G RAM，`page-alloc-64g`）

## 缺陷描述

- 预期（合理行为）：预留量应与镜像实际档位匹配，使 allocator 保留量接近实际占用。
- 实际：`RAM_ROOTFS_RESERVED` 按设计 D1 固定预留整个载运窗 `[0x240000000, 0x280000000)`（1 GiB，另加 DTB 窗口 1 MiB）；首轮实际镜像仅 128 MiB → **896 MiB RAM 处于"预留但未用"状态**。板上 region 清单证实：`[PA:0x240000000, PA:0x280000000) reserved`，而 fatload 实际装载 134217728 B。
- 定性：这是设计 D1 的既定取舍（载运窗容量固定 1 GiB 以容纳原始尺寸镜像），登记为**资源占用约束**跟踪，不是编码错误；修复杠杆是设计级调整，走 Plan 决策，不属于镜像操作（镜像压到多小都不改变预留量）。

## 影响

- 当前：K3 有效 allocator RAM 从 ~6.25 GiB 降为 ~5.25 GiB；单 hart 当前轮无功能影响。
- 潜在：MS14 多 hart 与组合长稳场景下有效内存上限被无谓压低；若未来多档镜像并存，按最大档预留的浪费随档位差扩大。

## 事件记录

None（未引发故障；用户主动登记的资源占用约束）。

## 处置

- scheduled → 后续 Plan 决策（未批准实施，不阻塞 MS18 首轮与 MS12–MS14 排期）。候选方案：
  1. 窗口常量收档：`RAM_ROOTFS_RESERVED` 的 rootfs 段改为与实际镜像档位匹配的固定值（如 256 MiB），一行改动 + host 测试常量同步；
  2. 按实际 extent 动态预留：预留时机需提前到 bootargs 解析之后——但 allocator 在 entry 之前已初始化，动态预留需要不同的回收/登记机制，可行性未调查（unconfirmed）。
- 若批准实施，随后续 change 进入（含 D1 设计条目修订与 host 测试更新）。

## 证据

- Act Response：`openspec/changes/ms18-com260-kit-standard-shell-rootfs/iterations/000-ram-rootfs/000-initial.md`（镜像压缩裁决：128 MiB 镜像、fatload 134217728 bytes read；板上 region 清单含 `[PA:0x240000000, PA:0x280000000) reserved`）
- 常量：`crates/axplat-riscv64-k3/src/ram_layout.rs`（`RAM_ROOTFS_BASE/WINDOW_END`、`RAM_ROOTFS_RESERVED`）
- Runbook：`.agents/runbooks/rootfs-image-shrink.md`（镜像可独立压缩至 ~16 MiB 的事实边界，与本 Issue 的"预留量与镜像尺寸解耦"互补）
