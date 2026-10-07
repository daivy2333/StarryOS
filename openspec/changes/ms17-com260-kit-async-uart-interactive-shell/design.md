# MS17 实现调查与设计

MS17 复用现有多 hart 软件结构，在 K3 单 hart 上补齐平台与用户态接缝；不创建另一套 UART、scheduler 或 tty。需求、默认范围与执行方案已批准，首轮执行契约ready。

## 调查基线

现场为 `k3` 分支、MS11 收尾后的 `aca5dfe9`；产品文件未改，`.agents` 三份既有用户改动不属于本 Plan。MS11 accepted 的 K3 RAM 启动、AIA、DMA/PHY、网络及原构建结论直接采信。MS08 已验收 QEMU SMP 网络；其 UART 专项运行与迁移仍有历史未执行项，不能写成 UART SMP 已完成。

### 实际入口与影响面

| 路径/符号 | 当前行为 | 本次接缝 |
|---|---|---|
| root/kernel Cargo features、`kernel/src/lib.rs` | k3 排除 drivers/file/mm/pseudofs/syscall/task/time；不启用 paging/task-ext/axfs | UART 模式先开放驱动；shell 模式再开放用户态模块和必要依赖 |
| `entry.rs::init` → K3 `run_first_byte_park` | 运行 GMAC/PHY/AIA/network 探针后停驻 | 独立 UART/shell 入口，跳过网络探针和链路等待 |
| K3 `irq.rs::IrqIfImpl` / `aia.rs::route_for` | 只分派 external vector；仅 source 1/60/133/277 可注册；STIE/SSIE 关闭 | 独立 timer handler + opt-in UART42；不替换 claim/dispatch |
| K3 `time.rs::TimeIfImpl` | 24MHz 换算与 SBI set_timer 已有，init_percpu 空 | opt-in 触发第一次 timer IRQ，让 runtime 既有 tick 自续 |
| registry `axruntime::init_interrupt` | 注册 timer closure，在回调中重新 set_timer、调用 axtask::on_timer_tick；初始化未主动 update_timer | 只修平台接缝，registry 不改；不重复初始化 runtime |
| `uart_init.rs::init_uart_hardware/start_copiers` | QEMU U8 与 D1 U32 两个 port；两个64KiB ring；copier once + 已发布 schedulable mask 放置 | 增加 PXA port 分支，保留 affinity/handle；初始化失败不得继续启动 |
| `uart_16550::async_::driver` | RX 先 arm 后 register，缺少最终重查；连续 Ready 可在一次 poll 内无界搬运；TX 含 D1 slow-poll/TEMT spin | 关闭竞态与公平性缺口；K3 选择 IRQ 模式，保留 D1 默认策略 |
| `ring_buffer.rs::push_batch` | 只写一次连续段，返回实际写入量；RX copier 忽略返回值 | 有空位的 wrap 不丢尾部；真实满容量分类计数 |
| `device_ops.rs::flush`、TX completion | 已有 ring-empty/active/staged/TEMT 四条件；DRAIN_WAKER 是单槽 | 复用四条件，补最后完成通知与竞争等待者广播 |
| `ntty_async.rs::ASYNC_TTY`、SerializedWriter | 唯一 raw reader/writer，writer clones 共用锁；RX PollSet 唤醒 tty-reader | UART qualification reader 结束后才能切到 tty，不建立第二 reader |
| `tty/terminal/ldisc.rs::InputReader` | canonical 仅保留 ascii_graphic，空格丢失；Vec 行缓冲无上限；空 VEOF 不交付事件；tty-reader while 可连续处理 | 保留空格/制表符，限定行容量，EOF/error ready，有限处理批次 |
| `tty/mod.rs`、`file/fs.rs`、`syscall/fs/ctl.rs` | 阻塞/短写/OUT ready 已接 UART；sys_ioctl 已把 FIONBIO 用户指针转换为 bool 给设备；TCSETSW/F 有 drain TODO | 复用 nonblocking 约定，不误改为设备再次解引用指针；本次不承诺 tcdrain/termios drain 扩展 |
| `mm/loader.rs` → `AddrSpace` / `task/user.rs` | D1 已有 eager ELF 字节加载；RISC-V copy_from_kernel、UserContext.run、syscall、join 已有 | 将既有内嵌入口开放到 k3-shell，闭合 ELF 校验、映射和失败清理 |
| `pseudofs::init_memory_root/mount_all`、`file::add_stdio` | MemoryFs 根，挂 /dev/console，stdio 通过 FS_CONTEXT 打开，进程 scope 内分配 fd0/1/2 | 复用无存储路径，不启用 axruntime fs/net 自动 probe |

`drivers/mod.rs` 中 bench、ntty 与用户态类型须按模式隔离；net_placement 的基础 placement 可复用，其 axnet 调用已经 gated qemu，K3 不启用 axnet。`syscall/mod.rs` 与 file 的网络分支已有 k3 排除门，保持该边界。QEMU port 的 `not(lichee-d1-async-uart)` 条件必须同时排除 K3，避免落入 U8/iomap 路径。

### 已实际运行的当前验证（2026-10-06）

1. `env RUSTFLAGS='-C linker=/home/daivy/projects/serial/work/StarryOS/scripts/cc-nopie.sh' cargo test --manifest-path crates/uart_16550/Cargo.toml --locked --offline --features async,telemetry --lib -- --test-threads=1`：`69 passed; 0 failed`，exit0。包含真实 production copier 的 park/wake、TX Full→恢复、四条件 drain 测试；不覆盖 PXA 或新的 RX arm 窗口。
2. `rustc --edition=2024 --test kernel/tests/tty_write_logic.rs -o /tmp/ms17-tty-write-baseline`，随后 `/tmp/ms17-tty-write-baseline`：`6 passed; 0 failed`，两个命令 exit0。ONLCR/短写基线成立，不证明 canonical/EOF 正确。
3. `rustc --edition=2024 --test tests/k3-aia-host-harness.rs -o /tmp/ms17-k3-aia-baseline`，随后 `/tmp/ms17-k3-aia-baseline --test-threads=1`：`16 passed; 0 failed`，两个命令 exit0。保留 MS10 register/claim/unregister 与 IRQ-save 行为。
4. 原 `riscv64-linux-musl-gcc -static -no-pie -fno-pie -Os -o /tmp/ms17-uart-existing-probe.elf tests/ms08_uart_smp_probe.c` 在此沙箱 `Bad system call`，明确失败；未把其后 shell 的退出码作为编译成功。
5. 改用系统 GCC 与**已有 musl** crt/headers/libc，编译同一个现有 probe，exit0：

   ```sh
   riscv64-linux-gnu-gcc -static -no-pie -fno-pie -Os -nostdinc -isystem /usr/lib/gcc-cross/riscv64-linux-gnu/11/include -isystem /opt/musl/riscv64-linux-musl-cross/riscv64-linux-musl/include -nostdlib /opt/musl/riscv64-linux-musl-cross/riscv64-linux-musl/lib/crt1.o /opt/musl/riscv64-linux-musl-cross/riscv64-linux-musl/lib/crti.o tests/ms08_uart_smp_probe.c -L/opt/musl/riscv64-linux-musl-cross/riscv64-linux-musl/lib -Wl,--start-group -lc -lgcc -Wl,--end-group /opt/musl/riscv64-linux-musl-cross/riscv64-linux-musl/lib/crtn.o -o /tmp/ms17-uart-existing-probe.elf
   riscv64-linux-gnu-readelf -h -l -r /tmp/ms17-uart-existing-probe.elf
   ```

   readelf exit0：`EXEC`、`RISC-V`、`RVC, double-float ABI`、两个 LOAD、无 INTERP、`There are no relocations in this file.`。只证明工具链可生成目标静态 ELF，不证明 MS17 loader/程序已运行。保留这一原生命令为当前环境构建选项，不新建 libc、ABI 或编译包装器。

普通 axnet 已知 cfg 缺陷留在 MS12；历史全量 host-test/source guard 与 VF2 编译失败不在 MS17 修复范围。没有本次真板结果。

## D1：模式与复用边界

- `k3-async-uart` 蕴含 k3，kernel 同名模式开放必要 UART 模块；平台新增 `runtime-irq` 与 `uart-irq`，分别负责 timer 和 source42。MS12 可仅消费 runtime-irq，UART42 不强迫网络模式激活 UART。
- `k3-uart-port-test` 蕴含 k3-async-uart，但入口只运行原生 port 资格载荷，不启动 generic copiers/tty。它是低层板测镜像选择，不是新的驱动实现。
- `k3-shell` 蕴含 k3-async-uart，加 paging、task-ext、axfs 内存根；可有 `k3-stdio-test` 选择内嵌 stdio probe。shell 与两种资格载荷互斥；QEMU/D1/VF2 与 K3 模式互斥。新 K3 模式暂拒绝 smp，Make 明确 SMP=1。
- 新原生 targets：`make k3-uart-port`、`make k3-uart`、`make k3-stdio`、`make k3-shell`，复用 k3 的 ARCH/MYPLAT/PLAT_CONFIG/MEM/BUS/FIT；不改变原 `make k3`。
- 既有 copier placement 在单 hart 将两个角色落到同一个已发布 hart。MS14 开启平台 secondary/SSIE/IPI/路由与多 hart 构建后，沿用这些角色、rings、wakers 和 handles，再做硬件验收；MS17 不提前实施该平台工作。

## D2：timer 与 AIA 生命周期

完整 supervisor timer vector 为 `(1usize<<63)|5`；在平台用独立 Atomic handler 槽，register CAS 不覆盖旧 owner，成功才打开 STIE；unregister 先 mask，再移除；set_enable/dispatch 只操作该槽，dispatch 调用一次并返回 timer vector，不走 APLIC source 查找。external vector bit9 保持 MS10 原子 claim+EOI→handler。SSIE 保持关闭。

runtime-irq 模式的 time::init_percpu 使用已有 SBI set_timer(0) 安排第一次事件；timer handler 在 axruntime 注册后、全局 IRQ 开启时接收事件并自行安排后续 tick。旧模式仍拒绝 timer 注册且不开 STIE。原 tick/timer wheel/timeout/drop 全部复用；多个 TimerFuture 和取消由既有每 CPU timer runtime 管理，MS17 不让这些 future 迁移 hart。

source42/EID42 为 LevelHigh，只有 uart-irq 下进入 route_for；原 SUPPORTED_ROUTES 四项与默认拒绝行为保持。复用 AiaChip register/mask/unregister 与 boot hart 路由，不另建 dispatcher，不复制 chip 初始化。

本板裁决事实（Cycle 001 板测，2026-10-07）：① APLIC `SETIPNUM` 软件强制 pending 对输入已 deassert 的 LevelHigh 源不生效（pending 跟随输入电平；MS10 的 source1 为 DETACHED 语义，不可外推）；source42 的 APLIC→IMSIC→claim 交付链由真实设备 IRQ 证实。② PXA IIR 对 IER 已掩码的 RX cause 照报，ISR 每次调用以 8-cause 预算耗尽收尾——有界无风暴，诊断口径上 rx_wakes 需 ÷8 才等于中断数。

## D3：PXA adapter 与通知 ownership

事实来源为 R69 本地 tgoskits PXA `mod.rs/regs.rs/irq.rs/rx.rs`，按 M42 采信适用操作：UART0 PA0xd4017000、IRQ42、stride4/U32、64B FIFO；115200 8N1 无硬件 flow control，保留 bootloader clock/reset/pins/divisor。UUE=0x40、RDI=1、THRI=2、RLSI=4、RTOIE=0x10；IIR RLS6/RDI4/CTI12/THRI2；LSR DR1/OE2/PE4/FE8/BI16/THRE32/TEMT64。

生产 port 放 `kernel/src/drivers/k3_uart.rs`；可测试寄存器核心放同目录的 `k3_uart_core.rs`，fake 只替换 MMIO，不复制算法。所有寄存器用32位 volatile，取低8位；VA 走平台 phys_to_virt，不套 QEMU U8，也不要求原 early identity mapping 在 paging 后继续存在。

IER cache、LSR destructive error latch 和 MMIO 修改处于同一短 IRQ-save 临界区。平台提供 runtime 模式的共享 UART IRQ-save guard，adapter 与保留的 early/fault console 使用同一 guard；优先使用已有 kspin，避免手写锁。启动只在未接收业务时清 FIFO，保留 UUE，RX/TX 通知 masked；Once/状态检查在 ring init 之前完成。注册失败回滚本次 handler/mask，返回明确阶段错误，不能先启动任务再仅打印失败。激活后不重新清 FIFO、不接受第二 owner。

RX copier 拥有 RDI/RLSI/RTOIE 的重臂；ISR 可关闭该组并 wake。TX copier 拥有 THRI 的 arm，ISR 可关闭并 wake。UART 状态不是 UART route mask 的替代；启动/关闭由初始化 owner 管理 AIA42。

每次 ISR 读取有限 cause/status，处理 RX/TX 组合、NO_INT+DR 与线路错误，mask 对应通知并唤醒已有 AtomicWaker；最多8次 cause 分类，不读 RBR、不写 THR、不打印、不分配、不等待任务持锁。task 持同一 guard 时 IRQ 关闭，ISR 不会打断持锁 task。未知 cause 留分类计数并 mask UART 通知，资格载荷在原生 deadline 内报告故障，不自旋重启。

各处 LSR 读（包括 TX/TEMT 查询）统一保存 PE/FE/BI 到当前 FIFO head 的 latch，在 RX 消费该字节时分类一次并丢弃错误字节；OE 单独计数，不据此丢正常字节，FIFO-error 汇总不误当当前字节损坏。有效后续字节继续交付。

reference IRQ 直接 drain RBR，本项目则把该动作放 copier。PXA Errata20 的 RTOIE 关闭→读 RBR→按 RX 请求恢复 sequence 也放 copier；arm 后以 LSR DR/error 最终重查兜住 pending 窗口。CTI/NO_INT+DR 分支有 native fake witness；不添加周期 RX 轮询。

RX FIFO 触发线（Cycle 001 裁决，2026-10-07，取代 D3 原"选择 FIFO 1B 阈值"）：激活采用参考 `startup()` 三步 FCR 序列（EN → EN|clrRX|clrTX → 0）+ 读 LSR 清 sticky + 有界排空陈旧 RBR，运行态 FCR=EN|PXAR8（0x41）。依据：参考 `regs.rs::select_fcr_flags_for` 在 115200 波特分档落 PXAR8，PXAR1（触发 00）仅 <2400 波特使用；Cycle 000 板测 UART42 RX 在触发 00 下无任何中断推进（两次复现）。激活清 FIFO 前先有界等待 TX TEMT（防清掉未发完的诊断输出，`[m` 截断根因）。触发线为本模式固定 115200 的常量推导，运行时不读 divisor。

## D4：有限 copier、wrap 与物理 drain

复用64KiB RX/TX rings、唯一 copier 和既有 active/staged 原子。RX check→register→arm→recheck，TX register→arm→final send/check。任何工作窗口均以实际字节/状态闭合，不依赖下一输入事件。

K3 通过 UartPort 的默认兼容扩展选择 IRQ 策略：每个 poll 最多256B 搬运，每次硬件调用最多64B；预算耗尽且确有 backlog 时 self-wake 后 Pending，让出调度。无进展时等待 IRQ，既无 D1 4096次 slow-poll，也无 idle 自唤醒。旧 port 默认策略保留。RX arm/recheck、wrap 和计数等公共正确性修复对所有 port 生效并回归。

RX push_batch 最多跨两个连续段提交可用容量，返回真实提交量；达到真正容量才丢余量并累计 rx_ring_dropped。hardware_overrun、parity/framing/break 与 ring drop 分开，数值只诊断，不作身份/归属验收。TTY echo 保持 best-effort，满时可短写；正文通过既有 blocking/OUT ready 恢复。

flush 保留 `ring_empty && !copier_active && staged_bytes==0 && TEMT`。THRE 仅可写，PXA 不提供独立 TEMT IRQ：在 TX copier 的软数据已处理、TEMT 仍false时，port 的 poll-completion hook 复用一个 axtask::future::sleep(1ms) future，使用 copier waker。future 存在 port 的任务侧状态中，由固定 TX copier 首次 poll/取消；ISR 不触碰 future。每次到期仅读一次 TEMT，仍忙再等待；真正空闲取消 future，不常驻检查 UART。tick 分辨率可以使1ms等待稍后恢复，不承诺1ms延迟性能。

最后 TEMT 为true时 TX copier 广播已有 tx.poll，并保留 DRAIN_WAKER 的旧兼容通知；flush 在 tx.poll 上登记和最终重查，避免单槽 DRAIN_WAKER 覆盖竞争者。注册容量沿已有 PollSet（64 slots，满时主动 wake 被替换者）契约，不扩建 waiter 框架。drop 的陈旧 waker 允许一次 spurious wake，不能影响活跃等待；外层 native timeout 只结束该 flush future，不清 accepted 数据或重置 owner。空 flush 立即成功。TX hook 默认实现维持非 K3 行为。

初始化、资格载荷和 shell 运行期的普通输出经同一个 SerializedWriter 入 TX ring；涉及 IRQ 回调时 writer 临界区使用 IRQ-save，避免同 hart 重入锁。平台 polling console 只留给早期和故障输出，共享硬件 guard；不得把普通资格输出/日志导回 polling。新模式用 LOG=off，显式阶段/错误通过异步输出；原模式 LOG 不变。panic 的 ax_println 保留平台同步故障路径，硬件访问有 owner guard；不新增 console 日志镜像或第二 ring producer。故障接管不宣称正常 drain 成功。

## D5：tty 与用户程序

tty-reader、canonical queue（256B）、poll_io、ONLCR、stdout SerializedWriter 全部沿现有结构。修复 canonical 空格/制表符与空 VEOF；规范行最大1024B（包含终止符），固定上限而非无限 Vec 增长。超过上限整行丢弃到换行/VEOF，交付一次 InvalidInput，shell 显示 line too long，不能执行截断前缀。已排队旧数据先读完，再交付 overflow/EOF event；event 等待消费期间暂停处理后续行，消费时 wake tty-reader。EOF 空行 read 返回0一次，poll IN 对 event 成立；有内容的 VEOF 交付内容，不伪造空 EOF。echo 仍 best-effort。tty-reader 每 poll 最多256输入字节，预算耗尽仅在确有工作时续醒，input/error/event 完成均走 register/recheck。把本次受改的 canonical 状态/事件处理提取为实际被 ldisc 调用的 `terminal/canonical.rs`，以 `kernel/tests/tty_input_logic.rs` 原生 rustc 测试；不复制另一套 line discipline。

初版 shell 以256B命令缓冲消费完整 canonical 行；超出其容量时读到行终止再报告错误，不执行任何前缀。EOF 在一条非空行后终止的语义由 canonical read 结果处理；不把 short read 当完整行边界。Ctrl-C 沿 foreground job-control/信号路径终止用户进程，报告实际退出结果，UART owner 保持。read/write Future 取消与退出不撤销驱动任务。

内嵌静态 C ELF 复用 `AlignedBytes`、eager 字节 loader、copy_from_kernel、UserContext、ProcessData/scope stdio、AxTaskExt、spawn_task/add_task_to_table/join；只给 k3-shell 开 paging/axfs/task-ext。memory root/mount_all 不启用块设备。启动顺序 UART→copiers→唯一 tty→MemoryFs/devices→完整 ELF/aspace/预先打开的stdio表→Process初始化/绑定tty→enqueue 用户任务；失败时报告明确 loader/stdio/spawn 层级，未发布对象随 owner drop 清理。已查 starry-process0.2：new_init 写一次全局INIT_PROC，不能假设 drop 能撤销；所有可恢复的ELF/mapping/open失败必须发生在new_init前，FD表成功后移入scope。一次镜像只初始化一个init；bind失败为启动不变量错误，终止到外部reset，不能重试new_init。实际 Process::exit 对init直接return，kernel do_exit 设置thread结束与exit_event，随后join复用；不根据该库过时的panic注释新增退出修复。

K3 内嵌入口要求 ELF64 little-endian RISC-V ET_EXEC、无 interpreter/动态重定位；在切片前检查 offset/filesz/memsz、加法溢出、segment/entry 可执行映射、用户范围及栈冲突。复用 parser/AddrSpace 已有错误能力；已有缺失才加最小检查，拒绝不能通过 min(len) 静默截短。将现有 eager 字节函数的实际校验/映射顺序提取到 `mm/embedded_loader.rs`，生产 AddrSpace 是真实backend，native mapped-memory替身只提供map/populate/write并可注入错误；不另建parser/loader框架。`file/stdio.rs` 同样承接实际stdio组装逻辑，fake只替换open结果。用独立原生 Cargo test fixture `tests/k3-embedded-user-host` include这些生产模块，依赖复用现有parser/axerrno/flatten_objects；直接断言文件边界、映射/BSS/栈内容和失败资源drop，不测试复制算法。用户段/栈 eager、BSS 零填、kernel/MMIO 映射继承；拷贝可执行数据后调用原生 RISC-V icache 同步，避免重新设计页表。共享 loader 校验变化需回归 D1 入口。

K3的严格格式限制只作用本次内嵌入口，保持其他调用者既有格式范围；公共的越界/截短检查则回归实际共享路径。stdio probe 仅是 `k3-stdio-test` 选中的内嵌用户 ELF，用真实 read/write/FIONBIO/ppoll/nanosleep 证明入口/argv、canonical ready、EAGAIN、非阻塞切换、EOF、exit；不提供产品诊断 CLI。不要移植 MS08 UART SMP probe 的迁移 ioctl 或未支持 tcdrain。R4 的物理 drain 在驱动测试和 kernel 启动/结束等待中验收，tcdrain/TCSETSW drain 扩展不是本次新增接口。

## D6：最小 shell 与终止

`kernel/resources/k3-shell.c` 生成静态 `k3-shell.elf`。只实现 help、echo <text>、sleep <毫秒>、exit：空行返回提示符，echo 保留文本内部空格；sleep 接受0..60000毫秒十进制，拒绝负数/溢出/额外参数，使用 nanosleep、EINTR 按既有 syscall 结果处理；未知命令有错误后继续。read/write 处理 EINTR、短写，0read 正常退出，持续错误报告并失败退出，不无限重试。无 fork/exec、管道、重定向、历史/补全或外部程序依赖。

shell exit0 后 kernel join 报告真实进程结果，经异步 writer 接受输出并以 native timeout(1s) 等待物理 drain；超时明确报告未完成，不清 UART。随后主任务停车，copiers/IRQ owner 保留至外部 reset，不实现未知热重启协议。

## 验证矩阵与人工边界

| 场景 | 直接 witness / 最短板测 |
|---|---|
| S1–S3 | native timer handler/生命周期 fake + 板上多个 sleep/取消后其余按期恢复；不得卸载 runtime 的活动 timer 来做真实 duplicate 测试 |
| S4–S5 | PXA 真实 core fake IIR/LSR/U32 + board 两次输入、孤立一字节停止继续输入仍返回 |
| S6–S8 | production adapter/copier native 注入四窗口、组合/no-int/error/unknown/重复/注册失败；board source42 两次真实 claim/wake/byte |
| S9–S12 | production ring/copier native wrap、真 Full、drop 后恢复、poll budget；board finite双向流同时 timer 恢复；完整64KiB边界由 native 精确字节比较，不要求人工辨认64KiB日志 |
| S13–S16 | production flush native 空/staged/TEMT=false→timer完成、最后 waiter、多个waiter/drop/timeout；board 发送尾部后 physical drain 返回 |
| S17–S20 | production ldisc native空格/编辑/分段读取/容量/EOF/readiness；stdio probe 真板 EAGAIN、ppoll、行/EOF、短写恢复与 Ctrl-C |
| S21–S22 | 生产 loader mapped-memory测试/拒绝测试，readelf原生目标检查，stdio ELF真板真实 syscall/argv/exit；故障fixture未入队且资源释放 |
| S23–S26 | shell production C command函数 native测试 + board help、echo、sleep1000、非法输入、长行、quiet10s后新输入、exit/EOF各一次 |
| S27–S28 | 每镜像沿R68 RAM/FIT操作并外部reset回原系统；受影响平台build/已有production tests；MS12原Cycle冻结保持 |

新 tests 在项目原生 Rust/C test framework 内调用生产模块/函数，fake 只替换设备/时间/地址空间；不以源码字符串断言替代行为，不建外部判定器。命令与每项≤20行决定性输出、exit/人工一句结果留 Act Response，Persisted Evidence none。真板由操作者执行，agent 不能把 host GREEN 当真板通过；等待操作者与 Review accepted 后再展开下一轮。

## 风险与交接

未知运行结果是必须执行的验收，不是把设计选择留给 Act：PXA reference 的 task-context errata20 sequence若不能推进最后字节，立即交 Plan，不添加 RX 轮询；paging/用户 trap 若出现 fault 按 loader/MMIO/IRQ 分层停止。MS17 单 hart、timer tick 与资格输出不提供多 hart/性能结论。

MS17 accepted 后，MS12 Plan 采信真正通过的 timer/IRQ/UART底座；timer 验收已完成可在新执行契约引用 accepted 结论，GMAC adapter、ordinary axnet cfg、网络 owner/runner/双向数据面仍归 MS12。原 MS12 ready Cycle 保留，不能原地改写；必要时同一 Iteration 创建 replan。后续 MS13 处理网络恢复，MS14 最后验证 NIC/UART多 hart及组合；性能方向仅 I19。

## 调查停止结论

设计选择已闭合，未要求需求简化。必要真实板测尚未执行；不承诺已具备目标行为。native工具链替代已生成同类静态 ELF；可复用能力、影响模块、验证入口已定位。范围外 Issue 候选：None 新增；已知 ordinary axnet/VF2/全量host失败保留原归属。本 Plan 不落账、归档或同步全局文档。
