## Context

R17 已复现当前质量 Gate 的五类问题：三个 feature-scoped imports、内嵌 `uart_16550` manifest 漂移、`deny(clippy::cargo/nursery)` 暴露的 lint、Cargo 自动 workspace membership，以及 kernel host/default feature 拓扑不完整。现有 UART 运行时路径已经过 QEMU/D1 验证，本 change 必须只清理构建和测试基线，不改变 ISR、copier、IER、TTY 或 drain 语义。

根 workspace 显式写了 `members = ["kernel"]`，但 workspace 根包和目录内 path dependencies 仍被 Cargo 自动纳入 members。于是对 `uart_16550` 运行 clippy 时，其 crate-level `deny(clippy::cargo)` 会检查 sibling package metadata。相邻 `../uart_16550/Cargo.toml` 提供了内嵌 manifest 缢失条目的直接 canonical 对照。

## Goals / Non-Goals

**Goals:**

- 恢复 `uart_16550` default/async/embedded-io/all-features 的 compile/test/clippy 基线。
- 消除三个 feature-scoped unused import，不破坏 D1 user/fullbench。
- 让 workspace lint 与可复用驱动 lint 各自只检查其拥有的 artifact。
- 建立不编译完整 kernel 的 early-console host harness，运行已有 6 个纯逻辑测试。
- 明确 ENV BLOCK、unsupported Gate 和 source regression 的报告边界。

**Non-Goals:**

- 不让整个 `starry-kernel` 在 host/default feature 下可测试。
- 不修改 UART 数据路径、telemetry 数据含义或 benchmark 语义。
- 不补齐所有 StarryOS package 的 keywords/categories 以迁就跨包 lint。
- 不修改用户级 `/home/daivy/.cargo/config`；只报告其 deprecation warning。
- 不解决当前 sandbox 对 `lwext4_rust` C 编译的系统调用限制。

## Decisions

### D1: 恢复 canonical manifest parity，不改写测试

内嵌 manifest 增加 `resolver = "3"`、optional `embedded-io = "0.7"`、`assert2 = "0.4.0"` dev-dependency，并将 feature 连接到 `dep:embedded-io`。

理由：相邻 canonical crate 与 registry 0.5 manifest 都证明这些是原有 crate 契约。改写 17 个 `assert2` 调用或删除 `embedded-io` feature 会扩大差异并隐藏复制遗漏。

替代方案：把测试改成标准 `assert!`。拒绝，因为失败根因是 manifest 漂移，不是测试表达方式。

### D2: 用精确 cfg 拆分 imports

`vec`、`ROOT_FS_CONTEXT` 和 `Mountpoint` 使用与 `init_memory_root()`/D1 user/fullbench 相同的三 feature 条件。通用 `FS_CONTEXT`、`FsContext` 和 VFS imports 保持原位置。

理由：这些符号有真实使用点，不是可删除死代码；cfg import 是最小改动。

替代方案：crate/module 级 `allow(unused_imports)`。拒绝，因为会掩盖后续真正的 import 漂移。

### D3: 将 uart_16550 排除出根 workspace lint 边界

根 workspace `exclude` 增加 `crates/uart_16550`。StarryOS 继续使用现有 path dependency；验证脚本或任务显式运行 `--manifest-path crates/uart_16550/Cargo.toml`。

理由：驱动有独立 package metadata、resolver 和严格 lint policy；将它隔离后，`clippy::cargo` 不再评价 sibling packages。该选择已获用户明确批准。

替代方案：补齐所有 sibling metadata。拒绝，因为这些字段与驱动正确性无关，且仍把 lint ownership 混在一起。

### D4: telemetry-off no-op 方法使用最小 clippy 修复

若编译器接受，在两个空方法上增加 `const`，保留原签名、可见性和 feature 分支。实施阶段先以 clippy RED 见证，修改后用 async-without-telemetry clippy GREEN 验证。

理由：方法无状态访问和副作用，`const fn` 不改变普通调用语义。

替代方案：针对 `missing_const_for_fn` 添加 allow。拒绝，当前 lint 可以直接满足且改动很小。

### D5: 使用 rustc host harness 测 early-console 纯逻辑

新增一个小型 harness，只通过 `#[path]` 引用真实 `kernel/src/platform/console.rs` 与 `early_console.rs`；通过 Makefile target 调用 `rustc --test`，输出二进制放在 `/tmp`。Harness 可局部 `allow(dead_code)`，因为它刻意只测试完整硬件模块中的纯逻辑子集。

设计探针已证明该方式能运行现有 6 个测试，结果 6 passed、0 failed。它不依赖 axfs、axnet、display、task-ext 或平台初始化。

替代方案：给 kernel 增加 host-test feature 并 cfg 掉大量模块。拒绝，影响面过大且容易形成与真实 kernel 不同的第二套模块图。

### D6: 验证矩阵显式分类结果

每条 Gate 只允许 `PASS`、`SOURCE FAIL`、`ENV BLOCK` 或 `UNSUPPORTED`。QEMU/D1 target 验证必须使用对应 Makefile/`PLAT_CONFIG`/musl 前置条件；当前 sandbox 的 `Bad system call` 只能记录为 ENV BLOCK。

理由：把环境阻塞算成功会失去证据，把它算源码回归会误导修复方向。

## Risks / Trade-offs

- [独立 crate 不再被根 `--workspace` 自动检查] → 在 tasks 和后续验证入口中显式列出 driver manifest commands。
- [增加 `assert2` 可能需要获取依赖] → 版本与 canonical crate 一致；若环境无法获取，记录 dependency-fetch ENV BLOCK，不改写测试绕过。
- [cfg 条件未来新增 D1 模式后再次漂移] → 将 import warning check 覆盖 smoke/kbench/userbench/fullbench-command，并保持 feature 列表集中可搜索。
- [直接 rustc harness 不经过 Cargo dependency resolution] → harness 只覆盖无外部依赖的 pure logic；系统行为仍由 QEMU/真板 Gate 负责。
- [QEMU full check 在当前 sandbox 不可运行] → 保留失败命令与 prerequisite，使用可运行的 driver/host/D1 Rust Gate，不声明 QEMU GREEN。

## Migration Plan

1. 复现并保存当前 RED：`assert2`、all-features embedded-io、两个 no-op clippy lint和三个 import warning。
2. 恢复 uart manifest parity，先让 check/test feature matrix GREEN。
3. 修复 feature imports 与 no-op lint，分别验证 telemetry off/on。
4. 隔离 workspace membership，确认 path dependency 仍解析到内嵌 crate，重新运行独立 clippy。
5. 加入 early-console host harness 和 Makefile target，要求 6/6 tests PASS。
6. 执行完整可用 Gate；环境阻塞逐项标注，不伪造结果。

回滚时按相反顺序移除 harness/target、workspace exclude、cfg/import 和 manifest 条目。任何回滚都不得只回退 manifest dependency 而保留对应 feature/test 源码。

## Open Questions

无。用户已批准 workspace lint 隔离；host test 采用已验证的最小 rustc harness，不扩展为全 kernel host test。

## Requirements Traceability Matrix

| Requirement | Tasks | Coverage | Simplification | Status |
|-------------|-------|----------|----------------|--------|
| 内嵌 `uart_16550` manifest 完整性 | 1.1-1.2、2.1-2.3、6.2 | 100% | None | Covered |
| Feature-scoped imports 无 warning 且功能保持 | 1.4、3.1-3.2、3.4、6.3-6.4 | 100% | None | Covered |
| `uart_16550` lint 边界独立 | 1.3、4.1-4.3、6.2 | 100% | None | Covered |
| Telemetry 开关两侧 API 与 lint 一致 | 1.3、3.3、4.3、6.2 | 100% | None | Covered |
| Kernel 测试与环境 Gate 分层 | 1.4、5.1-5.3、6.3-6.6 | 100% | None | Covered |
