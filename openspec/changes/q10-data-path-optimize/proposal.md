# Q10 数据路径优化

> 2026-06-11
> Source: `.claude/analysis/optimization-opportunity-audit.md`
> Refs: ldisc.rs, tty/mod.rs

## 为什么做

读路径经过 5 次数据拷贝，其中 C3（driver ringbuf → InputReader read_buf）和 C4（InputReader read_buf → ldisc buf_tx）在同一个 `InputReader::poll()` 中，可合并。ldisc 缓冲仅 80 字节，限制突发吸收。Tty::read_at 中 ldisc 锁跨 async wait 持有，阻塞 poll/select 的 waker 注册。

## 做什么

| 子任务 | 描述 | 关键文件 |
|--------|------|----------|
| Q10.1 | 合并 C3/C4 拷贝 | `ldisc.rs` |
| Q10.2 | ldisc 缓冲扩容 80→256 | `ldisc.rs` |
| Q10.3 | ldisc 锁拆分 | `tty/mod.rs` + `ldisc.rs` |
| Q10.4 | 性能基准重测 | benchmark |
| Q10.5 | Gate Q10 | cargo test |

## BDD 默认假设

- G1: 保持 InputReader 逐字节处理结构，仅减少一次 memcpy（合并 push_slice）
- G2: BUF_SIZE 80→256，额外 ~350B 栈开销可接受
- G3: ringbuf 本身 SPSC lock-free，拆分后无竞态
- G4: total_read 用闭包内局部 Cell/RefCell 持有
