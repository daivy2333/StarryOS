# Q10 任务清单

- [ ] **Q10.1** 合并 C3/C4 拷贝 — `ldisc.rs:82-124` InputReader::poll 中消除 read_buf→buf_tx 的中间拷贝
- [ ] **Q10.2** ldisc 缓冲扩容 — `ldisc.rs:24` BUF_SIZE 80→256
- [ ] **Q10.3** ldisc 锁拆分 — `tty/mod.rs:96` ldisc.lock() 不跨 block_on；`ldisc.rs:328` read 不要求 &mut self
- [ ] **Q10.4** 性能基准重测 — benchmark 对比 Q5.1/Q8 基线
- [ ] **Q10.5** Gate Q10 — cargo test + clippy + QEMU 启动验证

## 验收标准

- [ ] `cargo check` 0 错误
- [ ] `cargo clippy` 0 新增 warning
- [ ] QEMU Shell 交互正常
- [ ] benchmark 无退化
