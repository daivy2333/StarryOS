## 1. 固定基线与作用域

- [x] 1.1 在 `.claude/analysis/q32-console-cpu-efficiency-evidence/README.md` 记录分支、HEAD、dirty 状态和 toolchain；用于区分源码与旧产物；执行 `git status --short`、`git rev-parse HEAD` 和版本命令；预期得到可复现的 Console 起点。[Q32-R2]
- [x] 1.2 在 Q32 evidence README 记录 Q31 benchmark、D1 time helper、QEMU/D1 日志的固定 SHA-256；用于锁定只读 Async 输入；执行 `sha256sum` 并与 iteration 003 Plan Review 对照；预期 5 个 hash 全部一致。[Q32-R2, Q32-R10]
- [x] 1.3 在 `.claude/analysis/q32-console-cpu-efficiency-evidence/baseline/` 保存当前 Console benchmark 与 D1 time 源码 diff/hash witness；用于证明修复前状态；仅复制文本证据或记录 `git show`/`sha256sum` 输出；预期 witness 不修改产品文件。[Q32-R1, Q32-R2]
- [x] 1.4 用 `git diff -- tests/benchmark.c crates/axplat-riscv64-lichee-d1/src/time.rs crates/axplat-riscv64-lichee-d1/src/time_math.rs` 建立目标 allowlist；用于保护用户已有改动；逐项审查重叠；预期没有未归属的目标文件修改后再进入实现。[Q32-R2]

## 2. D1 时间换算 TDD

- [x] 2.1 在临时 host test harness 中先运行 Q31 的 12 个 `time_math` 边界测试对当前 D1 换算；用于建立 RED；保持产品源码不变并保存命令、退出码和输出；预期至少一个 24 MHz 用例因旧 1 MHz 假设失败。[Q32-R1]
- [x] 2.2 在 `crates/axplat-riscv64-lichee-d1/src/time_math.rs` 移植纯整数 24 MHz helper 与 12 个测试；用于固定换算边界；保持 `no_std` 可用且不引入依赖；预期 helper 覆盖零、余数、秒进位和大数。[Q32-R1]
- [x] 2.3 在 `crates/axplat-riscv64-lichee-d1/src/time.rs` 只替换 cycle-to-duration 接线；用于让平台时间读取使用已测 helper；保留 timer source 与其他平台语义；预期 diff 不包含非时间换算修改。[Q32-R1]
- [x] 2.4 重新运行 12 个 host 测试并保存 GREEN 输出；用于证明修复生效；记录编译命令、toolchain 和退出码；预期 12/12 通过且无 warning/error。[Q32-R1]

## 3. Console benchmark 契约移植

- [x] 3.1 在 `tests/benchmark.c` 以 Q31 固定版本为基线移植通用 helpers、raw schema 和 provenance；用于减少 harness 漂移；仅保留批准的 Console 差异；预期 diff allowlist 可逐项解释。[Q32-R2]
- [x] 3.2 在 `tests/benchmark.c` 移植 S11 write/completion 分段计时和字节校验；用于统一完成点；让同步 Console 返回路径保留真实 timing；预期不跨阶段重复累计轮数。[Q32-R3]
- [x] 3.3 在 `tests/benchmark.c` 移植 S41 五轮 CPU-work 测量；用于产生同 payload 的 `instret` raw 数据；沿用 `/proc/instret` 并标注 hart-wide proxy；预期每个有效 payload 有五轮 raw 与 summary。[Q32-R4]
- [x] 3.4 在 `tests/benchmark.c` 移植 S42 五轮 overlap 测量；用于展示同步 Console 的并发边界；允许 `overlap_ns=0`；预期零 overlap 不触发除零或错误状态。[Q32-R5]
- [x] 3.5 在 `tests/benchmark.c` 移植 S43 idle/loaded timer overshoot；用于比较 timer interference；无真实 overlap 时跳过 loaded aggregate；预期输出 `not-applicable` 而非伪零。[Q32-R6]
- [x] 3.6 在 `tests/benchmark.c` 增加 Console capability state；用于区分缺失诊断和真实零；将 S40/local TX counters 标为 `not-available`；预期不调用不存在的 Console counter API。[Q32-R7]
- [x] 3.7 在 `tests/benchmark.c` 统一短写、byte mismatch、completion error、timeout、取消和零分母处理；用于隔离坏样本；所有 summary 只累计 valid rounds；预期错误样本保留状态但不污染聚合。[Q32-R8]
- [x] 3.8 在 `tests/benchmark.c` 保留 `BENCH_BACKEND=polling-console` whitelist、通用标题及原有兼容 section；用于防止误测 Async backend；预期不包含 writer/TTY/polling/lock 产品实现修改。[Q32-R2, Q32-R7]

## 4. 静态与 host Gate

- [x] 4.1 对 S11 raw 字段和完成顺序运行 `rg`/小型 parser assertion；用于验证调用窗口与 completion 分离；预期 completed bytes 只在完整成功后计入。[Q32-R3]
- [x] 4.2 对 S41 payload、五轮循环、`instret` delta 和 summary 公式运行 parser assertion；用于锁定 Q31 口径；预期常量、单位和分母全部匹配。[Q32-R4]
- [x] 4.3 对 S42 零 overlap 构造 host 边界输入；用于验证同步路径；预期样本有效且派生逻辑无除零。[Q32-R5]
- [x] 4.4 对 S43 无 overlap、timer error 和 timeout 构造 host 边界输入；用于验证排除规则；预期 loaded 为 `not-applicable`，失败样本不进入 summary。[Q32-R6, Q32-R8]
- [x] 4.5 扫描 Console capability 输出和比较字段；用于防止用零伪装缺失诊断；预期所有 unsupported counter 都有明确状态。[Q32-R7]
- [x] 4.6 用 host compiler 构建 benchmark，并执行 warning、格式串和故障路径检查；用于尽早发现 C 级错误；预期编译成功且 parser assertions 全部通过。[Q32-R3, Q32-R8]
- [x] 4.7 审查 `git diff` 的产品路径 allowlist；用于执行 scope gate；预期除 benchmark 和 D1 time 两个模块外没有产品代码变化。[Q32-R10]

## 5. QEMU 证据

- [x] 5.1 按 `.claude/runbooks/qemu-build.md` 在普通 host shell 重建 benchmark、ELF、rootfs/image；用于避免 restricted compiler 假失败；记录全部命令和退出码；预期产物时间戳与源码对应。[Q32-R9]
- [x] 5.2 在 `.claude/analysis/q32-console-cpu-efficiency-evidence/console/` 记录 QEMU binary/image hash 与启动配置；用于建立虚拟环境 provenance；预期 manifest 可从零复现启动输入。[Q32-R9]
- [x] 5.3 在 QEMU 运行 benchmark 并保存未经编辑的 serial log；用于验证 section 协议和错误边界；预期 S11/S41/S42/S43、五轮 summary 和 terminal marker 完整。[Q32-R3, Q32-R4, Q32-R5, Q32-R6, Q32-R8, Q32-R9]
- [x] 5.4 用 parser 复算 QEMU raw/summary 并扫描 mismatch、drain error、timeout 与无效样本；用于完成 QEMU gate；预期协议通过，结论明确限定为 smoke validation。[Q32-R8, Q32-R9]

## 6. D1 实板证据

- [x] 6.1 用已通过 QEMU gate 的源码重建 D1 benchmark、ELF 和 boot image；用于保证实板输入同源；保存构建命令、退出码和 hash；预期 image manifest 与 QEMU manifest 指向同一源码。[Q32-R9]
- [x] 6.2 按既有烧录流程写入明确标识的 D1 image，并记录串口设备、波特率和板级事实；用于防止采到旧 image；预期启动日志能确认 Q32 benchmark identity。[Q32-R9]
- [x] 6.3 在 D1 运行时间 sanity 检查；用于验证 24 MHz helper 已接入真实平台；比较已知 delay/request 与 actual；预期不存在约 24 倍系统性误差。[Q32-R1]
- [x] 6.4 在 D1 运行完整 benchmark 并保存未经编辑的 serial log；用于取得真实 UART 数据；预期 S11/S41/S42/S43 按能力矩阵完成，失败样本不进汇总。[Q32-R3, Q32-R4, Q32-R5, Q32-R6, Q32-R7, Q32-R8, Q32-R9]
- [x] 6.5 用 parser 复算 D1 raw/summary，并扫描 terminal marker、字节完整性、timeout 和有效轮数；用于完成实板 gate；预期每个可比较 section 有完整 provenance 与有效结论。[Q32-R8, Q32-R9]

## 6A. D1 Timer IRQ Feature Repair

> Iteration 002 Plan Review accepted two deviations: no standalone 5ms timer smoke (full S43 suffices),
> and direct `/irq` enable instead of composite `lichee-d1-runtime-irq` feature.

- [x] 6A.1 S43 hang log frozen to Q32 evidence (iteration-000 dir).[Q32-R9,Q32-R11]
- [x] 6A.2 Feature RED confirmed: fullbench-command used only `irq-if` (stub), same as smoke.[Q32-R11]
- [x] 6A.3 `Cargo.toml`: fullbench-command/userbench/fullbench add `axplat-riscv64-lichee-d1/irq`. DEVIATION: no composite `lichee-d1-runtime-irq`; three features directly enable `/irq`. Accepted in Plan Review.[Q32-R11]
- [x] 6A.4 Feature matrix: smoke=`irq-if` only, runtime=`irq`+`riscv_plic`, no Async UART in any mode.[Q32-R11]
- [x] 6A.5 smoke/userbench/fullbench-command builds pass; ELF/image format verified.[Q32-R9,Q32-R11]
- [x] 6A.6 DEVIATION: no standalone 5ms timer smoke. Full S43 idle 5/5 PASS (250 absolute sleeps) provides equivalent readiness proof. Accepted in Plan Review.[Q32-R6,Q32-R11]
- [x] 6A.7 D1 full benchmark re-run: S43 idle 5/5 PASS, loaded 5/5 not-applicable, S10-S42 no regression. Done+exit 0.[Q32-R3–Q32-R9,Q32-R11]

## 7. 横向比较与文档

> Comparison 报告由用户自行生成。S43 aggregate 标记 `not-independently-recomputed`（每组仅 3/50 raw samples）。

- [x] 7.1 Q31/Q32 source/log/binary/image hashes finalized in evidence README.[Q32-R2, Q32-R10]
- [ ] 7.2 comparison common-field table — 用户自行生成。[Q32-R10]
- [ ] 7.3 QEMU/D1 separation + backend-specific fields — 用户自行生成。[Q32-R7, Q32-R9, Q32-R10]
- [ ] 7.4 `/proc/instret` limitations + no CPU utilization claims — 用户自行生成。[Q32-R4, Q32-R10]
- [ ] 7.5 Independent recalculation + S43 hash-anchored check — 用户自行生成。[Q32-R10]

## 8. 收口验证

- [x] 8.1 `git diff --check` + allowlist review.[Q32-R10]
- [x] 8.2 `openspec validate` strict/changes/specs all PASS.[Q32-R10]
- [x] 8.3 Act Response with changed files, evidence, deviations.[Q32-R9, Q32-R10]
