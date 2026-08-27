# Evidence: 007-automatic-integration-qualification / 000-initial

- Change: ms06-application-visible-async-network-stack
- Iteration: 007-automatic-integration-qualification
- Cycle: 000-initial
- Captured at: 2026-08-27
- Revision: `832abfead57e7ae0870d5b729b6875665d588582` (working tree；实现差异仅为
  Runbook/reference/本Cycle文档的 staged 编辑，无产品代码变化)
- Environment: host x86_64；axnet 独立 target 目录冷重建；按 K44 使用
  `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh"`（记录于 Iteration 004 D1）
- Trigger: 用户批准 Gate 2（2026-08-27，"更改gate状态，开始实施吧"）；用户豁免
  full suite race witness 从 100× 到 20×（原话："100次太多了，且没必要跑这么多次，
  豁免到20次，继续吧"）。

| ID | Origin | Acceptance | Claim | Artifact | Result |
|---|---|---|---|---|---|
| EV-007-000-01 | act-added | Task 6.1 GREEN：default-parallel host suites 无 flake 豁免 | 默认并行 full suite 在 20× 见证窗口内出现 SIGSEGV（run 16/20），下次 run 通过（非确定性）；R57 家族 flake 未闭合 | [blocker.md](blocker.md) | BLOCKED |
| EV-007-000-02 | user-required | 同上 | 用户回退后指令按 30× 复核（原话："这次我们再次一次性执行三十次，看看是否有类似错误"）；30/30 全过、0 crash，单次 wall 5.0–10.7s（无挂起/死循环） | [recheck-30x.md](recheck-30x.md) | PASS |

EV-007-000-01 的原始现场保留不变；EV-007-000-02 提供延续观察窗口，二者共同支持
"首次 crash 未在 30× 复核窗口内再现，概率低于 1/31" 的结论。用户于 2026-08-27
对残余 SIGSEGV 作出最终裁定（原话保留于 Cycle 000-initial Act Response Blocker
Resolution）：“我觉得这是机器偶发错误，因为触发几率很小排查困难，我们不在这里
阻塞，更改回复为通过，记录我的原话豁免”。该事件不作为 Task 6.1 Acceptance 1
阻塞项，本 Cycle 结论为**通过（用户豁免）**。

白名单理由：默认并行 full suite 出现 SIGSEGV（signal 11）是影响自动资格判断的实质
Blocker；崩溃非确定性、逐次复现成本高（单次 ~6s，偶发窗口 ~20× 内），关键现场无法
从 Act Response 摘要重建；Task 5.1 声称已消除该家族症状，需保留原始证据供 Plan 归因
并防止误判为环境问题。

适用限制：结论只覆盖当前 tree 在 host x86_64 默认并行 axnet ordinary full suite 的
偶发崩溃；不扩大到产品行为、QEMU runtime、D1 或 artifacts。