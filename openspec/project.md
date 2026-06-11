# Project Context — StarryOS

## 项目概览

StarryOS 是一个基于 RISC-V 的宏内核操作系统，使用 Rust 编写，基于 ArceOS 组件化架构。核心目标是**实现高性能异步串口通信**。当前活跃分支：`asyncuart-dev`（基于 `feat/uart-async-dev2` 整合分支）。

## 技术栈

- **语言**：Rust（nightly-2026-02-25，edition 2024）
- **目标平台**：RISC-V 64-bit（qemu-riscv64），四架构支持（RISC-V / LoongArch / AArch64 / x86_64）
- **OS 内核框架**：ArceOS 0.3.0-preview.2（axfeat / axalloc / axconfig / axdisplay / axdriver / axfs-ng / axhal / axinput / axlog / axmm / axnet-ng / axruntime / axsync / axtask）
- **异步框架**：`axtask::future` + `embassy_sync::AtomicWaker`（**不引入** embassy-executor）
- **串口驱动**：`uart_16550`（本地 path 依赖，v0.6.0）
- **构建**：Make + Cargo
- **测试**：QEMU 模拟 + 内核态 / 用户态 benchmark（`kernel/src/drivers/benchmark.rs` + `tests/benchmark.c`）
- **格式化 / 静态分析**：`rustfmt` + `clippy`
- **工具链**：RISC-V musl 工具链 `/opt/musl/riscv64-linux-musl-cross`

## 项目目录结构

```
StarryOS/
├── Cargo.toml              # workspace 配置（axfeat 等版本锁定）
├── Cargo.lock
├── Makefile
├── rust-toolchain.toml     # nightly-2026-02-25
├── rustfmt.toml
├── .axconfig.toml          # MMIO 范围、内存布局
├── kernel/                 # starry-kernel crate（核心实现）
│   └── src/
│       ├── drivers/        # 串口、GPU、输入等驱动
│       │   └── serial/     # 异步串口实现（Q1~Q7 落地处）
│       ├── file/           # VFS 文件抽象
│       │   └── pipe.rs     # 异步 I/O 模式参考
│       ├── pseudofs/       # 伪文件系统
│       │   └── dev/        # 设备节点
│       │       └── tty/    # TTY / ldisc / termios
│       └── syscall/        # 系统调用实现
├── src/main.rs             # 内核入口
├── tests/benchmark.c       # 用户态性能基准
├── scripts/benchmark.sh    # 自动化 benchmark
├── docs/                   # 设计文档与分析
│   └── analysis/           # 深度分析文档（13 份）
├── openspec/               # OpenSpec 规范（本次初始化新增）
│   ├── config.yaml
│   ├── project.md          # 本文件
│   ├── specs/              # 4 个 domain
│   │   ├── architecture/
│   │   ├── learned/
│   │   ├── references/
│   │   └── optimization/
│   │   # ~~rules/~~ — 2026-06-03 归档至 changes/archive/，规则全文整合到 ../CLAUDE.md
│   └── changes/            # 变更提案（含 rules domain 墓碑）
├── .claude/                # Claude Code 配置
│   ├── docs/               # 状态文档（SNAPSHOT / tasks / archive）
│   ├── skills/             # OpenSpec skills
│   ├── commands/           # OpenSpec slash commands
│   └── settings.local.json
└── ../uart_16550/          # 串口驱动子项目（独立 crate）
```

## 项目约束

### Git 约束

- 提交信息格式：`feat(uart-async): / fix(uart-async): / refactor(uart-async): / docs(uart-async):`
- 分支策略：`main ← feat/uart-async-*`（PR 合并）
- 当前活跃分支：`asyncuart-dev`（基于 `feat/uart-async-dev2`）
- **禁止把 Claude 列为 co-author / 共同创作者**（任何形式 `Co-Authored-By: Claude`）

### 编码约束

- 不修改任何外部 crate（`axfeat` / `axhal` / `axplat` / `axtask` / `axpoll` / `embassy-sync` / `uart_16550`）— 全部实现在 `kernel/src/drivers/serial/`
- NS16550 寄存器 stride **必须为 1**（范围 0x00-0x07）
- ISR 必须极简：读 ISR → 禁用中断 → `AtomicWaker::wake()` → 返回
- 数据搬运必须由单一后台协程完成，ISR 禁止直接操作 ring buffer
- `unsafe` 块必须有 `// SAFETY:` 注释

### 性能约束

- 115200 bps 上限 = 11.52 KB/s（硬件理论极限）
- QEMU 不仿真串口线延迟，吞吐量测试在 QEMU 上**不可信**（必须用真板验证吞吐量）
- Async 不可能超过阻塞 Console 的吞吐量；优势在不阻塞调用方
- 公平性能对比必须统一数据量与测试方法

### 架构约束

- 异步串口 vs Console 共存：Async 负责 Shell I/O / 用户态；Console 负责内核日志 / 早期启动 / panic
- VFS 设备注册必须用 `DeviceOps` trait + `Device` wrapper
- 缓冲必须用 `ringbuf::HeapRb` + `axpoll::PollSet`
- 异步运行时必须用 `axtask::future` + `AtomicWaker`，**不引入** `embassy-executor`

## OpenSpec 工作约定

- 所有产出物 MUST 用简体中文撰写
- 技术术语（API、ADR、MMIO、ISR、IRQ、TTY 等）保持英文原样
- OpenSpec 验证器要求每条 Requirement MUST 含大写 `SHALL` 或 `MUST` 关键字
- 每条 Requirement MUST 至少一个 `#### Scenario: {名}` + `WHEN/THEN` 块
- 变更提案走 `/opsx:propose` → `/opsx:apply` → `/opsx:archive` 流程
- 探索性想法用 `/opsx:explore`，不创建产物

## 跨项目引用

- **uart_16550**：`../uart_16550/` — 串口驱动子项目，独立 crate，独立 CLAUDE.md
- **父项目索引**：`../CLAUDE.md` — 跨子项目文档索引

## 关键文件速查

| 用途 | 路径 |
|------|------|
| 异步串口实现 | `kernel/src/drivers/serial/` |
| ISR 入口 | `kernel/src/drivers/isr.rs` |
| TTY / ldisc | `kernel/src/pseudofs/dev/tty/` |
| 内核 benchmark | `kernel/src/drivers/benchmark.rs` |
| 用户态 benchmark | `tests/benchmark.c` |
| 启动入口 | `src/main.rs` |
| 设备 MMIO 范围 | `.axconfig.toml` |
| QEMU 平台配置 | `~/.cargo/registry/.../axplat-riscv64-qemu-virt-0.3.1-pre.6/src/` |
