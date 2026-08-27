# BLOCKED: 默认并行 axnet ordinary full suite SIGSEGV（run 16/20）

- Task: 6.1 自动集成资格 — "default-parallel host suites 无 flake 豁免"
- Gate: Gate 5（竞争/ownership 见证，full-suite repeat-100，用户豁免为 20×）
- 发现位置: Task 6.1 competition witness 阶段，`race-full-suite` 第 16 次迭代
- 时间: 2026-08-27，本地串行循环内

## 执行命令

命令（每次迭代相同；循环带 `timeout 90`）：

```bash
env RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh" \
  cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib
```

前置：run 1–15 全部 `test result: ok. 371 passed; 0 failed`，exit 0，
单次 wall 约 5.5s（异常迭代为 12–17s）。

## 决定性输出（run 16，尾部 8 行）

```
test udp::tests::udp_queued_tx_drop_enqueues_into_fixture_service ... ok
test udp::tests::unbound_socket_reports_no_readiness ... ok
test wrapper::tests::adoption_installs_bridge_for_existing_handle ... ok
test wrapper::tests::duplicate_publish_is_idempotent_and_wakes_once ... ok
error: test failed, to rerun pass `--lib`

Caused by:
  process didn't exit successfully: /home/daivy/projects/serial/work/StarryOS/crates/axnet/target/debug/deps/axnet_ng-adb521d4f2e16d2b (signal: 11, SIGSEGV: invalid memory reference)
```

## 归因诊断

崩溃为**非确定性**：run 16 失败后立即重跑一次（同命令、同 tree、同 wrapper）
返回 exit 0 且 371/371 PASS（wall 5.3s）。SIGSEGV 出现在并行测试进程内，未归因到
单个测试；与 R57（并行 axnet 测试共享进程级 `SOCKET_SET`/`LISTEN_TABLE` 导致
SIGSEGV/SIGABRT）的家族症状一致，说明 Task 5.1 的隔离修复未覆盖该偶发窗口。

## Plan 预期 vs 实际

- Plan 预期：default-parallel full suites 稳定重复通过，无 flake 豁免，作为
  Iteration 008 人工 runtime 资格前提（Acceptance 1）。
- 实际：20× 见证窗口内 1 次 SIGSEGV；Iteration 006 的 ×3 通过未探测到该窗口。

## 影响

自动资格 Gate 不成立 → Iteration 008 single-hart QEMU runtime 资格未达成；
Task 6.1 其余 Gate（host-test、driver、qemu check、D1、artifacts、quality）不应
在失败层未归因前继续，避免掩盖首个失败层。

## 完成/部分工作

- 已完成：Gate 2 批准记录；axnet ordinary 371/371 exit 0、qemu-diagnostics 393/393
  exit 0；race-control ×100、race-v3 ×100 全过；full-suite run 1–15 通过。
- 部分：full-suite 20× 见证在 run 16 失败（用户豁免 100→20，第 16 次即失败）。

## 工作区状态

- 修改文件：`iterations/007-automatic-integration-qualification/000-initial.md`
  （Plan Context `draft`→`ready` + 批准记录）、本 Evidence 目录。
- 未修改产品代码、测试或验证命令；`/tmp/opencode/cc-nopie.sh` 为一次性本地工具
  （K44），不入库。
- HEAD `832abfead57e7ae0870d5b729b6875665d588582`；staged 编辑仍为
  Runbook/reference/本 Cycle 文档。

## 恢复条件

1. Plan 归因该 SIGSEGV（是否 R57 同根、是否新增共享边、是否需要修复或明确豁免）。
2. 修复或豁免获批后，重新执行默认并行 full suite 受影响的竞争/ownership 见证，
   不绕过默认并行调度，不使用 skip/ignore/串行替代。
3. 全量 Gate 重跑通过后方可继续 Task 6.1 其余阶段与 Iteration 008。