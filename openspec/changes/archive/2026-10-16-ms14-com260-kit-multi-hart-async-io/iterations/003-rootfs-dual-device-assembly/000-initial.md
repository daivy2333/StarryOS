# Iteration 003 / Cycle 000: 多 hart RAM rootfs 与 UART/GMAC 组装

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: T9–T11（4.1–4.3），T9 → T10 → T11。
- Acceptance gaps: None。
- Repair items: None。
- Inherited scope: R3 的固定角色、R6 的既有恢复/资源安全边界、R7 的最终组装、R8 的定向兼容检查与 RAM-only、R9 的 rootfs 载体前置。保留 000–002 已 accepted 的平台/NIC/UART 实现。
- Excluded scope: T12–T13 的 wget/nc 工具接通与最终真板使用；恢复矩阵、独立容量/C4/quiet/readiness 压力、故障注入资格、30 分钟长稳、历史全套 host/QEMU/旧模式回归、ICMP/DNS/TLS/IPv6、真实存储、性能、通用迁移及新驱动框架。

**Objective**

交付可构建的 `k3-smp-combined`：在既有 16-hart 平台上组装唯一异步 NIC owner/runner、UART copiers、普通 ext4 RAM rootfs 与动态 BusyBox `/bin/sh`。主任务/alarm/初始 shell 固定 affinity，用户子任务入队前继承父 mask；网络 MMIO 使用完整页表中的平台映射。目标产物为独立组合 FIT，硬件最终使用不在本轮声明中。

**Background**

用户 2026-10-16 批准“研发需要改代码的工作仍旧保留……改为bb rootfs shell的最终形态验证串口和网卡工作正常来作为最终验证”，并选择 wget+nc（TCP/UDP），ping 留待后续。当前指令“当前iter被接收，给出新的规划的下一轮iter”授权展开本轮，替代此前不展开下一轮的等待边界；未授权本轮实施。

原恢复/长稳专项已经取消，其缺口不重设为本轮门槛。保留研发先行测试和最小定向检查。用户手动真板操作、RAM-only 与不修改板载固件的边界保持。

**Investigation Facts**

- Current Baseline：002 `000-initial` 的 Act Response（2026-10-16 reported）与 Review（accepted）为直接前置：UART plane 12/12、新 SMP UART/raw FIT 构建成功、直接受影响旧构建成功；操作者两 boot 的 RX0/TX1、shell8、alarm0、字节流/短写/drain/交互成立，Ctrl-C/EOF/RESET 逐字确认按原用户豁免保留。001 已 accepted 的 NIC owner2/runner3/app9、四协议、两级容量/C4 与 quiet 结论继续有效。上述实现没有因本次计划调整而变化，不复跑基线。
- Current-State Evidence：2026-10-16 本会话实际读取下列代码；`git diff --numstat -- kernel/src/syscall/task/clone.rs kernel/src/syscall/task/clone3.rs kernel/src/platform/mac_probe.rs kernel/src/platform/k3_klib.rs crates/axtask/src/task.rs crates/axtask/src/future/time.rs` 无输出。与上一轮规划已经确认的表面一致，无产品实现新增；已有暂存/未暂存改动属于前序轮次或用户，不能覆盖。

| 代码表面 | 当前行为、消费关系与本轮责任 |
|---|---|
| `kernel/src/platform/k3_async_net.rs::board::run_k3_smp_net_boot` | 约1429行起 `-> !`。stage1–7 硬件 gate → mask60/133/277、unregister133 → arm BOARD_IRQ_ACCESS → 18页 GlobalPage 清零 → Gmac::new_async/init_rx/enable_tx → register133 → arm_phy_source60 → K3NetAdapter → `axnet::init_network(from_one(nic))` → placement → 唯一 owner/runner → duplicate-start 见证 → 固定应用载荷 → park。T9 提取到 runner 就绪为止的生产步骤，旧包装器仍跑其余专项；最终组合只调用可返回部分。 |
| 同文件的 owner/runner 生命周期 | `SMP_NET_HANDLES` 当前仅 `k3-smp-net` 编译；`start_rx_task_affinity` / `start_stack_runner_affinity` 已拒绝重复启动。owner Active 等待和 runner 首 poll 等待均5秒上限，失败报告并 park、不返回 shell；arena 发布后 backing 保留。提取后保留这些语义，不造新的生命周期框架或可重试安装 API。 |
| `k3_async_net.rs::board::{irq_reg_read,irq_reg_write}`、`k3.rs::K3Env` | 当前 source133 回调、K3Env MMIO 都直接将 PA cast 为 u32 指针；source60 handler 通过 K3Env 做 GPIO/MFPR ack，MDIO仍只在 owner/初始化上下文。三条 native 调用边必须改用平台 phys_to_virt，再做原有32位 volatile 访问。 |
| `mac_probe.rs::probe_and_report` | 当前函数直接读 MAC PA，只有 `k3.rs` 和 `run_k3_smp_net_boot` 两个生产调用者。已有 `phy_probe::ProbeEnv` 提供 mmio_read/write；将 identity probe 改为消费该 env，移除自身裸 PA 读，两个调用者均传已有 K3Env。原采样次数、地址、格式、分类与不写 MAC 的语义保持。 |
| 平台 `mem.rs`、`axmm::new_kernel_aspace`、`mm::copy_from_kernel` | K3 PHYS_VIRT_OFFSET=`0xffffffc000000000`。axruntime 在 entry 前切换完整页表；axmm 对 memory_regions 映射 `phys_to_virt(paddr)`，用户页表只复制高位 kernel 部分。不能靠原低位 identity 指针；不能用增加低位映射替代正确地址转换。UART/AIA 已使用平台高位地址。 |
| `k3_network_probe.rs::BoardBacking`、`k3_gmac` | arena base_pa 经 GlobalPage::start_paddr(virt_to_phys)；cache操作使用 arena VA，既有 descriptor、cookie、tail、cache/barrier、INSR ack 与恢复算法无需调整。 |
| `entry.rs` 的 k3-rootfs 分支（约310–481行） | UART init/copiers 后依次校验 DTB 指针/header、解析 /chosen initrdmem、校验 extent → RamDisk/ext4 → pseudofs → alarm → resolve/load_user_app(None,args,envs) → Process::new_init/tty/stdio → spawn/join → fs close/drain/park。没有 GMAC；alarm/初始用户任务没有 affinity。提取这条已交付链给旧 rootfs 与新组合共用，不复制第二套 loader/进程发布路径。 |
| `src/main.rs::CMDLINE`、`src/init.sh` | 初始参数是 `/bin/sh -c <init.sh>`，脚本执行 `sh --login`；正常登录与外部命令会创建子任务。新入口继续接收原 args/envs，不替换为内嵌静态 shell。 |
| `syscall/task/clone.rs::CloneArgs::do_clone` | `new_user_task` 返回默认 full mask；THREAD 分支共享原 ProcessData，其余分支 fork、clone aspace/FS/FD/信号；尾部设置 task_ext，再 `spawn_task(new_task)`、add_task_to_table。这里未继承父 `curr.cpumask()`。T11 只在这个共用子任务发布点之前设置继承 mask，不改 fork/FD/信号/错误语义。 |
| `syscall/task/clone3.rs::sys_clone3`、`sys_clone` | 两入口最终都调用 `CloneArgs::do_clone`，因此修改一个发布点覆盖普通子进程与线程，不建立另一套 clone。 |
| `syscall/task/execve.rs::sys_execve` | 在同一 task 上替换地址空间/context、进程数据和 CLOEXEC FD，不创建 TaskInner、不修改 cpumask；原样保留。 |
| `axtask::TaskInner`、`task/timer.rs` | TaskInner::new_common 默认 cpu_mask_full；set_cpumask 可在入队前设置。002 已有 `spawn_alarm_task_with_affinity`；TimerFuture 创建/poll/drop 都访问当前 hart wheel，所以主任务与所有带 deadline 的用户后代保持 singleton，不修改 timer runtime。 |
| root/kernel Cargo、`lib.rs`、`platform/mod.rs`、`entry.rs` | 现有 net+uart 全局互斥，SMP 例外分别是专项选择器；新组合需要独立例外。kernel `drivers/file/mm/pseudofs/task` 已由 k3-rootfs→k3-uart 打开。k3_klib stub 当前 async-net 恒编译，paging 时与 axruntime provider 冲突，必须仅在新组合排除本地 stub。 |
| `ram_layout`、FIT/Make | rootfs窗 `[0x240000000,0x250000000)`、DTB窗 `[0x2ef000000,0x2ef100000)` 已交付，128MiB min镜像可用；kernel load/entry `0x180000000`。旧 SMP FIT 的 DTB `0x181000000` 不可用于组合。新目标不依赖 k3-shell-elf，不下载/重做 rootfs。 |
| 现有测试 | `tests/k3-uart-plane-host-harness.rs` 用真实 placement；`tests/mac-probe-host-harness.rs` 用真实 mac_probe/phy_probe 和 FakeApmu；`tests/k3-embedded-user-host` 已有 axerrno/memory_addr、真实模块 `#[path]` 与 integration tests。现无真实 clone 发布路径的 host 用例，本轮在该现有包增加一份 integration test，直接 include 生产 clone.rs、只替换 OS 边界。 |

**Code and Critical Path**

新模式：`make k3-smp-combined SMP=16` → root/kernel feature → entry 唯一组合分支 → 主任务固定 boot → 统一 placement → init UART/start_copiers → 返回式 NIC 安装/固定 owner、runner → 共用普通 rootfs 用户态启动 → 固定 alarm、初始 shell8 → clone 后代继承父 mask → exec/网络工具（后续004）。

MMIO：MAC/APMU/GPIO/MDIO 操作 → ProbeEnv/K3Env → 一个最小 native read32/write32 实现 → axhal::mem::phys_to_virt → 32位 volatile；source133 的 BOARD_IRQ_ACCESS 回调走相同 native 实现；source60 仍走 K3Env，ISR无MDIO。

**Implementation Guidance**

先提取 T9，保留旧专项外层与输出消费方式。T10 同时完成新入口/构建和必要 paging 适配；普通 rootfs 启动体只保留一份，旧入口传“不指定 affinity”、新入口传明确 singleton。T11 的先行测试先在当前生产 clone 上观察 full mask 被错误入队，再在 `do_clone` 的发布前补父 mask 继承。

下述契约规定行为和变更面；局部名称、返回结构体与 helper 拆分可由 Act 选择。不能把源码 grep、include_str 文本断言或复制版 clone 当成运行测试。

**Task Contracts**

### T9: 可返回的单实例 NIC 安装与固定角色启动

- Requirement/Scenario: R6/R7/R8，S24 的组装前置、S26/S32。
- Depends on: 002 accepted。
- Targets: `kernel/src/platform/k3_async_net.rs::board::{run_k3_smp_net_boot,SMP_NET_HANDLES,smp_singleton,smp_fault}` 及必要 export/cfg；提取一个供同平台调用的安装函数。旧 k3_smp_net placement、诊断载荷与单 hart包装器只作既有消费方。
- Current behavior: 唯一 SMP NIC 安装只能通过永不返回的专项入口运行，成功后必启动诊断应用。
- Required behavior: 安装函数接收 caller 已从 schedulable 验证的 owner/runner hart 和输出回调，沿原硬件 stage1–7、IRQ/arena/adapter/Service 顺序完成安装，等 owner Active 与 runner 首 poll后成功返回；返回值保留旧载荷需要的 arena VA，handle 继续持有，arena不随返回释放。helper 不启动应用、duplicate-start 见证或其他资格载荷。旧 SMP net包装器从共享 policy取其原角色，调用该函数后执行原 duplicate-start/应用/监督/park；不增加可重试安装接口。
- Preserve: 18页 arena、source133 IOC、source60 arm/ack（原降级 note 语义保持）、5秒启动上限、同一个常驻 owner、Service安装一次、start拒绝后停止、错误时 backing保留；单 hart async/probe包装器的恢复与业务语义保持。
- Forbidden: 复制第二个 Service/NIC、重设 descriptor/cache/恢复算法、加入轮询兜底或自动 reset、将失败返回为可继续启动 shell、新增通用初始化框架。
- First test: 采信001 accepted 中 SMP NIC 构建及真实板测 GREEN、未改变的 adapter/Service测试结论作为重构前观察；不为纯提取重跑该板测或全包。提取后直接构建旧真实调用入口。
- GREEN condition: `make k3-smp-net SMP=16` exit0；原包装器仍消费返回的 arena并运行原载荷，输出回调贯穿提取的硬件/安装流程，成功能返回给新caller，失败仍停。Gate3逐项核对资源转移与旧调用链，不能用构建替代将来的最终硬件结果。
- Verification: 上述旧目标构建一次；静态自检安装顺序/所有权/调用者。未改生产 adapter/Service 的测试输入，不重跑26项或旧容量/恢复载荷。
- Stop when: 提取需要改变 owner/Service 生命周期、额外 device容器、恢复语义或 backing释放政策；返回 Plan。

### T10: 16-hart RAM rootfs 与 UART/GMAC 共存入口

- Requirement/Scenario: R3/R6/R7/R8/R9前置，S24/S26/S28/S31初始角色/S32。
- Depends on: T9。
- Targets: root/kernel `Cargo.toml`、`Makefile`、`.gitignore`、`kernel/src/{entry,lib}.rs`、`platform/mod.rs`；新增 `platform/k3_smp_combined.rs`、`tools/starry-k3-smp-combined.its`；共用普通 rootfs helper（可置于新 `platform/k3_rootfs.rs`）；`k3.rs::K3Env`、`mac_probe.rs::probe_and_report`、`k3_async_net.rs::board::{irq_reg_read,irq_reg_write}` 与最小 native MMIO read32/write32 模块 `platform/k3_mmio.rs`（新）；`tests/k3-uart-plane-host-harness.rs`、`tests/mac-probe-host-harness.rs`、`tests/k3-embedded-user-host/tests/k3_mmio.rs`（新）。
- Current behavior: 新目标和组合模块不存在，网络与 UART/rootfs 互斥；网络裸 PA指针在完整页表下不可沿用；paging时本地 axklib stub冲突。旧 rootfs启动链已交付但无固定初始角色或网络。
- Required behavior:
  - root feature `k3-smp-combined=[k3-smp,k3-rootfs,k3-async-net,starry-kernel/k3-smp-combined]`；kernel feature `[k3-smp,k3-rootfs,k3-async-net]`。不包含 k3-smp-net、k3-smp-uart、k3-stdio 或 k3-net-diagnostics。lib守卫只为新组合开放 net+uart/SMP例外，仍拒绝组合混入专项选择器/stdio/raw-port/诊断和其他平台。entry的新组合分支先于 rootfs/k3-smp，旧两个分支增加排除，不能同时执行。
  - 新Make默认16且只接受16，独立 `target/k3-smp-combined`、`.axconfig-k3-smp-combined.toml`、`out-k3-smp-combined`，MEM=6400M、BUS=mmio、LOG=off、IP=192.168.137.2/GW=192.168.137.1，沿既有PLAT_CONFIG。独立 FIT kernel load/entry=0x180000000，DTB=0x2ef000000，名称 `StarryOS_riscv64-k3-smp-combined.fit`；补 PHONY与OUT_DIR忽略规则，不依赖嵌入 shell ELF、不修改rootfs镜像。
  - 纯 `place_combined_roles` 消费升序、去重、范围合法的 schedulable集合和实际boot；entry还要求已发布16 harts。四后台角色取共享 place_roles、互异；boot须在集合，shell固定8须存在且不与四后台角色/alarm重叠。kernel从 drivers::placement单实例消费，host沿既有path include惯例。非法集合/角色失败即停，不改变 placement公式或虚构拓扑。
  - boot主任务在首个 TimerFuture前固定实际boot singleton；UART init/start_copiers一次，之后所有诊断走 k3_uart::diag_line；T9安装函数只调用一次，owner/runner用相同四角色分配。source42/60/133注册保持boot，不用无锁early console抢写活动UART。
  - 从旧rootfs分支提取 UART启动之后的普通 rootfs/loader/进程发布/join/fs-close/drain体，保留一份共用实现。旧单 hart入口仍init UART/copiers后调用，未指定affinity时沿原spawn/alarm API；新入口完成NIC后调用并指定shell8、alarmboot的入队前singleton。rootfs窗/DTB/header/initrdmem校验、唯一RamDisk/ext4、pseudofs/单tty、原args/envs、普通动态PT_INTERP加载、Process::new_init、stdio顺序与30秒结束drain保持；不构造memory root/embedded shell，不重新初始化已有UART/NIC。
  - paging新组合排除本地 k3_klib stub，使用axruntime provider；旧无paging网络仍编译该stub。native MMIO最小共同实现用 axhal::mem::phys_to_virt后执行read_volatile/write_volatile<u32>，K3Env与source133回调直接调用它，source60经K3Env覆盖。MAC identity probe改为接收既有ProbeEnv读取，两个生产调用者均传env；不再自行裸读PA。保持全部寄存器offset/读写序/采样格式与ISR无MDIO，不改DMA地址/cache算法、不新增动态映射或低位identity映射。
- Preserve: T9所有权；单UART port/copiers/tty，旧模式 feature隔离；rootfs预留在allocator前生效，RamDisk backing是校验过的保留窗，FS初始化一次；共享普通loader与初始进程发布语义；现有恢复、终态、四条件drain代码；不持久写入。
- Forbidden: 直接串接两个 `-> !` 入口、复制rootfs进程组装、第二tty/raw reader/Service、两份placement、axruntime/axmm外部依赖修改、修改rootfs布局/板载固件、引入网络资格载荷或测试判定层。
- First test:
  1. 在现有plane harness include组合生产模块并添加 `combined_` 用例：16-hart/boot0角色=共享policy、缺shell/非法集合拒绝、boot或shell碰撞拒绝；模块未存在时编译RED。`make k3-smp-combined SMP=16` 当前No rule RED。
  2. 在现有MAC harness经真实 `probe_and_report(env,emit)` 验证版本/debug的原读取序/分类及零写；旧签名无法消费env时RED。不要在host裸解引用设备PA。
  3. 现有embedded-user-host包新增 `k3_mmio` integration test，`#[path]` 引用native MMIO生产模块。只以host的 axhal::mem::phys_to_virt边界把测试PA映射到有效且不同地址的对齐u32 backing，实际执行生产read/write，观察读值与写入backing；缺模块RED。该模块必须由K3Env与真实IRQ回调消费，不能只测不被使用的地址计算器。
- GREEN condition: 新组合placement、MAC probe与MMIO定向测试通过；`make k3-smp-combined SMP=16` exit0且产出独立FIT；新目标SMP=1/17原生拒绝（同一精确16规则处理其余非16值）；旧rootfs与SMP net受影响目标仍可构建，根目录已有镜像不要求全矩阵重建。Gate3阅读调用边确认普通rootfs/所有权/paging provider/MMIO绑定。
- Verification: 使用下方命令表；新增测试只跑相关过滤项或新integration test，不重跑无关旧suite。rootfs布局/loader/stdio未变的语义采信前序GREEN；本task结束前完成新组合与直接受影响旧net/rootfs构建。之后T11仅改变共享clone内部的继承语句，用其真实调用测试和同一RISC-V组合构建覆盖，不再重复未变的Make/包装器检查。
- Stop when: 需要改变平台RAM边界、自动设备probe、通用映射/调度政策、MMIO宽度/寄存器协议、loader/Process发布语义，或不能保持单tty/唯一axklib；返回Plan。实际板上效果不在本轮编造。

### T11: 用户子任务首入队前继承 affinity

- Requirement/Scenario: R3/R7/R8/R9前置，S31及S26；最终工具真实使用归004。
- Depends on: T10。
- Targets: `kernel/src/syscall/task/clone.rs::CloneArgs::do_clone` 的共用发布点；`tests/k3-embedded-user-host/tests/clone_affinity.rs`（新），该包Cargo.toml/Cargo.lock仅增加测试所需既有依赖（dev-dependencies `bitflags="2.10"`、`linux-raw-sys="0.12"`，后者关闭default启用no_std/general，与kernel现有版本族一致）。`clone3.rs`/`execve.rs`/axtask只读核对，不改。
- Current behavior: `new_user_task`默认full mask，do_clone在设置task_ext后直接spawn_task；shell8创建的子任务可被入队到任一schedulable hart，TimerFuture轮归属不再固定。
- Required behavior: 在唯一 `spawn_task(new_task)` 之前，将现有parent `curr.cpumask()`继承给new_task，仍走既有spawn/add_task_to_table，不改变clone/clone3 ABI、flags、FD/FS/信号/aspace/fork/PID发布和返回errno。所有普通用户子进程及THREAD分支均继承父mask，exec保持当前task；不改变通用axtask::spawn默认策略或给所有内核任务继承affinity，不新增迁移/runtime机制。
- Preserve: 父mask本身、非singleton父mask的完整集合、子进程独立任务对象；已有clone3→do_clone调用链；先发布全部task_ext再入队的次序、clone3及exec的既有错误边界。
- Forbidden: 仅修改初始shell affinity或new_user_task全局默认来绕过clone发布点；仅测试复制mask的helper/源码字符串；对复制的do_clone实现测试；新增子进程probe镜像、QEMU专项或产品自检协议。
- First test: 在上述现有host包的一个integration test内 `#[path]` 直接include真实 `kernel/src/syscall/task/clone.rs`；bitflags/linux constants用现有依赖。内联最小OS替身仅满足当前Task/Process/FS/FD/用户context、VM写入和enqueue外部边界，不复制do_clone。fake当前parent有singleton8、fake新Task默认full16；调用真实 `CloneArgs::do_clone`：普通forkflags=empty与THREAD|VM|SIGHAND两条路径。在fake `spawn_task`捕获首次入队参数并断言child mask=parent，当前代码须行为RED（full16≠singleton8），不能以fake编译失败代替复现。另验证非singleton父mask原样继承且父不被改写；全部测试串行，fake只存在测试文件内，不建立独立fixture包/框架。
- GREEN condition: 同一真实do_clone调用在最小修复后全部GREEN；发布点收到父mask，父未修改，普通/线程共享路径都成立。exec代码不改mask的检查与clone3共用路径静态核对成立；组合目标在该共享修改后构建成功。T10的旧包装器检查继续采信。
- Verification: 运行下方 `--test clone_affinity -- --test-threads=1`，随后构建最终组合一次；如只做约定的clone继承修复，不重复未改的旧入口/Make检查。构建/host结论只证明本轮研发目标，不能写成BusyBox工具或真板SMP最终验收。
- Stop when: host用例不能调用真实do_clone而必须复制生产逻辑、需要改变Process/fork事务、父mask继承需新调度政策、或实际异常要求改timer runtime；停止并交Plan，不以增加测试框架解决。

**Invariants**

1. UART RX/TX copier、NIC owner/runner各唯一；rootfs/pseudofs/tty只初始化一次。IRQ在boot注册，跨hart调用用既有平台门。
2. packet/descriptor/cookie/epoch/PHY ack/cache契约不变，已发布DMA backing不得随提取函数返回或初始化失败释放。
3. 主任务先pin；alarm/初始shell首次入队前pin；clone后代继承父mask，exec不换mask。既有deadline wheel不迁移。
4. 活动UART之后只用既有序列化安全诊断；新入口不启动raw reader、内嵌最小shell或网络资格应用。
5. RAM rootfs/DTB保持既有高位预留窗；普通动态loader加载文件系统中的 `/bin/sh`，无fallback、无自动持久写入。
6. 遵守本次已批准的验证缩减：保留定向TDD/构建，不恢复独立恢复、长稳、历史全套回归或身份/判定工具。

**Non-goals**

工具/HTTP/对端实际通信与最终真板验收（004）；修复未阻塞最终目标的ISS02/ISS03；IRQ/DMA硬件新bring-up、性能、真实存储、ICMP/DNS/TLS/IPv6、通用驱动或迁移框架。既有完整行为不因取消验证而删除。

**Acceptance**

| ID | 覆盖 | 本轮直接判据 |
|---|---|---|
| A13 | R6/R7/R8、T9 | 可返回的生产NIC安装被旧包装器和新入口共同消费，唯一owner/runner、handle/backing与失败停止保持；旧真实入口构建成功，资源调用链自检一致 |
| A14 | R3/R7/R8/R9前置、T10 | 组合placement与MAC/MMIO定向先行测试GREEN；新feature/单入口/paging provider/共用rootfs链成立；SMP16组合构建产出独立高位DTB FIT，非16拒绝；旧rootfs/network构建不回归 |
| A15 | R3/R7/R8/R9前置、T11 | 真实do_clone的普通/THREAD路径观察到首次入队mask继承，非singleton原样继承且parent保持；行为RED→GREEN，clone3/exec消费者核对一致 |

不设置本轮板测Acceptance。A13–A15只构成可构建且做过必要定向测试的研发基线；最终UART/GMAC正常使用必须由004在真实BusyBox入口取得。

**Verification**

按任务顺序执行，修改前采信的GREEN来源已写在事实和契约中；本计划不先运行这些产品命令。命令中的输出均用直接测试/构建结果判定，不新增判定脚本。

| 阶段 | 原生命令/操作 | 通过条件 |
|---|---|---|
| T9 | `make k3-smp-net SMP=16` | 提取后旧入口编译/链接/FIT成功；其源码仍接原载荷 |
| T10 先行/构建 | `make k3-smp-combined SMP=16` | 新目标存在前观察No rule RED；本task实现后同命令GREEN，闭合该task再进入T11 |
| T10 placement | `rustc --edition=2024 --test tests/k3-uart-plane-host-harness.rs -o /tmp/ms14-combined-roles`，再执行 `/tmp/ms14-combined-roles combined_` | 新模块缺失RED；只运行新增组合用例，GREEN |
| T10 MAC probe | `rustc --edition=2024 --test tests/mac-probe-host-harness.rs -o /tmp/ms14-combined-mac`，再执行 `/tmp/ms14-combined-mac identity_probe_` | env接口先行RED；新增真实probe用例GREEN，read顺序/分类及无写成立 |
| T10 MMIO | `env RUSTFLAGS='-C linker=/home/daivy/projects/serial/work/StarryOS/scripts/cc-nopie.sh' cargo test --manifest-path tests/k3-embedded-user-host/Cargo.toml --offline --test k3_mmio -- --test-threads=1` | 生产native MMIO在替换后的phys_to_virt边界上读写正确backing；无设备PA裸解引用 |
| T11 clone | 同上cargo命令，将 `--test k3_mmio` 换为 `--test clone_affinity` | fake环境可执行真实do_clone，先观察full mask错误RED，再观察修复GREEN；无需运行其他integration suites |
| T10 直接兼容 | `make k3-rootfs`；`make k3-smp-net SMP=16` | 两个实际受影响旧入口exit0；T9之后表面已受T10 MMIO修改，因此此处是修改后必要检查 |
| T10 选择边界 | `make k3-smp-combined SMP=1`、`make k3-smp-combined SMP=17` | 1/17原生拒绝；Make未再改变时不在T11重复 |
| T11 修改后最终目标 | `make k3-smp-combined SMP=16` | 共享clone已修改，重新构建组合/FIT exit0；不通过配置声明代替硬件执行结论 |
| Gate3/4 | 每task自检、本轮完整修改diff自检与任务状态核对 | 符合Targets/Preserve/Forbidden；只完成有实际结果的任务 |

昂贵验证预算：本轮0次真板、0次QEMU runtime、0次全套回归。现有host/GREEN未改变表面采信；只对新增/改动路径运行上表定向测试。若T10/T11后有进一步修复，只复核失效的受影响检查。跨设备真实用户入口已明确预算到004，本轮不得宣称其通过。

**Requirements Traceability Matrix**

| Requirement | Task | First Test | Status |
|---|---|---|---|
| R3 固定角色/用户后代 | T10,T11 | combined_ placement；真实do_clone首入队mask行为RED | Covered |
| R6 既有恢复/安全边界 | T9,T10 | 001有效GREEN采信；生产安装/所有权调用链与旧入口构建 | Simplified（已批准取消独立恢复资格，保留安全代码） |
| R7 双设备最终组装的研发部分 | T9–T11 | 组合目标/模块缺失RED；MMIO/probe/clone定向测试 | Covered（最终使用归004） |
| R8 定向兼容/RAM-only | T9–T11 | 旧入口有效GREEN；改动后直接构建与原布局不变 | Simplified（已批准取消完整历史/逐轮回滚专项） |
| R9 普通rootfs载体前置 | T10,T11 | 原rootfs/loader/stdio有效GREEN；组合与clone先行测试 | Covered（wget/nc真实交换归004） |

R1/R2/R4/R5继续沿用000–002 accepted，不在本轮新增实现或重复专项。

**Gate 2 Readiness**

Gate 2 PASS：技术检查已完成，用户2026-10-16原话“批准”，本份计划获批，Status改为ready交Act；本轮限T9–T11，无真板专项。

**Persisted Evidence**

- Mode: none。定向RED/GREEN、构建退出码、修改符号、Gate3与自检结果写Act Response足够；不创建evidence目录，不采集硬件现场。

**Risks and Notes**

- 003不包含硬件使用结论；004仍须用户运行最终镜像。当前host工具清单只证明现有BusyBox带wget/nc，不证明它们在StarryOS上可运行。
- host clone用例替换的是不可在宿主启动的OS边界，生产do_clone必须直接include/调用；这不替代实际进程/驱动组合验收。测试fakes局限单个测试文件，无产品接口、单独CLI或fixture框架。
- 最小native MMIO模块用于多个实际调用者，不能为测试造一份不被消费的适配；offset、32位访问和原寄存器协议保持。
- 当前暂存集含前序产物与其他用户改动；不清理、不unstage、不代用户提交。历史Cycle及其Review不改写；本轮只在本Cycle的Act Response记录实施结果。

## Act Response

- Status: reported
- Date: 2026-10-16

### 实际改动

**T9 可返回的单实例 NIC 安装（4.1）**

- `kernel/src/platform/k3_async_net.rs`：`SMP_NET_HANDLES`、`smp_fault`、`smp_singleton`、`install_smp_net` 的 cfg 从 `feature = "k3-smp-net"` 扩为 `any(feature = "k3-smp-net", feature = "k3-smp-combined")`；新增组合侧 export `pub use board::install_smp_net`（cfg `all(k3, riscv64, k3-smp-combined)`）。001 T5 交付的 `install_smp_net` 本身即满足契约（接收 caller placement 的 owner/runner 与输出回调、stage4–7 硬件 gate → mask/unregister → arm → 18 页 arena → Gmac init → register133 → arm_phy_source60 → adapter → init_network → owner Active 5 s 上限 → runner 首 poll 5 s 上限 → 返回 arena VA；handle 留在 `SMP_NET_HANDLES`，backing 由 guard 保留，不启动应用/见证）。`run_k3_smp_net_boot` 一字未动，仍消费同一函数并叠加 duplicate-start 见证与诊断载荷。

**T10 组合入口与 MMIO 地址转换（4.2）**

- 新增 `kernel/src/platform/k3_mmio.rs`：`read32`/`write32`，唯一转换点 `axhal::mem::phys_to_virt` + 32 位 volatile；无低位映射、无动态映射。
- `kernel/src/platform/k3.rs`：`K3Env::mmio_read/mmio_write` 改调 `k3_mmio`（覆盖 source60 GPIO/MFPR ack 的全部 K3Env 调用边）；`run_first_byte_park` 的 `probe_and_report` 改传 `&mut env`。
- `kernel/src/platform/mac_probe.rs`：`probe_and_report` 从 `unsafe fn(emit)` 改为 `fn(&mut impl ProbeEnv, emit)`，读序/采样次数/分类/格式/只读语义不变；不再自持裸 PA 解引用。
- `kernel/src/platform/k3_async_net.rs`：`board::{irq_reg_read, irq_reg_write}`（source133 ISR 的 `BOARD_IRQ_ACCESS` 回调）改调 `super::super::k3_mmio`；`install_smp_net` 内 identity probe 调用同步更新。
- 新增 `kernel/src/platform/k3_smp_combined.rs`：纯 `place_combined_roles`（共享 `place_roles` 四后台角色互异、boot 须在集合、shell=8 须存在且不与四后台/alarm 重叠，6 个 fail-closed 错误变体）+ `run_smp_combined_boot`（placement → 16-hart kernel 侧 backstop → 主任务首 TimerFuture 前 pin boot → `init_uart_hardware`/`start_copiers` 一次 → `install_smp_net` 一次（owner/runner 取同一共享 policy，诊断走 `k3_uart::diag_line`）→ 共用 rootfs 体传 shell8/alarm-boot 固定角色）。
- 新增 `kernel/src/platform/k3_rootfs.rs`：`run_ram_rootfs_shell(args, envs, Option<FixedRootfsRoles>)`——MS18 验收链（DTB pointer/header/initrdmem 校验、唯一 RamDisk/ext4、pseudofs、alarm、`load_user_app(None,…)` 动态 PT_INTERP 加载、Process::new_init、stdio、join、fs-close、30 s drain）单份实现；`None` 沿原 `spawn_task`/`spawn_alarm_task`，`Some` 走 `spawn_task_with_affinity`/`spawn_alarm_task_with_affinity` 入队前 singleton。panic 分层标签保持 `[ms18] fault stage=…` 原状。
- `kernel/src/platform/mod.rs`：注册 `k3_mmio`（cfg k3）、`k3_rootfs`（cfg riscv64+k3-rootfs）、`k3_smp_combined`（cfg riscv64+k3-smp-combined）；`k3_klib` 改为 `all(k3-async-net, not(k3-smp-combined))`——paging 组合由 axruntime 提供唯一 axklib provider，旧无 paging 网络仍编译 stub。
- `kernel/src/lib.rs`：`k3-async-uart+smp` 与 `k3-async-net+smp` 两守卫增加 `not(k3-smp-combined)` 例外；`k3-async-net+k3-async-uart` 互斥守卫增加组合例外；新增守卫：组合模式拒绝 `k3-smp-net`/`k3-smp-uart`/`k3-smp-uart-raw`/`k3-stdio`/`k3-uart-port-test`/`k3-net-diagnostics`。
- `kernel/src/entry.rs`：新增组合分支（先于 k3-rootfs/k3-smp）；k3-rootfs 分支改为 init UART/copiers + begin 行后调用共用体（`None` 角色）；k3-smp 分支增加 `not(k3-smp-combined)` 排除；删除被提取体孤儿的 k3-rootfs import 块。
- 根 `Cargo.toml`/`kernel/Cargo.toml`：`k3-smp-combined = [k3-smp, k3-rootfs, k3-async-net]`（+ root 侧 `starry-kernel/k3-smp-combined`），UART42 路由经 k3-rootfs→k3-uart 闭包带入。
- `Makefile`：`k3-smp-combined` 目标（精确 SMP=16、隔离 `target/k3-smp-combined`/`.axconfig-k3-smp-combined.toml`/`out-k3-smp-combined`、MEM=6400M/BUS=mmio/LOG=off/IP=192.168.137.2/GW=192.168.137.1）+ `k3-smp-combined-fit` + PHONY；`.gitignore` 补 `out-k3-smp-combined/`；新增 `tools/starry-k3-smp-combined.its`（kernel load/entry `0x180000000`，FDT `0x2ef000000`）。

**T11 子任务首入队前继承 affinity（4.3）**

- `kernel/src/syscall/task/clone.rs`：`do_clone` 在 `spawn_task(new_task)` 前新增 `new_task.set_cpumask(curr.cpumask())`（唯一共用发布点，覆盖 sys_clone/sys_clone3/clone3；fork 与 THREAD 分支同享；clone ABI/flags/FD/FS/信号/fork/errno 未动；execve.rs 无 cpumask 引用，静态核对成立）。
- 新增 `tests/k3-embedded-user-host/tests/clone_affinity.rs`：`#[path]` 直接 include 生产 clone.rs，最小 OS 替身仅替换 Task/current/spawn（首入队捕获）、ProcessData/Aspace/VM 写、FD/FS 表视图与 UserContext；fake 父 singleton8、新任务默认 full16；三用例（fork、THREAD|VM|SIGHAND、非 singleton {2,3} 原样继承 + 父不被改写）。
- `tests/k3-embedded-user-host/Cargo.toml`：dev-deps 增加 `bitflags = "2.10"`、`kspin = "0.1"`、`linux-raw-sys = { version = "0.12", default-features = false, features = ["no_std","general"] }`（与 kernel 版本族一致；Cargo.lock 随之更新）。

### 与计划的偏差

1. 先行测试文件在工作树中已存在（`tests/k3-embedded-user-host/tests/k3_mmio.rs` 与两份 harness 的新增用例，内容与本 Plan First-test 规格逐条一致），本 Cycle 采纳为先行测试并完成 RED→GREEN；非新写。
2. T11 dev-deps 除契约点名的 bitflags/linux-raw-sys 外增加 `kspin = "0.1"`：真实 clone.rs 的 `use kspin::SpinNoIrq` 必需，属"测试所需既有依赖"。
3. `FixedRootfsRoles` 带 `Copy`（两处 match 消费）；组合错误枚举命名/拆分为 Act 局部选择（契约允许）。
4. 验证预算内的补充运行及理由：`make k3-smp-combined SMP=16` 共三次通过运行（T10 GREEN、T11 修改后最终目标——验证表明文要求、touch 强制重建的警告定向审计）；包内全套测试一次（dev-deps 改动影响该包整体构建图，属直接受影响面）。无自愈式重跑。

### 自检发现与遗留 Minor

- 已修复：组合构建首轮暴露两处编译错（`FixedRootfsRoles` 缺 Copy、`board` 模块内 `super::k3_mmio` 少一层路径），当轮修复并重跑受影响构建。
- 已修复：fake `FakeProc::parent` 初版误写 `AxResult`，真实 `starry_process 0.2.0` 为 `Option`（starry-process-0.2.0/src/process.rs:56）；改为 `Option` 后通过。排查中发现 **nightly-2026-02-25 的 core 已移除 `Result::ok_or`**（`Option::ok_or` 仍在），E0599 表面极具误导性——后续任何 host fake 或内核新代码不得使用 `Result::ok_or`（工作区现有代码均走 `Option::ok_or`，无现患）。
- 遗留 Minor（不阻塞）：依赖 crate 既有警告（axnet-ng/smoltcp/virtio-drivers/cargo config 弃用提示）数量与内容未变化；`starry-kernel` 自身零新增警告（touch 强制重建审计）。本报告不宣称任何真板最终使用结果；BusyBox wget/nc 真实交换与最终形态验收归 004。

### 验证命令与实际结果

| 阶段 | 命令 | 实际结果 |
|---|---|---|
| T10 先行 RED | `make k3-smp-combined SMP=16` | `No rule to make target 'k3-smp-combined'` |
| T10 先行 RED | `rustc --edition=2024 --test tests/k3-uart-plane-host-harness.rs …` | `k3_smp_combined.rs: No such file or directory` |
| T10 先行 RED | `rustc … --test tests/mac-probe-host-harness.rs …` | `E0061: this function takes 1 argument but 2 were supplied`（env 签名缺失） |
| T10 先行 RED | `cargo test … --test k3_mmio` | `k3_mmio.rs: No such file or directory` |
| T11 先行 RED | `cargo test … --test clone_affinity -- --test-threads=1` | 3/3 行为失败：首入队 mask `left: bits 65535 (full16)` vs `right: bits 256 (singleton8)` |
| T10 placement GREEN | `rustc … -o /tmp/ms14-combined-roles && /tmp/ms14-combined-roles combined_` | 3 passed; 0 failed |
| T10 MAC probe GREEN | `rustc … -o /tmp/ms14-combined-mac && /tmp/ms14-combined-mac identity_probe_` | 1 passed; 0 failed（读序/version/debug 分类/零写成立） |
| T10 MMIO GREEN | `cargo test … --test k3_mmio -- --test-threads=1` | 1 passed（PA 先经 phys_to_virt 到替换 backing 才 volatile 访问） |
| T10/T9 构建 GREEN | `make k3-smp-combined SMP=16` | exit 0；`StarryOS_riscv64-k3-smp-combined.fit` 产出（kernel load/entry `0x180000000`，FDT `0x2ef000000`，`dumpimage -l` 核对） |
| T10 选择边界 | `make k3-smp-combined SMP=1` / `SMP=17` | 均原生拒绝：`k3-smp-combined runs exactly SMP=16, got SMP=1/17`，exit 1 |
| T10 直接兼容 | `make k3-rootfs`；`make k3-smp-net SMP=16` | 均 exit 0，各自 FIT 正常产出 |
| T11 GREEN | `cargo test … --test clone_affinity -- --test-threads=1` | 3 passed; 0 failed |
| T11 包回归 | `cargo test … -- --test-threads=1`（全包） | clone_affinity 3 + contract 9 + k3_mmio 1 + ram_rootfs 21 = 34 全过 |
| T11 最终目标 | `make k3-smp-combined SMP=16`（clone 修复后） | exit 0，FIT 重新产出 |
| Gate 3 警告审计 | `touch kernel/src/lib.rs && make k3-smp-combined SMP=16` | starry-kernel 零警告；仅依赖 crate 既有警告 |
| Gate 3 静态核对 | grep 核对 | `clone3.rs:93 → do_clone`；`execve.rs` 无 `cpumask`；`probe_and_report` 三处调用点全部 env 化 |

### Persisted Evidence

None required（Plan Mode: none；以上命令结果足以支撑 A13–A15 的研发判据，无白名单情形）。

### Experience Candidates

None（`Result::ok_or` 移除为工具链事实而非项目缺陷，已记入自检发现供后续引用；无 Runbook 候选）。

### 未解决问题

无阻塞。004 展开依赖本 Iteration Review `accepted`；最终真板 BusyBox wget/nc 正常使用验收按计划由 004 承载，本轮不作任何声明。

## Plan Review

- Review Result: accepted

Reviewed 2026-10-16。独立阅读 003 全部变更面。本轮改动经未暂存 diff 精确隔离（`k3_async_net.rs` 31/19、`entry.rs` 36/187、`lib.rs` 37/8、`k3.rs` 6/7、`mac_probe.rs` 12/11、`platform/mod.rs` 25/2、`clone.rs` 8/0、root `Cargo.toml` +15、kernel `Cargo.toml` +9、`Makefile` 29/1、`.gitignore` +1、`tests/k3-embedded-user-host` Cargo +12/lock +52）加未跟踪新文件（`k3_mmio.rs`、`k3_smp_combined.rs`、`k3_rootfs.rs`、`tests/.../clone_affinity.rs`）与暂存区先行测试（`k3_mmio.rs` 测试、两 harness 新用例，偏差 1 采纳项）。Act 报告的 RED→GREEN 序列、三次组合构建 exit 0、SMP=1/17 原生拒绝、包内 34 测试、touch 强制重建零新增警告按公共规则采信（只读 git 检查确认覆盖未失效），未重跑。

**声明与代码一致性（独立检查通过）**

- T9：未暂存 diff 中 `run_k3_smp_net_boot` 零改动，"一字未动"成立；`SMP_NET_HANDLES`/`smp_fault`/`smp_singleton`/`install_smp_net` 四符号 cfg 从 `k3-smp-net` 扩为 `any(k3-smp-net, k3-smp-combined)`，新增组合侧 export（`k3_async_net.rs:1743-1748`）。`install_smp_net` 保持 stage4–7 gate → mask60/133/277+unregister133 → arm → 18 页 arena 清零 → `Gmac::new_async`/init_rx/enable_tx → register133 → arm_phy_source60（失败仅降级 note）→ adapter → `init_network(from_one)` → owner 5 s Active 上限 → runner 首 poll 5 s 上限 → 返回 arena VA；handle 入 `SMP_NET_HANDLES`、backing 由 guard 保留、失败一律 `smp_fault` park。旧包装器调用它后叠加 duplicate-start 见证与诊断载荷（1674 起）。
- T10 MMIO：`K3Env::mmio_read/write`、`board::{irq_reg_read,irq_reg_write}`（source133 `BOARD_IRQ_ACCESS` 回调）与 `probe_and_report` 三条生产调用边全部经 `k3_mmio::read32/write32`（`axhal::mem::phys_to_virt` + 32 位 volatile），无低位映射、无动态映射；`probe_and_report` 改为 `fn(&mut impl ProbeEnv, emit)`，读序/`READS_PER_REG` 采样/分类/只读语义未变，两个生产调用者（`k3.rs:110`、`k3_async_net.rs:1437`）均传 env；`run_first_byte_park` 同步传 `&mut env`。
- T10 入口唯一性：组合闭包（k3/k3-smp/k3-async-uart/k3-uart/k3-async-net/k3-rootfs，不含 k3-stdio）下逐分支推导——data-plane 分支被 `not(any(k3-stdio, k3-rootfs))` 排除、stdio 分支 k3-stdio 不在、k3-rootfs 分支被 `not(k3-smp-combined)` 排除、raw bringup 被 `not(k3-uart)` 排除——组合分支是唯一命中，`run_smp_combined_boot` 接管。lib.rs 三处既有守卫各加组合例外，新增组合守卫拒绝 k3-smp-net/k3-smp-uart/k3-smp-uart-raw/k3-stdio/k3-uart-port-test/k3-net-diagnostics。`k3_rootfs.rs` 与 MS18 原 entry 分支逐段同序（dtb pointer/header/parse → initrdmem/extent → mount → pseudofs → resolve → uspace → `load_user_app(None)` → `Process::new_init` → stdio → spawn → join → fs-close → 30 s drain），`[ms18] fault stage=…` panic 标签原样，`fixed` 差异点仅在 alarm/shell 的入队前 singleton 与两条 `[ms14]` 观察行；旧 k3-rootfs 分支 init UART/copiers + begin 行后传 `None`。
- T10 构建面：root `k3-smp-combined=[k3-smp, k3-rootfs, k3-async-net, starry-kernel/k3-smp-combined]`、kernel 侧 `[k3-smp, k3-rootfs, k3-async-net]` 逐字符合契约；Make 目标精确 `-ne 16` 拒绝（消息与 Act 一致）、TARGET_DIR/OUT_CONFIG/OUT_DIR 全隔离、MEM=6400M/BUS=mmio/LOG=off/IP/GW 如约、`.PHONY` 含两新目标；ITS kernel load/entry `0x180000000`、FDT `0x2ef000000`；`mod.rs` 的 `k3_klib` 排除 `k3-smp-combined`（paging 下 axruntime provider 唯一），三新模块注册正确（`k3_mmio` cfg k3、`k3_rootfs` cfg k3-rootfs、`k3_smp_combined` cfg 组合）。
- T10 组合体：`place_combined_roles` 四后台角色全部取共享 `place_roles` 实际返回并查两两互异，boot 须在集合、shell 固定 8 须存在且不与后台/alarm 重叠，6 个 fail-closed 变体；kernel 侧 `pub use crate::drivers::placement` / host 侧 `#[path]` 原位 include 双路单实例，`placement.rs` 零改动。`run_smp_combined_boot`：placement + 16-hart backstop → 主任务首个 TimerFuture 前 `set_current_affinity(boot)` → `init_uart_hardware`/`start_copiers` 一次 → `install_smp_net` 一次（owner/runner 同一共享 policy，诊断走 `k3_uart::diag_line`）→ `run_ram_rootfs_shell(Some(...))`；`pre`（ax_println）仅在 copiers 启动前使用，与 002 已验收 `run_smp_uart_shell_boot` 同型。
- T11：`clone.rs` 内唯一 `spawn_task` 发布点前新增 `new_task.set_cpumask(curr.cpumask())`（282/284 行），fork 与 THREAD 分支同享；`clone3.rs:93 → do_clone`、`execve.rs` 无 `cpumask` 静态核对成立。`clone_affinity.rs` 以 `#[path]` include 生产 clone.rs，fake 仅替换 OS 边界（task/current/spawn、ProcessData、FD/FS、UserContext、VM 写），`spawn_task` 捕获首入队 mask；fresh 任务默认 full16 vs fake 父 singleton8，缺失继承时是行为 RED 而非编译失败；三用例（fork、THREAD|VM|SIGHAND、非 singleton {2,3} 原样继承 + 父不被改写）与 First test 规格一致。
- 新测试核对：`k3_mmio.rs` 测试只替换 `axhal::mem::phys_to_virt` 边界、断言 PA≠backing 后实际执行生产 read/write；plane harness 3 个 `combined_` 用例（共享 policy+固定 shell8/alarm、非法集合/缺 boot/缺 shell、后台与 shell 碰撞 + boot=9 有效对照）；mac harness `identity_probe_` 用例断言 env 读序（version×2/debug×2）、分类、mmio_write panic（零写）。
- Invariants 1–6 成立：copiers/owner/runner/rootfs/tty 各一次；descriptor/cookie/epoch/cache 契约零改动（`gmac`/`k3_network_probe` 不在本轮 diff）；主任务先 pin、alarm/shell 入队前 pin、clone 后代继承；活动 UART 后诊断走 `diag_line`；RAM rootfs 高位窗与既有校验原样；无恢复/长稳/历史全套回归被恢复执行。
- Forbidden 项零违反：未串接两个 `-> !`、未复制 rootfs 进程组装（单份实现）、无第二 tty/raw reader/Service/placement、未动 axruntime/axmm 外部依赖、未改 rootfs 布局、T11 未绕过发布点、未测试复制实现。无身份型证据机制、无判定层；Evidence Mode none 被遵守（无 evidence 目录）；无 TODO/FIXME 残留。

**Findings**

1. [基线滞后 / 不阻塞] Plan Context T9 的 Current behavior（"唯一 SMP NIC 安装只能通过永不返回的专项入口运行"）相对 001 实际交付过时：`install_smp_net` 在 001 T5 已是可返回函数（旧包装器自那时消费它）。Act 发现后按契约终态仅做 cfg 扩展 + export，行为面与契约一致，Act Response 与 tasks.md 4.1 已如实记录。分类为 Plan 调查基线滞后，A13 终态判据（旧包装器与新入口共同消费）不受影响。
2. [Minor 注释 / 不阻塞] `entry.rs` 组合分支注释 "Selected before the k3-rootfs and k3-smp branches" 与物理顺序（组合分支位于 k3-rootfs 分支之后）不符——唯一命中由 cfg 排除保证（上已独立验证）。语义歧义无行为影响，不改。
3. [Minor 观察 / 不阻塞] `k3_device_irq_probe.rs:661/667` 仍含裸 PA volatile（cfg `k3`，组合模式参与编译），但其执行路径仅属 MS10/MS13 诊断模式入口，组合分支不可达；本轮契约 MMIO 范围明确为三条调用边。留档，不需要处理。
4. [Minor 风格 / 不阻塞] `k3_smp_combined.rs` 的 `|l| diag(l)` 等价 `diag`；`k3_rootfs.rs:201` `spawned.unwrap()` 在 `is_none` panic 之后冗余。零行为影响，仅记录。
5. [提交集卫生 / 用户决策，非本轮偏差] 本轮四个新文件未跟踪、`k3_mmio.rs` 测试与两 harness 用例在暂存区（偏差 1 的既存事实）；`/.axconfig*` 规则覆盖新 `.axconfig-k3-smp-combined.toml`。暂存集仍含前序产物（`.axconfig-k3-smp.toml` 等），与 000–002 处理一致，提交归用户决策。

**Acceptance Gaps**: None。A13（可返回安装被两消费方共同使用、唯一 owner/runner、handle/backing/失败停止保持、旧入口构建成功）、A14（placement/MAC/MMIO 定向 GREEN、feature/单入口/paging provider/共用 rootfs 链、SMP16 独立高位 DTB FIT、非 16 拒绝、旧 rootfs/network 构建不回归）、A15（真实 do_clone 普通/THREAD 首入队 mask 继承、非 singleton 原样继承、clone3/exec 核对一致）全部满足；研发基线边界（不宣称真板最终使用）与计划一致。

**Evidence**

- 独立阅读上列全部 diff 与新文件；分支唯一性经 feature 闭包逐分支推导；`run_k3_smp_net_boot` 未动经未暂存 diff 证实；clone3.rs:93 与 execve 无 cpumask 经 grep 核对。
- 采信来源：Act Response 验证命令表（T10 三项 RED、T11 3/3 行为 RED、四组 GREEN、`make k3-smp-combined SMP=16` exit 0+FIT、SMP=1/17 拒绝、`make k3-rootfs`/`make k3-smp-net SMP=16` exit 0、包内 34 测试、touch 重建零警告、grep 静态核对），覆盖未失效。

**Follow-up Decision**: 仅有非阻塞 findings（基线滞后说明、注释歧义、不可达代码观察、风格项、提交集卫生），A13–A15 全部满足，无实质问题或 Acceptance gap → `accepted`，不要求当前 Cycle 修复。Iteration Plan 不变。

**Next Cycle**: None

**Next Iteration**: 004（wget/nc 接通与最终正常使用，T12–T13）保留在 Map 中、本轮不展开——用户本轮指令为"进行审计"；展开时由 Plan 按 Iteration 004 的展开检查建立新 Plan Context 并重新过 Gate 2。
