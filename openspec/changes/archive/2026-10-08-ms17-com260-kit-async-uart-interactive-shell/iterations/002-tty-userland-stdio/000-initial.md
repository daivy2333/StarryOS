# Iteration 002 / Cycle 000: tty 规范输入与真实内嵌用户态标准 I/O

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: T5、T6（Iteration 002 既有任务）。
- Acceptance gaps: None。
- Repair items: None。
- Inherited scope: R5/R6/R8 已批准场景（S17–S22、S27–S28）与全局契约；design 裁决（K09 唯一 AtomicWaker、tty 层拥有 CR/LF 翻译、单 consumer）；Iteration 000/001 accepted 基线——timer/AIA 底座、PXA port 唯一 owner、完整数据面（copier 纪律、64KiB ring、drop 分类、四条件 drain、`make k3-uart`、板测 B5 全过）；板上裁决事实（EIE 掩码窗口、IIR 掩码照报、rx_wakes 口径失真）。
- Excluded scope: T7/T8（shell 与最终集成，Iteration 003）、SMP（MS14）、rootfs/块设备（MS18）、性能优化、受控迁移、MS12 网络、全局文档与经验产物。

**Objective**

K3 上 tty canonical 输入契约闭合（空格/tab/编辑、行界限、EOF、readiness）并接通真实内嵌用户态：静态 ET_EXEC 程序经内存根与 stdio 运行，probe 以真实 syscall（read/write/FIONBIO/ppoll/nanosleep/exit）验证，输出经数据面物理 drain。

**Background**

Iteration 001 accepted 交付完整 UART 数据面。本 Iteration 在其上打开 tty 层（drivers 树的 `ntty_async`/pseudofs tty 当前被 k3 门排除）并引入无存储用户态：程序内嵌内核镜像，规避 rootfs（MS18）。tty 的 CR/LF 翻译归本层（Iteration 000 raw 载荷的行接受不构成驱动/tty 语义）。

**Investigation Facts**

Current Baseline：Iteration 001 最终 Act Response（含 Review F2 有限修复与最终树复验）+ accepted Review。crate native 74/74、plane 3/uart 25/ms04 105、`make k3-uart` exit0（尾随空白修复后重建，FIT 含 `mode=k3-uart-dataplane begin`）、板测 B5 全过（echo/long≥256B/txfull 73728+short_writes=83/drain/quiet/B4）。工作区 = MS17 Cycle 000–001 全部改动（未提交）。

Current-State Evidence 与关键路径（T5/T6 表面，本 Plan 本次实读）：

1. tty 树：`kernel/src/pseudofs/dev/tty/`（`mod.rs`/`write.rs`/`pty.rs`/`ptm.rs`/`pts.rs` + `terminal/{mod,job,termios,ldisc}.rs`）。`ldisc.rs` 444 行，全局契约指名的四个缺口落位：`line_buf: Vec<u8>` 无上限（99 行）；行完成只在 `is_eol`/`VEOF`（164 行）；字符分类 `ch == b' ' || ch.is_ascii_graphic()`（210 行，空格/tab/编辑键语义缺口）；tty-reader `while reader.poll()` 两处（307/312 行，公平性）。生产 `canonical.rs` 尚不存在——规范输入逻辑在 `ldisc.rs` 内，T5 按全局契约目标建立生产 canonical 模块（从 ldisc 提取或等价组织，拆分面为非实质选择），必须被 native 测试直接驱动、fake 只替换字节/队列边界。
2. `kernel/src/drivers/ntty_async.rs`（50 行）：`ASYNC_TTY: Arc<Tty<ArceOsReader, ArceOsWriter>>` 单例，`ProcessMode::External` 把 tty-reader waker 桥到 RX ring 的 PollSet；唯一 reader/writer 构造带 SAFETY 论证（QEMU/D1 形态）。当前被 `drivers/mod.rs` 的 k3 门排除；`uart_init` 的 `TtyWrite`/`TtyWriteReady` impl 也在 k3-uart 下门出（Iteration 001 deviation 1）——本 Iteration 打开 tty 时须恢复/适配并保持唯一 consumer 论证。
3. 既有测试：`kernel/tests/tty_write_logic.rs` 6 用例（写侧/ONLCR；改 ONLCR 表面才需重跑）；`kernel/tests/tty_input_logic.rs` 待建（T5 RED 载体）。
4. 用户态表面：`kernel/src/mm/loader.rs:454 load_embedded_user_app`（D1 eager 路径，cfg `lichee-d1-userbench`，经 `load_user_app_from_elf_data` 自字节切片映射）；entry.rs userbench 参考序（`ASYNC_TTY` + `crate::file::FD_TABLE` + `pseudofs` + `ProcessData`/`Thread`/`new_user_task` + `spawn_alarm_task` + `copy_from_kernel`/`new_user_aspace_empty`）；`kernel/src/file/mod.rs:225 add_stdio` 为 stdio 构造入口。`tests/k3-embedded-user-host`（Cargo 测试包）与 `tests/k3_stdio_probe.c` 均待建。
5. feature 链现状：root `k3-uart=[k3-async-uart, starry-kernel/k3-uart]`；`k3-stdio`（及 Iteration 003 的 `k3-shell`）尚不存在，按全局契约新增。
6. 数据面不变量承接：输出经 `ArceOsWriter` 单 producer；`tx_completion` 四条件 drain 与 `wait_tx_drain` 已在 Iteration 001 交付，probe 结束输出复用同一物理 drain 判据。
7. 参考仓库经验（R74/R62，按 M42 分级）：R74 已裁决最小 shell 路线零存储依赖（用户程序内嵌内核镜像），UFS/rootfs 属增强路线——本 Cycle 即该路线的 tty/用户态段；参考形态 rtsh 依赖的内核能力面（tty 规范行编辑、UART 交互输入、用户进程、ppoll）与本 Cycle probe 的 syscall 集一致；R74 观察到 rtsh 行编辑使用退格/Ctrl-U/Ctrl-W，本 Cycle 按已批准全局契约交付 ERASE/KILL，WERASE 如 Iteration 003 shell 需要则在该轮裁决；许可证边界——rt-async/rtsh 自有代码不复制，`k3_stdio_probe.c` 为本仓新写；R62 裁决低层 Gate 不以 shell/rootfs/workload 充当成功条件，本 Cycle 验证以 probe 自身直接观察为准。

**Implementation Guidance**

先 tty：建立生产 canonical 模块并写 native RED（四缺口逐项），再打开 K3 tty 门（ntty_async/uart_init impl/ASYNC_TTY）并保持单 consumer；后用户态：提取 embedded_loader 生产模块（native RED 覆盖拒绝契约）→ K3 stdio 形态与内存根 → probe 构建 → 板测。ELF/映射/打开等可恢复失败必须在 `Process::new_init` 之前完成（全局契约 D 裁决：new_init 是一次提交点）。

**Task Contracts**

### T5：tty canonical 输入契约

- Requirement/Scenario：R5；S17–S20。
- Depends on：None（与 T6 并行可开发，板上验收依赖 T6 载体）。
- Targets：`kernel/src/pseudofs/dev/tty/terminal/ldisc.rs`、`terminal/mod.rs`、生产 canonical 模块（新 `canonical.rs` 或等价组织）；`kernel/src/drivers/ntty_async.rs`、`uart_init.rs` TtyWrite/TtyWriteReady 门、`drivers/mod.rs`；`kernel/tests/tty_input_logic.rs`（新）。
- Current behavior：空格/tab 不保留、空 `VEOF` 无 event、`line_buf` 无上限、tty-reader `while reader.poll()` 可能不公平；K3 构建无 tty。
- Required behavior：①空格/tab 与编辑键（ERASE/KILL）按 termios 语义处理；②canonical 行界限 1024B，超长整行 discard 并产生一次 `InvalidInput`，后续行恢复；③`VEOF` 产生 0-read event；旧 queued 数据先读；④readiness 包含 error/EOF 且与真实 read 一致；⑤tty-reader 每次推进有界（等价于每 poll 256 输入字节预算），不无限 while；⑥单 consumer 保持；echo best-effort；正文 blocking/ONLCR 不变；signal/取消不销毁 UART 底座。⑦K3 `k3-uart` 形态打开 tty（`ASYNC_TTY` 唯一 reader/writer 论证延续 Iteration 001 owner 语义）。
- Preserve：FIONBIO 现有正确转换、job control、`sys_ioctl` bool 转换、既有 ONLCR/PTY 语义、tty_write_logic 6 用例语义（改 ONLCR 表面才重跑）。
- Forbidden：新增用户态 `tcdrain`/`TCSETSW` drain 实现、完整终端 API、第二 tty-reader、复制 ldisc 作测试 stand-in。
- Test witness：`kernel/tests/tty_input_logic.rs` 新用例驱动生产 canonical/ldisc（fake 只替换字节源与队列边界）：空格/tab 保留、EOF 0-read、overflow 整行 discard+单次 InvalidInput+恢复、wrap 分段读、背压、取消事件。RED = 新用例断言失败/缺失编译失败。
- GREEN condition：新用例 GREEN 且 tty_write_logic（按需）不回归。
- Verification：`rustc --edition=2024 --test kernel/tests/tty_input_logic.rs -o /tmp/ms17-tty-input-test` 后运行（exit 0）；改 ONLCR 表面时重跑 tty_write_logic 同型命令。
- Stop when：必须扩展 termios/API、修改多 reader 语义、overlong 前缀仍可执行、或只能靠无界缓冲保活。

### T6：无存储用户态 stdio

- Requirement/Scenario：R1/R5/R6/R8；S17–S22/S27–S28。
- Depends on：T5（tty 就绪后 spawn）。
- Targets：root/kernel `Cargo.toml`（`k3-stdio` feature）、`kernel/src/lib.rs`/`entry.rs` 用户态 gates；生产 embedded loader（新 `kernel/src/mm/embedded_loader.rs`，自 `loader.rs` eager 路径提取）；`pseudofs` 内存根；`file/mod.rs` stdio 组装；`ProcessData`/`Thread` 构造；`tests/k3-embedded-user-host`（新 Cargo 包）与 `tests/k3_stdio_probe.c`（新）；`Makefile` `k3-stdio`。
- Current behavior：内嵌用户入口仅 D1（`lichee-d1-userbench`）；K3 无用户态；loader segment slice 可被 `min(len)` 截短，无明确拒绝契约。
- Required behavior：①`k3-stdio` 形态选择真实静态 ET_EXEC 载荷（`tests/k3_stdio_probe.c` 构建，内嵌镜像）；②内存根与 stdio 完成后才 spawn（`new_init` 一次提交点）；kernel/MMIO 段与 USER 段/栈权限正确、BSS 零、argv 传递、icache 同步；③ELF 拒绝契约：格式错/越界/非法 entry/动态载荷明确拒绝且可恢复（失败对象释放、FD 表不发布）；④probe 以真实 syscall 验证：read（行）、write（回显）、FIONBIO（EAGAIN）、ppoll（就绪等待）、nanosleep（timer 恢复）、exit（join 结果）；⑤结束后 kernel 物理 drain（四条件）。
- Preserve：现有 ELF parser/`ProcessData`/syscall 语义、QEMU/D1 构建与 Iteration 000/001 门（`make k3-uart` 交付判据不变）、网络模块继续排除于 K3 形态。
- Forbidden：rootfs/axruntime fs probe、registry fork、新 ELF 加载框架、移植 SMP 迁移 ioctl、动态链接支持。
- Test witness：`tests/k3-embedded-user-host` 加入无效/截短/entry/mapping/open 失败用例（RED：生产路径缺失或行为不符），断言实际生产函数的顺序/返回/未发布与 drop；合法 loader 行为复用 D1 既有 GREEN 结论（覆盖未变）。probe 源静态编译 `cc -std=c11 -Wall -Wextra -Werror`。
- GREEN condition：host 包 GREEN、`readelf -h -l -r` 静态判据成立、`make k3-stdio` exit0、板测 probe 全阶段。
- Verification：agent——`env RUSTFLAGS='-C linker=/home/daivy/projects/serial/work/StarryOS/scripts/cc-nopie.sh' cargo test --manifest-path tests/k3-embedded-user-host/Cargo.toml --offline -- --test-threads=1`；probe ELF `readelf -h -l -r`；`make k3-stdio` exit0；受改共享门（`make build`，改 D1 共享 loader/tty 表面时 `make lichee`）；`git diff --check`。操作者（B6）——装载后 probe 依次打印 argv→EAGAIN（FIONBIO）→输入完整行后 ppoll/read 返回→sleep 恢复→EOF/退出实际 join 结果→kernel drain 行；一句话结果记入 Act Response。
- Stop when：必须支持动态链接/重定位/存储、扩大用户内核地址范围或更新依赖；paging/trap 异常先停在本层；tty-reader 成为第二消费者。

**Invariants**

1. Iteration 000/001 全部不变量与板上裁决事实继续有效；真实输入 IRQ 仍是 S4 唯一验收。
2. 唯一 owner：ASYNC_TTY 唯一 reader/writer；用户态 probe 结束前不出现第二消费者；`new_init` 前全部可恢复失败已关闭。
3. K09/K26：不引入第二套 executor/ring/waker。
4. QEMU/D1/VF2 构建与 74 项 crate 测试、既有 kernel 侧测试语义不回归。
5. raw 载荷（`k3-uart-port-test`）与数据面载荷（`k3-uart`）行为不变。

**Non-goals**

shell 命令（Iteration 003）、SMP（MS14）、rootfs/块设备（MS18）、性能优化、完整 POSIX 终端 API、fork/exec 生态。

**Acceptance**

| ID | 可观察结果 | 映射 |
|---|---|---|
| A1 | tty_input_logic 新用例 RED→GREEN（空格/EOF/overflow/恢复/分段/背压/取消）；tty_write_logic 按需不回归 | T5 / R5 / S17–S20 |
| A2 | K3 `k3-uart` 形态 ASYNC_TTY 打开且单 consumer 论证成立（代码 review + 构建） | T5 / R5 |
| A3 | k3-embedded-user-host GREEN（含拒绝契约用例）；readelf 静态判据；`make k3-stdio` exit0；共享门不回归 | T6 / R6/R8 / S21–S22/S27 |
| A4 | 板上 B6：probe argv/EAGAIN/行读/ppoll/sleep/exit/join + kernel 物理 drain | T6 / R1/R5/R6 / S17–S22/S28 |

RTM（本 Cycle）：R5→T5/T6 Covered；R6→T6 Covered；R1→T6 Covered；R8→T6 Covered。R2–R4 已由 Iteration 001 闭合；R7 属 Iteration 003。

**Verification**

V1（agent，native）：tty_input_logic（RED→GREEN）与 tty_write_logic（按需）；k3-embedded-user-host（RED→GREEN）。
V2（agent，build）：probe 静态编译 + readelf；`make k3-stdio` exit0；`make build`；改 D1 共享表面时 `make lichee`；`git diff --check`（必须在最终树上 exit 0）。
V3（操作者，B6）：probe 全阶段 + drain；无结果则 A4 pending，不展开 Iteration 003。
V4（失败处理）：probe/trap 异常按 Stop when 停在本层并交 Plan；三次失败规则适用。

**Gate 2 Readiness**

PASS — 2026-10-07 用户计划审计原话"批准"。调查闭合（Iteration 001 accepted 基线 + 本 Plan 实读的 tty/ldisc 缺口落位、ntty_async 单 consumer 形态、D1 loader/stdio 参考路径 + R74/R62 参考仓库经验核对）、契约自包含、追踪完整、验证为最简直接判定；无 BLOCKED/WAIVED、无 Missing/Simplified、无待 Act 决定的实质未知项（canonical 模块拆分面、内存根组织、probe 命令集微调为非实质选择，留在 Act）。Plan Context 离开 draft 改为 ready；下一边界为 Act 指令。

**Persisted Evidence**

- Mode: none

命令、≤20 行决定性输出、exit 与操作者一句话结果足以判定。

**Risks and Notes**

- `make host-test` 的 ms04 pre-existing 失败已由 Iteration 001 关闭；本 Iteration 重跑要求"无新失败"。
- probe 的 EOF 场景需要操作者以 Ctrl-D（或等价 VEOF 键）触发；终端 CR/LF 差异已由 tty canonical 层吸收（Iteration 000 裁决的归属反转点）。
- D1 共享 loader/tty 表面若被修改，`make lichee` 必跑；D1 既有行为以 GREEN 结论为界，不扩大验证。
- 用户态地址范围与权限沿用 QEMU/D1 已验证实现；如需扩大即触发 Stop when。
- R74 参考形态的行编辑含 Ctrl-W（WERASE）；本 Cycle 契约不含 WERASE，Iteration 003 shell 如需按需裁决（避免现在扩scope）。

## Act Response

- Status: reported

### Completed tasks

- **T5（tty canonical 输入契约）**：完成。
  - 新生产模块 `kernel/src/pseudofs/dev/tty/terminal/canonical.rs`：`CanonicalInput` 状态机（ERASE/KILL 编辑、空格/tab 保留、1024B 行界限、超长整行 discard→一次 `InvalidInput`、`VEOF` 0-read 事件、跨队列 wrap 的分段投递）；`CanonicalEvents` 事件闩锁（EOF/overflow，跨 waiter 取消持久）；`resolve_read`/`readable` 让 readiness 与 read 共用同一解析；`POLL_INPUT_BUDGET=256`。`core/alloc`-only，native 测试直接驱动。
  - `ldisc.rs`：`InputReader.poll(&mut budget)` 接入 canonical（单行投递纪律保留：前一行未投递完不分类新输入）；tty-reader 闭包两个 drain pass 共享预算，预算耗尽自重排队（wake_by_ref，不依赖已过触发沿的 IRQ）；`poll_read()` 用 `readable()`；`read()` 在数据不足 vmin 时以闩锁事件解析（EOF→`Ok(0)`、overflow→`InvalidInput`）；`drain_input` 同步清事件。
  - K3 门（T5⑦）：`lib.rs`/`drivers/mod.rs`/`uart_init.rs` 门修改——k3-uart 形态编译 `file`/`mm`/`pseudofs`/`syscall`/`task`/`time`/`ntty_async`/`ASYNC_TTY` 与 `TtyWrite`/`TtyWriteReady` impl；`ntty_async` SAFETY 注释更新为 K3 boot-path 互斥论证。`make k3-uart` exit 0（A2）。
- **T6（无存储用户态 stdio）**：完成。
  - 新生产模块 `kernel/src/mm/embedded_loader.rs`：D1 eager 路径提取为 backend-trait 形式（`ElfMapBackend`/`SegFlags`/`ElfLoadParams`），合法镜像序列与 D1 逐字节一致；新增拒绝契约——格式错/程序头表越界/段文件字节越界（取代旧 `min(len)` 静默截短）/非 ET_EXEC/entry 不在可执行段，全部在首个映射调用前拒绝。`loader.rs` 增加 `AddrSpaceBackend` 桥与 `load_embedded_elf_into_aspace`，D1 eager 路径委托之（行为不变）。
  - 新生产模块 `kernel/src/file/stdio.rs`：`publish_stdio`（stdin→stdout→stderr 共享端点、open 失败零发布、插入失败传播）+ `file/mod.rs` 的 `StdioTable`/`open_stdio_files` 适配。
  - `tests/k3-embedded-user-host`（workspace 外 Cargo 包，仿 `k3-polling-net-host`）：9 用例（有效镜像顺序/截短段/越界 phdr/动态/坏 entry/后端失败传播 + stdio 顺序/open 失败/add 失败）。
  - `tests/k3_stdio_probe.c` + `kernel/resources/k3_stdio_probe.elf`（`make k3-stdio-elf`）：argv→FIONBIO EAGAIN→整行 read 回显→ppoll 等待+read→nanosleep 300ms→VEOF 0-read→exit(42)。`readelf -h -l -r` 判据成立。
  - entry.rs k3-stdio 分支：copiers 先启动 → 内存根挂载（devfs console 构造即 ASYNC_TTY 构造点，tty-reader 成为唯一 RX consumer）→ 可恢复阶段（stdio 预开、ELF 加载，失败即 panic 报阶段、无发布）→ `Process::new_init` 提交点 → spawn → join → `wait_tx_drain` 四条件物理 drain（30s 有界）→ wfi。
  - `wait_tx_drain` 从 k3.rs 载荷上移为 `uart_init` 的共享 async fn（等价搬移，载荷与 stdio 出口复用同一四条件判据）。
  - feature 链：root/kernel `k3-stdio = ["k3-uart", ...]`；`k3-uart` 增加 tty 编译面所需 `dep:axfs`/`axfeat/paging`/`axfeat/task-ext`；`Makefile` 新增 `k3-stdio`/`k3-stdio-elf`（单 hart SMP 拒绝与 k3-uart 同型）。

### Deviations from Plan

1. `ReadOutcome::WouldBlock`（契约文字为"无事件保持 WouldBlock"的语义命名）；canonical 拆分面按计划授权的非实质选择落地为独立模块。
2. `k3-uart` feature 增加 `dep:axfs`/`axfeat/paging`/`axfeat/task-ext`：T5⑦ 要求 k3-uart 形态打开 tty/userland 编译树，axfs（pseudofs 依赖）与 paging/task-ext（lib.rs userland 模块编译前提）是其必要后果；k3-uart 资格载荷运行时行为不变（lazy_static 不构造即零成本），`make k3-uart` 交付判据不变。
3. tty-reader 预算耗尽自重排队（`wake_by_ref`）：契约⑤要求"每 poll 256 输入字节预算"，预算耗尽时若无自重排队，后续字节将等待一个已过触发沿的 RX IRQ 造成 lost wakeup；属契约内必要实现细节。
4. probe 构建适配：`_GNU_SOURCE`（musl ppoll 声明）、`ppoll` 4 参数 ABI（本机 musl 交叉工具链）、补 `<sys/ioctl.h>`。
5. 验证增量：`make lichee-userbench`（契约 V2 未点名）——D1 userbench 分支的 loader 委托路径需要构建不回归证据，属 Invariant 4 的必要覆盖。

### Self-review findings

- 修复：测试 wrap 用例初版的 Ring 借用冲突与期望计算错误（测试侧）；`poll()` 初版丢失"单行投递期间不分类新输入"纪律（会损坏投递中行），已恢复并在测试 `feed()` 中按同一纪律建模。
- 修复：`AddrSpaceBackend` 关联错误 `AxError: From<B::Err>` where 约束；loader 路径以 `super::embedded_loader` 解析（Rust 2018 兄弟模块不进裸路径）；`pseudofs::init_memory_root`/imports 的 cfg 补 `k3-stdio`；FD_TABLE guard 的显式 `&mut *table` deref 与中间绑定。
- 遗留 Minor：无。
- 候选 Experience：None（probe 构建适配为一次性环境事实，非可重复高风险路径，不构成 Runbook/Issue 候选）。

### Verification

1. V1 `rustc --edition=2024 --test kernel/tests/tty_input_logic.rs -o /tmp/ms17-tty-input-test && /tmp/ms17-tty-input-test`：RED（canonical.rs 缺失编译失败）→ GREEN：
   ```
   running 11 tests
   ...
   test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   ```
   exit 0（覆盖空格/tab、ERASE/KILL、EOF 0-read 单次、旧数据先读、1024B 整行 discard+单次 InvalidInput+恢复、wrap 分段、背压、取消事件持久、预算常量钉定）。
2. V2 `env RUSTFLAGS='-C linker=…/scripts/cc-nopie.sh' cargo test --manifest-path tests/k3-embedded-user-host/Cargo.toml --offline -- --test-threads=1`：RED（embedded_loader.rs 缺失编译失败）→ GREEN：`test result: ok. 9 passed; 0 failed`，exit 0。
3. V3 `make k3-uart`：exit 0（FIT `Data Size: 278720 Bytes`；tty 树在 k3-uart 形态编译，资格载荷保持 raw owner）。A2 成立。
4. V4 `make k3-stdio`：exit 0（探针 ELF + 内核 + FIT）。readelf 判据：`Type: EXEC (Executable file)`、`Machine: RISC-V`、`Flags: 0x5, RVC, double-float ABI`、两个 LOAD（`R E` + `RW`，BSS FileSiz 0x98 < MemSiz 0x2e8）、无 INTERP、`There are no relocations in this file.`。A3 agent 侧成立。
   - probe 在板测轮内两次硬化（契约授权的"probe 命令集微调"非实质选择）：EOF 故障诊断行（n/errno/byte0）→ 定位为操作者输入序列触发的 S18 数据先行语义（非缺陷）；随后改为 leftover 报告+重试循环（首 个 `n==0` 为判定点，最多 8 次）。两次均 `make k3-stdio` exit 0，最终形态由当轮板测直接验证。
5. V5 `make build`（QEMU 默认门，共享 tty/loader 表面）：exit 0。
6. V6 `make lichee`、`make lichee-userbench`（D1 共享 loader/tty 面 + 委托路径）：均 exit 0。
7. V7 `make host-test`：agent 自跑 11 个套件全部 `0 failed`（含 ms04 105 passed）；末段由操作者手动运行完成（`ms09-board-facts: all host tests passed`）——无新失败，ms04 遗留保持关闭。
8. V8 `git diff --check`：通过（最终树）。`openspec validate`：`Change 'ms17-com260-kit-async-uart-interactive-shell' is valid`。
9. 采信既有结论：uart_16550 74/74（Iteration 001 Act Response，本 Iteration 未触碰该 crate，覆盖未变化）；tty_write_logic 6 用例（未改 ONLCR 表面，按契约不需重跑）。

### Acceptance mapping

- A1：V1（RED→GREEN）✓；tty_write_logic 按需条款未触发。
- A2：V3 + ntty_async/entry 的 boot-path 互斥单 consumer 论证（代码 review）✓。
- A3：V2 + V4 + V5/V6 共享门不回归 ✓。
- A4（板上 B6）：**通过（2026-10-07 操作者板测）**。装载 `StarryOS_riscv64-k3.fit`（RAM boot，同 R68 runbook 流程），串口逐项：argv（argc=1, argv=k3_stdio_probe）→ FIONBIO EAGAIN → 阻塞行读回显（"123"）→ ppoll 等待+行读（"321"）→ nanosleep 300ms → EOF `result=read-0`（先 `leftover bytes=1`：操作者先按空格再按 Ctrl-D，非空行收到 EOF 按契约投递当前行（POSIX 语义、S18 数据先于 EOF），随后空行 Ctrl-D 闩锁、下一次 read 返 0）→ `stage=exit code=42` → kernel `stage=drain done (TxCompletion four conditions)`。无 fault 行。`task.join()` 打印恒为 0 属既有内核表面语义（join 读任务级退出码原子量；`exit_group` 真实码存 process 层 `do_exit(1<<8)`），判定以 probe 自身行为为准（R62）。Iteration 003 可展开。

### Persisted Evidence

None required（mode: none；命令、输出与操作者一句话结果足以判定，无白名单情形）。

### Unresolved issues

- 无阻塞遗留。A4 板测已通过（见 Acceptance mapping）；Iteration 003 展开以 Plan Review `accepted` 为边界。
- 观察项（非缺陷，不计 Issue）：`task.join()` 对经 `exit_group` 退出的进程恒返回 0（任务级退出码原子量不随 syscall 退出更新，真实码在 process 层），QEMU init 路径同语义；VF2 构建入口 pre-existing 失败维持原归属，继续作为未落账 Issue 候选。

## Plan Review

- Review Result: accepted
- Reviewed: 2026-10-07（独立阅读完整 diff 与新增文件，核对 Act Response 声明与代码一致）

**Findings**

阻塞项：无。以下均为非阻塞 Minor/观察项，不要求当前 Cycle 修复：

1. Minor（基线保留，非 Act 偏离）：`CanonicalSpec::is_kill` 沿用 `ECHOK` 门控 KILL——旧 `ldisc.rs` 原有语义原样迁入 canonical 模块；POSIX 中 ECHOK 只控制 KILL 的回显而非 KILL 是否生效。默认 console termios 置位 ECHOK，全部已验收形态（QEMU/D1/K3）行为正确，canonical 测试 spec 亦按默认值断言。完整 termios 语义属 change Non-goals（完整终端 API），留作未来 termios 完整化的已知边界。
2. Minor（注释措辞）：`stdio.rs` 模块 doc 表述 "stdin, then stdout, then stderr"，实际插入序列为 stdin → stderr 克隆 → stdout——与旧 `add_stdio` 逐语句一致（fd1/fd2 为同一输出端点的两个 `Arc` 克隆，可观察行为与文档描述等价）。纯注释精度问题。
3. 观察项：`open_console` 每次 open 独立取 `FS_CONTEXT` 锁（旧实现一次锁两次 open）；发生在 boot 单线程阶段，语义等价。

独立核验通过的关键面（代码 review，非 Act 自检）：

- 单 consumer：全仓 `AsyncUartReader::new` 仅两处——`platform/k3.rs` 数据面载荷（cfg `not(k3-stdio)`，泄漏 raw owner）与 `ntty_async.rs` 懒构造 `ASYNC_TTY`；两 boot path 由 feature 互斥（entry.rs 分支 cfg 互斥、k3.rs 载荷与常量均补 `not(k3-stdio)`），`k3-uart`（非 stdio）boot 不调 `init_memory_root`、不解引用 `ASYNC_TTY`。ntty_async SAFETY 注释与实际互斥结构一致。
- Forbidden 面：diff 无 tcdrain/TCSETSW/axruntime fs probe/registry fork；`kernel/src/syscall/`、tty `write.rs`/`pty.rs`/`ptm.rs`/`pts.rs` 零改动（FIONBIO 转换、job control、ONLCR/PTY 语义保持；tty_write_logic 不重跑的采信成立）。
- loader 等价性：旧 `mapping_flags` 恒含 USER 位，与新 `seg_to_mapping_flags` 等价；stack populate 保持 RW-无-USER、heap map populate=true、写序 per-LOAD map→populate→write，合法镜像序列与 D1 eager 逐调用一致（host 契约测试第 1 例直接断言该序列）；拒绝契约（格式/phdr 越界/段字节越界取代 `min(len)` 截短/非 ET_EXEC/entry 不在可执行段）全部先于首个 backend 调用，且 `ph.offset > len` 先于加法溢出可能被拒。`load_user_app_from_elf_data` cfg 为 D1 三 feature（委托目标的子集门），QEMU 形态不编译。
- entry 顺序：copiers → 内存根/挂载/alarm → stdio 预开、aspace+ELF（可恢复，失败 panic 报阶段、无发布）→ `Process::new_init` 提交点 → stdio 发布（失败=不变量失败 panic 到 reset）→ spawn → join → `wait_tx_drain`（30s 有界，自 k3.rs 等价上移）→ wfi。契约②③逐项成立。
- tty-reader：`budget` 于每次 poll_fn 唤醒重置 256、两段 drain 共享、预算耗尽 `wake_by_ref` 自重排队（消除已过触发沿 IRQ 的 lost wakeup）；单行投递纪律（pending 期间不分类新输入）保留；`readable`/`resolve_read` 共用闩锁使 readiness 与 read 一致。
- 工程卫生：`tests/k3-embedded-user-host/target/` 与 `kernel/resources/k3_stdio_probe.elf` 均被既有 `.gitignore` 规则（`**/target/`、`*.elf`）覆盖，`make k3-stdio` 依赖 `k3-stdio-elf` 保证 fresh clone 重建；`k3-stdio` Make 目标 SMP>1 拒绝与 `k3-uart` 同型。

**Acceptance Gaps**

None。A1（V1 11/11 RED→GREEN）、A2（V3 + 单 consumer 代码核验）、A3（V2 9/9 + V4 readelf + V5/V6 共享门）、A4（2026-10-07 操作者板测 B6 全阶段 + kernel drain）均满足。

**Evidence**

- Act Response V1–V8 结论直接采信（来源：本 Cycle Act Response；覆盖范围未失效——最终树与 Act 声明的变更面一致，`git diff --check` 通过后无后续改动）。
- 采信既有结论：uart_16550 74/74（Iteration 001，crate 未触碰）、tty_write_logic 6 用例（ONLCR 表面未触碰）。
- Plan 独立检查：完整 diff（16 修改 + 6 新增文件全读）、canonical.rs/ldisc.rs/embedded_loader.rs/stdio.rs/entry.rs/Cargo/Makefile/probe/host 包逐文件核验（见 Findings 引用位置）。

**Follow-up Decision**

accepted。T5/T6 契约（含 T5 ①–⑦、T6 ①–⑤、Preserve/Forbidden、Invariants 1–5）经独立代码核验全部满足；3 项非阻塞 finding 记录在案，无需当前 Cycle 修复或 rework Cycle。

**Iteration Plan Update**

None。

**Next Cycle**

None。

**Next Iteration**

`iterations/003-minimal-interactive-shell/000-initial.md`（draft；Gate 2 待用户计划审计批准后转 ready）。
