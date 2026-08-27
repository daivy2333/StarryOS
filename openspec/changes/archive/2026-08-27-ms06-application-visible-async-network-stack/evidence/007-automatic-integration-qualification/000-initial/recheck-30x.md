# Recheck: 用户二次授权 30× full suite 复核（未复现 SIGSEGV）

- Change: ms06-application-visible-async-network-stack
- Iteration: 007-automatic-integration-qualification
- Cycle: 000-initial
- Captured at: 2026-08-27
- Revision: `832abfead57e7ae0870d5b729b6875665d588582`（工作树；本 Cycle 无产品代码修改）
- Environment: host x86_64；axnet 独立 target；按 K44 使用
  `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh"`
- Trigger: EV-007-000-01（run 16 SIGSEGV）后，用户要求复核（原话："这次我们再次
  一次性执行三十次，看看是否有类似错误"）。

## 命令

```bash
for i in $(seq 1 30); do
  timeout 120 env RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh" \
    cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib
done
```

## 决定性结果

- 30/30 迭代全部 `test result: ok. 371 passed; 0 failed`，exit 0，无 crash。
- 单次 wall 5.0–10.7s（无 timeout、无挂起、无死循环）。
- 结合 EV-007-000-01 的 run 1–15：连续 45 次通过中出现 1 次 SIGSEGV（run 16），
  复核 30× 未复现 → 维持"非确定性、未归因"结论，概率窗口低于 1/31。

## 结论与边界

- 提供了足够的 fresh 失败率观察：首次 crash 未在 30× 复核窗口内再现。
- 不因此判定 crash 已修复或归因；残余内存安全事件仍须 Plan 裁决是否接受
  （默认并行 full suite 的偶发 SIGSEGV 与 Task 6.1「无 flake 豁免」Acceptance 的关系）。
- 本文件不覆盖 EV-007-000-01（blocker.md 保留原始现场与恢复条件描述）。