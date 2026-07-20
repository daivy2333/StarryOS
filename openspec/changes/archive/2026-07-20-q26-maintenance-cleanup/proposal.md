## Why

Q26 的维护项已超过 90 天，其中 memtrack 的构建入口和 API 已漂移，非法命令顺序还可能触发内核 panic。TTY、device mmap 和 PTY 中也保留了无构造或无调用接口，需要在不改变现有运行语义的前提下收窄。

## What Changes

- 修复 `MEMTRACK=y` 的 Cargo feature 传播和 memtrack 对当前 `pseudofs`、`axalloc::tracking` API 的适配。
- 将 memtrack 约束为单一调试 session；非法命令、无序 `end`、重复或并发 session 返回错误，不得 panic。
- 删除无构造者的 `ProcessMode::Manual` 及其内部处理分支，保留 UART/PTY 的 `External` 和 PTY master 的 `None`。
- 删除无调用者的 `create_pty_master`，保留 `/dev/ptmx` 使用的 `Ptmx::create_pty`。
- 删除无生产者的 `DeviceMmap::ReadOnly` 及 `sys_mmap` 对应分支，保留 `None`、`Physical` 和 `Cache`。
- 将 `clear_elf_cache`、`cleanup_task_tables` 收窄为 memtrack feature 内部 helper。
- 验证现有 `LTO=y` release 构建入口；开发构建继续默认关闭 LTO。

## Capabilities

### New Capabilities

- `maintenance-cleanup`: 规定 memtrack 调试协议、TTY processing mode、预留接口清理和 release LTO Gate。

### Modified Capabilities

无。

## BDD 场景草图

### memtrack 正常 session

- 前置：以 `MEMTRACK=y` 构建，tracking 未启用。
- 动作：向 `/dev/memtrack` 写入 `start\n`，执行 workload，再写入 `end\n`。
- 结果：输出本 session 的内存分类，随后关闭 tracking。
- 边界：分析过程不得破坏 allocator、task table 或 ELF loader 状态。

### memtrack 非法或竞争命令

- 前置：session 处于 idle、active 或 analyzing。
- 动作：写入未知命令、无序 `end`、重复 `start`，或并发改变 session。
- 结果：调用返回错误，session 保持可判定状态。
- 边界：不得 panic、死锁或遗留 tracking enabled。

### TTY 与 PTY 保持现状

- 前置：UART 使用 `External`，PTY master/slave 分别使用 `None`/`External`。
- 动作：删除 `Manual` 后执行 console、VTIME、signal 和 PTY smoke。
- 结果：Shell 输入、超时、Ctrl+C、`/dev/ptmx` 和 PTY 双向 I/O 保持可用。
- 边界：不得恢复 Manual polling 或 `wake_by_ref` 自唤醒。

### 删除 PTY 重复入口

- 前置：`create_pty_master` 无调用者，`Ptmx::create_pty` 被 open 路径调用。
- 动作：删除前者并打开 `/dev/ptmx`。
- 结果：仍创建 master/slave，并在 `/dev/pts` 注册 slave。
- 边界：不得删除或改写实际 ptmx open 路径。

### 删除未实例化 mmap 行为

- 前置：设备只产生 `None`、`Physical` 或 `Cache`。
- 动作：删除 `ReadOnly` 及其 match 分支，验证 framebuffer、loop device 和普通 mmap。
- 结果：现有 mmap 行为不变，编译期 match 完整。
- 边界：不新增 mmap capability，不恢复已取消的 user ring 方案。

### release LTO

- 前置：开发构建默认关闭 LTO，Make 已支持 `LTO=y`。
- 动作：分别执行普通 release 与 LTO release 构建，并运行同版聚焦 benchmark。
- 结果：两种构建均成功；LTO 结果记录构建方式、产物和指标。
- 边界：QEMU 数据只用于同环境回归，不用于真板吞吐声明。

## Impact

- Build：`Makefile`、`make/build.mk` 的 feature/LTO 验证路径。
- Memory/task：`kernel/src/pseudofs/dev/memtrack.rs`、ELF cache 和 task table helper。
- TTY/VFS：`ldisc.rs`、PTY 入口、`DeviceMmap` 与 `sys_mmap`。
- 验证：Cargo feature resolution、kernel build、QEMU console/PTY/memtrack smoke、release LTO benchmark。
- 不修改 registry crate、UART driver、ISR、Q24/Q30 范围或默认开发性能策略。
