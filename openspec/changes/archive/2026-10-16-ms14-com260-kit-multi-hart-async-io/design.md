## Context

目标和批准范围见 `proposal.md`。2026-10-16 先调整未执行的 003/004 为必要研发与最终 BusyBox 正常使用；003 现已 accepted。用户随后要求最后 004 补充 UART 线速占比与原厂 Linux 网卡对比，选择只测量、不设最低性能门槛，沿用 Windows 原生对端。000–003 已交付 Cycle 保持不可变。下列 2026-10-14、HEAD `9179205d` 的调查属于历史现场；后续当前依据为 003 accepted 与新增 D9/004 Cycle。

### Current-State Evidence

| 表面 | 实际实现与影响 |
|---|---|
| `crates/axplat-riscv64-k3/src/boot.rs::_start` | 只有主入口，boot 页表含 kernel 所在 1 GiB identity/high-half 与 MMIO block；次级入口缺失。已有 `dma::cbo` 的 64B clean 与 fence 可用于页表发布。 |
| `power.rs::PowerIfImpl`、`init.rs::InitIfImpl` | cpu_num 来自配置，缺 cpu_boot 和 secondary hooks；APLIC global、boot hart记录与 local IMSIC目前都从主 init_later执行。 |
| axruntime `0.3.0-preview.2/src/{lib,mp}.rs` | 主链为 paging→platform init_later→scheduler（注册唯一 IPI）→start_secondary_cpus→timer handler注册→INITED_CPUS barrier→main；次级为 percpu→early hook→ENTERED_CPUS→paging-secondary→later hook→scheduler-secondary→barrier→SIE→idle。HSM返回错误后若仅return，runtime会继续等待，故平台必须显式停止。 |
| `irq.rs::IrqIfImpl` | timer走独立slot，external走AIA；S_SOFT注册/dispatch缺失。send_ipi已走SBI，注释中EID0描述不符合参考固件SSIP路径。 |
| `timer_irq.rs::TimerSlot`、`time.rs::init_percpu` | handler全局CAS，register只开调用hart STIE；time只set_timer(0)。secondary必须独立开STIE/SSIE；set_enable必须识别timer/soft全vector。 |
| `aia.rs::AiaChip`、`HardwareBackend` | target取BOOT_HART；EIE是调用hart的本地CSR。register/unregister及set_enable共同改变全局APLIC和本地EIE。SMP远端mask/unmask必须不误操作caller文件；stopei claim+EOI先于handler可保持。 |
| `crates/axtask/src/{api,run_queue,ipi,future/mod}.rs` | runqueue发布后Release schedulable；affinity先提交再入队；AxWaker选合法runqueue，成功远端Ready后发IPI；init_scheduler对IPI注册失败assert。该机制复用，无第二IPI owner。 |
| `axtask/src/future/time.rs::{TimerFuture,sleep_until}` | TimerKey创建/poll/drop均访问当前hart的percpu TIMER_RUNTIME。持有deadline的任务固定affinity，避免隐式迁移造成poll/cancel访问另一wheel；不扩大为迁移修复。 |
| `kernel/src/drivers/{placement,uart_init,k3_uart,k3_uart_core}.rs` | 四角色纯policy已有；UART启动已使用singleton且重复启动fail closed。PXA IER/LSR/ISR的批次经平台同一Acquire/Release UART锁；ISR退出锁后wake。K3末字节TEMT依赖native 1ms等待。 |
| `kernel/src/platform/k3_async_net.rs::board::run_k3_async_net_service` | 初始化adapter/Service后将整个schedulable mask传给owner/runner；入口`-> !`，和UART入口互斥。source133 ISR不取driver锁；source60 arm当前仅probe构建启用。 |
| `k3_gmac::adapter`、`axnet::{async_rx,stack_runner,service,wrapper}` | adapter Mutex保护Core，Service唯一owner推进descriptor；runner持Service→SocketSet，unlock后发布wake。当前queue/link/socket恢复机制与诊断请求可复用。 |
| root/kernel Cargo、Makefile、`platform/k3_klib.rs` | 两种旧async模式拒绝SMP且互斥；K3 targets递归传APP_FEATURES，普通顶层`+= smp`对命令行变量失效；网络stub无paging，shell引入paging时必须避免重复axklib符号。 |

### 可采信运行与适用性

- QEMU多hart最终记录：`../archive/2026-09-26-ms08-qemu-multi-hart-correctness-baseline/iterations/005-per-driver-smp16-fixed-qualification/003-rework.md`及其Evidence。16 hart、固定NIC角色、timer-disabled IPI、双向帧、Full/readiness/quiet已通过；UART专项、migration、reset/link和组合未计通过。
- `git diff 5df918cd HEAD -- crates/axtask/src kernel/src/critical_section_policy.rs kernel/src/drivers/placement.rs crates/axnet/src/stack_runner.rs`的对应表面未变化。queue owner与UART表面后来变化，按最近单hart/host结果使用，不能套用旧QEMU全量资格。
- 最近网络：`../archive/2026-10-14-ms13-com260-kit-single-hart-network-recovery/iterations/002-socket-combination-final-acceptance/001-rework.md` reported/accepted；生产host包、phy32/32、gmac37/37、axnet默认/queue/qemu三面486/516/519及双构建通过。容量/C4第二轮、fail-stop末场景与插回后持续稳定未取得完整板测，不能用历史豁免满足本次R6。
- 最近UART：`../archive/2026-10-08-ms17-com260-kit-async-uart-interactive-shell/iterations/003-minimal-interactive-shell/000-initial.md`及前两轮Act记录：UART crate74/74、tty11/11、shell20/20、embedded9/9，单hartstdio/shell与受影响构建通过；不能推定多hart硬件通过。
- MS09原始 `../archive/2026-10-02-ms09-com260-kit-observable-link-baseline/evidence/001-board-ram-boundary/000-initial/uboot-readonly.txt`：SBI TIME/IPI/RFENCE/HSM，Linux两次Brought up 16 CPUs；同目录board-facts online/possible/present=0-15。固件能力作为既有事实采信，不重新要求收集。
- 本次真实只读命令：fdtget从`tools/k3_com260_ifx.dtb`读取16 AP节点；CPU0/8均有ssaia、zicbom、cbom-block-size=64；reg0/f，ISA分别rv64imafdcvh/rv64imafdcv。它们是硬件预期，实际StarryOS执行仍由首轮板测验证。
- 本次GNU make原生复现：显式`APP_FEATURES=k3-async-net SMP=4`时普通`APP_FEATURES += smp`仍输出`APP_FEATURES=k3-async-net origin=command line`，新模式须在feature闭包内显式包含smp。
- 本次 `cargo tree --locked --offline --target riscv64gc-unknown-none-elf --no-default-features --features k3-net-diagnostics -e features -i axsync` exit0，确认axsync/multitask实际启用。`axsync-0.3.0-preview.2/src/mutex.rs`使用短spin/yield后Event等待，不是ISS03所写的纯自旋Mutex；停摆事实保留，但该归因不采信，不据此换锁。
- 基线构建/全套测试：SKIPPED，已有适用结论直接采信，当前仅制定计划。没有新增真板/QEMU运行结论。

### 参考资料

按M42采信同板参考的寄存器与用法；本会话已读取 `k3/.claude/analysis/rt-async-amp-k3-multi-hart-adaptation.md`、`docs/interrupts/k3-interrupt-and-time.md`、DMA/known-gaps及实际tgoskits `someboot/src/{power,arch/riscv64/entry,arch/riscv64/sbi}.rs`、`axplat-dyn/src/{init,power}.rs`。参考仓库`e6d0557`、tgoskits`19219411d`。其K3 profile未开启SMP，因此复用HSM/cache/entry经验，不引用为本板SMP资格；不移入someboot/somehal、AMP协议、OpenSBI替换或RP的PMA设置。

### 2026-10-16 后续范围调查

本次只读调查已确认下列入口、消费关系与变更面；没有改产品代码，没有运行板测或重跑已通过的内核测试。

| 代码/数据表面 | 当前事实与后续影响 |
|---|---|
| `k3_async_net.rs::board::run_k3_smp_net_boot` | 1429 起 `-> !`：硬件 stage1–7 → mask 60/133/277 → arm IRQ access → 18 页 arena → GMAC/adapter → 唯一 `axnet::init_network` → 固定 owner/runner → 启动诊断应用 → park。需把诊断应用前的安装与启动提取成可返回步骤；旧专项包装器继续执行原载荷。安装失败不返回到 shell，也不重启第二 owner。 |
| 同文件 `SMP_NET_HANDLES`/`smp_singleton` 与 source60 | helper 当前只编译于 `k3-smp-net`，新组合需要扩大必要 cfg；source60 沿原生产武装，保留 INSR/INTN ack。新入口不依赖诊断 feature、不主动 reset、不运行原恢复载荷。 |
| `entry.rs` 的 `k3-rootfs` 分支 | UART init/copiers → DTB/header/initrdmem 检查 → RamDisk/ext4 → pseudofs → alarm → resolve/load 普通动态 `/bin/sh` → `Process::new_init`/stdio/tty → spawn/join → fs close/物理 drain。现无 GMAC，alarm 与用户任务未固定。复用该链，不采用 `k3_smp_uart` 的内嵌 ELF/内存根。 |
| `src/main.rs::CMDLINE`、`src/init.sh` | `/bin/sh -c <init.sh>` 最后执行 `sh --login`，因此即便 shell 不启动外部工具也会走 clone/exec。只 pin 最初用户任务不能保证后续工具固定。 |
| `syscall/task/clone.rs::CloneArgs` 的创建路径、`clone3.rs` | clone/clone3 共用子任务组装，当前尾部 `spawn_task(new_task)`；新 TaskInner cpumask 默认为 full（`axtask/src/task.rs`），未继承父 mask。应在所有用户子任务首入队前按父 `current().cpumask()` 设置子 mask；这是 fork/clone 的 affinity 继承，非通用迁移框架。 |
| `syscall/task/execve.rs::sys_execve` | 在当前 task 上替换地址空间、context 与进程数据，不创建新 TaskInner，不改 cpumask；继承后的固定 mask 可跨 exec 保持。 |
| `task/timer.rs::spawn_alarm_task_with_affinity`、`axtask::future::TimerFuture` | 002 已提供 alarm 入队前 affinity 变体；主任务 pin boot、alarm pin boot、初始 shell pin8，子任务继承 singleton，使创建/poll/drop 操作相同 per-hart wheel。复用，不改 timer runtime。 |
| `drivers/placement.rs::place_roles` | 四后台角色已按升序 schedulable 与 boot anchor 统一计算；组合从 `drivers` 的一个实例消费，不另 include 一份 policy。四角色互异、shell8存在且不与角色重叠，否则 fail closed。 |
| root/kernel Cargo、`lib.rs`、`platform/mod.rs`、`entry.rs` | 当前 async-net+async-uart 守卫禁止组合，SMP 例外分别依赖独立 net/uart 模式。新 `k3-smp-combined` 显式含 k3-smp、k3-rootfs、k3-async-net，仅为该入口开放组合/SMP；不含 `k3-smp-net`/`k3-smp-uart`/`k3-stdio`/诊断 feature，不运行两个旧 `-> !` 入口。旧 rootfs 与 SMP 平台分支需排除新入口。 |
| `platform/mod.rs::k3_klib` | 当前 async-net 恒编译本地 `__axklib_0_3_mem_iomap` stub；rootfs 引入 paging 后 axruntime 提供实现。新组合须排除本地 stub，旧无 paging 网络形态仍保留；不改外部 axruntime 或引入第二 provider。 |
| `k3.rs::K3Env::{mmio_read,mmio_write}`、`mac_probe.rs::probe_and_report`、`k3_async_net.rs::board::irq_reg_read/irq_reg_write` | 三处实际 volatile 访问仍将物理地址直接 cast 为指针；source60 ISR 也消费 K3Env。平台偏移为 `0xffffffc000000000`，axruntime 在 entry 前调用 axmm::init_memory_management；axmm::new_kernel_aspace 只映射 `phys_to_virt(region.paddr)`，用户页表也只复制高位 kernel 部分。组合必须让上述 MMIO 访问使用平台 phys_to_virt，高位映射覆盖 MAC/APMU/GPIO/APBC/MFPR；不修改设备寄存器参数或新增低位 identity 映射。UART 和 AIA 已使用高位映射。 |
| `k3_network_probe.rs::BoardBacking`、平台 mem/ram_layout | arena PA 经 `GlobalPage::start_paddr(virt_to_phys)`，无需改 DMA/cache 算法；rootfs 窗 `[0x240000000,0x250000000)`、DTB 窗 `[0x2ef000000,0x2ef100000)` 必须沿用，不能复用旧 SMP FIT 的低 DTB 地址。 |
| `syscall/net/{socket,io,opt}.rs` → `file/net.rs::Socket` → `axnet::{TcpSocket,UdpSocket,SocketOps}` | 已有 AF_INET TCP/UDP、connect/read/write/send/recv、poll/select 与 timeout options 接线；无 ICMP socket。wget 走 HTTP/TCP，nc 用 TCP/UDP；只因工具实际返回的 syscall 错误才规划有限修复，不扩展 DNS/TLS/IPv6/ICMP。 |
| `rootfs-riscv64-min.img` | `debugfs` 只读确认 `/bin/busybox` 为 825088B，普通 RISC-V ET_DYN，解释器 `/lib/ld-musl-riscv64.so.1`；从镜像只读导出到 `/tmp/starry-bb-plan`，用 qemu-riscv64 用户态执行 `busybox --list`，已有 wget/nc/cat/cmp/sleep。nc `--help` 明确支持 `-n/-u/-w`，wget `--help` 支持 `-Y/-T/-O`；无需下载或重编 BusyBox。此结果只证明工具存在和 CLI，不证明 StarryOS 上能运行。 |
| 测试入口 | `tests/k3-uart-plane-host-harness.rs` 消费实际 placement/启动 guard；`tests/k3-embedded-user-host/tests/ram_rootfs.rs` 消费实际 ram_layout；`tests/k3-async-net-host` 覆盖生产 adapter/Service/socket。已有 `tests/ms01_socket_baseline.c` 可按真实缺口复用。clone affinity 先行测试须在展开 003 时由 Plan 确定能行使实际子任务入队路径的最小入口，不接受只测复制 mask 的同义用例。 |

运行记录：修改前 `openspec validate ms14-com260-kit-multi-hart-async-io --strict` exit 0；`debugfs -R 'stat /bin/busybox' rootfs-riscv64-min.img`、`readelf -l /tmp/starry-bb-plan/busybox`、`busybox --list` 均实际运行。导出文件起初没有执行位，第一次工具清单和两条 help 均 exit1/无输出；检查 `file/readelf/ls -l` 后确认导出权限，给 `/tmp` 副本补执行位，清单 exit0、help 输出上述选项。未修改源镜像，不把工具取证计为产品验收。

最新可采信基线：000–003 的 Review 均 accepted。003 最终 Response 的组合 FIT exit0、真实 clone 首入队继承测试3/3、MMIO/placement/MAC 定向 GREEN及包内34测试直接采信；本次 `git diff --name-only` 无未暂存产品改动，暂存区已包含这些交付。仅补查004新增涉及的 socket/tty/clock/devfs/工具表面，不重复产品构建/板测。004 已获用户2026-10-16原话“批准”，ready，尚未调用Act。

## Decisions

### D1 独立模式族保持旧单 hart 边界

- 选择：已交付的 `k3-smp`、`k3-smp-net`、`k3-smp-uart` 保持；后续新增 `k3-smp-combined`，root 闭包为 `[k3-smp, k3-rootfs, k3-async-net, starry-kernel/k3-smp-combined]`，kernel 闭包为 `[k3-smp, k3-rootfs, k3-async-net]`。不包含两个专项选择器或 k3-net-diagnostics。组合最终形态固定 SMP=16，其他数值原生拒绝；旧目标的 2..=16 诊断范围不变。
- 原因：旧模式仍有正式SMP拒绝与单hart验收边界。直接放开旧guard会混淆入口和组合语义。
- 影响：root/kernel Cargo、Makefile、kernel guards/entry 与平台模块。仅新组合允许 async-net+async-uart 与 rootfs+SMP，仍拒绝 k3-stdio/k3-shell/raw-port 及其他专项选择器混入；不放开旧模式。
- 替代：直接去掉旧guard、用APP_FEATURES顶层追加smp，均拒绝。不创建通用mode registry或新CLI。
- 新产物名称 `StarryOS_riscv64-k3-smp-combined.fit`，独立 TARGET_DIR/OUT_CONFIG/OUT_DIR；kernel load/entry 保留 `0x180000000`，DTB 使用 rootfs 已交付的 `0x2ef000000`，rootfs 另行加载至 `0x240000000`。128 MiB 现有镜像可复用，不变更板载持久介质。`dumpimage -l` 只确认可装载形态，不作为行为验收。

### D2 复用现有SMP runtime，只补K3启动接口

- 选择：AP采用已核对的dense0–15映射；boot raw hart不假定0。secondary从SBI a0=hart、a1=独立stack PA进入，安装已发布boot页表、fence.i、修正high-half SP并调用axplat secondary main。主核生成boot页表后沿已有64B CBO/fence发布，不重写该页表或重复clear BSS。
- 原因：axruntime已承担percpu、最终paging、scheduler和barrier；无需移植参考仓库的另一启动栈。
- 影响：boot/init/power与平台smp feature。HSM unavailable、start error、越界/非AP目标在PowerIf处原生报告后park，不能return到runtime的ENTERED_CPUS等待。成功启动后的runtime初始化链保持现有行为；首轮板测若成功SBI请求后无entry/ready，则停止并返回Plan，不扩展通用启动watchdog。
- 替代：本地化整个axruntime、重写CPU枚举/online协议、改板载OpenSBI均拒绝；实际异常若推翻dense映射或接口假设，回Plan。

### D3 本地timer/soft slot与全局handler生命周期分离

- 选择：保持timer全局handler一次注册；secondary later hook独立打开STIE/SSIE并set_timer(0)。S_SOFT使用一个固定slot，CAS注册；dispatch清SSIP后调用唯一axtask handler，返回完整IPI vector。SMP runtime timer/IPI handler常驻，运行期unregister返回None且无副作用；局部停止仅set_enable，避免只mask一核却移除所有核共享handler。旧单harttimer保留masked-before-remove及重复注册语义。set_enable(timer/soft)只改caller本地CSR，不访问APLIC。旧runtime-irq但非smp模式不新增soft使能。
- 原因：注册发生在主hart，而timer/soft enable是per-hart；handler不得每核重复注册。
- 影响：`irq.rs`、`timer_irq.rs`及一个最小固定soft-IRQ core（如`ipi_irq.rs`），复用现有host CSR seam；全局`TimerSlot::enabled`不得用于推断其他hart STIE。
- 替代：第二axtask/axipi owner、用GENMSI/EID冒充S_SOFT、为全部IRQ建动态dispatcher均拒绝。

### D4 外设IRQ固定boot hart，远端只改全局source门

- 选择：source1/42/60/133/277的register/unregister仍在boot hart；其target/EIE由boot注册生命周期维护。SMP set_enable(device)从任意hart只操作对应APLIC全局SETIE/CLRIE，不改本地EIE；保留source白名单与同source操作的原生位写。boot注册成功后EIE保持到boot unregister，APLIC门承担runtime mask/unmask。secondary初始化只清自己的EIE，不重置global或BOOT_HART。
- 原因：NIC owner可以远端执行，APLIC门是全局、EIE是本地；固定IRQ归属已能证明跨hart唤醒，无需增加IRQ迁移机制。
- 影响：AIA增加仅global门的host-testable helper，IrqIf在smp设备分支调用；远端device register拒绝false、unregister拒绝None且无硬件副作用。timer/IPI slot不受此限制。
- 替代：远端IMSIC MMIO、每个hart预开全部设备EIE、IPI代理路由重编程均拒绝。单hart沿用原AiaChip流程，不动已经验收的mask丢MSI事实。

### D5 固定所有带deadline的验收任务，直接覆盖跨簇

- 选择：后台四角色沿用共享 place_roles（默认 boot0 时 RX0/TX1/NIC2/stack3）；BusyBox 初始用户任务固定8、主任务及 alarm 固定 boot，均先提交 singleton 再创建 deadline。clone/clone3 共用子任务组装在入队前继承父 cpumask；exec 保持当前任务 mask。已交付专项的网络应用9保留，但最终组合不启动该诊断应用。不新增一轮实际 hart/IPI 专项；复用现有只读观察，既有结论采信。
- 原因：TimerFuture操作当前wheel，且IRQ/软件wake会选择允许的本地runqueue；未固定的应用可能隐式换hart。固定是本次范围，非受控迁移功能。
- 影响：复用 axtask API 与 002 的 `spawn_alarm_task_with_affinity`；`syscall/task/clone.rs` 在唯一子任务发布点继承父 affinity。优先直接使用 TaskInner/set_cpumask 和既有 spawn，不新增运行时或复制另一套进程创建路径。已有非法 mask/未 schedulable 拒绝保持。
- 替代：修整个timer runtime的迁移、按架构分叉ordering、为核拓扑加通用调度框架均拒绝。AP 8/9缺失或ISA与既有DTB冲突时首轮板测失败，不默默改为只测X100。

### D6 BusyBox RAM rootfs 的单实例组装

- 选择：NIC 初始化拆出可返回的安装/启动步骤，保留旧 `-> !` 专项包装器与原诊断载荷。该步骤消费确定的 owner/runner singleton 和调用者输出函数，保留硬件 gate、IRQ access 发布、arena、adapter、Service、Active 等待及 handle 保留顺序；不主动运行 duplicate-start、容量、quiet 或恢复资格。source60 沿原生产初始化武装，保留 ISR 无 MDIO、owner 消费 INSR。
- 最终入口在 boot hart 固定主任务 → 取统一角色 → 初始化 UART/copiers → 以 `k3_uart::diag_line` 安全输出安装 NIC 并启动唯一 owner/runner → 按 ram_layout 校验并挂载唯一 ext4 RAM root → mount pseudofs（唯一 tty reader）→ 启动固定 alarm → 普通 loader 及 stdio/Process 组装 → 入队前 pin 初始 `/bin/sh` → join/既有 fs close/drain。任何前置失败不继续 shell，不重新初始化已发布设备。初始进程及子进程使用普通动态 loader，不用内嵌最小 shell fallback。
- paging 启用时使用 axruntime 的 axklib，组合 cfg 排除本地 k3_klib stub；旧无 paging 网络形态保留 stub。K3Env、MAC identity probe 与 source133 ISR 的 MMIO 使用平台 phys_to_virt，覆盖 source60 经 K3Env 的 GPIO ack；32-bit volatile 宽度、寄存器 offset/读写序和 IRQ 无 MDIO 语义保持，不引入动态 iomap、低地址补映射或用户设备直通。四角色都从 `drivers::placement` 的同一编译实例取值；不让旧 net 模块额外 include 另一份 policy。
- 活动UART后的网络诊断不得用无锁early console；经既有共享guard及LSR latch-aware输出，负载中仅边界/错误/必要状态行，避免长日志改变业务字节或quiet。
- 原因：两个永不返回入口不能串接，raw reader 不能与 tty 竞争；rootfs/FIT 地址、paging 与用户子进程是最终使用链的必要组成。
- 替代：只取消 compile_error、复制第二 Service/tty、先交付内嵌最小 shell 再做第二轮 rootfs 资格均不采用。直接交付用户指定的最终载体。

### D7 恢复实现保留，修复以最终使用的实际阻塞为界

- 选择：保留既有 request→resident owner→epoch/门→stop/SWR/原 arena 重建→link→socket terminal 实现和 backing retention。取消后续 Up/Down reset 矩阵、delayed/stuck/fatal 资格、容量/C4 第二轮及门关闭入流的专项验证，不创建替代压力载荷。
- ISS02/ISS03 保持现状；从原恢复任务删去预设调试和修复，不直接将它们标成已解决。只有 BusyBox 启动、wget/nc 或串口共同使用实际被阻塞，才由 Plan 定位最小调用边、给有限修复契约，再由 Act 先复现后修复；不得把新设计留给 Act。ISS03 的纯自旋归因仍不采信。
- 原因：用户把交付限定为正常最终使用。删去专项资格不要求删除产品恢复能力，也不证明多 hart reset/拔插鲁棒性。
- 替代：无故障先换锁、加周期 link 轮询、重分配 DMA arena、自动 reset 到成功均拒绝。若实际阻塞涉及新的控制面/恢复政策，返回 Plan。

### D8 必要研发测试与一次最终使用验收

- host 仅为实际修改建立先行测试（新功能/修复 RED→GREEN，提取重构采信有效 GREEN 后保持），运行最小相关测试与构建；不自动执行 production 全包、axnet 三 feature 全套、QEMU SMP 或所有旧模式。未变表面采信，新增失败才做定向诊断。不建立测试设施项目、脚本判定层或身份协议。
- 003 只完成研发与目标构建，不设真板专项，现已 accepted；004 按用户新增要求预算一次 StarryOS 最终形态会话（正常使用 + D9 短时测量），另一次原厂 Linux TCP 基线阶段，均由操作者执行。失败保留当前任务，新的修复设计返回 Plan，不把重复到通过当验收，也不新增恢复/长稳会话。
- 最终输入为已验收 128 MiB RAM rootfs 与新组合 FIT：沿 R68 进入 U-Boot，先 `fatload usb 0:0 0x240000000 rootfs-riscv64-min.img`，立即 `setenv starry_rootfs_size ${filesize}`，再 `fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3-smp-combined.fit`，`setenv bootargs initrdmem=0x240000000,0x${starry_rootfs_size}`、`setenv fdt_high 0xffffffffffffffff`、`bootm 0x2e0000000`。保持 rootfs DTB 高位窗口，不 `saveenv`。上述启动步骤复用 MS18 `001-standard-shell/001-replan.md` 已实测路径。
- 最短行为见证：BusyBox 提示符下输入/编辑与输出一条 echo，执行外部命令；在 PC `192.168.137.1` 用原生 HTTP server 提供已知文本 `hello.txt`，板上执行 `wget -Y off -T 5 -O /tmp/bb-download http://192.168.137.1:8000/hello.txt` 并观察退出码/`cat` 内容；PC 普通 TCP/UDP echo 对端分别回复板上 `nc -n -w 3 192.168.137.1 26002` 与 `nc -u -n -w 3 192.168.137.1 26001` 输入的短文本。nc 的输入/回显本身经过 UART 与 GMAC；结束后再输入 echo，直接看到 shell 仍正常工作。004 展开时给出 PC 原生 socket 命令与 nc 正常终止方式，不增加专用 peer 协议/validator。
- wget/nc 复用 `rootfs-riscv64-min.img` 中的 BusyBox applets，可用 `/bin/busybox wget`/`/bin/busybox nc` 直接调用，不以 symlink/PATH 缺失要求重做 rootfs。静态 IPv4、局域网 HTTP/TCP/UDP 足够，不引入 DNS、DHCP、TLS、ICMP 或真实存储。
- 初始挂载、普通 loader、四条件 drain、取消/超时及资源安全代码保持；不以独立 EOF/SIGINT/drain/账本矩阵或外部 RESET 逐字确认增加最终门槛。外部 RESET 仍可用于操作者结束/故障回滚，不构成 runtime reset 结论。
- Evidence默认none：各会话的决定性原生结果、PC实际内容、资源状态与一句话人工结论可完整写Act Response；不创建Evidence占位目录。无法低成本复现且摘要会丢关键结构的异常，按公共规则由Act另行保留最小现场。

### D9 UART 实线吞吐与原厂 Linux TCP 对比

- 来源与范围：用户本轮明确增加性能测试，并选择无最低门槛、Windows 原生 Python/PowerShell。只承接 I19 中本次必要的短时测量；优化、CPU/p99、UDP性能、多流/offload及完整矩阵仍在范围外，不同步全局 I 文档。R10/R11 与 T14/T15 为新增映射，全部留在最后004。
- 必要代码/工具调查：已读取 `k3_smp_combined`/普通 rootfs/clone、`syscall/net/{socket,io,opt}`、`file/net.rs`、`io_mpx/{poll,select}`、`pseudofs/dev/tty` 与 `syscall/fs/ctl`、`syscall/time`/devfs。现有 AF_INET TCP/UDP、普通 FD、read/write/poll 与 monotonic clock 入口齐备，但完整工具运行仍须真板。UART TCSETSW 当前未物理 drain，tty 的 B38400 元数据不表示硬件速率；本轮保持固定115200硬件，不据此修 tty。`/dev/console` 实际连接 ASYNC_TTY，`/dev/shm` 是 MemoryFs，zero/null是实际设备实现。具体调用链和边界已全部写入004，不要求Act回读此设计。
- 实际工具检查：现有镜像导出副本 `/tmp/starry-bb-plan/busybox` 的 `--list` 与 dd/nc/stty/cmp help经 qemu-riscv64 用户态执行，确认 dd 的 fullblock/status=none、nc 的-n/-u/-w、stty/cmp已存在。对该 dd 的一次 `-strace` 运行显示首尾 CLOCK_MONOTONIC、262144 bytes实际输出、exit0；这只证明工具计时方法，不是StarryOS/真板性能。源rootfs未修改，无需下载/重编BusyBox。
- UART：115200 8N1 的理论载荷为115200/10=11520 B/s=11.25 KiB/s。TX/RX各一次256 KiB连续零数据；TX由Windows byte[]接收，首字节到末字节计时，排除命令回显/GUI渲染；RX在唯一tty raw/-echo、dd fullblock前台等待后，由PC连续发送，计到板上dd完成记录回到PC，注明含短回执开销。用原生文件长度/比较核对实际数据，恢复原termios和交互。报告B/s、`10*Bps/115200*100`线速利用率及`8*Bps/115200*100`有效数据位占比；后者理论最大80%，不能混用。写入/入队结束、fsync或stty不能替代实线完成。
- TCP：沿用同板口/网线/Windows物理网卡，单连接、32 MiB，分别测板→PC、PC→板，每方向预先固定3个样本，每个OS共6个。发送数据从RAM准备，接收端计时：板→PC用Windows原生Python recv计数/perf_counter；PC→板统一用同一BusyBox `nc | dd` 的实际完整接收量/monotonic时间，包含pipeline启动。原厂Linux在RAM中直接用从现有镜像导出的BusyBox与musl loader，避免两侧换负载；不改系统/bin/lib，不装软件包、不调整offload/窗口。全部原值、中位数/min-max及同方向StarryOS/Linux比例写Response。固定3样本属于预定时序测量，不择优、不复跑到满意。
- 条件与解释：记录实际协商速率/MTU、默认调度/offload差异；环境信息只帮助解释，不建立身份匹配或自动拒绝。测量是整条用户态/协议栈/驱动路径的短时对比，不能归因于独立驱动收益或硬件峰值。短收/超时没有完整字节证据，原生现象保留，不能标完整样本；Linux数据未取得不得勾选T15。低速度本身不是失败，不引出优化任务。
- 选择原生工具和控制台I/O而不复用历史benchmark：`tests/benchmark.c`默认全矩阵且口径/配置不适合本轮；`network_benchmark.c`依赖专用控制协议/身份字段。它们不在此次可达负载内，不调用、扩展或改造；不引入iperf构建/发布、新测速程序或脚本判定层。PC命令只收发真实数据并计时，原生结果由操作者观察；不加握手、PASS解析、CLI或专用fixture。
- API依据：Windows只使用byte[]方式的 [SerialPort.Read](https://learn.microsoft.com/en-us/dotnet/api/system.io.ports.serialport.read?view=netframework-4.8.1)，不混用BaseStream/字符API；网络使用[Python标准socket](https://docs.python.org/3/library/socket.html)的普通TCP/UDP操作，累计短recv。本轮自包含命令、timeout/恢复与验收见004 Cycle。

## Risks and Deferred Investigation

- 000–002 的 16 AP、timer/IPI、NIC/UART 真实执行已有 accepted 结论；最终正常使用不重复这些专项，也不将旧结论扩大为恢复/长稳资格。
- 固定boot IRQ增加IRQ hart负载：本次只验证有界正确性，不以性能测量要求IRQ balancing。
- ISS02 持续恢复、ISS03 停摆的具体根因仍未确认；独立排障退出当前验收，仅在阻塞最终使用时按 D7 处理。取消覆盖的风险及用户原话在 proposal 中，不标记问题关闭。
- 单hart网络/PXA已验收的tail、pad、INSR、FCR/Errata序不重设计；新证据推翻它们时返回Plan。
- 兼容检查只覆盖实际改动。003 已闭合 clone affinity 的先行测试：在既有 embedded-user-host 包的 integration test 直接 include 生产 clone.rs，调用真实 do_clone，通过最小OS替身捕获首次enqueue参数；先观察默认full mask错误，再做生产发布点继承修复。普通/THREAD及非singleton路径都覆盖，不改变生产接口或新增测试框架。具体事实与执行契约以003 Cycle为准。
- wget/nc 在 StarryOS 上的完整运行结果尚无；CLI/ELF 可用不等于 syscall 兼容。004 已基于003 accepted明确实际调用边与原生操作，当前无预设产品修复项；真实新 syscall/设备/恢复缺口必须返回 Plan，不预先授权无限补齐 Linux。原厂Linux和Windows尚未执行本轮操作，没有预填性能结果。

## Implementation and Rollback

000–003 保持已交付。004准备现有工具，用户在最终形态真实使用wget/nc及串口，然后采集UART/TCP测量并外部RESET回原厂Linux取得TCP基线。RAM/FIT与回滚手段保持，不saveenv、不改官方固件；原厂工具只放RAM。真实阻塞先记录并返回Plan制定有限修复，不以增加测速设施或自动reset解决。

## Planning Readiness

Gate 1 PASS：用户批准展开最后004，并明确新增性能测量、无最低门槛及Windows对端。004调查/Task Contract/原生操作/RTM技术检查已闭合；用户2026-10-16原话“批准”，Gate 2 PASS，Cycle ready。未调用Act。全局tasks、M/R/I均不写入，当前范围以本变更proposal/tasks为准，执行依据为004 Cycle。
