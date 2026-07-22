# Iteration 002: D1 Timer IRQ and S43 Revalidation

## Plan Context

- Status: ready-for-audit
- Round: 002
- Parent: `iterations/001-evidence-correction-and-d1-comparison.md`

**Objective**

修复 D1 Console runtime 的 timer IRQ feature wiring。先用最小 absolute sleep 证明任务可被
唤醒，再重跑 S43，取得完整的 idle 与 Console loaded applicability 证据。

**Background**

Iteration 001 的 D1 日志在 S43 首个 idle group 永久等待。Review 证明 fullbench-command
只启用了 `axplat-riscv64-lichee-d1/irq-if`，实际使用 `irq_stub.rs`。axruntime 注册 timer
handler 时收到 false，但继续启动；SBI deadline 到达后没有平台 handler 调用
`axtask::on_timer_tick()`。

Q31 Async fullbench-command 继承平台 `/irq`，使用真实 `irq.rs`，因此相同 S43 能完成。
Async UART 只是在 feature graph 中间接带入 IRQ 能力，不应重新加入 Console。

**Current Baseline**

- Branch: `console-lichee`；HEAD: `73b8973ad5ae198a07ce730f830b6d6e1db93718`。
- Root feature `lichee-d1`: `axplat-riscv64-lichee-d1/irq-if`。
- userbench/fullbench/fullbench-command 均继承 `lichee-d1`，没有平台 `/irq`。
- smoke 与三种 runtime 的 feature graph 当前相同：platform `default + irq-if`。
- `irq_stub.rs`: register=false，handle=None，set_enable no-op。
- `irq.rs`: stimer enable、S_TIMER handler table、trap dispatch 和 PLIC 已存在，并在 Q31 D1
  验证过。
- `time_math.rs` 与 Q31 hash 一致，12/12 host tests PASS。
- Console D1 RED log: `docs/d1_console.md`，停在 `s43-phase=idle`。
- Console S42: write≈554.66 ms，line window=542.535 ms，overlap=0。
- Async S42: write≈1.6 ms；Async S43 loaded write≈0.49–0.53 ms，存在 backlog window。

**Relevant Code**

- 根 `Cargo.toml`：`lichee-d1*` feature composition。
- `crates/axplat-riscv64-lichee-d1/src/lib.rs`：`irq.rs` 与 `irq_stub.rs` 的 cfg 选择。
- `crates/axplat-riscv64-lichee-d1/src/init.rs::init_later`：真实 IRQ percpu init gate。
- `crates/axplat-riscv64-lichee-d1/src/irq.rs`：S_TIMER register/enable/handle 与 PLIC。
- `crates/axplat-riscv64-lichee-d1/src/irq_stub.rs`：smoke-only link stub。
- `tests/benchmark.c::collect_abs_sleep_samples`：S43 absolute sleep 调用。
- `tests/benchmark.c::main`：新增 fail-fast timer wake smoke 的位置。

**Critical Path**

```text
freeze S43 hang log + image hash
  -> feature graph RED
  -> add D1 runtime IRQ composite
  -> smoke/runtime feature matrix GREEN
  -> target build matrix
  -> QEMU timer smoke
  -> D1 5 ms timer wake smoke
  -> D1 full S43
  -> evidence/hash/OpenSpec checks
```

**Implementation Guidance**

1. 先冻结当前 D1 hang log、benchmark ELF 和 boot image hash，不覆盖 RED。
2. 在根 `Cargo.toml` 增加 `lichee-d1-runtime-irq`。它继承 `lichee-d1` 并启用
   `axplat-riscv64-lichee-d1/irq`。
3. userbench、fullbench、fullbench-command 改为继承 runtime IRQ 组合。smoke 继续继承
   `lichee-d1`，不得启用 `/irq` 或 `riscv_plic`。
4. 不修改 `irq.rs`、`irq_stub.rs`、`time.rs`、`clock_nanosleep` 或 scheduler。若现有
   `irq.rs` 在 build/boot gate 失败，停止并返回 Plan，不在本轮扩展 IRQ 架构。
5. 在 benchmark 的测量 section 前增加一次 5 ms absolute timer wake smoke。调用前输出并
   drain request/deadline，返回后输出 actual/overshoot/rc。该结果只做 readiness Gate。
6. QEMU 先验证新 section 和输出协议。QEMU 不证明 D1 timer/PLIC。
7. D1 先运行 timer smoke。它未返回时停止，不再等待完整 benchmark。
8. timer smoke 通过后运行 S43。idle 5/5 必须 PASS。Console loaded write 若耗尽 line
   window，5 groups 必须为 `not-applicable`，不得产生 loaded aggregate。
9. 复查 S10–S42，确认 backend 仍为 `polling-console`，没有 UART IRQ telemetry 或 Async
   section 被启用。

**Invariants**

- 不启用 `lichee-d1-async-uart`，不恢复 UART copier/IRQ/ring/waker。
- polling Console 的同步 write、CONSOLE_LOCK、drain 和 TTY 语义不变。
- smoke 保持无 PLIC 的最小 bring-up 边界。
- 不修改 Q31 evidence，不用 QEMU 代替 D1 timer 证据。
- 不把 Console loaded `not-applicable` 当作失败，也不伪造 loaded aggregate。
- 不处理 S43 percentile 的最终 comparison 口径；留给 D1 evidence Review。

**Non-goals**

- 拆分 timer-only IRQ backend。
- 修改 axruntime 对 IRQ register failure 的通用处理。
- 优化 timer overshoot、Console throughput 或调度策略。
- 生成最终 Async/Console comparison。
- 更新全局状态或归档 change。

**Acceptance**

- A1 [Q32-R9,Q32-R11] S43 hang log、source、ELF、image hash 已冻结，feature RED 可复现。
- A2 [Q32-R11] 三种 D1 runtime feature graph 含平台 `irq` 与 `riscv_plic`；smoke 只有
  `irq-if`，所有 Console modes 均不含 Async UART feature。
- A3 [Q32-R11] smoke、userbench、fullbench-command build 通过；ELF/image 格式与 entry
  合法，smoke 最小启动边界不变。
- A4 [Q32-R6,Q32-R11] QEMU 和 D1 timer wake smoke 返回成功；D1 request=5 ms，输出
  actual、overshoot 和 rc，不再永久等待。
- A5 [Q32-R6,Q32-R9] D1 S43 idle 5/5 PASS；loaded 根据同步 write 判为 5/5
  `not-applicable reason=no-overlap-window`，且没有 loaded aggregate。
- A6 [Q32-R3–Q32-R9] S10–S42 correctness 保持；S42 zero overlap 有效；backend 仍为
  `polling-console`，counter 仍为 unsupported/not-available。
- A7 [Q32-R1–Q32-R11] host tests、feature matrix、target builds、OpenSpec strict/global
  validations 和 `git diff --check` 全部通过。

**Verification**

```bash
# RED/GREEN feature matrix
cargo tree -e features -p starryos \
  --features lichee-d1-smoke --target riscv64gc-unknown-none-elf \
  -i axplat-riscv64-lichee-d1
cargo tree -e features -p starryos \
  --features lichee-d1-fullbench-command --target riscv64gc-unknown-none-elf \
  -i axplat-riscv64-lichee-d1

# Existing host/target regressions
gcc -Wall -Wextra tests/benchmark_classify_test.c -o /tmp/q32-classify-test
/tmp/q32-classify-test
rustc --test crates/axplat-riscv64-lichee-d1/src/time_math.rs \
  -o /tmp/q32-time-test
/tmp/q32-time-test
cargo check --package axplat-riscv64-lichee-d1 \
  --target riscv64gc-unknown-none-elf --features irq

make lichee
make lichee-userbench
make lichee-fullbench-command
file tests/benchmark kernel/resources/benchmark.elf \
  starry-lichee-fullbench-command-boot.img
readelf -h kernel/resources/benchmark.elf
readelf -r kernel/resources/benchmark.elf
sha256sum tests/benchmark kernel/resources/benchmark.elf \
  starry-lichee-fullbench-command-boot.img

openspec validate q32-console-cpu-efficiency-benchmark --strict
openspec validate --changes
openspec validate --specs
git diff --check
```

D1 运行证据必须保存完整串口日志和显式 benchmark exit。先检查 timer smoke，再检查 S43
idle/loaded group count、aggregate presence、Done、byte correctness 和 drain errors。

**Risks and Notes**

- 完整 `/irq` 会初始化 PLIC，但不会自动注册 UART IRQ handler。feature 与日志必须证明
  polling backend 未改变。
- 若 smoke feature 出现 `irq`/`riscv_plic`，Gate 失败；不能以 smoke 也能启动为理由接受。
- 若 timer smoke 仍 hang，当前 feature 根因不完整。保留日志并返回 Plan，不修改 sleep 为忙等。
- 若启用 `/irq` 后在 PLIC init 前 fault，停止在平台 Gate，不运行 benchmark。

## Act Response

- Status: pending

**Implemented**

Pending.

**Changed Files and Symbols**

Pending.

**Deviations from Plan**

Pending.

**Verification Evidence**

Pending.

**Remaining Issues**

Pending.

**Commit or Diff Reference**

Pending.

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

Pending.
