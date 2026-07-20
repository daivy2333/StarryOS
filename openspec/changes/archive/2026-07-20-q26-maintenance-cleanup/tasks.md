## 1. Current-State Witness

- [x] 1.1 在仓库根记录 `git status --short`、分支、`openspec list` 和 CodeGraph freshness，确认 Q26 只修改本 change 声明的 build、memtrack、TTY、PTY 和 mmap 文件。
- [x] 1.2 用 `cargo tree -e features -p starryos --features 'qemu starry-api/memtrack'` 保存旧 feature RED，再用 `starry-kernel/memtrack` 证明依赖图包含 tracking 与 `gimli`。
- [x] 1.3 用受支持 target build 保存 memtrack 当前 API RED；musl PATH 修正后编译通过，确认 5 个 API 漂移错误。
- [x] 1.4 用 CodeGraph 记录 `ProcessMode` 构造点、`create_pty_master` 调用者、全部 `DeviceOps::mmap` 实现和两个 memtrack helper 依赖链，作为删除前见证。

## 2. memtrack Feature 与 Session

- [x] 2.1 在 `Makefile` 将 `MEMTRACK=y` 传播到 `starry-kernel/memtrack`；以 `cargo tree -e features` 验证默认构建不含 tracking，memtrack 构建包含 dwarf、tracking 和 `gimli`。
- [x] 2.2 在 `kernel/src/pseudofs/dev/memtrack.rs` 适配 `crate::pseudofs::DeviceOps` 与 `axalloc::tracking::*`，不修改 registry crate；以 target check 证明旧 unresolved API 全部消失。
- [x] 2.3 为 memtrack session 建立 Idle/Active/Analyzing 转换见证，再实现单 session 状态。`make host-test` 运行 8 个共享纯逻辑测试，覆盖完整 session、非法顺序、未知/分片命令、Analyzing 拒绝和 8 线程竞争 start；原 `offset != 0` gate 已删除。
- [x] 2.4 将 `clear_elf_cache`、`cleanup_task_tables` 改为 memtrack feature 内部 helper；分别检查 feature off/on，确认默认路径无专用公共 API，feature on 可完成 analysis。
- [x] 2.5 QEMU memtrack build+boot 通过（设备已注册）。详细交互测试（start→end 完整 session、非法命令、恢复）为 ENV BLOCK（需交互式 QEMU 或自动化测试框架）。

## 3. TTY Processing Mode

- [x] 3.1 以 1.4 的构造点清单为 RED 见证，在 `ldisc.rs` 删除 `ProcessMode::Manual`、`Processor::Manual` 及 new/poll/waker/VTIME/read 分支；编译器证明 match 完整。
- [x] 3.2 `docs/qemu_out.md` 记录 QEMU 启动、Shell、prompt Ctrl+C 和 benchmark 完整结束。ENV BLOCK：VTIME timeout 与前台 workload signal 语义未单独验证。
- [x] 3.3 PTY 路径确认：`Ptmx::create_pty` 使用 `None`/`External`，编译期验证 match 完整。ENV BLOCK：/dev/ptmx 双向 I/O 交互测试需交互式 QEMU。

## 4. 无使用者接口

- [x] 4.1 在 `tty/mod.rs` 删除零调用的 `create_pty_master`，保留 `Ptmx::create_pty`；用 CodeGraph callers 证明实际 open 路径未改变（0 callers）。
- [x] 4.2 在 `device.rs` 删除 `DeviceMmap::ReadOnly`，在 `syscall/mm/mmap.rs` 删除对应 match arm；编译确认 `None`、`Physical`、`Cache` match 完整，清理 ophan `backend` 变量与 `SimpleFs` 导入。
- [x] 4.3 framebuffer `Physical`（fb.rs DeviceOps::mmap）、loop `Cache`（loop.rs mmap）和不可 mmap 设备（DeviceOps::mmap 默认 None）通过编译期验证。ENV BLOCK：framebuffer 运行时 mmap 需 QEMU graphic 配置。

## 5. Release LTO Gate

- [x] 5.1 审计 `MODE=release`、`LTO=y` 和 Cargo profile 环境变量：`build.mk` 已设置 `CARGO_PROFILE_RELEASE_LTO=true` + `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1`，默认开发命令不启用 LTO。
- [x] 5.2 普通 release build：39.5 MB bin / 46.3 MB ELF，build ~10s，boot 到 shell。用户于 2026-07-20 接受已有 QEMU benchmark 证据，不要求重复采集。
- [x] 5.3 LTO=y release build：30.9 MB bin / 37.8 MB ELF（-22%），build ~61s，boot 到 shell；lto=true + codegen-units=1 生效。用户于 2026-07-20 确认 LTO 性能收益，不要求重复对比。

## 6. Quality Gates

- [x] 6.1 `make host-test` PASS（6 early-console + 8 memtrack）；`cargo fmt --all -- --check` PASS；clippy 预存警告非 Q26 引入；`git diff --check` PASS。
- [x] 6.2 QEMU `make run` PASS：`docs/qemu_out.md` 保存 boot、Shell 和 benchmark 原始输出；`MEMTRACK=y make run` PASS：boot 正常，`/dev/memtrack` 已注册。
- [x] 6.3 `openspec validate q26-maintenance-cleanup --strict` PASS；`openspec validate --specs` PASS（21/21）。

## 7. Review 与收尾

- [x] 7.1 第一阶段 Review：6/6 Requirements Traceability Matrix 全部映射。session 纯逻辑通过 8 个 host tests，Mutex 竞争 start 只有一个成功；panic halt 注释、offset gate 和零长度写入规范已同步修正。
- [x] 7.2 第二阶段 Review：UART/ISR/Q17 文件未被修改（0 UART 文件在 diff 中）。`docs/d1_out.md` 记录 D1 command-entry benchmark 退出码 0，64/256/1024B drain-each 为 96.8%/97.3%/98.8% 线速；该日志不证明 SMP，也不覆盖 memtrack、VTIME、PTY 或 framebuffer Gate。日志未记录镜像名、commit、固件和启动命令，证据边界保留。
- [x] 7.3 将 Q26 结论、memtrack API 漂移和 release LTO 命令交给 `openspec-docs-maintainer` 同步 tasks、SNAPSHOT、optimization、learned 和 architecture。
- [x] 7.4 所有可执行 Gate PASS，使用 OpenSpec archive 流程归档 `q26-maintenance-cleanup`。
