# tasks.md — 任务追踪

> 由 project-rules-generator 初始化，由 project-docs-assistant 日常维护。
> 条目格式: <!-- T{编号} --> 标记开头，支持 grep 精确定位。

---

## 进行中

<!-- 添加时格式: <!-- T{编号} --> - [ ] {任务描述} -->

### Phase 0: 基础设施

<!-- T0.1 --> - [ ] Embassy 运行时集成到内核
  - 添加 embassy-sync 依赖到 kernel/Cargo.toml
  - 验证 axtask::future + AtomicWaker 可协作
  - 验证：异步任务可调度执行

<!-- T0.2 --> - [ ] 中断框架搭建
  - 封装 RISC-V 中断控制器（PLIC）接口
  - 实现 UART 中断号注册和回调机制
  - 验证：中断回调可触发

**Gate P0**: `make run` 编译通过 + 中断回调可触发

---

## 待办

<!-- 添加时格式: <!-- T{编号} --> - [ ] {任务描述} -->

### Phase 1: 异步串口驱动

<!-- T1.1 --> - [ ] Ring Buffer 实现
  - 实现 MPSC 无锁环形缓冲区
  - 支持 split 模式（读/写独立）
  - 验证：单线程单元测试通过

<!-- T1.2 --> - [ ] UartAsyncDriver 核心结构
  - 封装 MMIO 寄存器操作
  - 实现 interrupt-driven read/write
  - 验证：基本收发可用

<!-- T1.3 --> - [ ] 中断驱动收发集成
  - RX 中断 → 填充 ring buffer → 唤醒 waker
  - TX 中断 → 从 ring buffer 取数据 → 写入硬件
  - 验证：echo 回环测试通过

**Gate P1**: echo 回环测试通过（10s 稳定运行无丢失）

### Phase 2: DMA 传输

<!-- T2.1 --> - [ ] DMA 缓冲区管理
  - PageBox 对齐分配
  - 物理地址映射
  - 验证：分配/释放正确

<!-- T2.2 --> - [ ] 流式 DMA 收发
  - virtio-console DMA 通道配置
  - 零拷贝读取路径
  - 验证：大数据块传输校验通过

<!-- T2.3 --> - [ ] DMA + 中断混合策略
  - 小数据走中断，大数据走 DMA
  - 阈值可配置
  - 验证：混合模式切换正确

**Gate P2**: 1MB 数据传输校验通过 + 性能提升可测量

### Phase 3: 内核集成

<!-- T3.1 --> - [ ] 替换现有串口驱动
  - 兼容现有 `console::putchar`/`getchar` 接口
  - 渐进式替换，保持回退能力
  - 验证：内核启动串口输出正常

<!-- T3.2 --> - [ ] 系统调用对接
  - read/write syscall 支持 uart fd
  - 支持 poll/epoll 异步通知
  - 验证：用户态程序可读写串口

<!-- T3.3 --> - [ ] 文件系统集成
  - uart 设备注册到 devfs
  - 支持 open/close/read/write
  - 验证：`cat /dev/uart` 可工作

**Gate P3**: 内核完整启动 + 用户态串口交互正常

### Phase 4: 性能优化

<!-- T4.1 --> - [ ] 批量传输优化
  - write 批量合并（coalescing）
  - read 预取策略
  - 验证：吞吐量提升可测量

<!-- T4.2 --> - [ ] 自适应策略
  - 根据负载动态调整中断/DMA 阈值
  - 低功耗模式支持
  - 验证：不同负载下策略切换正确

<!-- T4.3 --> - [ ] 性能基准
  - 建立吞吐量/延迟基准
  - 与原轮询驱动对比
  - 验证：性能指标达标

**Gate P4**: 性能基准达标 + 稳定性 24h 测试通过

### 依赖说明

- P1 依赖 P0 完成
- P2 依赖 P1 完成
- P3 依赖 P1 完成（可与 P2 并行）
- P4 依赖 P2 + P3 完成

---

## 阻塞项

<!-- 添加时格式: <!-- T{编号} --> - {阻塞描述} - {原因} -->

<!-- TB1 --> - 无
