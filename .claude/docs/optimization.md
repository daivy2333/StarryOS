# optimization.md — 优化记录

> 由 project-rules-generator 初始化，由 project-docs-assistant 日常维护。
> 条目格式: <!-- O{编号} --> - {问题描述}，每条含当前影响、建议方案。

---

## 远期优化方向

<!-- O1 --> - 零拷贝 RX 路径
  - 当前影响: 硬件 → ringbuf → 用户空间，两次 memcpy，高吞吐场景下拷贝开销占比显著
  - 建议方案: 让用户空间直接映射 ringbuf 的物理页（需 VMA 支持）
  - 优先级: 中 | 阶段: M6

<!-- O2 --> - NAPI 风格批量轮询
  - 当前影响: 中断频率在 >2 Mbps 时可能过高，高波特率下 CPU 占用上升
  - 建议方案: 中断触发后切换到轮询模式，处理完毕后切回中断
  - 优先级: 高 | 阶段: M4

<!-- O3 --> - DMA 支持
  - 当前影响: CPU 搬运数据占用周期，高吞吐场景 CPU 占用高
  - 建议方案: 通过 virtio-console 流式 DMA 卸载 CPU
  - 优先级: 高 | 阶段: M6

<!-- O4 --> - 中断合并 (coalescing)
  - 当前影响: 每个 FIFO 阈值触发一次 IRQ，高中断频率场景 IRQ 开销累积
  - 建议方案: 硬件级 FCR 阈值调大 + 软件级延迟合并
  - 优先级: 中 | 阶段: M4

<!-- O5 --> - 优先级调度
  - 当前影响: 后台协程可能被其他任务抢占，延迟抖动增大
  - 建议方案: 若 axtask 支持优先级，提高协程优先级；否则可参考 Embassy InterruptExecutor 多优先级模式，在 axtask 之上实现多优先级调度域
  - 优先级: 低 | 阶段: M4+

<!-- O6 --> - ringbuf 溢出策略
  - 当前影响: 当前丢弃溢出数据，数据丢失无恢复
  - 建议方案: 添加溢出统计 + 流控通知（CRTSCTS）
  - 优先级: 中 | 阶段: M2

<!-- O7 --> - uart_16550 crate 后续优化（详见 uart_16550/.claude/docs/optimization.md O6-O8）
  - 批量读写 API (try_receive_batch/try_send_batch) — 减少逐字节 MMIO 访问 | M4
  - FIFO 深度可配置化 — 适配非标准 16550 兼容芯片 | M5
  - DMA 模式寄存器完整控制 — M6 阶段 DMA 传输所需 | M6

<!-- O15 --> - ldisc 行编辑性能优化
  - 当前影响: canonical mode 下逐字符处理（CR/NL 转换、信号检查、echo 输出），每个字符都触发 output_char
  - 建议方案: raw 模式零开销跳过所有行编辑；canonical mode 批量处理而非逐字符
  - 优先级: 中 | 阶段: M2 (termios raw 模式)
  - 参考: kernel/src/pseudofs/dev/tty/terminal/ldisc.rs InputReader::poll()

<!-- O16 --> - PTY ringbuf 性能优化
  - 当前影响: PTY buffer 仅 4096 字节，高频读写时唤醒频繁；SpinNoPreempt 锁有开销
  - 建议方案: 增大 PTY buffer 至 64 KiB（与 AsyncUart 对齐）；考虑无锁 ringbuf 原子操作
  - 优先级: 低 | 阶段: M4 (性能优化，与 AsyncUart buffer 对齐)
  - 参考: kernel/src/pseudofs/dev/tty/pty.rs PTY_BUF_SIZE

<!-- O17 --> - 中断分发效率优化
  - 当前影响: register_irq_waker 通过全局 BTreeMap 查找 IRQ 号，每次中断有查找开销
  - 建议方案: IRQ 号直接映射（数组索引）而非 BTreeMap；批量中断处理减少 PLIC claim/complete MMIO 延迟
  - 优先级: 中 | 阶段: M4 (与 NAPI 批量处理协同)
  - 参考: axtask::future::poll.rs POLL_IRQ BTreeMap

<!-- O18 --> - 系统调用层批量读取
  - 当前影响: 每次 read() 都有 block_on(poll_io) 包装，waker 注册/唤醒有开销
  - 建议方案: VFS 层批量读取接口；raw 模式零拷贝路径（直接从 rx_buf 到用户空间）
  - 优先级: 低 | 阶段: M2+ (与 termios raw 模式协同)

---

## 性能洞察

<!-- O19 --> ### 中断频率
- FCR 阈值 14 字节时，115200 bps 下 ~823 IRQ/秒
- ISR 开销 < 100 ns（清 IIR + AtomicWaker::wake + mret）
- 1 Mbps 下 ~7,143 IRQ/秒，CPU 占用 ~3.57%

<!-- O8 --> ### 延迟分解
- RX 总延迟 = T_ISR + T_WAKE + T_DRAIN + T_COPY + T_RETURN
- 目标: < 500 µs @ 115200 bps
- 瓶颈通常在 T_WAKE（协程调度延迟）

---

## 性能基准目标

<!-- O9 --> 吞吐量 @115200: > 10 KB/s (90% 线速) | 测量: 5 秒批量传输
<!-- O10 --> 延迟 P50: < 500 µs | 测量: 100 次单字节往返取中位数
<!-- O11 --> 延迟 P99: < 2 ms | 测量: 同上取 99 百分位
<!-- O12 --> 空闲 CPU: 0%（完全挂起） | 测量: 无数据 10 秒检查 CPU 统计
<!-- O13 --> 数据完整性: 100% 匹配 | 测量: 1 MB 随机数据 MD5 校验
<!-- O14 --> 内存泄漏: < 1 KB 增长 | 测量: 10,000 次 open/close 后
