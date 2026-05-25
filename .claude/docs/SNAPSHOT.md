# SNAPSHOT.md - 项目快照

> Generated at 2026-05-24
> Last updated: 2026-05-24

---

## 当前状态

**Phase**: P0 (基礎设施) - 准备中
**Status**: 分支已创建，文档体系已建立，尚未开始编码
**Branch**: feat/uart-async (from main)

---

## 项目结构

```
StarryOS/
├── kernel/src/
│   ├── config/           # 内核配置
│   ├── entry.rs          # 内核入口
│   ├── file/             # 文件系统核心
│   │   ├── pipe.rs       # 异步管道（参考）
│   │   └── event.rs      # EventFd（参考）
│   ├── lib.rs            # 模块注册
│   ├── mm/               # 内存管理
│   ├── pseudofs/         # 伪文件系统
│   │   └── dev/          # /dev 设备注册
│   ├── syscall/          # 系统调用
│   └── task/             # 任务管理
├── docs/uart-async/      # 设计文档（7 个 md）
├── .claude/docs/         # 开发文档体系
│   ├── tasks.md          # 任务跟踪
│   ├── learned.md        # 学习记录
│   ├── references.md     # 参考资料
│   ├── architecture.md    # 架构决策
└── CLAUDE.md             # 项目约束规则
```

---

## 技术栈

| 类别 | 技术 | 版本 |
|------|------|------|
| 语言 | Rust | nightly-2026-02-25 |
| 目标 | RISC-V 64-bit | qemu-riscv64 |
| 异步 | axtask::future | 0.3.0-preview.2 |
| 轮询 | axpoll | 0.1.2 |
| 硬件 | NS16550 UART | QEMU virt |
| 缓冲 | ringbuf | 0.4.8 |
| 构建 | Make + Cargo | - |

---

## Git 状态

**当前分支**: feat/uart-async
**基线分支**: main (2e075ac)
**未提交更改**: CLAUDE.md + .claude/docs/ (新增)

---

## 关键文件

| 文件 | 作用 | 状态 |
|------|------|------|
| kernel/src/file/pipe.rs | 异步管道参考 | 稳定（已验证 poll_io 模式） |
| kernel/src/file/event.rs | EventFd 参考 | 稳定（已验证 Pollable 模式） |
| kernel/src/pseudofs/device.rs | 设备注册框架 | 稳定（DeviceOps trait） |
| kernel/src/drivers/serial/ | 新增串口驱动 | 待创建 |
| docs/uart-async/*.md | 设计文档 | 已完成（Draft） |

---

## 当前工作

### 进行中

- [x] 创建 feat/uart-async 分支
- [x] 建立 .claude/docs 文档体系
- [x] 生成 CLAUDE.md 项目约束

### 待办

- [ ] P0.1: Embassy 运时时集成到内核
- [ ] P0.2: 中断框架搭建
- [ ] P1.1: Ring Buffer 实现
- [ ] P1.2: UartAsyncDriver 核心结构
- [ ] P1.3: 中断驱动收发集成

### 阻塞

- 无

---

## 技术决策记录

| 决策 | 选择 | 原因 | 时间 |
|------|------|------|------|
| 异步运行时 | axtask::future + embassy-sync::AtomicWaker | 最小侵入，复用现有 | 2026-05-24 |
| 与控制台关系 | 独立硬件 /dev/ttyS0 | 隔离风险 | 2026-05-24 |
| VFS 接口 | DeviceOps trait | 与现有设备一致 | 2026-05-24 |
| 缓冲策略 | ringbuf::HeapRb + PollSet | 已验证，零额外依赖 | 2026-05-24 |
| termios | 可切换，默认 raw | 高性能与功能兼得 | 2026-05-24 |
| 硬件抽象 | AsyncUart trait | 可扩展多硬件 | 2026-05-24 |

---

## 性能目标

| 指标 | 目标 | 基线 |
|------|------|------|
| 最大波特率 | 1 Mbps (可扩展至 2 Mbps) | - |
| RX 延迟 | < 500 µs | 115200 bps |
| 吞吐量 | > 90% 线速 | 115200 bps |
| CPU 利用率（空闲） | 0% | - |
| 多端口并发 | 4 端口 | - |
| 缓冲区大小 | 可配置，默认 64 KiB | - |

---

## 最近修改

| 时间 | 文件 | 改动类型 |
|------|------|----------|
| 2026-05-24 | CLAUDE.md | 新增（项目约束规则） |
| 2026-05-24 | .claude/docs/tasks.md | 新增（任务跟踪） |
| 2026-05-24 | .claude/docs/learned.md | 新增（学习记录） |
| 2026-05-24 | .claude/docs/references.md | 新增（参考资料） |
| 2026-05-24 | .claude/docs/architecture.md | 新增（架构决策） |

---

## 下一步

1. **P0.1**: 在 kernel/Cargo.toml 添加 embassy-sync 依赖
2. **P0.2**: 封装 RISC-V PLIC 中断接口，实现 UART 中断注册
3. **验证**: `make run` 编译通过，中断回调可触发
