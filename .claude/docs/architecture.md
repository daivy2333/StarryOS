# architecture.md — 架构决策记录

> 由 project-rules-generator 初始化，由 project-docs-assistant 日常维护。
> 条目格式: <!-- A{编号} --> ### {DATE} - {决策标题}，每条含决策、原因、影响、替代方案。

---

<!-- A1 --> ### 2026-05-24 - 使用 axtask::future + embassy-sync::AtomicWaker，不引入完整 Embassy

- **决策**: 异步运行时基于 `axtask::future`（block_on + poll_io + register_irq_waker），仅引入 `embassy-sync::AtomicWaker` 用于 ISR 中安全唤醒
- **原因**: axtask 已有调度器，embassy-executor 会冲突；embassy-sync 无 OS 依赖可单独使用；Pipe/EventFd 已验证 axtask::future 模式
- **影响**: 不引入 embassy-executor/time，保持内核调度器独立性；需自己定义 AsyncUart trait
- **替代方案**:
  - ❌ 完整引入 Embassy (executor + HAL + sync) — 与 axtask 冲突
  - ❌ 仅用 embedded-io-async traits — 仍需自建 IRQ 绑定
  - ✅ axtask::future + AtomicWaker — 最小侵入，复用现有
- **风险**: 未来需要 Embassy HAL 高级特性（如 DMA 链）时需重新评估

---

<!-- A2 --> ### 2026-05-24 - 串口与控制台关系——独立硬件 /dev/ttyS0

- **决策**: 新增独立硬件串口（QEMU `-serial` 多路配置），注册为 `/dev/ttyS0`，不影响现有 `/dev/console`
- **原因**: 隔离风险，独立开发测试；初期不破坏控制台稳定性
- **影响**: 需在 QEMU 启动参数添加第二个 `-serial`；`/dev/console` 和 `/dev/ttyS0` 是两个独立设备
- **替代方案**:
  - ❌ A: 复用同一硬件替换 Console — 可能破坏控制台稳定性
  - ✅ B: 独立硬件独立设备 — 隔离风险
  - ⚠️ C: 替换 axhal::console 底层 — 远期选项，初期影响面太大
- **远期**: 异步框架成熟后可将 axhal::console 底层替换为同一 AsyncUart 实现

---

<!-- A3 --> ### 2026-05-24 - VFS 接口——DeviceOps trait

- **决策**: 实现 `DeviceOps` trait，通过 `Device` wrapper 注册到 `/dev`
- **原因**: 所有现有 `/dev` 设备都通过 DeviceOps 注册；Device struct 自动处理转换链；as_pollable() 提供 poll/select/epoll 支持
- **影响**: 注册代码与 event/fb 等设备一致；offset 参数对串口无意义（流设备）可忽略
- **替代方案**:
  - ❌ 直接 impl FileLike — 需重复实现 fd 管理逻辑，破坏现有模式
  - ✅ DeviceOps — 与现有设备一致，复用转换链

---

<!-- A4 --> ### 2026-05-24 - 缓冲策略——ringbuf::HeapRb + PollSet

- **决策**: 复用 `ringbuf::HeapRb<u8>` + `axpoll::PollSet`，每个方向各一个（rx_buf + rx_wakers / tx_buf + tx_wakers）
- **原因**: HeapRb 在 Pipe 中已验证；零额外依赖；Producer/Consumer 分离；默认 64 KiB 与 Pipe 一致
- **影响**: HeapRb 不是中断安全——硬件 FIFO 和内核 ringbuf 之间的搬运必须由单一后台协程完成；内存占用每端口 128 KiB
- **替代方案**:
  - ❌ 手写环形缓冲区 — 容易边界 bug，重复造轮子
  - ❌ embassy-sync::Channel — 额外依赖，每个元素需单独分配
  - ✅ ringbuf::HeapRb + PollSet — 已验证，零额外依赖

---

<!-- A5 --> ### 2026-05-24 - termios 支持——可切换，默认 raw

- **决策**: 默认 raw 模式，termios 作为可选功能通过 ioctl 动态启用
- **原因**: 高性能数据通道需要 raw 字节流零开销；终端交互需要 termios 行规则；两者兼得
- **影响**: 默认路径零开销；termios 启用时复用现有 Termios 和 ldisc 逻辑；行规则处理在 read_at/write_at 中完成
- **替代方案**:
  - ❌ 始终 raw — 无法支持终端应用
  - ❌ 始终 termios — 所有数据路径都有行规则开销
  - ✅ 可切换默认 raw — 高性能与功能兼得

---

<!-- A6 --> ### 2026-05-24 - 硬件抽象——AsyncUart trait

- **决策**: 定义 `AsyncUart` trait，初期实现 `Uart16550`，为 `DwApbUart` 等预留实现位
- **原因**: StarryOS 支持四架构（RISC-V/LoongArch/AArch64/x86_64），初期均用 16550 但未来可能需要其他型号
- **影响**: AsyncUart 使用 axtask::future 异步语义；可在此基础上实现 embedded-io-async::Read/Write 获得生态兼容性
- **替代方案**:
  - ❌ 直接使用 embedded-io-async — 缺少 IRQ/FIFO 信息
  - ❌ 硬编码 16550 — 不支持其他硬件
  - ✅ 自定义 AsyncUart trait — 精确匹配需求，可扩展

---

<!-- A7 --> ### 2026-05-25 - Console与AsyncUart共存策略——先独立后统一

- **决策**: 采用方案 C"先独立后统一"——AsyncUart 作为独立 `/dev/ttyS0` 设备，Console 保持不变；远期统一
- **原因**: 初期隔离风险，不破坏控制台稳定性；AsyncUart 可独立开发测试；远期框架成熟后可将 axhal::console 底层替换为 AsyncUart 实现
- **影响**: QEMU 需添加第二个 `-serial` 参数；Console 和 AsyncUart 操作不同硬件实例，无竞态；远期统一时需设计 Console 输出重定向到 AsyncUart TX 路径
- **替代方案**:
  - ❌ A: 复用同一硬件替换 Console — 可能破坏控制台稳定性
  - ❌ B: 替换 axhal::console 底层 — 初期影响面太大
  - ✅ C: 先独立后统一 — 隔离风险，渐进演化
- **远期**: 异步框架成熟后统一，参见 ADR-002

---

<!-- A8 --> ### 2026-05-25 - 中断分发架构——ISR → AtomicWaker → copier 任务

- **决策**: 采用模型 1"ISR → AtomicWaker → copier 任务"——ISR 极简原则，数据搬运推迟到任务上下文
- **原因**: ISR 中不能持有 Mutex 或做阻塞操作；HeapRb 的 Producer/Consumer 不是中断安全；copier 任务在任务上下文中可安全获取 Mutex；AtomicWaker 是 ISR 安全的（无锁，原子操作）。此模型源自 Embassy 中断→异步配合流程（learned.md L46）：外设完成→中断→HAL 路由信号→执行器通知→任务继续，我们将"HAL 路由"替换为 AtomicWaker::wake()，"执行器"替换为 axtask::future
- **影响**: ISR 只做三件事：读 IIR → 禁用已触发中断 → AtomicWaker.wake()；copier 任务是唯一操作硬件 FIFO 和 ringbuf 的角色，天然无竞态；需一个 RX copier 任务和一个 TX copier 任务
- **替代方案**:
  - ❌ 模型 2: ISR 直接写 ringbuf — HeapRb 非中断安全，数据竞争风险
  - ❌ 模型 3: 轮询模式 — CPU 空转，高延迟，不符合"高性能"目标
  - ✅ 模型 1: ISR → AtomicWaker → copier — ISR 极简，数据安全，已验证模式
- **参考**: Pipe 的 block_on(poll_io(...)) 模式、N_TTY 的 tty-reader copier 模式

---

<!-- A9 --> ### 2026-05-25 - uart_16550 依赖策略——本地最新版 path 依赖

- **决策**: 使用本地 `/home/daivy/projects/uart_16550` 最新版（v0.6.0），通过 `path = "../../uart_16550"` 依赖
- **原因**: 本地 v0.6.0 完整覆盖所有中断控制 API（set_interrupt_enable、interrupt_identification、InterruptType 枚举、try_send/try_receive、FifoTriggerLevel、Config）；不需要修改 axplat/axhal；便于后续直接对 uart_16550 crate 升级优化
- **影响**: kernel/Cargo.toml 添加 path 依赖；与 axhal 中已有的 uart_16550 v0.4.0 可能版本冲突，需处理（方案 C 下两者操作不同硬件实例，可共存）；发布时需解决路径依赖
- **替代方案**:
  - ❌ A: 直接依赖 crates.io v0.4.0 — 缺少中断控制 API，需自己封装
  - ❌ B: 发布本地 crate 到 crates.io 后统一升级 — 发布流程耗时，需上游协调
  - ✅ 本地最新版 path 依赖 — 立即可用，便于直接升级优化
- **关键发现**: 之前分析中认为"缺失"的 API 全部已在本地 v0.6.0 中提供
