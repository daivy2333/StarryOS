# Q32 Console CPU 效率收尾同步清单

本文用于把 `console-lichee` 的 Q32 测试结果同步回 `uart-lichee`，并收口 Q31/Q32。
测试与日志采集已经完成；本文只列同步、证据冻结和归档动作，不要求重新运行 QEMU 或 D1。

当前证据锚点是 `console-lichee` 提交 `f61def3`。不要整体 cherry-pick 该提交：它包含
Console 产品代码、二进制、日志、Q31 输入和分支专属状态。

## 同步前封口

先在 `console-lichee` 完成以下动作，并创建一个独立的文档收尾提交：

- 将 `docs/qemu_console.md` 冻结为
  `.claude/analysis/q32-console-cpu-efficiency-evidence/console/qemu-rootfs.log`。
- 将 `docs/d1_console.md` 冻结为
  `.claude/analysis/q32-console-cpu-efficiency-evidence/console/d1-fullbench-command.log`。
- 更新 Q32 evidence README 中的 HEAD、源码、日志、binary、ELF 和 image hash；删除
  “D1 evidence pending”等过期状态。
- 在 `comparison/` 生成 Q31 Async 与 Q32 Console 的 common-field 表和结论边界。
- 根据实际证据核销 Q32 tasks；独立 5 ms timer smoke 和 runtime composite feature 未实施，
  应记录为接受的 deviation，不得勾成已执行。
- 保留 iteration 002 Plan Review；不要移动实际日志后改写其 hash。

最新 Console 输入如下：

| 输入 | SHA-256 |
| --- | --- |
| `tests/benchmark.c` | `32656017a293fcf3607de520632a53c3500b8b0dc3d9db8a204a7b0a8343e377` |
| `crates/axplat-riscv64-lichee-d1/src/time.rs` | `580f6cce22c881d936df783155e3a60689ea74e061b5b3bdbbd62d05a490b9ec` |
| `crates/axplat-riscv64-lichee-d1/src/time_math.rs` | `7839991923685473b85711ef87d8cc871024644f3a59a24e9dff27ca762bfd43` |
| QEMU Console log | `67b7bb0260b717ad91adee3112c65bbc308f44a2d2a681dcc05ffad0094e227c` |
| D1 Console log | `b3f11fce62696e92077cd3f9693520708df739f42ed755f1eab8ffb513555aaf` |
| benchmark binary | `2ce5c072870d1fab7b4f47c742d0408834a2a0a7607296246c06c3a94c6894a2` |
| D1 benchmark ELF | `2f0d869a0c558d02031630de7668f7119a7be11c42e10bb895f0a10e72d5387b` |
| D1 boot image | `1e85b6127a1e75306d5969d4eedb7cab50a795d55f20450f932820585a309bad` |

Q31 Async QEMU/D1 日志 hash 分别为 `a9ce8a34...` 和 `50a2a876...`。comparison 必须引用
`.claude/analysis/q31-cpu-efficiency-evidence/async/` 的冻结日志，不得引用临时 `docs/` 副本。

## 必须同步回 uart-lichee

完成封口后，只从文档收尾提交恢复这些路径：

| 路径 | 用途 |
| --- | --- |
| `openspec/changes/q32-console-cpu-efficiency-benchmark/` | Q32 proposal、design、spec、tasks、iterations 和最终 Review |
| `.claude/analysis/q32-console-cpu-efficiency-evidence/` | Console baseline、QEMU/D1 冻结日志、hash manifest 和 comparison |
| `.claude/analysis/q31-console-cpu-efficiency-port.md` | Console 移植范围、差异和根因分析 |
| `docs/q32-console-cpu-efficiency-doc-sync.md` | 本清单及归档入口 |

如需保留人工阅读副本，可同步 `docs/qemu_console.md` 与 `docs/d1_console.md`，但权威输入必须是
evidence 目录中的冻结日志。`docs/` 副本后续允许被新实验覆盖，不能作为归档 hash 的目标路径。

## 不同步产品代码和产物

以下内容具有 Console 分支语义，不应回写 `uart-lichee`：

- `Cargo.toml` 中 polling Console runtime 的 IRQ feature 接线。
- `tests/benchmark.c`、`tests/benchmark_classify.h` 和测试二进制。Async 分支保留 Q31 版本；
  comparison 依靠冻结日志和 source hash 对齐，不靠覆盖源码。
- `crates/axplat-riscv64-lichee-d1/src/time.rs` 与 `time_math.rs`。Q31 已有同一
  `time_math.rs` hash，不能用 Console 文件覆盖 Async 分支。
- `kernel/resources/benchmark.elf`、`tests/benchmark` 和
  `starry-lichee-fullbench-command-boot.img`。记录 hash 即可，不提交或跨分支复制生成物。
- `.claude/docs/SNAPSHOT.md`、`.claude/docs/tasks.md`、Console polling capability 和
  Console 专属归档。目标分支按自身状态手工维护。

不要整体覆盖 `.claude/analysis/README.md`、`openspec/specs/references/spec.md` 或
`openspec/specs/improvements/spec.md`。这些索引都需要在目标分支手工合并。

## comparison 必须保留的边界

最终报告应把 QEMU 与 D1 分开。QEMU 只证明协议、构建和运行路径，不用于判断硬件性能。
D1 报告至少包含：

| 指标 | Async | Console |
| --- | ---: | ---: |
| S41 64 B instructions/byte | 32818.08 | 1194.25 |
| S41 256 B instructions/byte | 32792.23 | 1105.27 |
| S41 1024 B instructions/byte | 44715.58 | 1105.50 |
| S42 median overlap efficiency | 0.5353 | 0.0000 |
| S43 idle P50 | 9.533 ms | 8.424 ms |
| S43 loaded P50 | 25.782 ms | not-applicable |

这些值是 CPU-work proxy 和 workload 行为，不是 CPU utilization。Console S42 的 0 是同步写
没有剩余 overlap window；Console S43 loaded 是 not-applicable，不能补成 0。S43 日志每组
只输出三个 raw sample，aggregate 必须标记 `reported, hash-anchored,
not-independently-recomputed`。

## 索引与状态手工合并

在 `uart-lichee` 手工完成：

- `.claude/analysis/README.md` 增加 Q32 evidence、Console port 分析和 comparison 入口。
- `openspec/specs/references/spec.md` 增加最终 comparison 的 R 引用；保留目标分支已有 R 项。
- `.claude/docs/SNAPSHOT.md` 记录 Q31/Q32 已形成同口径 D1 对照，并写明 QEMU 限制。
- `.claude/docs/tasks.md` 关闭 CPU-efficiency 采集任务；将归档动作保留为唯一待办，直到归档完成。
- 如 comparison 形成可复用结论，再由 docs maintainer 判断是否提升到 knowledge；不要在同步时
  直接复制 Console 分支的全局状态。

## change 核销与归档

归档前使用对应 OpenSpec skill 完成以下顺序：

1. Review Q31/Q32 tasks，确认已完成项、接受的 deviation 和不适用项都有证据。
2. 同步两项 change 的 delta specs；不要用 Q32 Console spec 覆盖 Q31 Async spec。
3. 先归档 `q31-async-uart-cpu-efficiency-benchmark`，验证通过后再归档
   `q32-console-cpu-efficiency-benchmark`。
4. 更新 SNAPSHOT、tasks 和 references 中的 active/archive 路径。
5. 归档后再次运行全局 OpenSpec validation 和链接检查。

本文不授权直接归档。只有在冻结日志、comparison、tasks 核销和索引更新全部完成后，才调用
`openspec-archive-change`。

## 同步后检查

```bash
sha256sum \
  .claude/analysis/q32-console-cpu-efficiency-evidence/console/qemu-rootfs.log \
  .claude/analysis/q32-console-cpu-efficiency-evidence/console/d1-fullbench-command.log
rg -n 'S41|S42|S43|not-applicable|not-independently-recomputed' \
  .claude/analysis/q32-console-cpu-efficiency-evidence/comparison
rg -n 'q31|q32|cpu-efficiency' \
  .claude/analysis/README.md .claude/docs/SNAPSHOT.md .claude/docs/tasks.md
openspec validate q31-async-uart-cpu-efficiency-benchmark --strict
openspec validate q32-console-cpu-efficiency-benchmark --strict
openspec validate --changes
openspec validate --specs
git diff --check
git status --short
```

检查结果必须满足：

- QEMU/D1 Console 冻结日志 hash 分别为 `67b7bb02...` 和 `b3f11fce...`。
- Q31 Async 日志未被改写，Q31/Q32 common-field 值可回溯到各自 D1 日志。
- comparison 没有把 QEMU 当硬件证据，没有把 unavailable/not-applicable 补零。
- Console 产品代码、二进制和分支专属状态没有进入 `uart-lichee`。
- 两个 change 在归档前 strict validation 通过；归档后 active/archive 路径一致。
