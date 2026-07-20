# maintenance-cleanup Specification

## Purpose

规定 memtrack 调试协议、TTY processing mode 收敛、无使用者接口清理、release LTO 入口和分层验证边界。

## Requirements
### Requirement: memtrack feature 可构建并注册设备

`MEMTRACK=y` MUST 启用当前 `starry-kernel/memtrack` feature，并 SHALL 在 devfs 注册 `/dev/memtrack`；默认构建 MUST NOT 启用 allocator tracking。

#### Scenario: 启用 memtrack 构建

- **WHEN** 使用受支持的 QEMU target 和 `MEMTRACK=y` 构建
- **THEN** Cargo MUST 启用 `starry-kernel/memtrack`、`axalloc/tracking`、dwarf 和 `gimli`
- **AND** memtrack 源码 MUST 使用当前 `pseudofs` 与 `axalloc::tracking` API

#### Scenario: 默认构建

- **WHEN** 未设置 `MEMTRACK=y`
- **THEN** `/dev/memtrack` MUST NOT 注册
- **AND** tracking 专用依赖和 helper MUST NOT 进入默认 feature 路径

### Requirement: memtrack session 安全

memtrack MUST 只允许一个 Idle、Active 或 Analyzing 状态的 session；非法命令和非法状态转换 MUST 返回错误，并 MUST NOT panic、死锁或遗留 tracking enabled。

#### Scenario: 完成有效 session

- **WHEN** idle 状态收到完整 `start\n`，workload 执行后 active 状态收到完整 `end\n`
- **THEN** memtrack MUST 只报告本 session generation 范围内仍存活的 allocation
- **AND** 分析结束后 MUST 回到 idle 并关闭 tracking

#### Scenario: end 先于 start

- **WHEN** idle 状态收到 `end\n`
- **THEN** 写操作 MUST 返回错误
- **AND** MUST NOT 调用要求 tracking state 的 allocation visitor

#### Scenario: 重复或并发命令

- **WHEN** active 状态再次收到 `start\n`，或 analyzing 状态收到 `start\n`、`end\n`
- **THEN** 竞争转换 MUST 只有一个成功
- **AND** 失败调用 MUST NOT 改变 session baseline 或 allocator tracking 状态

#### Scenario: 未知或分片命令

- **WHEN** 非零长度的单次写入不是完整 `start\n` 或 `end\n`
- **THEN** 写操作 MUST 返回错误
- **AND** session 状态 MUST 保持不变
- **AND** 零长度写入 MUST 作为无副作用 no-op 返回成功

### Requirement: TTY processing mode 收敛

TTY line discipline MUST 只保留 `External` 和 `None` processing mode；UART 与 PTY slave MUST 使用 `External`，PTY master MUST 使用 `None`。

#### Scenario: UART 外部唤醒

- **WHEN** UART RX copier 推送输入并唤醒已注册 waker
- **THEN** 唯一 `tty-reader` task MUST 处理输入
- **AND** line discipline MUST NOT 使用 Manual polling 或 `wake_by_ref` 自唤醒

#### Scenario: PTY master 和 slave

- **WHEN** 打开 `/dev/ptmx` 并进行双向 PTY I/O
- **THEN** master MUST 使用 `None` 并由对端数据唤醒
- **AND** slave MUST 使用 `External` 处理 line discipline 输入

#### Scenario: TTY 超时和 signal

- **WHEN** 用户使用 VTIME read 或向前台终端输入 Ctrl+C
- **THEN** 超时与 signal 行为 MUST 保持
- **AND** 删除 Manual 分支 MUST NOT 引入 busy loop 或 lost wakeup

### Requirement: 无使用者接口删除

Q26 MUST 删除无调用者的 `create_pty_master` 和无生产者的 `DeviceMmap::ReadOnly`，同时 MUST 保留实际 PTY 与 mmap 路径。

#### Scenario: ptmx 创建 PTY

- **WHEN** 用户打开 `/dev/ptmx`
- **THEN** `Ptmx::create_pty` MUST 创建 master/slave
- **AND** slave MUST 注册到 `/dev/pts`

#### Scenario: 现有 device mmap

- **WHEN** framebuffer、loop device 或不可 mmap 设备请求 shared mapping
- **THEN** `Physical`、`Cache` 和 `None` 行为 MUST 保持
- **AND** match MUST NOT 包含没有生产者的 `ReadOnly`

#### Scenario: memtrack helper 生命周期

- **WHEN** memtrack feature 关闭
- **THEN** `clear_elf_cache` 和 `cleanup_task_tables` MUST NOT 作为无条件公共预留 API 编译
- **AND** feature 开启时它们 MUST 可由 memtrack analysis 调用

### Requirement: release LTO Gate

生产 release 构建 MUST 能通过 `LTO=y` 启用 LTO 和单 codegen unit；开发构建 MUST 默认保持 LTO 关闭。

#### Scenario: 普通 release 构建

- **WHEN** 使用默认 release 配置构建
- **THEN** 构建 MUST 不设置 release LTO override
- **AND** 现有开发迭代策略 MUST 保持

#### Scenario: LTO release 构建

- **WHEN** 使用 `LTO=y` 执行 release 构建
- **THEN** Cargo release profile MUST 启用 LTO 和 `codegen-units=1`
- **AND** 构建产物 MUST 可启动并完成聚焦 benchmark

#### Scenario: LTO 证据边界

- **WHEN** 比较普通 release 与 LTO release
- **THEN** 证据 MUST 记录环境、命令、payload、timer 和产物
- **AND** QEMU 结果 MUST NOT 被用作真板吞吐结论

### Requirement: Q26 分层验证

Q26 MUST 按 feature、build、runtime、integration 和 workload 顺序验证；结果 MUST 分类为 PASS、SOURCE FAIL、ENV BLOCK 或 UNSUPPORTED。

#### Scenario: 环境阻塞

- **WHEN** musl、rootfs、PLAT_CONFIG 或 sandbox C 编译限制阻止 Gate
- **THEN** 结果 MUST 记录为 ENV BLOCK 和对应前置条件
- **AND** MUST NOT 声明源码或运行时 PASS

#### Scenario: 运行时敏感行为

- **WHEN** 验证 memtrack、TTY、PTY 或 mmap 行为
- **THEN** MUST 使用受支持的 QEMU 或对应设备路径
- **AND** host check MUST NOT 替代运行时证据
