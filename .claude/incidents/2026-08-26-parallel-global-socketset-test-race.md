# 并行 axnet 单元测试共享进程级全局 socket 状态间歇性失效

- Status: open
- Occurred: 2026-08-26（MS06 Iteration 004 Cycle `001-replan` Act 执行期间发现；问题本身早于本 Cycle 存在）
- Environment: Linux x86_64 开发机；Rust `nightly-2026-02-25`；host 单元测试经 `cargo test`（`RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh"` 非 PIE wrapper）；本地 vendored smoltcp（0.13.1 基础）+ hashbrown 0.16.1；分支 `net-k3`，HEAD `1ea51427d8692f5a12b87a0403b940e73d43fed3` 加未暂存 Cycle 实现
- Source: `openspec/changes/ms06-application-visible-async-network-stack/iterations/004-terminal-readiness-and-qemu-acceptance/001-replan.md` 的 Act Response（reported）及其验证过程

## 影响

- 无产品、数据或硬件影响；未进入 QEMU runtime 或交付物。
- 工程时间影响显著：Act 会话中相当比例耗时用于区分"本 Cycle 回归"与"既有基础设施竞争"，包括一次导致 4 个文件三方合并冲突的 `git stash pop` 恢复操作。
- 交付风险：验证 Gate 可能读到假 RED（偶发单测失败、SIGSEGV/SIGABRT 进程崩溃），若不归因会误判实现回归。
- 测试可信度：并行套件结果不可作为唯一通过依据，需要确定性口径补充。

## 时间线

以下均为 2026-08-26，按会话顺序：

1. Task 3.2 RED 阶段组合运行 `{tcp_connect_entry, send_entry}`：首次出现进程异常——先报 `panic in a destructor during cleanup`，随后 `malloc_consolidate(): unaligned fastbin chunk detected` SIGABRT。此时产品代码仍为 Cycle 000 版本（仅新增测试），排除产品修复引入。
2. qemu-diagnostics 全量套件连续两轮在已知 `async_rx::reclaim_hold_drains_to_real_driver_full_without_observing_again` 上 FAILED（376 passed）；该测试每次隔离重跑均 ok。
3. E1 基线实验：`git stash push --keep-index` 暂存本 Cycle 未暂存改动后，基线 diagnostics 全绿（368/368，单次）。
4. `git stash pop` 在 readiness/tcp/udp/wrapper 四文件产生内容冲突（本 Cycle 编辑与已暂存 Cycle 000 实现触及同一区域）；经 `git show :2:/ :3:` 提取两个 stage blob、`git update-index --cacheinfo` 精确恢复 index 与 worktree 的 staged/unstaged 边界，行数核对后 drop stash。
5. 恢复后一次过滤运行在 13 个测试通过后 SIGSEGV（signal 11）死亡；另一轮 ordinary 全量出现 1 个未具名单测失败；随后 ordinary 连续多轮全绿（357/357 ≥5 次，含连续 3 次）。
6. 子集二分：A `every_bridge_ends_committed`（既有线程测试）单独 40 轮失败 2 次；B 本 Cycle 新增 tcp/udp terminal 集 40 轮失败 15 次；C wrapper publication 集 40 轮 0 失败。
7. 对照组 D（仅既有测试，均共享全局 SOCKET_SET churn）：**17/40 失败**——高于 B。
8. E4 归因实验：用 rescue 的 index blob 将产品四文件字节级换回 Cycle 000 版本，D 组 **10/25 失败**——与 42.5%（B）统计不可区分，证明竞争先于本 Cycle 且与本 diff 无关；随后恢复本 Cycle 版本并验证。
9. E2 实验：保留本 Cycle 产品改动、`--skip` 全部 15 个新/改测试，diagnostics 全量仍复现 async_rx flake——该 flake 与新测试的存在性无关。
10. 口径适配：Task 3.1 publication 子集并行 ×100 = 100/100；Task 3.2 terminal+interleave 子集单线程 ×100 = 100/100；范围化 fmt 后 ordinary/diagnostics 复跑，ordinary 全绿，diagnostics 仅剩已知 async_rx flake。
11. Act Response 以 `reported` 收尾，完整归因写入其 "Pre-existing instability attribution" 与 Verification Evidence 表。

## 触发与根因

- Confirmed:
  - 失效需要特定共调度：仅当多个对进程级 `crate::SOCKET_SET` / `LISTEN_TABLE` 做 add/remove/iterate churn 的测试并行时出现；确定性子集（wrapper publication 语义集）并行 ×100、terminal 集单线程 ×100 均 100% 通过；ordinary 全量可连续多轮全绿。
  - 典型失败面：`smoltcp/src/iface/socket_set.rs:103/126`（`get`/`get_mut` 对陈旧句柄 panic "handle does not refer to a valid socket"）、hashbrown 0.16.1 `raw/mod.rs:3250` 与 `control/tag.rs:29` 内部一致性断言；偶发 SIGSEGV/SIGABRT（`malloc_consolidate` 报堆损坏）表明存在真实内存不安全，而非普通断言失败。
  - 归因结论（E2/D/E4 三组对照）：竞争先于 Cycle `001-replan` 的 diff，且与新测试是否存在无关；旧产品代码同口径失败率（10/25 ≈ 40%）与本 Cycle 改动下（17/40 ≈ 42.5%）统计不可区分。
  - 已知伴随现象：diagnostics profile 的 `async_rx::reclaim_hold_...` flake 自父 Cycle Review 起即有记录，本次再次复现且隔离重跑恒过。
- Inferred:
  - 最可能机制是句柄复用碰撞或某条绕过 `SOCKET_SET.inner` 锁的全局访问路径（如延迟回收句柄与新建句柄同值并存），在并行调度窗口内触发 smoltcp `SocketSet` 陈旧句柄访问并破坏下游 hashbrown 注册表内存。此推断与全部观测一致，但未定位到确切 UB 点。
- Unconfirmed:
  - 具体 UB 所在符号/代码路径（smoltcp `SocketSet`、axnet registry、Drop 延迟回收三者之一或多者）。
  - 是否与 `every_bridge_ends_committed...` 的真线程测试有叠加效应（其单独也有 2/40 失败）。
  - 是否存在环境因素（宿主内存压力、沙箱调度）放大概率。

## 检测与恢复

- 检测：`cargo test` 间歇失败；panic 定位在 socket_set/hashbrown 内部断言；进程信号 6/11。隔离重跑同一测试恒过——这是识别"非确定性竞争"而非产品缺陷的关键信号。
- 缓解（已生效，非修复）：
  - 确定性见证改用单线程循环（×100）与不受竞争影响的子集并行循环取证；
  - 全量套件以多次运行 + 隔离重跑交叉判定，单一失败须先归因再计数；
  - 对已知 async_rx flake 维持父 Cycle 先例：单独报告、不计入 Acceptance 判定。
- 失效的保护：没有任何机制隔离单元测试与进程级全局 `SOCKET_SET`/`LISTEN_TABLE`；`SERIAL` 锁只覆盖部分 fake-clock 测试。
- 恢复操作记录：调查中的 `git stash pop` 冲突已用底层命令精确恢复（见时间线第 4 条），工作树 staged/unstaged 边界保持原状；该恢复路径端到端成功、可重复且属高风险 git 操作，具备 Runbook 候选资格（未创建，见后续动作）。

## 后续动作

- 建议另立专门 change 修复根因（候选方向：per-test 隔离 wrapper 实例、为全局态 churn 测试补齐串行化、或消除句柄复用窗口）；走 `openspec-plan` 流程，本 Incident 不实施。
- 关联已知项：diagnostics async_rx flake（父 Cycle Review 已认可的非阻塞项）。
- 未自动登记的候选（待用户授权由 Maintainer 写入）：
  - Improvement 候选：axnet 单元测试对进程级全局 socket 状态缺乏隔离，并行运行间歇失效（证据：本 Incident 时间线与归因数据）。
  - Runbook 候选：`git stash pop` 内容冲突的 stage blob 精确恢复流程（`git show :2:/ :3:` + `git hash-object -w` + `git update-index --cacheinfo` + worktree 回写，含行数核对验证）。
  - Knowledge 候选：本仓库并行 `cargo test` 结果不能单独作为 Gate 通过依据，需确定性口径补充（归因数据见 Source）。

## 证据

- 主来源：`openspec/changes/ms06-application-visible-async-network-stack/iterations/004-terminal-readiness-and-qemu-acceptance/001-replan.md` — Act Response（Status: reported）的 Verification Evidence 表与 Pre-existing instability attribution 小节（含 E2/D/E4 命令与计数）。
- 会话期日志（临时目录，可能已被清理，关键摘录已并入上文）：`/tmp/opencode/focused_1.log`（SIGSEGV 运行尾部）、`/tmp/opencode/rescue/s2_*.rs` 与 `s3_*.rs`（stash 冲突恢复用的 index/stash blob，行数 624/615、889/995、561/670、449/607 与编辑前读取值吻合）。
- 关键决定性输出摘录：
  - `thread '...' panicked at crates/smoltcp/src/iface/socket_set.rs:103/126 ... "handle does not refer to a valid socket"`
  - `malloc_consolidate(): unaligned fastbin chunk detected` → SIGABRT；一次 `signal: 11, SIGSEGV`
  - D 组（本 Cycle 产品）：`fail=17/40`；E4 组（Cycle 000 产品字节回换）：`fail=10/25`
  - 确定性口径：publication ×100 `pass=100 fail=0`；terminal+interleave 单线程 ×100 `pass=100 fail=0`；ordinary `357 passed; 0 failed` 多轮
- 适用限制：/tmp 证据为易失性；归因基于当日该环境的采样次数（n=25–40/组），概率结论而非穷尽证明。
