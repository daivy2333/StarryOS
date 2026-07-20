## Context

Q26 横跨 build、allocator tracking、task/loader helper、TTY 和 VFS mmap。当前 `MEMTRACK=y` 仍传递旧的 `starry-api/memtrack` feature；源码也使用迁移前的 `crate::vfs` 和 `axalloc` 根级 API。TTY 已在 Q7 切换到 `External`，但 `Manual` 及其自唤醒分支仍被保留。

`create_pty_master` 没有调用者，实际 ptmx open 路径调用 `Ptmx::create_pty`。`DeviceMmap::ReadOnly` 被 `sys_mmap` 匹配，但没有设备返回它。Make 已通过 `LTO=y` 设置 release profile 环境变量，因此不需要在 Cargo.toml 中长期启用 LTO。

## Goals / Non-Goals

**Goals:**

- 恢复可构建、可运行且不会因非法命令 panic 的 opt-in memtrack。
- 删除 TTY、PTY 和 mmap 中已证实无使用者的分支。
- 保持 UART、PTY、mmap 和开发构建的现有行为。
- 建立普通 release 与 LTO release 的可复现 Gate。

**Non-Goals:**

- 不修改 UART driver、ISR、ring、copier 或 Q24/Q30 语义。
- 不增加 memtrack 命令、输出格式或常驻后台任务。
- 不清理所有模块级 `allow(dead_code)`。
- 不增加新的 device mmap capability。
- 不用 QEMU benchmark 声明真板吞吐。

## Decisions

### D1: 修复并保留 opt-in memtrack

**决策**：将 Make feature 改为 `starry-kernel/memtrack`，适配 `crate::pseudofs::DeviceOps` 和 `axalloc::tracking::*`。`clear_elf_cache`、`cleanup_task_tables` 仅在 memtrack feature 下以 crate 内可见性编译。

**原因**：devfs 注册和调试协议已经存在，修复范围小于删除后重建。Q24 之外的内存泄漏排查仍需要该工具。

**影响**：默认构建不增加依赖或 tracking 开销；`MEMTRACK=y` 会启用 dwarf、allocator tracking 和 `gimli`。

**替代**：删除 memtrack 及两个 helper。拒绝，因为现有工具只发生 API 漂移，没有被新机制替代。

### D2: memtrack 使用三态 session

**决策**：`MemTrack` 设备实例维护 Idle、Active、Analyzing 三态。`start\n` 只允许 Idle → Active；`end\n` 只允许 Active → Analyzing → Idle。未知命令、重复命令和竞争转换返回错误。

**原因**：只检查 allocator 的 tracking bool 无法阻止 analysis 期间的新 session，也不能区分重复 `start`。三态转换能在执行分析前拒绝第二个 `end`，并避免无序 `end` 进入 `allocations_in()`。

**影响**：协议仍是单次完整写入 `start\n` 或 `end\n`。失败不得改变 session 或 allocator tracking 状态。

**替代**：仅在 `end` 前调用 `tracking_enabled()`。拒绝，因为 check 与状态变化之间存在竞争窗口。

### D3: TTY 只保留 External 和 None

**决策**：同时删除 `ProcessMode::Manual`、`Processor::Manual` 和所有匹配分支。UART 与 PTY slave 继续使用 `External`；PTY master 继续使用 `None`。

**原因**：当前分支没有 Manual 构造者。保留其 `wake_by_ref` 路径会掩盖 Q7 已解决的 yield storm 模式。

**影响**：`LineDiscipline::new`、`poll_read`、`register_rx_waker`、VTIME 和普通 read 的 match 会收窄。外部行为必须由 QEMU 和 PTY smoke 证明不变。

**替代**：只移除 `#[allow(dead_code)]`。拒绝，因为 Manual 本身确实无使用者，编译 warning 不是问题根因。

### D4: 删除两个独立的预留接口

**决策**：删除 `create_pty_master`，保留 `Ptmx::create_pty`；删除 `DeviceMmap::ReadOnly` 和 `sys_mmap` 对应分支。

**原因**：前者是重复入口，后者没有生产者。Q21/Q22 已取消，当前没有 user-ring mmap 需求。

**影响**：`/dev/ptmx`、framebuffer `Physical` mmap、loop `Cache` mmap 和默认 `None` 必须保持。

**替代**：添加未来用途注释继续保留。拒绝，因为这些接口已超过 90 天未使用，且没有活跃 change 依赖。

### D5: LTO 继续由 release 构建参数控制

**决策**：使用现有 `LTO=y` 入口设置 `CARGO_PROFILE_RELEASE_LTO=true` 和 `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1`。开发默认值不变，不向 Cargo.toml 写入永久 profile。

**原因**：该入口满足 ADR-034 的发布要求，也避免增加日常构建时间。

**影响**：实施阶段需要证明普通 release 和 LTO release 均能构建，并记录同环境指标。

**替代**：在 workspace 和驱动 Cargo.toml 中永久设置 `lto = true`。拒绝，因为 Cargo 顶层 profile 已能覆盖依赖，且会影响开发构建。

### D6: 按依赖层验证

**决策**：依次验证 feature resolution、target build、memtrack runtime、TTY/PTY/mmap smoke、release/LTO build 和 benchmark。结果只使用 PASS、SOURCE FAIL、ENV BLOCK、UNSUPPORTED。

**原因**：完整 QEMU 构建可能受 musl、rootfs 或 sandbox C 编译限制。环境阻塞不能替代源码或运行时证据。

**影响**：无法运行的 Gate 必须记录命令和前置条件；运行时敏感行为不能只靠 host check。

**替代**：以 `cargo check` 代替所有系统验证。拒绝，因为 TTY、PTY、mmap 和 memtrack 都有运行时路径。

## Risks / Trade-offs

- [memtrack 分析期间发生内部 panic] → no_std panic 会终止执行；正常返回路径必须关闭 tracking 并恢复 Idle。
- [删除 Manual 后遗漏 match 分支] → 先建立构造点和分支清单，再由编译器和 QEMU 行为 Gate 双重验证。
- [mmap 删除影响现有设备] → 审计所有 `DeviceOps::mmap` 实现，并运行 framebuffer/loop 可用 Gate；无对应设备时记录配置边界。
- [LTO 构建受 sandbox 阻塞] → 记录 ENV BLOCK，不把历史数据当作本次 PASS。
- [Q26 与 Q17 同时活跃] → Q26 不修改 Q17 的 UART 内存序文件，也不改变其 18/19 状态。

## Migration Plan

1. 保存 feature、memtrack 编译错误、Manual 构造点和预留接口调用图。
2. 修复 memtrack build 与 session，先通过 feature 和 runtime Gate。
3. 删除 Manual，验证 console、VTIME、signal 和 PTY。
4. 删除 PTY/mmap 预留接口，验证实际入口。
5. 执行普通 release、LTO release 和聚焦 benchmark。
6. Review 后更新 Q26 状态并归档 change。

回滚按提交边界执行：memtrack、TTY、预留接口和 LTO 证据互不依赖。不得通过恢复 Manual 或无调用 API 来修复无关回归。

## Open Questions

无。用户已认可探索结论；本设计保留现有命令协议和开发构建默认值。

## Requirements Traceability Matrix

| Requirement | Task(s) | Coverage | Simplification | Status |
|-------------|---------|---------:|----------------|--------|
| memtrack feature 可构建并注册设备 | 1.2-1.3、2.1-2.2、2.5、6.2 | 100% | None | Covered |
| memtrack session 安全 | 2.3、2.5、7.1 | 100% | None | Covered |
| TTY processing mode 收敛 | 1.4、3.1-3.3、6.2 | 100% | None | Covered |
| 无使用者接口删除 | 1.4、2.4、4.1-4.3 | 100% | None | Covered |
| release LTO Gate | 5.1-5.3、7.2 | 100% | None | Covered |
| Q26 分层验证 | 1.1-1.4、6.1-6.3、7.1-7.4 | 100% | None | Covered |
