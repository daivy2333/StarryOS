# Iteration 003 / Cycle 000: 最小交互 shell 与最终集成交接

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: T7、T8（Iteration 003 既有任务）。
- Acceptance gaps: None。
- Repair items: None。
- Inherited scope: R7/R8 已批准场景（S23–S28）与全局契约；design 裁决（K09 唯一 AtomicWaker、tty 层拥有行编辑、单 consumer）；Iteration 000–002 accepted 基线——timer/AIA 底座、PXA port 唯一 owner、完整 UART 数据面（copier 纪律、64KiB ring、四条件 drain）、tty canonical 输入契约、内存根/ELF/stdio 无存储用户态与 probe 板测 B6 全阶段；观察项：`task.join()` 对 `exit_group` 恒 0（真实码在 process 层）。
- Excluded scope: SMP（MS14）、rootfs/块设备（MS18）、MS12 网络执行（仅交接指针）、性能优化、受控迁移、Recorder/Maintainer/全局文档产物。

**Objective**

内嵌最小 shell（help/echo/sleep/exit + 异常输入恢复）在 K3 上以真实交互交付 MS17 用户面；结束语义（exit/EOF/SIGINT 结束 user 而 kernel owner 保持、有界 drain、R68 复位）与受影响兼容验证闭合，并写入 MS12 交接指针。

**Background**

Iteration 002 accepted 后，tty canonical、内存根用户态与 stdio 均已板上验收；本 Iteration 把一次性 probe 载体升级为可重复交互的 shell，并承担 change 收口前的最后一轮集成与兼容检查。

**Investigation Facts**

Current Baseline：Iteration 002 最终 Act Response + accepted Review（2026-10-07）——tty native 11/11、host 契约 9/9、`make k3-stdio`/`k3-uart`/`build`/`lichee(-userbench)` exit 0、`make host-test` 无新失败、板测 B6 全阶段（argv/EAGAIN/行读/ppoll/sleep/EOF/exit 42/kernel 四条件 drain）。工作区含 Iteration 002 全部改动（未提交）。

Current-State Evidence 与关键路径（T7/T8 表面，本 Plan 本次实读）：

1. 嵌入选择链：`entry.rs` k3-stdio 分支以 `include_bytes!("../resources/k3_stdio_probe.elf")` 嵌入镜像、`probe_args=["k3_stdio_probe"]`；feature 链 root/kernel `k3-stdio`（kernel 侧 `k3-stdio=["k3-uart"]`）；`Makefile` `k3-stdio-elf` 用 `$(BENCH_CC) $(BENCH_CFLAGS)` 编译 `tests/k3_stdio_probe.c`，`k3-stdio` 依赖之并带 SMP>1 拒绝（同 `k3-uart` 型）。shell 形态按全局契约新增 `k3-shell` feature（含 `k3-stdio`）与 `k3-shell`/`k3-shell-elf` 目标，entry 在 k3-shell 下嵌入 `kernel/resources/k3-shell.elf`、argv[0] 为 shell 名；boot 序（copiers→内存根→可恢复阶段→`Process::new_init`→spawn→join→`wait_tx_drain`→wfi）整体复用，不改提交点结构。
2. 生产 C 测试先例：`tests/ms04_rx_probe_test.c` 模式——`#define <MOD>_TESTING` 后 `#include` 生产 `.c` 源，直接驱动生产函数、`assert` 断言、fake 只替换系统边界。T7 的 `tests/k3_shell_test.c` 按同型 include `kernel/resources/k3-shell.c` 的生产 parse/dispatch（include 机制为非实质选择）。
3. 行编辑能力边界：tty canonical 已交付 ERASE/KILL、空格/tab 保留、1024B 行界（超长整行 discard→一次 `InvalidInput`→恢复）；WERSE 未实现（Iteration 002 Risks 记录为本轮裁决点）。最小 shell 以 `read()` 整行读、行编辑由 tty 提供，不需要 WERSE，不扩 termios。
4. 退出/信号面：probe 已验证 exit → join → `wait_tx_drain` 四条件 30s 有界 drain；EOF（VEOF 空行 0-read）已验收；SIGINT 路径（ldisc `check_send_signal`：ISIG + 前台组）已有代码但 probe 未覆盖——T8 板测含 Ctrl-C 观察项，若不触发按诊断边界归 tty/信号层，不先改 shell。
5. 共享 UART/兼容面：uart_16550 crate 74/74（Iteration 001，本轮预期不触碰该 crate）；kernel `tty_write_logic` 6 用例；受影响构建门 = `make build`（QEMU）、`make k3`（网络形态，本轮不改网络面）、`make lichee`/`lichee-userbench`（仅当 D1 共享表面被改动）。
6. MS12 交接对象：`openspec/changes/ms12-com260-kit-async-bidirectional-data-plane/{README,design,tasks}.md` 与其冻结首轮 Cycle（不改写）；交接内容为 accepted 底座引用与 replan 边界说明。
7. tty 1024B 行界与 shell 256B buffer 的交互：≤1024B 的行由 tty 整行投递，shell 侧超 buffer 行必须丢弃反馈后读至行尾（不执行截断前缀）；>1024B 行由 tty 整行 discard，read 得到一次 `InvalidInput`，shell 作为可反馈错误处理。

**Implementation Guidance**

先 shell 核心：`kernel/resources/k3-shell.c` 生产 parse/dispatch + `tests/k3_shell_test.c` native RED→GREEN；再形态：`k3-shell` feature/Make 目标/entry 嵌入选择；然后受影响构建与既有门；最后板测交互（B7）与 MS12 交接指针。entry 复用 k3-stdio 提交点结构，只换嵌入镜像与 argv。

**Task Contracts**

### T7：最小 shell

- Requirement/Scenario：R7；S23–S25。
- Depends on：None（板上验收依赖 T8 载体）。
- Targets：`kernel/resources/k3-shell.c`（新，生产命令 parse/dispatch 核心）；`tests/k3_shell_test.c`（新，native 测试）；root/kernel `Cargo.toml` `k3-shell` feature；`Makefile` `k3-shell`/`k3-shell-elf`；`kernel/src/entry.rs` 嵌入选择。
- Current behavior：无 K3 shell；k3-stdio 只嵌 probe，交互不可重复。
- Required behavior：①命令集：`help`、`echo <文本>`、`sleep <ms>`（0..60000）、`exit 0`；②异常输入恢复：空行、未知命令、参数错误、整数溢出、超长行反馈后继续；③256B 命令 buffer：完整读行后执行，超 buffer 行不执行截断前缀（丢弃反馈后读至行尾），tty `InvalidInput`（>1024B 整行 discard）作为一次可反馈错误；④I/O 纪律：read/write 处理 EINTR 与 short write，持久错误退出；sleep 走 `nanosleep`；⑤`k3-shell` 形态：feature 含 `k3-stdio`，entry 嵌入 shell ELF、argv[0]=shell 名，Make SMP>1 拒绝与 k3-stdio 同型。
- Preserve：既有 user/syscall/tty 语义、`k3-stdio`（probe）形态可构建、`k3-uart-port-test`/`k3-uart` 载荷行为、D1 共享表面（预期零改动）。
- Forbidden：fork/exec、脚本/管道/重定向、网络命令、工具框架、新 libc/依赖、修改 tty/ldisc/loader 行为。
- Test witness：`cc -std=c11 -Wall -Wextra -Werror` 编译并执行 `tests/k3_shell_test.c`（include 生产 `k3-shell.c`，fake 只替换 read/write/sleep 系统边界）——先 RED（生产源缺失编译失败或断言失败）后 GREEN；覆盖参数/多命令/文本空格/长行/short write/EINTR。
- GREEN condition：native tests exit 0；shell ELF `readelf -h -l -r` 静态判据（ET_EXEC、RISC-V、无 INTERP、无重定位）；`make k3-shell` exit 0。
- Verification：agent——native test binary exit 0；readelf；`make k3-shell`。板测交互属 T8。
- Stop when：parser 必须新 libc/依赖，或只能通过裁剪命令/修改用户态要求实现。

### T8：最终交互与兼容交接

- Requirement/Scenario：R7/R8；S23–S28。
- Depends on：T7。
- Targets：shell 入口/退出路径（`k3-shell.c` + entry.rs k3-shell 分支复用 T6 提交点/drain 序）；受影响 build 与 tests；MS12 README/design/tasks 交接指针。
- Current behavior：shell 形态未运行；MS12 首轮 ready 暂停待底座。
- Required behavior：①重复交互：help、echo 文本、`sleep 1000` 后 echo 恢复；②错误恢复：非法参数与超长行反馈后继续；③quiet 10s 后新输入有效（IRQ/唤醒恢复）；④结束语义：exit / Ctrl-D（EOF）/ Ctrl-C（SIGINT）分别结束 user 而 kernel owner 保持——join 报告实际结果后 `wait_tx_drain` 有界 drain（超时明确失败、不清数据）；⑤R68 RAM boot 环内外部 reset 回原系统；⑥MS12 交接：仅准确指针（MS17 accepted 底座引用 + 恢复需必要 replan），不改冻结 Cycle、不跳过网络要求。
- Preserve：原 `make k3`、QEMU/D1 构建与共享 driver 行为；冻结 MS12 Cycle、全局 tasks/M/R/I 与 Recorder 产物。
- Forbidden：本任务不另建证据工具测试；无额外日志协议。
- Test witness：退出/错误/等待语义采信 T6/T7 生产 tests（覆盖未变化直接采信）；新增变化有先行 RED 或既有 GREEN。
- GREEN condition：受改共享 UART suite、`make k3`、`make k3-shell` 通过；板测 B7 全场景通过；交接指针写入。
- Verification：agent——`make k3`（网络排除面不动）、`make build`、改 D1 共享表面时 `make lichee`、`make host-test`（无新失败）、`git diff --check`；构建实际待测 shell FIT（避免同名产物被旧 target 覆盖）。操作者（B7）——装载后依次：help → echo 文本 → sleep 1000 → echo 恢复 → 非法参数反馈恢复 → 超长行反馈恢复 → quiet 10s → 新命令有效 → exit（或 Ctrl-D / Ctrl-C 对照轮）→ kernel drain 行 → R68 复位回原系统；一句话结果记入 Act Response。
- Stop when：需要 SMP/网络/存储以通过 shell 验收；退出丢数据却报成功；原平台产生本次回归；board 未提供结果。

**Invariants**

1. Iteration 000–002 全部不变量继续有效：唯一 RX consumer（boot-path 互斥）、copier 纪律、四条件 drain、tty canonical 契约、`new_init` 前可恢复失败。
2. K09/K26：不引入第二套 executor/ring/waker。
3. `k3-stdio`（probe）形态可构建、行为不回归；`k3-uart-port-test`/`k3-uart` 载荷行为不变。
4. QEMU/D1 构建与既有测试语义不回归（VF2 pre-existing 失败维持原归属，不重复全量追绿）。
5. MS12 冻结产物不改写。

**Non-goals**

网络命令、完整 POSIX shell、SMP（MS14）、rootfs/块设备（MS18）、性能优化、受控迁移、第二 UART。

**Acceptance**

| ID | 可观察结果 | 映射 |
|---|---|---|
| A1 | k3_shell native tests RED→GREEN + readelf 静态判据 + `make k3-shell` exit 0 | T7 / R7 / S23–S25 |
| A2 | 板上 B7：重复交互/错误恢复/quiet/exit（EOF/SIGINT 对照）/drain/R68 复位 | T7+T8 / R7/R8 / S23–S28 |
| A3 | 受影响门不回归（`make k3`/`build`、按需 `lichee`、host-test 无新失败） | T8 / R8 / S26–S27 |
| A4 | MS12 交接指针准确写入（不改冻结产物） | T8 / R8 / S28 |

RTM（本 Cycle）：R7→T7/T8 Covered；R8→T8 Covered。R1–R6 已由 Iteration 000–002 闭合。

**Verification**

V1（agent，native）：`cc -std=c11 -Wall -Wextra -Werror` 编译执行 `tests/k3_shell_test.c`（RED→GREEN）。
V2（agent，build）：shell ELF readelf 判据；`make k3-shell`、`make k3`、`make build` exit 0；改 D1 共享表面时 `make lichee`；`make host-test` 无新失败；`git diff --check` exit 0。
V3（操作者，B7）：交互全场景 + drain + R68 复位；无结果则 A2 pending，change 不收口。
V4（失败处理）：交互异常按 Stop when 停在本层并交 Plan；三次失败规则适用。

**Gate 2 Readiness**

PASS — 2026-10-07 用户计划审计原话"批准"。调查闭合（Iteration 002 accepted 基线 + 本 Plan 实读的嵌入选择链、C native 测试先例、行编辑/信号能力边界、共享门与 MS12 交接对象）、契约自包含（T7/T8 变更面、Preserve/Forbidden、Stop when 齐备）、追踪完整（R7/R8→T7/T8 Covered）、验证为最简直接判定；无 BLOCKED/WAIVED、无 Missing/Simplified、无待 Act 决定的实质未知项（native 测试的 include 机制、shell 反馈输出措辞为非实质选择，留在 Act）。Plan Context 离开 draft 改为 ready；下一边界为 Act 指令。

**Persisted Evidence**

- Mode: none

命令、≤20 行决定性输出、exit 与操作者一句话结果足以判定。

**Risks and Notes**

- WERSE 裁决：最小 shell 行编辑由 tty canonical ERASE/KILL 提供，不需要 WERSE，不扩 termios；未来 shell 需词删除时再裁决。
- SIGINT 板测依赖 ISIG 默认置位与前台进程组绑定（既有 ldisc 路径）；若 Ctrl-C 不触发，按诊断边界归 tty/信号层排查，不先扩 shell。
- shell exit 码观察以 shell 自身输出为准（`task.join()` 恒 0 为已记录观察项，非缺陷）。
- 嵌入镜像构建顺序：`k3-shell` 必须依赖 `k3-shell-elf`（`*.elf` 被 git 忽略，fresh clone 需先构建）。

## Act Response

- Status: reported

### 实际改动

T7（最小 shell）：

- `kernel/resources/k3-shell.c`（新）：生产 parse/dispatch 核心。命令 `help`、`echo <text>`（保留文本内部空格）、`sleep <ms>`（十进制 0..60000，拒绝负数/非数字/溢出/多余参数，走 nanosleep，EINTR 按 rem 续等）、`exit [code]`（无参=0，0..255，溢出/多参拒绝）。空行/纯空格返回提示符；未知命令回显词后继续；256B 命令缓冲（255 字符+NUL），超长行整行丢弃、读至行终止、不执行截断前缀；tty >1024B 整行 discard 的 EINVAL 交付映射为同一"line too long"一次性反馈。read/write 全部 EINTR 重试、short write 续写、0 返回按持久错误；持久错误经 stderr 报告后退出（read 错误 exit 1、写错误经 fd2 报告 exit 1）；EOF（含非空行后 EOF 的 partial line 丢弃）exit 0。边界 `k3_boundary_{read,write,nanosleep}` 为唯一 fake 面；测试态 `K3_SHELL_TESTING` 下不定义 main。
- `tests/k3_shell_test.c`（新）：native 测试，include 生产源，fake 只替换三个系统边界（脚本化 stdin、捕获 fd1/fd2、写故障注入 fail/EINTR/zero/半写、sleep 记录+EINTR/失败注入）。20 项覆盖：参数/多命令/文本空格/空行/未知命令/help 多参/sleep 校验+0 边界+溢出/exit 形态/255 与 256 字节行界/EINVAL 整行丢弃（含部分前缀不执行）/立即与部分 EOF/read EINTR/read 持久错误/半写完成/写 EINTR/写 0 返回/写持久错误。
- root `Cargo.toml`：新增 `k3-shell = ["k3-stdio", "starry-kernel/k3-shell"]`；kernel `Cargo.toml`：新增 `k3-shell = ["k3-stdio"]`。
- `Makefile`：新增 `k3-shell`（SMP>1 拒绝，与 k3-stdio 同型构建参数）与 `k3-shell-elf`（BENCH_CC 静态 ELF）；.PHONY 同步。

T8（集成与交接）：

- `kernel/src/entry.rs` k3-stdio 块：cfg 按载荷选嵌入 ELF 与 argv[0]/任务名（`k3-shell` ↔ `k3_stdio_probe`），boot 序（copiers→内存根→可恢复阶段→`new_init`→spawn→join→`wait_tx_drain` 30s 四条件→wfi）未改；diag mode 行改为按 `payload_name` 动态输出。
- MS12 交接指针（仅指针，冻结首轮 Cycle 未触碰）：`ms12-.../README.md`、`tasks.md`（替换"当前尚未产生新的 MS17 实现基线"为底座已交付事实+指针）、`design.md`（新增 2026-10-07 交接指针段），指向 `../ms17-com260-kit-async-uart-interactive-shell/tasks.md` 的 accepted 状态；GMAC/queue/socket 面仍归 MS12，T1 timer/IRQ/feature 接缝恢复时以 MS17 accepted 结论为输入。

### 与计划的偏差

1. 嵌入选择实现：`const PAYLOAD_ELF: &[u8]` 丢失 include_bytes 定长信息（E0308），改为按 cfg 分臂声明 `PAYLOAD_ELF_LEN` + `EMBEDDED_PAYLOAD`（原 Iteration 002 模式的 cfg 复用）；契约要求的行为不变，cfg 剥离臂不读另一 ELF 文件，两个形态可独立构建。
2. `exit` 支持无参形式（=0）：S26 场景原文"操作者执行 exit"要求裸 `exit` 可用；`exit 0` 为其显式参数形式，契约的 `exit 0` 完整覆盖。
3. diag mode 行由字面 `mode=k3-stdio` 改为 `mode={payload_name}`：诊断措辞属 Plan 留给 Act 的非实质选择；probe 形态下该行信息量不减。
4. 测试预期修正（无生产改动）：EINVAL fake 初版断言与契约语义矛盾（EINVAL 整行丢弃的字节不会到达 shell），改为按已投递字节数触发 EINVAL，并补"部分前缀不执行"用例；生产代码未因此改动。

### 自检与修复

- Gate 3（T7）：发现 sleep/exit 数值错误路径向 `cmd->arg` 的赋值为死代码，已删除并重跑 native 测试 GREEN。其余检查：命令面/错误面/容量界与契约一一对应；无 fork/exec/管道/重定向/新依赖；k3-stdio probe 形态构建验证通过（字符串与 Iteration 002 等值）。
- Gate 3（T8）：MS12 三文件仅指针改动，`git diff --stat` 证实其 `iterations/` 无变化；FIT 为最后构建的 shell 镜像（避免同名覆盖）。
- 遗留 Minor：Makefile 顶层 `ifeq ($(shell test $(SMP) -gt 1; ...))` 在 SMP 未定义时每次 make 打印 `test: -gt: unary operator expected`（pre-existing，影响所有目标，本轮未触碰；不构成 Issue 准入）。

### 验证（agent）

1. TDD 见证：`cc -std=c11 -Wall -Wextra -Werror tests/k3_shell_test.c -o /tmp/ms17-k3-shell-test`
   - RED（实现前）：`fatal error: ../kernel/resources/k3-shell.c: No such file or directory`，exit 1。
   - GREEN（实现后）：`k3 shell tests: 20 passed`，exit 0（Gate 3 修复后重跑仍 20/20）。
2. shell ELF 静态判据：`riscv64-linux-musl-readelf -h -l -r kernel/resources/k3-shell.elf`，exit 0：`Type: EXEC`、`Machine: RISC-V`、无 INTERP 段、`There are no relocations in this file.`
3. `make k3-shell`：exit 0（含 SMP>1 拒绝逻辑、FIT 重打包，StarryOS_riscv64-k3.fit 20:06 为最后构建的 shell 镜像）。
4. 受影响构建门：`make k3-stdio` exit 0（probe 形态保持）；`make k3` exit 0（网络面未触碰）；`make build` exit 0（QEMU）。
5. `make host-test`：exit 0，全部 suite `0 failed`（SNAPSHOT 记录的 ms04 既有失败在本轮工作区已不复现；未由本轮改动触碰）。
6. `git diff --check`：exit 0（无空白错误）。
7. `openspec validate ms17-com260-kit-async-uart-interactive-shell`：`Change ... is valid`，exit 0。

### Persisted Evidence

Plan 设定 `none`；Act 最初 `None required`。B7 完成后用户明确要求保存串口输出（白名单：用户明确要求），已创建：

- `evidence/003-minimal-interactive-shell/000-initial/README.md`（来源/结论/文件/限制）
- `evidence/003-minimal-interactive-shell/000-initial/b7-serial-round1.txt`
- `evidence/003-minimal-interactive-shell/000-initial/b7-serial-round2.txt`

### Board B7（操作者结果，2026-10-07）

操作者按 R68 RAM boot 装载 `StarryOS_riscv64-k3.fit`（20:06 shell 镜像）完成两轮交互，裁决原话："异步串口正常"；串口原文存 Evidence `b7-serial-round1.txt` / `b7-serial-round2.txt`。逐项结果：

1. 启动链：`[ms17] mode=k3-shell begin ...` → shell banner → `sh> `，两轮均成立。
2. 重复交互（S23）：`help` 输出完整命令行（round1），`echo l2isdja` → `l2isdja` 回显（round2）。
3. sleep（S24）：`sleep 10` 后提示符返回，后续命令正常响应。
4. 非法输入恢复（S25）：`ls ./`、`helo…`（→`unknown command`）、`sleep abc`、`exit -1`（→usage）各自反馈后回到提示符。
5. 超长行：>256 字符一行 → `[shell] line too long, discarded`，截断前缀未执行，`exit` 仍正常。
6. quiet 后新输入：每条命令均在空闲间隔后输入并得到响应（IRQ/唤醒恢复持续成立）；无单独 10 s 计时项。
7. exit（S26）：`exit` → `[shell] exit code=0` → kernel `stage=exit join=0` → `stage=drain done (TxCompletion four conditions)` 后停车。
8. 对照轮：Ctrl-C（SIGINT）结束用户进程且 kernel owner 保持（round1）；Ctrl-D（EOF）未单独跑，其 read-0 语义由 Iteration 002 B6 板测（同一 tty 路径）与 T7 native 测试覆盖。
9. 外部 reset（S27）：操作者执行并确认回原系统（原话"reset我顺手做了"）。
10. 备注：round1 `helo^Hp^H^H → helop` 为终端 Backspace 发送 0x08 所致（内核 VERASE 默认 0x7f，`terminal/termios.rs:41`，与 Linux 默认一致），非内核缺陷，无需改动。

### Experience Candidates

None（无范围外实质缺陷；Makefile SMP 未定义时的解析期提示为 pre-existing Minor，不满足 Issue 准入）。

### 未解决问题

- Ctrl-D（EOF）结束路径未在 B7 单独执行：语义由 Iteration 002 B6 板测（同一 tty read-0 路径）与 T7 native 测试覆盖，是否需补跑交 Plan Review 裁决。
- quiet 项无单独 10 s 计时观察；空闲后新输入响应在整个会话中持续成立，按操作者裁决接受。
- 其余 A1–A4 均有新鲜证据；A2 板测经操作者裁决通过（原话"异步串口正常"）。

## Plan Review

- Review Result: accepted
- Reviewed: 2026-10-07（独立阅读完整 diff、生产 shell 源、20 项 native 测试、feature/Make/entry 分臂、MS12 交接 diff、B7 串口原文与 FIT 构成）

**Findings**

阻塞项：无。非阻塞项：

1. Minor（组合覆盖，Act 留裁事项 1）：B7 未单独执行 Ctrl-D（EOF）轮。裁决接受——kernel 侧 VEOF→read-0 已由 B6 板测验收（同一 `ASYNC_TTY`/ldisc/canonical/read 路径）；shell 侧 EOF 反应由 T7 native 测试直接覆盖（`test_eof_immediate`/`test_eof_after_partial_line_discards_it`）；"user 结束→owner 保持→drain"的内核侧语义在 B7 已由 exit 与 SIGINT 两条不同终止路径板测。契约 B7 步骤原文"exit（或 Ctrl-D / Ctrl-C 对照轮）"由 Ctrl-C 对照轮满足。组合链各层均有直接证据，无需补跑。
2. Minor（观察窗口，Act 留裁事项 2）：quiet 无单独 10 s 计时项。裁决接受——10 s quiet + quiet 后新 IRQ 有效已在 Iteration 001 B5 于同一 UART 数据面（copier/IRQ/base 未变）板测验收；tty-reader 无任何时间性状态（唤醒只依赖 PollSet 注册 + RX IRQ）；B7 全会话中每条命令均在空闲间隔后得到响应，且操作者裁决接受。
3. 观察项（非缺陷）：round1 `helo^H…→helop`——操作者终端 Backspace 发送 0x08，内核 VERASE 默认 0x7f（`terminal/termios.rs:41`，与 Linux 默认一致），0x08 经 canonical 分类丢弃。发送 DEL（0x7f）的终端行编辑正常；该边界与 Linux 同型，无需改动。
4. 观察项（非缺陷）：两轮 drain 完成后的 `^C^C`/`^C^C^[a` 输入——内核已停入 wfi、无消费者，字节落 RX ring，无害且符合停车语义。
5. Minor（pre-existing）：Makefile 顶层 `ifeq ($(shell test $(SMP) …))`（第 31 行）在 SMP 未定义时打印 `test: -gt: unary operator expected`——已在 HEAD 核实为既有行，本轮未触碰，不满足 Issue 准入。

独立核验通过的关键面（代码 review，非 Act 自检）：

- 生产 `k3-shell.c`：契约 ①–④ 逐项成立——命令集（help/echo/sleep 0..60000/exit 0..255，十进制纯数字、负号即 malformed、`ULONG_MAX` 先验防溢出）；异常恢复（空行/纯空格/未知命令/参数错误/超长行反馈后继续）；256B buffer（255 字符+NUL 容纳，256 字符行整行丢弃读至行尾、不执行截断前缀；tty EINVAL 映射同一"line too long"一次性反馈）；I/O 纪律（read/write EINTR 重试、short write 续写、write 0 返回按持久错误、持久错误经 stderr 报告退出、sleep EINTR 按 rem 续等、EOF/EOF-partial exit 0）。无 fork/exec/管道/重定向/网络/新依赖。
- `tests/k3_shell_test.c`：`#define K3_SHELL_TESTING` include 生产源，fake 仅替换 `k3_boundary_{read,write,nanosleep}` 三个系统边界；20 用例与 Act 声明一一对应，覆盖超出见证要求（EINVAL 部分前缀不执行、EOF 两种形态、write 0 返回、sleep 0 零 syscall）。
- 形态面：root/kernel `k3-shell` feature 链、Makefile `k3-shell: k3-shell-elf`（`*.elf` 忽略下 fresh clone 可重建）+ SMP>1 拒绝同型 + .PHONY；entry cfg 分臂（`payload_name`/`PAYLOAD_ELF_LEN`/`EMBEDDED_PAYLOAD`）互不引用对方 ELF、两形态可独立构建，boot 提交点序未改——与 Deviation 1 的 E0308 理由和 `make k3-stdio` 独立构建通过一致；板测串口 `mode=k3-shell` 直接证实载荷选择。
- MS12 交接：`git diff --stat` 仅 README/design/tasks 三文件 +4/-2 行，`iterations/` 零改动；指针内容准确（accepted 底座清单、GMAC/queue/socket 归属不变、replan 边界保留）。A4 成立。
- FIT 构成（同名覆盖契约要求）：`strings` 检验最终 `StarryOS_riscv64-k3.fit`——shell banner（"k3 minimal interactive shell"）存在、probe 字符串（"stage=argv begin"）为零：最终 FIT 确为 shell 镜像。
- Evidence（用户点名白名单）：README 注明来源与限制；两份串口原文与 Act 声明的 B7 逐阶段吻合（boot 链/help/echo/sleep/非法参数 usage/超长行 discard/`exit code=0`→`stage=exit join=0`→四条件 drain/Ctrl-C 对照/reset）。

**Acceptance Gaps**

None。A1（native 20/20 RED→GREEN + readelf + `make k3-shell` exit 0）、A2（B7 两轮全场景 + 操作者裁决"异步串口正常"）、A3（`make k3-stdio`/`k3`/`build` exit 0、host-test 无新失败）、A4（MS12 指针写入且冻结产物未动）均满足；两项留裁事项按上文裁决关闭。

**Evidence**

- Act Response 验证 1–7 结论直接采信（来源：本 Cycle Act Response；最终树与 Act 声明变更面一致，`git diff --check` 通过后无后续改动）。
- 采信既有结论：uart_16550 74/74（Iteration 001，本轮未触碰）、tty_write_logic 6 用例（未触碰）、tty_input_logic 11/11（canonical 面未触碰）、k3-embedded-user-host 9/9（loader/stdio 面未触碰）。
- Plan 独立检查：完整 diff、k3-shell.c/k3_shell_test.c 逐行、entry/Cargo/Makefile 分臂、MS12 diff、B7 串口原文、FIT `strings` 构成判定、Makefile 第 31 行 HEAD 溯源。

**Follow-up Decision**

accepted。T7/T8 契约（含 Deviations 1–4 均经代码核证为非实质或必要修正）与 Invariants 1–5 全部满足；5 项非阻塞 finding 记录在案，无需当前 Cycle 修复。Iteration 003 为最后一个逻辑 Iteration，MS17 全部 8 项任务闭合。

**Iteration Plan Update**

None。

**Next Cycle**

None。

**Next Iteration**

None（change 全部 Iteration 完成；收尾归 `openspec-docs-maintainer`，属用户决策）。
