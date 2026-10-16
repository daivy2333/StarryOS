# Iteration 002 / Cycle 000: UART 多 hart 与用户交互

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: T7–T8（3.1–3.2），按 T7→T8 执行。
- Acceptance gaps: None。
- Repair items: None。
- Inherited scope: R4（S10–S14）与 R3 的 UART 角色面（S08–S09）、R8 受影响旧构建；Iteration 000 已交付的 AP/local timer/S_SOFT/固定设备路由/跨簇 IPI 底座与 `k3-smp` 构建隔离；MS17 已验收的 PXA port/copier/tty/shell 单 hart 契约；真板操作由用户手动执行（change 级授权原话覆盖，见 000 Cycle）。
- Excluded scope: 网络恢复矩阵与 ISS02/ISS03 修复（T9–T11）、组合长稳（T12–T13）、受控迁移功能、第二 UART、性能优化、rootfs/存储、以 QEMU/Linux 结果替代 K3。

**Objective**

16-hart `k3-smp-uart` 模式族下，UART 沿既有 PXA port/共享硬件锁/copier/ring/tty 契约运行：RX/TX copier 经共享 placement 取 singleton；tty+内嵌 shell 子形态的 shell 用户任务固定 hart 8（A100 簇）、alarm 与主任务固定 boot hart 后才创建 deadline 等待；raw 专项子形态复用 MS17 Iteration 001 载荷覆盖 FIFO/ring wrap、短 RX 末事件、Full→恢复、drop 分类、有界公平、竞争/取消/timeout 与四条件物理 drain；两子形态按独立 boot 选择、不同时消费 RX；旧单 hart UART 家族与构建不回归。

**Investigation Facts**

- Current Baseline: 2026-10-15 Iteration 001 accepted——`k3-smp-net` NIC 稳态板测 A5–A8 成立（owner/runner/app actual hart 与 placement 一致、四协议、两级容量/C4、quiet 冻结→resume）；host 面 plane harness 7/7、k3-async-net-host 26/26、四个构建 exit 0（来源：001 Cycle Act Response 与 Plan Review，覆盖未变化）。MS17 单 hart UART 基线已验收：PXA port（115200 8N1、三步 FCR+PXAR8、Errata20 序、唯一 owner 激活）、64KiB copier 双 ring、check-register-arm-recheck、四条件物理 drain（末字节 TEMT 由 native 1 ms timer hook）、tty canonical 与内嵌 shell（B7 全场景）；native 面 uart_16550 crate 74/74、tty_input_logic 11/11、tty_write_logic 6、embedded-user-host 9/9（MS17 Iteration 003 采信记录，本轮输入未变时继续采信）。
- Current-State Evidence（本轮新查，2026-10-15 工作树）：
  - copier placement 接缝已存在且 SMP-ready：`uart_init.rs::start_copiers`（545）自行从 `schedulable_cpu_mask()` 构造升序列表、以 boot anchor 调 `place_roles`、对 `uart_rx`/`uart_tx` 构造 singleton mask、`spawn_with_name_affinity` 提交、handle 存入 `RX/TX_COPIER_TASK`、`CopierStartGuard` claim/rollback 重复启动 fail-closed（panic，非 Result——板上不做主动重复见证，语义由 plane harness 既有 CopierStartGuard 用例承载）。16 升序 schedulable、anchor=0 时 RX=0、TX=1（X100 簇）。
  - 实际 hart 观察接缝已存在：`uart_smp_snapshot::record_pinned/record_harts/snapshot()`（MS08 遗产）——`start_copiers` 已用 `record_harts` 包裹两个 copier future 记录实际 poll hart；K3 ISR（`k3_uart.rs::k3_uart42_irq_handler`，101）锁内分类、锁外 wake（RX_WAKER/TX_WAKER），`activate_port` 在调用 hart（boot）注册 AIA route 42。
  - 主任务/应用/alarm 的 affinity 能力：`axtask::set_current_affinity(mask) -> bool`（api.rs:327，mask 含当前 cpu 时无迁移）、`spawn_task_with_affinity(TaskInner, mask) -> Option<AxTaskRef>`（api.rs:239）、`spawn_raw_with_affinity`（api.rs:264）均既有。`task/timer.rs::spawn_alarm_task`（270）当前无 affinity（`spawn_raw`），其 `alarm_task` 体含 `timeout_at` deadline 等待——design D5 明确"alarm 增加最小 affinity 入口，旧 API 保留"。
  - entry.rs UART 家族分支序（现状）：k3-async-uart+k3-uart raw 分支（125，`not(any(k3-stdio, k3-rootfs))`）→ k3-stdio 分支（146，cfg 仅 `k3-stdio`：init_uart_hardware → start_copiers → 内存根/挂载 → `spawn_alarm_task()` → 内嵌 ELF（k3-shell 经 `#[cfg(feature="k3-shell")]` 选择）→ `new_user_task` → `axtask::spawn_task`（无 affinity）→ join → `block_on(timeout(30s, wait_tx_drain))` → wfi）。主任务首个 TimerFuture 为 drain timeout。
  - raw 载荷：`platform/k3.rs::run_uart_data_plane`（474，`-> !`）阶段为 duplicate-activate 拒绝见证 → echo×2 → long ≥256B → txfull 短写/恢复 → 四条件 drain → quiet 10 s → post-quiet echo → errors 账本行（rx_dropped/oe/pe/fe/bi/fifoerr/closed 分类）；行读经 check-register-recheck（`read_line`）。载荷含 `sleep`/`timeout` TimerFuture——在主任务上下文运行。
  - 守卫与闭包：kernel lib.rs `k3-async-uart`+`smp` compile_error（36）需开 `k3-smp-uart` 唯一例外（沿 001 的 k3-smp-net 先例）；`k3-async-net`+`k3-async-uart` 互斥守卫已拒绝与网络模式组合。root 家族：`k3-uart=[k3-async-uart, axfeat/paging, starry-kernel/k3-uart]`、`k3-stdio=[k3-uart, …]`、`k3-shell=[k3-stdio, …]`；`k3-smp-uart` 蕴含 `k3-shell`（即蕴含 k3-stdio+k3-uart+k3-async-uart 全链）时，单 hart raw 分支被 `not(k3-stdio)` 自然排除，stdio 分支与 000 的 k3-smp 平台见证分支需补 `not(k3-smp-uart)`。
  - Makefile：`k3-uart`/`k3-stdio`/`k3-shell` 均 SMP>1 拒绝（367/376/391），`k3-shell-elf`（399）产出 `kernel/resources/k3-shell.elf`（已存在）；`k3-smp-uart` 目标应依赖 `k3-shell-elf`。`.gitignore` 现有 `/.axconfig.*` 模式不匹配 `.axconfig-k3-*.toml`（001 Review Finding 2，check-ignore 证实）。
  - 测试入口：`rustc --edition=2024 --test tests/k3-uart-plane-host-harness.rs`（已 include placement/copier_start_guard/k3_smp_net，7 用例）；`rustc --edition=2024 --test tests/k3-uart-host-harness.rs`（k3_uart_core 寄存器批次 ~26 用例）；`kernel/tests/tty_input_logic.rs`/`tty_write_logic.rs`（kernel crate 集成测试）；`cargo test --manifest-path tests/k3-embedded-user-host/Cargo.toml`；`cargo test -p uart_16550`（74）。
- Code and Critical Path:

| 入口/符号 | 本轮责任 |
|---|---|
| root/kernel Cargo、Makefile、`tools/starry-k3-smp-uart.its`/`starry-k3-smp-uart-raw.its`（新增） | 新 `k3-smp-uart` 模式族与 `k3-smp-uart-raw` 子形态选择器闭包、目标与 FIT；旧目标行为不变 |
| `kernel/src/platform/k3_smp_uart.rs`（新增） | 纯接缝（roles 组合 + shell hart 校验，host 可测）+ 两个 SMP boot 入口（cfg 门控） |
| `kernel/src/entry.rs`、`kernel/src/lib.rs`、`kernel/src/platform/mod.rs` | 两个新分支（先于 stdio/k3-smp 见证分支）；stdio 与 k3-smp 见证分支补 `not(k3-smp-uart)`；`k3-async-uart`+`smp` 守卫开例外 |
| `kernel/src/task/timer.rs` | `spawn_alarm_task` 增加最小 affinity 变体（纯增，旧 API 逐字保留） |
| `kernel/src/drivers/uart_init.rs`、`k3_uart.rs`、`k3_uart_core.rs`、`platform/k3.rs` 载荷 | 仅消费（start_copiers/run_uart_data_plane/wait_tx_drain/diag_line），不改 |
| `tests/k3-uart-plane-host-harness.rs` | 新接缝先行测试 |

**Implementation Guidance**

先在 plane harness 写新接缝 RED（模块缺失编译失败；`make k3-smp-uart SMP=16`/`make k3-smp-uart-raw SMP=16` 无目标天然 RED），再接 Cargo/Make/entry/lib/mod，随后新模块（纯接缝 + boot）与 timer.rs alarm 变体，最后两镜像与 FIT。boot 内 placement 与 `start_copiers` 使用同一输入（升序 schedulable + boot anchor），`place_roles` 确定性保证两者一致；不硬编码 RX/TX 值，观察行打印 seam 实际返回。shell 实际 hart 用一次性 first-write 内核侧记录（新形态包装 stdio 输出文件或等价新增接缝；不改共享写路径语义）。

**Task Contracts**

### T7: `k3-smp-uart` 模式族与固定角色接线

- Requirement/Scenario: R3/S08–S09（UART 面）、R4 前置、R8/S26。
- Depends on: 001 accepted。
- Targets: root/kernel Cargo 新 `k3-smp-uart`/`k3-smp-uart-raw`；Makefile 两目标与两 `.its`；`kernel/src/entry.rs` 分支序；`kernel/src/lib.rs` 守卫例外；`kernel/src/platform/mod.rs` 模块门；`kernel/src/platform/k3_smp_uart.rs`（新，纯接缝 + 两 boot）；`kernel/src/task/timer.rs` alarm affinity 变体；`tests/k3-uart-plane-host-harness.rs` 先行测试。
- Current behavior: 无多 hart UART 模式；`k3-uart`/`k3-stdio`/`k3-shell` SMP>1 拒绝；stdio 分支的 shell 用户任务与 alarm 无 affinity；主任务未固定。
- Required behavior: 新闭包只新增显式依赖（root `k3-smp-uart = [k3-smp, k3-shell, starry-kernel/k3-smp-uart]`、root `k3-smp-uart-raw = [k3-smp-uart, starry-kernel/k3-smp-uart-raw]`，kernel 侧 `k3-smp-uart = [k3-smp, k3-shell]`、`k3-smp-uart-raw = [k3-smp-uart]`——子形态选择器沿 k3-shell 之于 k3-stdio 的先例；raw 镜像包含 stdio 树编译但不构造 ASYNC_TTY、不挂内存根，RX 消费者为唯一 raw reader）。`make k3-smp-uart SMP=16` 与 `make k3-smp-uart-raw SMP=16` 产出隔离目录与独立 FIT（与 k3-smp 同 RAM 布局：load/entry 0x180000000、FDT 0x181000000），SMP=1/>16 拒绝，2/4/8 允许有界诊断（raw 形态；shell 形态因固定 shell hart 需 ≥9 harts，不足即入口 fail-closed park）。`k3-smp-uart` 目标依赖 `k3-shell-elf`。`.gitignore` 补 `out-k3-smp-uart/`、`out-k3-smp-uart-raw/` 独立行，并把 `/.axconfig.*` 修正为覆盖 `.axconfig-*.toml` 变体的模式（本轮新 OUT_CONFIG 产物不得滞留为未忽略噪声；同时闭合 001 Review Finding 2 的面向后续一半）。新模块纯接缝：`place_smp_uart_roles(升序 schedulable, anchor) -> Result<SmpUartRoles, …>`（uart_rx/uart_tx 来自 `place_roles` 实际返回、alarm_hart=anchor；错误含 InvalidSchedulable 与 copier 重叠 fail-closed）+ `shell_hart_for(schedulable, &roles) -> Result<usize, …>`（固定 SHELL_HART=8；Missing/Collide fail-closed；raw 形态不调用）。shell boot：placement 行 → `init_uart_hardware` → 主任务 `set_current_affinity(boot singleton)`（先于任何 TimerFuture）→ `start_copiers` → 有界等待后 copier pinned/actual 观察行（uart_smp_snapshot）→ 内存根/挂载 → alarm affinity 变体（boot singleton）→ 内嵌 k3-shell.elf 加载 → `spawn_task_with_affinity`（shell singleton，拒绝即 park）→ shell 实际 hart 一次性观察行 → join → drain timeout(30 s) → park。raw boot：placement 行 → `init_uart_hardware` → 主任务固定 → `start_copiers` → copier 观察行 → `run_uart_data_plane()`（逐字复用，含其 duplicate-activate 拒绝见证）。
- Preserve: MS17 全部硬件与数据面契约（FCR/PXAR8/Errata20 序、三步激活、唯一 owner、SPSC ring、check-register-arm-recheck、四条件 drain、tty canonical、shell 命令集）；CopierStartGuard claim/rollback 语义；placement 公式与容量权威；ISR 锁内分类/锁外 wake；旧单 hart UART 家族行为与 Makefile SMP 拒绝；000/001 的 k3-smp/k3-smp-net 入口与构建。
- Forbidden: 改 placement 公式、建第二 copier/port/tty、把 QEMU/Linux 结果当 K3 证据、动网络/恢复/组合面、为证据新增身份机制、修改 `run_uart_data_plane`/`start_copiers`/`k3_uart_core` 生产逻辑。
- First test: plane harness 新接缝用例（模块缺失 RED）：16 hart placement（RX=0/TX=1/anchor=0 形态、值来自 place_roles）、anchor 内/外、shell hart 9 缺失边界（<9 harts fail-closed）、shell 与 copier 重叠拒绝、非法集合 fail-closed；`make k3-smp-uart SMP=16`/`make k3-smp-uart-raw SMP=16` No rule RED。
- GREEN condition: 新接缝用例 GREEN；`make k3-smp-uart SMP=16` 与 `make k3-smp-uart-raw SMP=16` exit 0 产出 FIT；`make k3`、`make k3-uart`、`make k3-stdio`、`make k3-shell`、`make k3-smp-net SMP=16` 旧目标 exit 0 且形态不变；新入口 placement/copier/shell/alarm 观察行可达（实际 hart 判定归 T8 板测）。
- Verification: `rustc --edition=2024 --test tests/k3-uart-plane-host-harness.rs`（扩展后全量一次）；轮末受影响面按 Iteration Plan 边界（未触碰的 uart_16550/core/tty/user-host 面采信既有结论）；`make build`（QEMU）与 `make lichee`（D1）各一次（timer.rs 为共享面纯增）。
- Stop when: place_roles/start_copiers 接缝不足以支撑组装、stdio 家族分支排他无法保持、固定 affinity 与 TimerFuture 生命周期冲突需改共享 timer runtime；返回 Plan。

### T8: UART 多 hart 板上专项（两子形态）

- Requirement/Scenario: R4/S10–S14、R3/S06/S09、R8/S27。
- Depends on: T7。
- Targets: `k3-smp-uart`/`k3-smp-uart-raw` FIT；操作者板测与 Act Response。
- Current behavior: 无 K3 多 hart UART 运行结论；单 hart raw 载荷与 shell 交互已在 MS17 板测成立。
- Required behavior: 一次板测会话、两次独立 boot（先 raw 后 shell 或反之，不同时消费 RX）：raw 子形态按 MS17 plane 操作序驱动——双向 echo 内容/次序、long ≥256 B 行、txfull 短写只重试未接受字节→恢复→四条件 drain、quiet 10 s 后一行新输入（短 RX 末事件/readiness 与实际 read 一致）、errors 账本行 drop 分类有界；shell 子形态按 MS17 shell 操作序——help/echo/sleep/exit、行编辑（ERASE/KILL）、EOF 0-read、Ctrl-C（SIGINT）对照、异常输入恢复、退出后 30 s 上限物理 drain；两 boot 均观察 placement/copier pinned+actual/shell（含 first-write 实际 hart）/alarm 行与跨簇（shell 8↔copiers 0/1）。操作者会话不通过则停止；真实结果通过后才填 Response。drop 分类的过载语义若操作者无法稳定制造自然过载，按 proposal 裁决由既有 core/crate harness GREEN 采信，板上账本行如实记录。
- Preserve: payload 不判 PASS（操作者按串口裁决）；R68 RAM 启动路径与无持久写入边界；一次板测预算（首次失败按场景停止定向诊断，不盲试）。
- Forbidden: QEMU/主机结果替代板测、两 RX 消费者同 boot 并存、新增判定协议/身份工具、把 configured mask 当实际执行。
- First test: 采信 T7 GREEN 与未变化的既有 host 结论作为先行观察；本 task 为集成板测，不新增镜像身份测试。
- GREEN condition: 下方 Acceptance 全部真实满足且无未执行/豁免项；Fail/超期停止不盲试到 PASS。
- Verification: 轮末 plane harness 全量一次；两 FIT 构建一次。最短人工步骤（复用 Runbook `com260-uboot-usb-starryos-ram-boot` 与 000 FIT 流程）：raw boot——fatload `StarryOS_riscv64-k3-smp-uart-raw.fit` → bootm → 观察 placement/copier 行后按 MS17 plane 操作序逐阶段驱动至 errors 账本行；shell boot——fatload `StarryOS_riscv64-k3-smp-uart.fit` → bootm → 观察 placement/copier/shell/alarm 行后按 MS17 shell 操作序交互至 exit 与 drain 行；任何 fault 行停止；外部 RESET 回官方 Linux 并以一句话报告。
- Stop when: 专项首个目标失败、actual hart 与 affinity 不符、账本不闭合、外部 RESET 未回官方系统；blocked 交 Plan。

**Invariants**

1. 唯一 copiers/port：`CopierStartGuard` 与 `activate_port` 既有 fail-closed 语义保持，无第二次启动路径。
2. RX 消费者互斥：raw 子形态不构造 ASYNC_TTY/不挂内存根；shell 子形态 tty reader 为唯一 RX 消费者。
3. TimerFuture 不迁移：主任务（raw 载荷与 drain timeout）、alarm、shell 用户任务、copiers 全部在首次入队/首个 deadline 等待前提交 singleton；跨簇唤醒不依赖目标周期 timer。
4. 旧单 hart UART 家族（k3-uart-port/k3-uart/k3-stdio/k3-shell）行为、SMP 拒绝与 `make k3`/`k3-smp`/`k3-smp-net` 构建保持。
5. 完成声明基于真实板测行为；configured mask/FDT/构建输出不替代实际执行。
6. 共享 UART 锁批次、ISR 锁内分类/锁外 wake、`k3_uart_core` 寄存器序保持（core harness 采信面）。

**Non-goals**

网络恢复/reset/link-flap 矩阵（ISS02 判别）、30 min 组合长稳、受控迁移、性能矩阵、第二 UART、shell 网络工具、host 之外的判定工具。

**Acceptance**

| ID | 覆盖 | 直接结果 |
|---|---|---|
| A9 | R3/S08–S09/T7,T8 | copier pinned/actual hart 与 place_roles 结果一致且互异；shell 用户任务实际执行 hart=8（first-write 观察行）；alarm singleton 提交（启动行）；重复启动/非法 affinity 拒绝且无第二角色（CopierStartGuard host GREEN + spawn 拒绝 fail-closed） |
| A10 | R4/S10–S11/T8 | raw boot 双向内容/次序正确（echo×2、long ≥256 B wrap）；短 RX 末事件可达（quiet 10 s 后 post-quiet echo；CTI 末字节路径）；readiness 与实际 read 一致（check-register-recheck） |
| A11 | R4/S12–S13/T8 | txfull 短写只重试未接受字节→容量恢复→四条件物理 drain（末字节 TEMT 经 1 ms native timer hook）；errors 账本 drop 分类有界；两方向与 timer 推进（quiet sleep 实际到期） |
| A12 | R4/S11/S14、R8/S26–S27/T7,T8 | shell boot 行编辑/EOF 0-read/poll readiness 与实际 read 一致；sleep 超时生效（hart 8 timer）；Ctrl-C 对照与异常输入恢复；退出后 30 s 上限 drain；旧构建不回归；外部 RESET 回官方 Linux |

**Verification**

| Requirement | Task | First Test | Status |
|---|---|---|---|
| R4 | T7,T8 | 新接缝 RED；plane harness GREEN；两 boot 板测判定行 | Covered |
| R3（UART 面） | T7,T8 | placement 纯函数既有 GREEN；新 seam RED→GREEN；板测 copier/shell actual hart 观察 | Covered |
| R8 | T7,T8 | `make k3`/`k3-uart`/`k3-stdio`/`k3-shell`/`k3-smp-net` 复核；两 FIT 实际运行与外部 RESET | Covered |

（R3 的 S06 timer 恢复面由 000 A1–A4 与本轮 hart 8 timer/sleep 实际到期共同承载；R5 归 001，R6 归 003，R7 归 004，不在本批次。）

**Gate 2 Readiness**

技术检查：本轮调查、契约、先行测试、真实入口、停止条件与 RTM 已闭合；无 TBD。Gate 2 PASS：用户 2026-10-15 批准（原话"批准"），Status 改为 `ready` 交 Act；真板操作由用户手动执行。

**Persisted Evidence**

- Mode: none。命令、退出码、两 boot 判定行与操作者一句话报告写 Act Response 足够。

**Risks and Notes**

- anchor=boot hart 0 为板上实测（000 会话）；契约以 `place_roles` 实际返回为准，不硬编码 0/1。若 boot hart 使 shell=8 与 copier 重叠，seam fail-closed park，不虚构拓扑。
- shell=8 与 alarm=anchor（boot hart）为本计划裁决：shell 为 D5 命名的 A100 应用 hart；alarm 为内核原生 deadline 任务，与固定主任务同 boot hart，不引入第三个硬编码 hart。
- shell 实际 hart 观察机制（一次性 first-write 记录）为新增接缝，Act 可选包装 stdio 输出文件或等价内核侧一次性记录点；不得改共享写路径语义。
- raw 子形态镜像包含 stdio 树编译但未构造（沿 k3-stdio→k3-uart 既有先例）；shell 形态 SMP<9 时入口 fail-closed（ShellHartMissing），不静默改测。
- UART route 42 固定 boot hart 注册（000 D4 底座）；ISR hart 不做板上重复见证，copier/shell/alarm/主任务的实际 hart 为本轮观察对象。
- ISS02/ISS03 不在本轮（003 展开前由 Plan 补读补查）；MS17 历史 UART 未验收项（受控迁移等）不运行代替。

## Act Response

- Status: reported
- Date: 2026-10-16

### 实际改动

新增（既有单 hart 行为零删改，Preserve 达成）：

- `kernel/src/platform/k3_smp_uart.rs`（新）：纯接缝 `place_smp_uart_roles`/`SmpUartRoles`（uart_rx/uart_tx 取 `place_roles` 实际返回、`alarm_hart=anchor`、`CopierCollide`/`InvalidSchedulable` fail-closed）与 `shell_hart_for`（`SHELL_HART=8`，`ShellHartMissing`/`ShellHartCollide` fail-closed，raw 形态不调用）；共享 policy 以 `pub use crate::drivers::placement`（内核，drivers 树在本闭包存在）+ `#[path]` include（host harness，无 feature）双路单实例。kernel-only 侧：`boot_support`（`singleton`/`published_schedulable`/`fault`/`wait_copier_harts`，与 `start_copiers` 同一升序 schedulable 构造）；`shell_boot::run_smp_uart_shell_boot`（placement 行→init_uart_hardware→主任务 `set_current_affinity(boot singleton)`→`start_copiers`→有界 copier actual 观察→内存根/挂载→`spawn_alarm_task_with_affinity`→内嵌 k3-shell.elf 组装（与 MS17 stdio 分支逐段同型）→`spawn_task_with_affinity(shell singleton)`→first-write 有界观察→join→drain timeout(30 s)→park）；`raw_boot::run_smp_uart_raw_boot`（同序至 copier 观察行→逐字调用 `run_uart_data_plane`）。shell 实际 hart 观察接缝：`ShellFirstWriteFile`（新形态 stdout/stderr delegate，首次 write 记录执行 hart，全部方法逐字委托）经 `ShellStdioTable`（`Arc<dyn FileLike>` 型表的发布适配器，复用 `publish_stdio` 的顺序/空表断言/失败映射，共享 `StdioTable` impl 零改动）发布。
- `kernel/src/entry.rs`：两个新分支（shell=带 `not(k3-smp-uart-raw)`、raw）；stdio 分支与 000 k3-smp 平台见证分支补 `not(feature = "k3-smp-uart")`。
- `kernel/src/lib.rs`：`k3-async-uart`+`smp` 互斥守卫开 `not(feature = "k3-smp-uart")` 唯一例外（沿 001 k3-smp-net 先例）。
- `kernel/src/platform/mod.rs`：`k3_smp_uart` 模块门（`target_arch=riscv64` + `k3-smp-uart`）。
- `kernel/src/task/timer.rs`：`spawn_alarm_task_with_affinity(AxCpuMask) -> bool` 纯增变体（`spawn_raw_with_affinity`，入队前提交 affinity）；旧 `spawn_alarm_task` 逐字保留。
- `Cargo.toml`（root）：`k3-smp-uart = [k3-smp, k3-shell, starry-kernel/k3-smp-uart]`、`k3-smp-uart-raw = [k3-smp-uart, starry-kernel/k3-smp-uart-raw]`（契约原文闭包；runtime-irq/uart-irq 经既有 `k3-async-uart` 链传递）。
- `kernel/Cargo.toml`：`k3-smp-uart = [k3-smp, k3-shell]`、`k3-smp-uart-raw = [k3-smp-uart]`。
- `Makefile`：`k3-smp-uart`（依赖 `k3-shell-elf`）与 `k3-smp-uart-raw` 两目标 + 两 fit 目标；隔离 `target/k3-smp-uart[-raw]`/`OUT_CONFIG`/`out-k3-smp-uart[-raw]`，SMP 2..=16 默认 16、1/>16 拒绝，同 k3-smp 的 MEM/PLAT_CONFIG 窗口；`.PHONY` 补齐。
- `tools/starry-k3-smp-uart.its`、`tools/starry-k3-smp-uart-raw.its`（新）：与 k3-smp.its 同 load/entry 0x180000000/FDT 0x181000000，载荷指向各自隔离 out 目录，无身份字段。
- `.gitignore`：`/.axconfig.*` 修正为 `/.axconfig*`（覆盖 `.axconfig-k3-*.toml` 变体，闭合 001 Review Finding 2 面向后续一半）；补 `out-k3-smp-uart/`、`out-k3-smp-uart-raw/` 独立行。
- `tests/k3-uart-plane-host-harness.rs`：新增 5 个 k3_smp_uart 接缝用例（16 hart placement 值来自 place_roles、anchor 内/外、shell hart 9 缺失边界、shell 与 copier 重叠拒绝、非法集合 fail-closed）。

### 与计划的偏差

1. `kernel/src/platform/k3.rs`：`run_uart_data_plane` 与 `PLANE_LINE_LIMIT`/`PLANE_TX_BURST_BYTES`/`plane_line_complete` 的 cfg 从 `not(k3-stdio)` 扩为 `any(not(k3-stdio), k3-smp-uart-raw)`——raw 子形态闭包含 `k3-stdio`，原 cfg 会把载荷整体排除出 raw 镜像，与契约 "raw boot 调 run_uart_data_plane（逐字复用）" 冲突；函数体逐字未动，所有旧形态 cfg 结果不变（k3-uart/stdio/shell/rootfs 单 hart 家族构建回归证实）。
2. `kernel/src/drivers/uart_smp_snapshot.rs`：新增只读 `copier_last_poll(Copier) -> Option<usize>`——Plan 采信的 `snapshot()` 为 `#[cfg(feature = "qemu")]`，K3 boot 的 copier pinned/actual 观察行需要非 QEMU 读取通道；纯读取既有 `RX_COUNTER/TX_COUNTER`，未动任何记录语义。
3. shell 形态有界等待上限取 `START_DEADLINE=5 s`（契约要求"有界等待"未定值；沿 001 `SERVICE_START_NANOS` 量级）；first-write 观察与 copier 观察共用该上限。

### 自检发现、已修复与遗留 Minor

- 已修复（GREEN 循环内）：`pub(super)` 引导函数被 `pub use` 拒绝（E0364/E0603，改 `pub`）；`table_scope` 的 `unused_mut` 警告；async 块内 `block_on(sleep(...))` 嵌套执行器笔误（编译前即改为 `.await`，未进入构建）。
- 遗留 Minor 1：raw 载荷的 MS17 原文行 `[ms17] mode=k3-uart-dataplane begin … smp=1` 的 `smp=1` 为逐字复用的旧标签，本轮实际 harts=16（`[ms14]` begin 行承载真实拓扑）；契约禁止改载荷生产逻辑，留 Plan Review 裁决。
- 遗留 Minor 2：shell banner 与 first-write 观察行一次交叉输出（`she[ms14]…ll, 'help'…`）——shell 任务（hart 8，经 ASYNC_TTY）与 boot 任务（hart 0，diag）两个写者经 SerializedWriter 交替，内容完整、顺序可解释；本轮新增观察行引入的显示交错，与 001 Minor 1 同类。

### 验证（agent 侧，真实运行）

- RED：`rustc --edition=2024 --test tests/k3-uart-plane-host-harness.rs` → 编译失败（`k3_smp_uart.rs` 不存在，exit 1）；`make k3-smp-uart SMP=16` → `No rule to make target`（exit 2）。
- `rustc --edition=2024 --test tests/k3-uart-plane-host-harness.rs`：**12/12 通过**（基线 7/7 + 新增 5/5）。
- `make k3-smp-uart SMP=16` exit 0（产出 `StarryOS_riscv64-k3-smp-uart.fit`，1.1 MB 含内嵌 shell ELF）；`make k3-smp-uart-raw SMP=16` exit 0（`StarryOS_riscv64-k3-smp-uart-raw.fit`，452 KB，shell boot 经 cfg 排除证实不依赖 shell.elf）。
- SMP 边界：两目标 `SMP=1`/`SMP=17` 均 ERROR 拒绝。
- 旧目标回归：`make k3`、`make k3-uart`、`make k3-stdio`、`make k3-shell`、`make k3-smp-net SMP=16` 全部 exit 0；共享面（timer.rs 纯增）：`make build`（QEMU）、`make lichee`（D1）各 exit 0。
- `.gitignore`：`git check-ignore -v` 证实 `.axconfig-k3-smp-uart.toml`/`.axconfig-k3-smp-uart-raw.toml`/`out-k3-smp-uart/`/`out-k3-smp-uart-raw/` 全部命中。
- 未触碰面采信既有结论：uart_16550 crate（74/74）、k3_uart_core harness（~26）、tty（11/11+6）、embedded-user-host（9/9）本轮输入未变化。

### 真板结果（操作者执行，2026-10-16，两轮主序）

判定行均来自串口原文（两份新 FIT，boot_hart=0、harts=16）：

- A9（R3/S08–S09/T7,T8）：raw 与 shell 两 boot 均 `placement rx=0 tx=1 alarm=0 harts=16 anchor=0`（shell 形态另 `shell=8`）且 `copiers pinned rx=0 tx=1 actual rx=0 tx=1`——pinned/actual 一致且互异；shell 形态 `alarm spawned hart=0`、`shell spawned hart=8`、`shell first-write hart=8 expected=8`（first-write 一次性记录证实 shell 用户任务实际执行 hart=8，跨簇 8↔0/1 成立）；raw 形态 `duplicate-activate result=rejected`。
- A10（R4/S10–S11/T8）：echo 两行完整回显（len=5/3）；`long received_len=512 echo=done`（≥256 B wrap）；quiet `begin=10s` 后操作者零输入，`quiet done` 后一行立即回显（`post-quiet echo=done len=23`，含窗内排队字节的旧数据先读行为，与 MS17 V3 len=18 同型）——短 RX 末事件/readiness 与实际 read 一致。
- A11（R4/S12–S13/T8）：`txfull drained requested=73728 accepted=73728 short_writes=89`（只重试未接受字节，终端零刷屏）→ `drain done (TxCompletion four conditions)`；`errors rx_dropped_bytes=0 … unknown=0 oe=0 pe=0 fe=0 bi=0 fifoerr=0 closed=false`（drop 分类有界全零）；quiet 10 s 计时实际到期（timer 推进直证）。
- A12（R4/S11/S14、R8/S26–S27/T7,T8，部分 + 豁免）：shell boot `help`/`echo hello smp uart`/`sleep 10` 完成/行编辑（`echo abx` + DEL 修字 → 回显 `abxc`）/非法输入恢复（`sleep a` → usage、512 B 行 → `line too long, discarded` 后继续）→ `exit` → `[shell] exit code=0` + `[ms14] uart stage=exit join=0` → `stage=drain done`（30 s 上限内）。旧构建不回归由 agent 侧 7 个构建 exit 0 承载。
- 观察期间两 boot 之间经外部 RESET 与重新 fatload 衔接成功。

### 用户裁决与豁免（原话保留）

- "我看了，正常工作，不阻塞在这里，填写回复吧直接"（2026-10-16）。
- 豁免项：①Ctrl-C（SIGINT）对照 boot；②EOF 0-read（Ctrl-D）boot；③外部 RESET 回官方 Linux 的逐字确认。风险与缓解：EOF/SIGINT 结束语义的生产面（k3-shell.c、tty canonical、signal 路径）本轮零改动，其板上行为由 MS17 已验收轮次承载（stdio probe EOF read-0、shell B7 含 SIGINT 对照轮、shell native 20 项含 EOF exit 0）；RESET 链路本轮两 boot 间实际工作，且 000（A1–A4）与 001 同链路已确认回官方系统。T8 契约 GREEN condition "无未执行/豁免项"按用户显式豁免收口。

### Persisted Evidence

None required（Mode none）：全部判定行、操作者串口原文与裁决原话已完整写入本 Response。

### Experience Candidates

None：板测装载复用既有 Runbook（仅文件名不同）；两条 Minor 观察项不构成缺陷台账准入。

### 未解决问题

无阻塞项。本 Cycle 等 Plan Review；003（恢复与跨 hart 等待闭合）依赖本轮 accepted 后由 Plan 展开（展开前按 Iteration Plan 补读 ISS02/ISS03）。

## Plan Review

- Review Result: accepted

Reviewed 2026-10-16。独立阅读 002 全部变更面：新文件 `k3_smp_uart.rs`（586 行全文）、`tools/starry-k3-smp-uart.its`/`starry-k3-smp-uart-raw.its`、`tests/k3-uart-plane-host-harness.rs` 新增 5 用例，与 `entry.rs`/`lib.rs`/`platform/mod.rs`/`task/timer.rs`/root+kernel `Cargo.toml`/`Makefile`/`.gitignore`/`uart_smp_snapshot.rs`/`k3.rs` 的 diff；判定行与代码 format string 逐一对应；host 12/12、七个构建 exit 0、SMP=1/17 拒绝与 check-ignore 按 公共规则 › 验证 采信（只读 git 检查确认工作区即 Act 结论产生后的同一代码面；采信的既有 native 面——uart_16550 74/74、core harness、tty 11/11+6、embedded-user-host 9/9——本轮输入未变化，Act 的采信声明成立）；真板结论按操作者串口原文与"我看了，正常工作，不阻塞在这里，填写回复吧直接"裁决核对。未重复运行已采信验证。

**声明与代码一致性（独立检查通过）**

- T7 接缝：`place_smp_uart_roles` 的 RX/TX 全部来自 `place_roles` 实际返回（无硬编码 0/1），`alarm_hart=anchor`、`CopierCollide`/`InvalidSchedulable` fail-closed；`shell_hart_for` 固定 `SHELL_HART=8`，`ShellHartMissing`/`ShellHartCollide` fail-closed，raw 形态不调用（`raw_boot` 无此调用，代码证实）。双路单实例声明成立：kernel 侧 `pub use crate::drivers::placement`、host 侧 `#[path]` 原位 include，`placement.rs` 本身零改动（不在 diff 中）。
- T7 接线：root `k3-smp-uart=[k3-smp, k3-shell, starry-kernel/k3-smp-uart]`、`k3-smp-uart-raw=[k3-smp-uart, starry-kernel/k3-smp-uart-raw]` 与 kernel `k3-smp-uart=[k3-smp, k3-shell]`、`k3-smp-uart-raw=[k3-smp-uart]` 逐字符合契约闭包；entry.rs 新两分支先于 stdio/k3-smp 见证分支，stdio 与 k3-smp 见证分支补 `not(k3-smp-uart)`、单 hart raw 分支经既有 `not(any(k3-stdio,…))` 自然排除；lib.rs `k3-async-uart`+`smp` 守卫仅开 `not(k3-smp-uart)` 例外；Makefile 两目标依赖 `k3-shell-elf`、SMP 2..=16 默认 16（1/>16 ERROR）、TARGET_DIR/OUT_CONFIG/OUT_DIR 全隔离、MEM=6400M 与 k3-smp 同窗口、`.PHONY` 补齐六目标；两 ITS 与 k3-smp.its 同 load/entry 0x180000000、FDT 0x181000000，载荷指向各自隔离 out 目录，无身份字段（crc32 为 mkimage 既有标准形态）。
- T7 timer：`spawn_alarm_task_with_affinity` 为纯增变体（`spawn_raw_with_affinity` + 返回 bool），旧 `spawn_alarm_task` 逐字保留（diff 仅新增）；`make build`/`make lichee` 共享面复核由 Act 承载。
- 偏差 1（k3.rs cfg）核实：全文件 diff 共 55 行，仅为四个符号的 cfg 从 `not(k3-stdio)` 扩为 `any(not(k3-stdio), k3-smp-uart-raw)` 与注释补充，`run_uart_data_plane` 函数体逐字未动；raw 闭包含 `k3-stdio`，不扩 cfg 则载荷被排除，偏差必要且最小。
- 偏差 2（`copier_last_poll`）核实：纯读取既有 `RX/TX_COUNTER` 的只读函数，记录语义零改动；`snapshot()` 的 `#[cfg(feature="qemu")]` 限定使 K3 boot 确需非 QEMU 读取通道，偏差成立。
- 等价声明核实：`published_schedulable` 与 `start_copiers`（`uart_init.rs:545-553`）同构造——同一 `schedulable_cpu_mask()`、同一升序循环，上界 `placement::MAX_CPU_NUM` 即 `axconfig::plat::MAX_CPU_NUM`（`placement.rs:25`），boot 与 copier 消费同一输入；`ShellStdioTable::add` 与共享 impl（`file/mod.rs:227-245`）逐段同型（`count()==0` 空判、inherent `FlattenObjects::add`、`TooManyOpenFiles` 映射），共享 `StdioTable` impl 零改动；`ShellFirstWriteFile` 全方法逐字委托、仅首 write 记录执行 hart，stdout/stderr 共享同一 witness（Arc 克隆），stdin 不包装。
- Invariants 1–6 在代码中成立：`CopierStartGuard`/`activate_port` 未动（`uart_init.rs`/`k3_uart.rs` 不在 diff 中）；raw 子形态不构造 `ASYNC_TTY`、不挂内存根（`raw_boot` 直接进 `run_uart_data_plane`），shell 子形态 tty 为唯一 RX 消费者；两 boot 的主任务 `set_current_affinity(boot singleton)` 均先于 `start_copiers` 与首个 TimerFuture（`wait_copier_harts` 的 `block_on(timeout)` 在 pin 之后），alarm/shell affinity 经 `spawn_*_with_affinity` 入队前提交；旧家族构建回归由 Act 七个 exit 0 承载。
- 板测判定行与代码输出一一对应（`[ms14] uart` placement/copiers/alarm spawned/shell spawned/shell first-write/stage=exit/stage=drain 行、`[ms17]` 载荷行、`duplicate-activate result=rejected`）；A9–A12 与 Act Response 真板结果节的串口原文一致；三项豁免（Ctrl-C 对照、EOF 0-read、RESET 逐字确认）按用户显式豁免收口，原话与风险缓解已留档，生产面零改动成立（豁免项相关代码均不在本轮 diff 中），MS17 已验收轮次承载合理。
- Evidence Mode none 被遵守（change 内无 evidence 目录）；无 TODO/FIXME 残留、无身份型证据机制、无孤儿代码（全部新符号有消费方）。

**Findings**

1. [Act 遗留 Minor 1 裁决 / 不阻塞] raw 载荷 `[ms17] … smp=1` 旧标签：契约禁改载荷生产逻辑、要求逐字复用，标签为 MS17 遗留显示文本，真实拓扑由 `[ms14] uart … harts=16` begin 行承载，板测操作者未误读。裁决：保持逐字复用，不改；留档即可。
2. [Act 遗留 Minor 2 确认 / 不阻塞] banner 与 first-write 观察行一次交错：成因在代码中确认——shell 任务（hart 8，经 ASYNC_TTY 异步写路径）与 boot 任务（hart 0，`diag_line`）两写者经 SerializedWriter 交替，内容完整、顺序可解释，与 001 Minor 1 同类显示交错。不阻塞。
3. [Review 观察 / Minor 测试覆盖备注] `SmpUartRoleError::CopierCollide` 无专属 UART host 用例（单 hart 输入 `rx==tx` 路径）；该变体不在契约 First test 清单内、分支三行且语义平凡，`place_roles` 层已有单 hart 用例。仅记录，不要求补测。
4. [提交集卫生 / 用户决策，非本轮偏差] 001 Finding 2 的忽略规则缺口已由本轮 `/.axconfig*` 修复闭合（check-ignore 证实两个新 OUT_CONFIG 命中）；暂存集仍含 `.axconfig-k3-smp.toml`（000 产物，已入 index 需显式 unstage 才会被新规则忽略）与 `tests/network_benchmark-host-asan`（000 遗留），另 `k3_smp_uart.rs`、两 ITS、001/002 Iteration 目录为未跟踪——均属用户提交前决策，与 000/001 处理一致。

**Acceptance Gaps**: None。A9–A12 全部满足且有真实板测记录、操作者裁决与三项显式豁免留档；上述 Minor findings 不阻塞任何 Acceptance，按公共规则不创建返工。

**Evidence**

- Diff 与新文件独立阅读（本 Review 上列检查项）；判定行格式核对到 `k3_smp_uart.rs` 的 format string；等价声明核对到 `uart_init.rs:545`、`placement.rs:25`、`file/mod.rs:227`、`stdio.rs:32` 源码。
- 采信来源：Act Response 验证节（plane harness 12/12、`make k3-smp-uart SMP=16`/`make k3-smp-uart-raw SMP=16` exit 0、SMP=1/17 拒绝、七旧目标 exit 0、check-ignore 命中），覆盖未变化；真板两轮判定行与操作者原话。

**Follow-up Decision**: 仅有非阻塞 Minor findings（旧标签裁决保持、显示交错确认、测试覆盖备注、提交集卫生），A9–A12 全部真实满足，无实质问题或 Acceptance gap → `accepted`，不要求当前 Cycle 修复。Iteration Plan 不变。

**Next Cycle**: None

**Next Iteration**: 003（恢复与跨 hart 等待闭合，T9–T11）保留在 Map 中、本轮不展开——用户 2026-10-16 指示（原话"进行审计，没问题就接收，但不用展开下一iter"）；展开时由 Plan 补读 ISS02/ISS03 并按 Iteration Plan 的展开前检查执行。
