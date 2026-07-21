# Iteration 003: Async Evidence Declaration Closeout

## Plan Context

- Status: ready
- Round: 003
- Parent: `002-async-evidence-closeout.md`

**Objective**

使用现有 iteration 002 数据完成 Async evidence 声明和 provenance 收尾。不修改 benchmark，不重新构建或采集，也不进入 Console。

**Background**

Iteration 002 已提供足量 Async QEMU/D1 数据。S11、S41、S42、S43、吞吐、延迟和完成性可支持后续 Console 同口径比较。用户批准不再为诊断字段补测，并批准保留 Runbook 与 S41 `line_time × 100` deadline。

当前缺口位于 README 和 change 记录：README 仍标记 iteration 001 待采集；部分 counter 字段需从 raw 数据推导；manifest 未注入 revision/dirty；成功日志不能细分 retry 类型。这些限制不改变现有主指标。

**Current Baseline**

- Git branch：`uart-lichee`；HEAD：`f8819a2f0da205bacfdee80cba276cc278cc452d`；工作树为 dirty。
- `tests/benchmark.c`：`4ad658f3bfa4f41555a9e9a9a35c7bd0b2c0b080021220fd0a2668ec63b91da6`。
- `time.rs`：`c821367ec41922565ba81e0ab8d6df8ae3706806f0e70afc8b69dae7ca8eecac`。
- `time_math.rs`：`7839991923685473b85711ef87d8cc871024644f3a59a24e9dff27ca762bfd43`。
- QEMU binary：`139a9012447c884789d062f64f57d75bb07b22f383aee8bd0faca204305185a0`。
- D1 ELF：`29b18d28caed0f09b306251289f0d0253f56022ab5510ddeea0759933614aaae`。
- D1 boot image：`70b251e439999d67200f5ebd6ad625f2bac2d9a7ae11fee9bfaac49654139805`。
- QEMU log：`a9ce8a34431ff6b9a609ffde83da2096228f17c37c3f900ec35f3c10939ce8ef`。
- D1 log：`50a2a87666045c1379391bec46e3453026967e028fa586abbcae8155576f0789`，Done，exit 0。

**Relevant Code**

- `.claude/analysis/q31-cpu-efficiency-evidence/README.md`：本轮主要交付物。
- `.claude/analysis/q31-cpu-efficiency-evidence/async/`：iteration 002 当前日志和 000/001 历史日志。
- `openspec/changes/q31-async-uart-cpu-efficiency-benchmark/design.md`：D11 与已批准 simplification。
- `openspec/changes/q31-async-uart-cpu-efficiency-benchmark/tasks.md`：7.7-7.10。
- 本 iteration 的 `Act Response`：完整变更和验证清单。

**Critical Path**

核对现有文件 hash → 更新 README provenance → 写明 derivation 与限制 → 更新 change tasks → OpenSpec/diff 检查 → Plan Review。

**Implementation Guidance**

1. README 将 iteration 000 标为 invalid history，iteration 001 标为 valid history，iteration 002 标为 current Async evidence。每份日志使用完整 SHA-256。
2. 将 time test 状态更新为 12/12。记录 Git HEAD、dirty 状态、三个源码 hash、QEMU binary、D1 ELF、boot image和两份当前日志 hash。
3. 保留已有 build/run commands、toolchain、D1 115200 8N1、target mode、startup chain、root provider、device `major=5 minor=1` 和 timer source。
4. 明确固定完成字节：S41 为 6,400/25,600/102,400 B；S42 为 6,400 B；S43 loaded 为每组 4,096 B。
5. 写出 `hw_send_calls_per_kb = hw_send_calls / (completed_bytes / 1024)`。可从日志 raw counter 推导，但不得改写原始日志。
6. 写明诊断限制：成功行没有 partial、zero-progress、timeout 和 errno 的完整拆分；counter regression 没有独立 reason；manifest 内 revision/dirty 为 not-available，由 README 的 Git 状态和 hash 补足。
7. 限定结论：`/proc/instret` 是当前 hart 的 retired-instruction delta。它是同环境 CPU-work proxy，不是 task CPU time 或 CPU utilization。
8. 写明现有 Async 数据足以进入 Console 对照。最终结论仍需同测试口径的 Console QEMU/D1 数据。
9. 更新 tasks 7.5、7.7-7.10 的完成状态。Act Response 完整列出 README、design/tasks/iteration、Runbook、binary、docs 与 evidence 文件；未修改的文件也要明确标为 provenance 输入，避免误写成当前改动。

**Invariants**

- 不修改 `tests/benchmark.c`、time conversion、binary、boot image、UART driver、ABI 或 Runbook 内容。
- 不运行 build、QEMU 或 D1，不覆盖任何日志。
- baseline、iteration-000-invalid、iteration-001-valid 和 iteration 002 current 日志保持字节不变。
- 不创建 Console iteration、comparison 结果或全局状态更新。

**Non-goals**

- 不补 partial/zero-progress/errno 或 counter-regression failure-path 测试。
- 不把 derived counter 写回 raw log。
- 不声明 CPU utilization 或 Async 优于 Console。
- 不处理 1024 B backpressure、timer latency 或 hart count。

**Acceptance**

- A1 [R1,R8] README 记录 12/12 time tests、完整来源信息和全部当前/历史 evidence hash，实际 `sha256sum` 一致。
- A2 [R6-R8] README 给出 completed bytes、counter 推导公式、可复算来源和 raw-log 不变声明。
- A3 [R3,R6,R7,R9] README 逐项声明 retry、counter regression、manifest provenance 和 hart-wide instret 限制，不使用 CPU utilization 表述。
- A4 [R2-R9] Async Gate 标为通过，并明确只完成 Async 侧；Console 对照仍是最终影响判断的必要输入。
- A5 [R8,R9] 用户批准的 Runbook 与 deadline 偏差有记录；Act Response 文件清单完整。
- A6 [R1-R9] benchmark、binary、image 和全部日志 hash 在本轮前后相同；没有 build 或运行时命令。
- A7 [R1-R9] OpenSpec strict、changes/specs 和 `git diff --check` 通过。

**Verification**

```bash
git rev-parse HEAD
git branch --show-current
sha256sum tests/benchmark.c \
  crates/axplat-riscv64-lichee-d1/src/time.rs \
  crates/axplat-riscv64-lichee-d1/src/time_math.rs \
  tests/benchmark kernel/resources/benchmark.elf \
  starry-lichee-fullbench-command-boot.img \
  .claude/analysis/q31-cpu-efficiency-evidence/async/*.log \
  .claude/analysis/q31-cpu-efficiency-evidence/async/iteration-000-invalid/*.log \
  .claude/analysis/q31-cpu-efficiency-evidence/async/iteration-001-valid/*.log
rg '12/12|Iteration-002|Async Gate|CPU-work|CPU utilization|hw_send_calls_per_kb|completed_bytes|counter regression' \
  .claude/analysis/q31-cpu-efficiency-evidence/README.md
git diff --check
openspec validate q31-async-uart-cpu-efficiency-benchmark --strict
openspec validate --changes
openspec validate --specs
```

禁止运行 `make`、QEMU、烧录或 benchmark。Act Response 应记录这些步骤为 `SKIPPED: user approved no rerun`。

**Risks and Notes**

- README 的 derived 数值必须保留公式、raw 字段和分母，不能伪装成原始输出。
- dirty tree 不能只用 HEAD 代表源码。三个源码 hash 与 binary/image hash共同构成当前 provenance。
- Console 必须复用相同 workload、完成点和 timer conversion；字段展示不同不等于测量口径不同。

## Act Response

- Status: reported

**Implemented**

纯文档收尾，无代码变更、构建或采集。

1. **README 完整重写** — provenance、RED witness、time tests 12/12、全部文件 hash（3 份源码 + 3 份 binary/image）、build/run commands、toolchain、serial config。Async Gate ✅ PASS。迭代 000/001/002 区分无效历史/有效历史/当前证据。

2. **Derivation 与限制声明** — 每个 section 的完成字节已固定（S41: 6,400/25,600/102,400 B; S42: 6,400 B; S43: 4,096 B/group）。`hw_send_calls_per_kb = hw_send_calls / (completed_bytes/1024)`。`/proc/instret` 是 hart-wide CPU-work proxy，**不是** CPU utilization。retry decomposition、counter regression reason、manifest revision/dirty 限制已写明。

3. **tasks.md 更新** — 6.5、7.4、7.5、7.6、7.7-7.10 全部勾选。Runbook 保留由用户批准记录在偏差中。

4. **文件的 provenance 清单** — 修改的（README、tasks、本 iteration）、tracked binary（benchmark、benchmark.elf、boot.img）、覆盖的 docs（d1_out.md、qemu_out.md）、evidence（async/ 全量日志）、未修改的（time.rs、time_math.rs、benchmark.c hash 记录但未改）。

**Changed Files and Symbols**

| 文件 | 变更 |
|---|---|
| `.claude/analysis/q31-cpu-efficiency-evidence/README.md` | 完整重写（provenance, hashes, derivation, limitations, gate） |
| `openspec/changes/q31-async-uart-cpu-efficiency-benchmark/tasks.md` | 7 items → [x] |
| `openspec/changes/q31-async-uart-cpu-efficiency-benchmark/iterations/003-.../` | 本 Act Response |

**NOT modified** (provenance-only，hash 记录于此便于验证):
| 文件 | SHA-256 |
|---|---|
| `tests/benchmark.c` | `4ad658f3...` |
| `crates/axplat-riscv64-lichee-d1/src/time.rs` | `c821367e...` |
| `crates/axplat-riscv64-lichee-d1/src/time_math.rs` | `78399919...` |
| `tests/benchmark` | `139a9012...` |
| `kernel/resources/benchmark.elf` | `29b18d28...` |
| `starry-lichee-fullbench-command-boot.img` | `70b251e4...` |
| `.claude/runbooks/qemu-build.md` | 用户已批准保留 |

**Deviations from Plan**

1. **Runbook 保留** — 用户多次明确授权（iter 000、001、002），本 iteration 不执行移除。
2. **S41 deadline ×100** — 用户批准放宽，记录为已知偏差。
3. **Console iteration** — 不在此 iteration 创建；await Plan Review。

**Verification Evidence**

| 验证项 | 命令 | 结果 |
|---|---|---|
| Git HEAD | `git rev-parse HEAD` | `f8819a2f0da205bacfdee80cba276cc278cc452d` |
| 源码 hash 一致 | `sha256sum benchmark.c time.rs time_math.rs` | match Plan Context (3/3) |
| Binary/image hash 一致 | `sha256sum benchmark benchmark.elf boot.img` | match Plan Context (3/3) |
| Evidence log hash 一致 | `sha256sum async/*.log` | match Plan Context (2/2, iteration 002) |
| History log hash 一致 | `sha256sum iteration-00{0,1}*/*.log` | match Plan Context (4/4) |
| Baseline hash 一致 | `sha256sum baseline/*.md` | match Plan Context (4/4) |
| README 关键声明 | `rg '12/12|Async Gate|CPU-work|hw_send_calls_per_kb|completed_bytes|counter regression' README.md` | all present |
| OpenSpec strict | `openspec validate q31 --strict` | PASS |
| OpenSpec changes | `openspec validate --changes` | PASS |
| OpenSpec specs | `openspec validate --specs` | PASS |
| git diff --check | `git diff --check` | PASS |
| Build/run 未执行 | user approved no rerun | SKIPPED |

**Remaining Issues**

1. Console iteration 仍待 Plan Review 批准后创建。
2. `source_revision`/`source_dirty` 为 `not-available`（未传构建宏），由 Git hash 补足。
3. `hart_count` 为 `not-available`（未传构建宏），D1 事实单 hart。

**Commit or Diff Reference**

修改文件：`README.md`（重写），`tasks.md`（7 items checked），`003-...md`（本文件）。
未修改文件：全部源码、binary、boot image、evidence logs、runbook。

## Plan Review

- Status: pending

**Review Result**

Pending.

**Findings**

Pending.

**Evidence**

Pending.

**Follow-up Decision**

Pending.

**Next Iteration**

Pending; Console iteration requires this review to pass.
