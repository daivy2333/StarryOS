# tasks.md — 任务追踪

> 由 project-rules-generator 初始化，由 project-docs-assistant 日常维护。
> 条目格式: <!-- T{编号} --> 标记开头，支持 grep 精确定位。

---

## Milestone 概览

| Milestone | 目标 | Gate | 依赖 |
|-----------|------|------|------|
| **M0** | 基础设施就绪 | `make run` + 中断回调触发 | — |
| **M1** | 中断驱动串口可用 | echo 回环 10s 稳定无丢失 | M0 |
| **M2** | 异步 API + VFS 集成 | 用户态读写 /dev/ttyS0 | M1 |
| **M3** | Console 统一 | Console 底层替换为 AsyncUart | M2 |
| **M4** | 性能优化 | 性能基准达标 + 稳定性测试 | M2 |
| **M5** | 真板验证 | VisionFive2 实际验证 | M4 |
| **M6** | DMA 探索 | DMA 通道可用（远期） | M4 |

---

## M0: 基础设施就绪

> 目标: 让内核具备异步串口所需的所有底层能力——中断路由、AtomicWaker、uart_16550 本地依赖

<!-- T0.1 --> - [ ] 添加 uart_16550 本地 path 依赖到 kernel/Cargo.toml
  - 覆盖所有中断控制 API（set_interrupt_enable、interrupt_identification、InterruptType）
  - 与 axhal 中 uart_16550 v0.4.0 共存（两者操作不同硬件实例）
  - 验证: cargo check 编译通过

<!-- T0.2 --> - [ ] 添加 embassy-sync 依赖到 kernel/Cargo.toml
  - 仅引入 embassy-sync::AtomicWaker，不引入 executor/time
  - 验证与 nightly-2026-02-25 兼容性
  - 验证: cargo check 编译通过

<!-- T0.3 --> - [ ] QEMU 添加第二个串口
  - Makefile/qemu.mk 添加 `-serial mon:stdio` + 第二 `-serial` 配置
  - axconfig 添加第二 UART 的 MMIO 地址和 IRQ 号
  - 验证: QEMU 启动后两个串口均可见

<!-- T0.4 --> - [ ] UART 中断注册与回调触发验证
  - 通过 register_irq_waker 或 register_irq_hook 注册第二 UART 的 IRQ
  - ISR 读取 IIR 判断中断源（RX/TX），触发 AtomicWaker::wake()
  - 验证: 在 QEMU 中向第二串口发送字符，内核中断回调触发

**Gate M0**: `make run` 编译通过 + 第二串口中断回调可触发

---

## M1: 中断驱动串口可用

> 目标: 中断驱动的串口收发工作，双 Ring Buffer + copier 任务模型验证通过

<!-- T1.1 --> - [ ] Ring Buffer 实现
  - rx_buf + tx_buf 各一个 HeapRb<u8>（默认 64 KiB）
  - rx_wakers + tx_wakers 各一个 PollSet
  - 验证: 单线程单元测试通过（push/pop/wake 流程）

<!-- T1.2 --> - [ ] AsyncUart trait + Uart16550 实现
  - 定义 AsyncUart trait（try_read/try_write/enable_rx_intr/disable_rx_intr/...）
  - Uart16550<MmioBackend> 实现 AsyncUart
  - 验证: cargo check 通过，MMIO 操作封装正确

<!-- T1.3 --> - [ ] ISR → AtomicWaker → copier 任务模型实现
  - ISR: 读 IIR → 禁用已触发中断 → AtomicWaker.wake()
  - RX copier: poll_fn 循环，被唤醒后从硬件 FIFO 读到 rx_buf，唤醒 rx_wakers
  - TX copier: poll_fn 循环，从 tx_buf 读到硬件 FIFO，使能 TX 中断
  - 验证: 中断到来 → copier 任务唤醒 → 数据搬运完成

<!-- T1.4 --> - [ ] Echo 回环测试
  - RX copier 收数据 → 写到 tx_buf → TX copier 发出
  - 验证: echo 回环 10s 稳定运行无丢失

**Gate M1**: echo 回环测试通过（10s 稳定运行无数据丢失）

---

## M2: 异步 API + VFS 集成

> 目标: 用户态程序可通过 /dev/ttyS0 正常读写串口，poll/select/epoll 可用

<!-- T2.1 --> - [ ] UartAsyncDriver 实现 DeviceOps
  - read_at: block_on(poll_io(...)) → 从 rx_buf 读取
  - write_at: block_on(poll_io(...)) → 写入 tx_buf
  - as_pollable: 返回 Some(self)，支持 poll/select/epoll
  - 验证: DeviceOps 接口编译通过

<!-- T2.2 --> - [ ] 注册 /dev/ttyS0 到 devfs
  - 在 pseudofs/dev/mod.rs 的 builder 中添加 ttyS0 设备
  - 验证: 内核启动后 /dev/ttyS0 可 open

<!-- T2.3 --> - [ ] 用户态串口交互验证
  - 用户态程序 open("/dev/ttyS0") → read → write
  - poll/epoll 监听串口事件
  - 验证: 用户态读写正常，异步通知工作

<!-- T2.4 --> - [ ] termios 支持（可切换，默认 raw）
  - 默认 raw 模式零开销
  - ioctl TCGETS/TCSETS 可动态启用 termios 行规则
  - 验证: raw 模式数据正确，termios 模式 Ctrl+C 等特殊字符处理

**Gate M2**: 用户态程序读写 /dev/ttyS0 正常 + poll/epoll 可用

---

## M3: Console 统一

> 目标: Console 底层替换为 AsyncUart 实现，消除轮询路径，所有串口输出走异步

<!-- T3.1 --> - [ ] Console 输出重定向到 AsyncUart TX 路径
  - axhal::console::write_bytes → 写入 AsyncUart tx_buf（不再直接 MMIO）
  - 内核启动早期保留 earlycon（直接 MMIO），后续切换到异步路径
  - 验证: 内核启动串口输出正常，无遗漏

<!-- T3.2 --> - [ ] Console 输入统一
  - N_TTY 的 Console reader 替换为 AsyncUart rx_buf 读取
  - tty-reader copier 与 AsyncUart RX copier 合理对接
  - 验证: 键盘输入正确传递到用户态

<!-- T3.3 --> - [ ] 移除第二串口（回归单硬件）
  - QEMU 配置恢复单串口，Console 和 AsyncUart 共用同一硬件
  - 验证: 单硬件模式下 Console + 用户态串口均正常

**Gate M3**: Console 与 AsyncUart 统一，内核完整启动 + 用户态串口交互正常

---

## M4: 性能优化

> 目标: 性能基准达标，CPU 空闲时零占用，吞吐量接近线速

<!-- T4.1 --> - [ ] 批量传输优化
  - uart_16550 try_receive_batch/try_send_batch API（减少逐字节 MMIO）
  - write coalescing: 多个短 write 合并为一次 tx_buf 写入
  - 验证: 吞吐量提升可测量

<!-- T4.2 --> - [ ] NAPI 风格批量轮询
  - 中断触发后切换到轮询模式处理 FIFO 残留数据
  - 处理完毕后切回中断等待
  - 验证: 高波特率下 IRQ 频率降低，吞吐量不降

<!-- T4.3 --> - [ ] 空闲 CPU 零占用
  - 无数据时 copier 任务挂起（poll_fn 返回 Pending）
  - CPU 进入 WFI 休眠
  - 验证: 无数据 10s，CPU 统计显示 0% 串口占用

<!-- T4.4 --> - [ ] 性能基准建立
  - 吞吐量 @115200: > 10 KB/s (90% 线速)
  - 延迟 P50 < 500 µs, P99 < 2 ms
  - 数据完整性: 1 MB 随机数据 MD5 校验
  - 验证: 基准指标达标

**Gate M4**: 性能基准达标 + 与原轮询驱动对比有明显提升

---

## M5: 真板验证

> 目标: 在 VisionFive2 等真实硬件上验证异步串口驱动

<!-- T5.1 --> - [ ] VisionFive2 平台适配
  - 确认板上 UART 型号（是否 16550 兼容）
  - AsyncUart trait 支持 DwApbUart 等不同硬件
  - 验证: 交叉编译通过

<!-- T5.2 --> - [ ] 真板中断验证
  - PLIC 中断号与 QEMU 不同，需适配
  - 验证: 板上中断回调触发

<!-- T5.3 --> - [ ] 真板串口收发验证
  - echo 回环测试
  - 性能基准测试
  - 验证: 板上串口工作正常

**Gate M5**: 真板串口收发正常 + 性能数据可信

---

## M6: DMA 探索（远期）

> 目标: 探索 DMA 传输通道，为极高吞吐量场景做准备

<!-- T6.1 --> - [ ] DMA 通道可行性评估
  - 评估 virtio-console DMA 模式是否可用于串口数据传输
  - 评估 StarryOS 内存管理是否支持 PageBox 对齐分配
  - 验证: 可行性报告输出

<!-- T6.2 --> - [ ] DMA 缓冲区管理（如果可行）
  - PageBox 对齐分配
  - 物理地址映射
  - 验证: 分配/释放正确

<!-- T6.3 --> - [ ] 流式 DMA 收发（如果可行）
  - 零拷贝读取路径
  - 大数据块传输校验
  - 验证: 数据校验通过

<!-- T6.4 --> - [ ] DMA + 中断混合策略
  - 小数据走中断，大数据走 DMA
  - 阈值可配置
  - 验证: 混合模式切换正确

**Gate M6**: 1 MB 数据 DMA 传输校验通过 + 与纯中断模式性能对比

---

## 依赖关系

```
M0 → M1 → M2 → M3（统一 Console）
               ↘ M4 → M5 → M6
```

- M2 完成后，M3 和 M4 可并行推进
- M3（统一 Console）风险较高，建议在 M4 稳定后再做
- M5 需要真实硬件，实习期间进行
- M6 是远期探索，依赖 M4 稳定基础

---

## 阻塞项

<!-- 添加时格式: <!-- T{编号} --> - {阻塞描述} - {原因} -->

<!-- TB1 --> - 无