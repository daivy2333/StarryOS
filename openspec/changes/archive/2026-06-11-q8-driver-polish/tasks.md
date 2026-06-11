# Q8 任务清单

## Wave 1 — 正确性修复（P0）

- [ ] **Q8.1** NAPI 退出修复 — `async_driver.rs:51` 添加 `total==0` 时重置 `consecutive=0` + `enable_rx_intr()`
  - QA: QEMU 启动后停止向串口发数据，确认 CPU 归零
- [ ] **Q8.2** ISR 去锁化 — `isr.rs:9-10` 消除 `uart_instance().lock()`
  - QA: ISR 仍正常触发，RX/TX copier 工作正常
- [ ] **Q8.3** IER 写路径规范化 — `uart_init.rs:72` 用 uart_16550 API 替代裸 `write_volatile`
  - QA: IER 读写与 CACHED_IER 一致，无规则违规
- [ ] **Q8.3a** uart_16550 添加 `set_ier()` — `uart_16550/src/lib.rs`
  - QA: `cargo build` 通过

## Wave 2 — 热路径优化（P1）

- [ ] **Q8.4** copier waker 去重简化 — `async_driver.rs:53-55,82-84`
  - QA: benchmark 无退化
- [ ] **Q8.5** DRAIN_WAKER 条件唤醒 — `isr.rs:20` + `ctl.rs`
  - QA: tcdrain 功能正常，无性能退化

## Wave 3 — O46 AtomicWaker 推广（P2）

- [ ] **Q8.6** signalfd PollSet→AtomicWaker — `signalfd.rs`
  - QA: signalfd 读/ poll 正常
- [ ] **Q8.7** event PollSet→AtomicWaker — `event.rs`
  - QA: eventfd 读写正常
- [ ] **Q8.8** pipe PollSet→AtomicWaker — `pipe.rs`
  - QA: pipe 读写 / poll / close 正常
- [ ] **Q8.9** pidfd PollSet→AtomicWaker — `pidfd.rs` + `task/mod.rs` + `task/ops.rs` + `wait.rs`
  - QA: pidfd poll 正常，进程退出时正确唤醒
- [ ] **Q8.10** 性能回归测试 — benchmark 对比 Q5.1 基线
- [ ] **Q8.11** Gate Q8 — `cargo test` + `cargo clippy` + benchmark 全部通过

## 验收标准

- [ ] `cargo build` 通过
- [ ] `cargo test` 通过
- [ ] `cargo clippy` 0 新增 warning
- [ ] QEMU `make run` 内核正常启动，Shell 交互正常
- [ ] NAPI 退出后 CPU 归零（top/cycles 验证）
- [ ] ISR 中无锁操作
- [ ] IER 路径通过 uart_16550 API
- [ ] benchmark 性能不低于 Q5.1 基线
