# Iteration 000 / Cycle 000: RAM rootfs 挂载与文件读取

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: 1.1、1.2。
- Acceptance gaps / Repair items: None。
- Inherited scope: 已批准的 RAM rootfs 普通 shell 范围；单 hart、临时启动、官方存储与持久环境保持完整。
- Excluded scope: 本轮不 spawn shell；动态 ELF、命令组合与最终恢复交后续轮。不接入真实存储/网络/SMP，不修改 MS12 契约或全局路线。

**Objective**

K3 从临时加载且已保护的 RAM image 挂 ext4，通过 VFS 解析并读取实际 BusyBox/musl 文件；缺范围参数明确拒绝；完成输出后按既有 drain/park 收尾。此结果只证明 RAM block/VFS 路径。

**Investigation Facts**

- Current Baseline: `9a75c9f3`，k3，调查开始工作树 clean。MS17 accepted 的单 hart timer/AIA、UART copier/tty、paging/静态用户态构建与板测结论沿用，覆盖未变，未重跑。来源为 `openspec/changes/archive/2026-10-08-ms17-com260-kit-async-uart-interactive-shell/` 最终各轮反馈/Review；该结论不覆盖新 RAM/ext4 或动态用户态。
- `src/main.rs::main` 把 `/bin/sh -c src/init.sh` 传给 `entry::init`；K3 stdio 入口忽略 args，从 embedded ELF 启动。`k3-uart` 同时打开 paging/task-ext 与 file/mm/pseudofs/syscall/task 模块。
- `entry::init` 的 K3 data-plane 分支条件为 k3-async-uart + k3-uart + !k3-stdio，会启动 copier 后进入 raw qualification。新模式必须在该条件中排除，并拥有唯一 boot 分支；新模式不依赖 k3-stdio。
- `crates/axplat-riscv64-k3/src/mem.rs::phys_ram_ranges` 为 kernel base 0x180000000 到安全上界 0x2f0000000，reserved 目前为空。`axruntime::rust_main` 先读 `axhal::mem::memory_regions`，然后 init_allocator，再建 paging，之后 entry；axhal 从 RAM ranges 减去 reserved 后给 allocator FREE ranges。
- 初始 boot 页表只映射 kernel 所在 1 GiB 块及 UART；rootfs 所在高 RAM 只能在完整 paging 后访问。DTB 0x181000000 在初始 kernel 块内；axhal::init_early 已保存传入的物理 DTB arg。
- 原生只读检查 `stat`：kernel `.bin` 934080 B，DTB 150998 B，stock rootfs 1073741824 B，exit 0。rootfs ext4 在 byte0，无 GPT；`debugfs stats` clean/4096 B block，`stat /bin/sh` symlink→`/bin/busybox`，exit 0。BusyBox 825088 B、musl 613016 B；readelf 确認 BusyBox ET_DYN、RISC-V、PT_INTERP `/lib/ld-musl-riscv64.so.1`，exit 0。
- `axfs::init_filesystems` 消费 `AxDeviceContainer<AxBlockDevice>::take_one`，`fs::new_default` 在 ext4 feature 下选 lwext4，随后 ROOT_FS_CONTEXT.call_once；不存在 GPT/bootargs selector。坏 FS 走已有初始化 expect；不替换错误恢复语义。
- `pseudofs::init_memory_root` 也写 ROOT_FS_CONTEXT，禁止在新模式调用；`mount_all` 为第二轮使用。首轮不构造 tty、不启动 raw RX reader。
- 当前依赖已有 `axdriver_block 0.1.4-preview.3::ramdisk_static::RamDisk`，持有 `'static mut [u8]`，new 要求基址/长度 512 对齐；读写在同一 backing slice，flush 即完成。启用 axdriver/dyn 后 AxBlockDevice 为 Box<dyn BlockDriverOps>，可用 from_one；不要调用另一套驱动 runtime。
- `axruntime` 的 fs-ng feature 会在 entry 前自动 probe/mount。本模式仅直接启用 kernel 的 axfs/ext4 与 block dependency，不能启用 axfeat/fs-ng-ext4。
- `axhal::dtb` 用已保存 arg→phys_to_virt→Fdt::from_ptr；为避免未检查 pointer/totalsize 或未预留 FDT 被覆盖，本模式先验证 pointer 在 `[0x181000000,0x182000000)`，再校验 header/长度，用已有 fdt-parser 读取有界 bytes 与 /chosen bootargs。该 crate 已在 lock 中；raw_value 可避免其 str() 对无效数据的 unwrap。
- 已有 host harness `tests/k3-embedded-user-host` 通过 path 引入生产纯逻辑；本轮复用该测试入口，不另建判定器。现有经验命令使用 scripts/cc-nopie.sh host linker。

**Code and Critical Path**

U-Boot 两次 fatload→临时 bootargs `initrdmem=0x240000000,0x40000000`→axhal 保存 DTB arg→平台 reserved ranges 在 allocator 前排除 RAM rootfs+DTB→完整 RAM paging→K3 新 entry→校验范围→唯一 static-backed RamDisk→单设备 container→axfs/ext4 ROOT_FS_CONTEXT→FS_CONTEXT resolve/symlink/read→native console 文件结果→30 s 物理 drain/park。

**Task Contracts**

### 1.1: 隔离模式、启动范围与 RAM 保护

- Requirement/Scenario: S2/S5/S9；D1/D3。
- Depends on: None。
- Targets: root/kernel/platform Cargo features；`crates/axplat-riscv64-k3/src/mem.rs::reserved_phys_ram_ranges`；新生产纯 RAM layout/extent 校验模块；`kernel/src/lib.rs`/`src/main.rs` guards；Makefile 新 k3-rootfs target；现有 host harness 的新增测试。
- Current behavior: 无 RAM image reservation/boot range consumer，k3-uart 会进入 raw qualification。
- Required behavior: 新 k3-rootfs 依赖 k3-uart，平台仅此模式预留 `[0x240000000,0x280000000)` 与 `[0x181000000,0x182000000)`；rootfs 起点必须为 0x240000000，长度 >0、512 对齐、checked end ≤0x280000000。从 /chosen bootargs 消费唯一 initrdmem 十六进制地址/长度项；缺失、重复、非法数字、溢出或越界明确失败，失败前不构造内存 slice。DTB 指针/header/totalsize 在已预留 1 MiB 内且合法，然后借已有 parser 消费有界数据。不得在初始页表阶段访问 rootfs。
- Required behavior: make target 单 hart，拒绝 SMP>1，拒绝与 k3-stdio/k3-shell/raw port qualification 组合；保留已有跨平台 guards。kernel .bin/FIT staging 均 <16 MiB，DTB <1 MiB，以原生 stat/dumpimage 判定；地址用配置/共享常量，避免独立复制不同边界。
- Preserve: 原平台 RAM/MMIO、安全上界、kernel/FDT/FIT 地址与旧模式；原始 rootfs 不修改；不把 DTB revision/Hash 当启动条件。
- Forbidden: Registry 修改、全局任务写入、身份/evidence 工程、新增 SD/UFS/USB driver、自动 runtime fs-ng 初始化、性能优化。
- Test witness: 在现有 host harness 引入生产范围函数，先观察 RED：有效 extent、zero/错起点/非512长度/越界/加法溢出、缺/重复/非法参数、DTB header/长度越界；同一生产 reserved ranges 经原生范围差集后 allocator free 不包含 image/DTB。测试只覆盖可达内存访问边界，不写命令输出判定器。
- GREEN condition: host tests 全过；新模式 native build 成功；旧模式不增加 RAM reservations；冲突组合编译失败且错误指向模式冲突。
- Verification: `env RUSTFLAGS='-C linker=/home/daivy/projects/serial/work/StarryOS/scripts/cc-nopie.sh' cargo test --manifest-path tests/k3-embedded-user-host/Cargo.toml --offline -- --test-threads=1`；`make k3-rootfs`；`make k3-rootfs SMP=2`（期望拒绝）；`cargo check -p starry-kernel --target riscv64gc-unknown-none-elf --no-default-features --features k3-rootfs,k3-stdio --offline`（期望互斥 compile_error）。检查 `stat -c '%s'` 和 `dumpimage -l` 的原生大小/地址，不新增验证脚本。
- Stop when: 实际板 RAM/FDT 交接与这些已确认地址矛盾；现有依赖接口不能承载设计；所需变更影响用户已批准范围或 shared state ownership。

### 1.2: 唯一 RAM block、ext4 与实际文件读取

- Requirement/Scenario: 本轮 S1 的文件来源前置、S2/S5/S8；D2/D4。
- Depends on: 1.1。
- Targets: `kernel/src/entry.rs::init` 新分支与 qualification cfg；RAM rootfs 初始化模块；kernel Cargo block/ext4/ramdisk-static features；Makefile 构建接入及本变更内临时启动说明。
- Current behavior: K3 memory root + static ELF；未用 real ext4；所有 axfs 的 ROOT_FS_CONTEXT owner 必须保持唯一。
- Required behavior: 先启动已验收 UART/copier，再从有效 boot extent 建唯一 static-backed RamDisk，Box 成 AxBlockDevice 并以 from_one 传给 axfs；不调用 init_memory_root，不 auto-probe，不构造第二 driver。首轮仅 resolve `/bin/sh` 到 BusyBox，读取 BusyBox 和 musl 的 ELF header（必须观察 ELF magic），报告实际路径/文件大小；然后依照已有 30 s physical drain 成功或 timeout 结果 park。此首轮阶段分支后续由任务2.1替换，不新增永久 probe 模式或临时 shell。
- Preserve: 既有 UART/timer ownership、既有 ext4 错误层与原始 image；filesystem backing 在 mount 生命周期稳定，只有一份可变 slice。
- Forbidden: 以日志“mount ok”或假提示符代替 VFS 文件 read；rootfs driver 暴露真实官方 storage；本轮 spawn 用户进程；修改普通 loader/exec/tty；自写 block adapter 代替已存在实现。
- Test witness: 新特性的缺参数/错误范围测试先 RED；第一次板上合法 image 必须实际读取文件。坏 FS 用用户可丢弃 RAM image 直接观察 ext4 初始化失败，不修复源 image。没有板上结果不能声称挂载通过。
- GREEN condition: host 边界测试和 `make k3-rootfs` 通过；板上 `/bin/sh` 解析结果、BusyBox/musl 大小与 ELF magic 均正确，drain 有明确结果；缺 initrdmem 启动明确报错，不访问 image、不 fallback。
- Verification: 同1.1命令；真板最短步骤见下。现有 k3-shell 构建作为受影响旧入口检查：`make k3-shell`，成功后需要板测的新 FIT 重新由 `make k3-rootfs` 的必要构建产生，避免误用同名输出；更优先把该旧模式检查排在新模式最终打包前，避免重复已通过构建。
- Stop when: 参数未交接、实际地址与板态不符、mount/read 异常；按实际失败阶段返回规划，三次失败不第四次盲试；不得以猜测修补 loader 或启动环境。

**Invariants**

单 hart；rootfs 和 DTB 在 allocator 前预留且整个引用生命周期有效；唯一 block/root owner；唯一 UART hardware/copier；无活动 raw RX 消费者；只写 RAM；不 saveenv，不刷固件/GPT/官方分区。native 验证结果不由外部 wrapper 判定。

**Acceptance**

- A1：1.1生产范围测试覆盖合法/非法参数与 allocator 预留，GREEN；新模式成功构建，互斥/SMP 明确拒绝。
- A2：1.2真板读 ext4 中的 `/bin/sh` 目标、825088 B BusyBox 和 613016 B musl，两个头部均为 ELF；不是静态内嵌载荷。无 image 参数时在范围层失败；坏 FS 在文件系统层失败。
- A3：输出后观察 physical drain done 或明确 timeout，再 park；旧 k3-shell 构建保留。shell 启动与最终 RESET 不计本轮完成，交后续轮。

**Verification**

自动验证见各契约；范围测试为本轮新 RED→GREEN，MS17 已验收结果按覆盖采信，不重跑无关网络全量套件。QEMU 本轮 SKIPPED：未改变共享用户态运行链，且不能验证 K3 RAM/FDT handoff；后续共享代码变化再做相应回归。

板测属于实施后的外部能力边界，不能在缺观察时完成本轮：

1. 复制新模式 FIT 和原始 image 到既有 U 盘；按 R68 进入 U-Boot。`fatload usb 0:0 0x240000000 rootfs-riscv64.img`，直接观察 1073741824 bytes read；失败停止。
2. `setenv starry_rootfs_size ${filesize}`；`fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit`，成功且小于16MiB；失败停止。
3. `setenv bootargs initrdmem=0x240000000,0x${starry_rootfs_size}`；`setenv fdt_high 0xffffffffffffffff`；`bootm 0x2e0000000`。观察 rootfs 范围、VFS 实际文件路径/大小与 ELF header，以及 drain/park；用户用一句话记录所见结果。
4. 缺参数：复位重进 U-Boot，仅载内核 FIT，`setenv bootargs` 后 bootm；直接观察范围参数缺失错误，无文件读取/回退 shell。
5. 坏FS：只加载可丢弃的坏 image 到同一预留窗口，传递其合法对齐长度后 bootm；直接观察 ext4 初始化失败。不得修改原始 image或官方磁盘；失败后外部 RESET 恢复。坏 image 可由通用工具在 /tmp 创建零填充文件，不新增 fixture 管理协议。

步骤1–3成功一次即可；步骤4–5各一次即可。板上不可操作时反馈明确写 blocked 和所缺行为结果，不把等待事项作为可执行任务，不展开下一轮。

**Gate 2 Readiness**

PASS：调查、设计、任务契约和当前轮验证已自包含；用户已批准完整计划，原话“批准计划”。新产品基线尚未存在，RED 在实施时建立；未把未执行板测计为通过。

**Persisted Evidence**

- Mode: none

当轮反馈中的原生命令决定性输出/退出码与用户一句话板测结论足够；不创建 Evidence 占位目录。

**Risks and Notes**

`k3-rootfs` 首轮只是 ext4 文件路径，没有 shell；完整需求由后续两轮覆盖。禁止以首轮 accepted 宣称本次功能或原 MS18 完成。若动态 shell 需要修复 loader/tty，其实现契约在第二/三轮调查展开，当前轮不得提前扩展。

## Act Response

- Status: reported

自动化部分（任务 1.1 全部、1.2 的代码与 host/构建验证）与板测步骤 1–3（合法镜像挂载/文件读取/drain）已完成；剩余步骤 4–5（缺参数拒绝、坏 FS 失败层）等待执行人各一次记录后，本 Response 收口为 `reported`。未执行的人工项不计入完成声明。

### Board Witness（步骤 1–4，2026-10-08 板测，执行人串口记录）

- 128 MiB 镜像 `fatload` 134217728 bytes read（17 s，7.5 MiB/s，看门狗窗口内）；FIT 611872 B。
- 完整启动序列通过：allocator（slab）→ full paging → AIA（init=global/percpu done）→ RR scheduler → IRQ handlers → `Primary CPU 0 init OK` → entry。
- `stage=dtb-ok paddr=0x181000000 totalsize=155648`（U-Boot 就地改写后的 DTB，仍在 1 MiB 窗口内）→ `extent base=0x240000000 size=0x8000000`（bootargs `initrdmem` 解析+校验通过）→ `mount-begin` → axfs_ng `use block device 0: "ramdisk"` / `filesystem type: "ext4"` → `mount-ok` → `resolve-ok resolved=/bin/busybox` → `file-ok /bin/busybox size=825088 elf_magic=true` → `file-ok /lib/ld-musl-riscv64.so.1 size=613016 elf_magic=true` → `pre-drain` → `drain done (TxCompletion four conditions)` → park。A2 文件级事实与 A3 drain 观察逐字达成。
- 步骤 4（缺参数拒绝，PASS）：官方 U-Boot bootargs（`boot_mode=nor earlycon=sbi console=ttyS0,115200 ...`，无 `initrdmem`）启动 → `panicked at kernel/src/entry.rs:345 [ms18] fault stage=bootargs bootargs="..." err=MissingInitrdmem`——范围层明确失败，未访问 image、未挂载、无 fallback；同时证明官方启动链不会意外进入本模式。
- 步骤 5（坏 FS）：用户明确豁免，原话「剩下最后一个不用做，我们已经验证功能正常，直接回复报告就好」。风险：坏 FS 在文件系统层的失败表现未做板上直接观察，当前依据为既有 `axfs::init_filesystems` 的 expect 错误层语义（坏 FS 走 `fs::new_default` 失败 panic，不新增错误恢复）；如后续轮次需要可直接补测，不阻塞首轮。

### Actual Changes

- 新增 `crates/axplat-riscv64-k3/src/ram_layout.rs`：纯 `core` 生产模块 — 载运窗常量（rootfs `[0x240000000,0x280000000)`、DTB `[0x181000000,0x182000000)`、512 对齐、40 字节 FDT header）、`parse_initrdmem`（唯一十六进制 `initrdmem=<hex>,<hex>` 项；缺失/重复/非法数字/溢出拒绝）、`validate_rootfs_extent`（零长/错起点/非 512/checked 越界）、`validate_dtb_pointer`/`validate_dtb_header`（窗口内、magic、totalsize 有界）与 `RAM_ROOTFS_RESERVED`。
- `crates/axplat-riscv64-k3`：`ram-rootfs` feature（空集）+ `pub mod ram_layout`；`mem.rs::reserved_phys_ram_ranges` 仅在该 feature 下返回两个预留窗口，其余模式保持 `&[]`。
- 根 `Cargo.toml`：`k3-rootfs = ["k3-uart", "axplat-riscv64-k3/ram-rootfs", "starry-kernel/k3-rootfs"]`。
- `kernel/Cargo.toml`：`k3-rootfs` feature = k3-uart + `axfs/ext4` + `axplat-riscv64-k3/ram-rootfs`（预留 edge 下沉到 kernel，绕过 root 的直接构建也必然带预留）+ `dep:axdriver`（`dyn`+`block`）+ `dep:axdriver_block`（`ramdisk-static`）+ `dep:fdt-parser`（均为 optional，仅本模式编译）。
- `kernel/src/lib.rs`：`k3-rootfs` 与 `k3-stdio`/`k3-shell`/`k3-uart-port-test` 互斥 compile_error（SMP 拒绝沿用既有 `k3-async-uart`+`smp` 守卫）。
- `kernel/src/entry.rs`：data-plane 分支条件补 `not(feature = "k3-rootfs")`；新增 k3-rootfs 首轮阶段分支（注释明确由任务 2.1 替换）——UART/copiers/alarm task → DTB 指针/header 校验 → fdt-parser `raw_value` 读 bootargs → 解析+校验 extent（失败在构造 slice 前 panic，stage=dtb-pointer/dtb-header/dtb-parse/bootargs）→ 唯一 `'static` RamDisk → `AxDeviceContainer::from_one` → `axfs::init_filesystems` → VFS resolve `/bin/sh` 并读 BusyBox/musl（ELF magic 硬断言、路径/大小上报）→ 30 s 物理 drain → wfi park。
- `Makefile`：`k3-rootfs` target（SMP>1 拒绝、与既有 K3 target 相同的 MEM/PLAT_CONFIG 形态）+ `.PHONY`。
- `tests/k3-embedded-user-host/tests/ram_rootfs.rs`：18 个 host 契约测试（`#[path]` 直引生产模块）：有效/满窗 extent、零长/错起点/非512/越界/加法溢出、bootargs 缺失/重复/非法数字（含 K 后缀拒绝）/空载荷/溢出、DTB 指针/header/totalsize 越界、有效 stock header（150998 B）、生产预留范围经原生差集后 allocator free 不含两窗口。
- `Cargo.lock`：仅 axdriver/dyn 传递闭包新增（dma-api 0.7.3、rd-block 0.1.2、rdif-block 0.7.1、spin_on、pin-utils），无既有包版本漂移。

### Deviations from Plan

- 计划的调查事实写「axdriver/dyn 后可用 from_one」，实施确认无偏差；追加一处计划未明说的加固：平台预留 feature edge 同时放进 kernel `k3-rootfs` feature（D3 的「传递平台预留 feature」在 root 之上再加一层，防止绕过 root feature 的直接构建漏掉预留）。不构成实质问题。
- 首轮 host 测试两处期望值笔误（`0x2400_0000` 少一位）在 GREEN 前修正为板事实 `0x2_4000_0000`；生产解析结果自始正确。
- 板测输入调整（用户裁决，原话「进行压缩」）：真板 fatload 原始 1 GiB 镜像超出官方启动链遗留的看门狗窗口（USB 装载 35–100+ s 触发自动重启），且主机 e2fsprogs 1.46.5 因镜像的 orphan_file 特性（1.47+）无法 resize。改为 `debugfs rdump` 只读提取原镜像内容树 + `mkfs.ext4 -d`（fakeroot 归一 uid/gid 0:0）重建 `rootfs-riscv64-min.img`（134217728 B = 128 MiB，512 对齐；`e2fsck -f` exit 0 clean；BusyBox md5 `fdb59f1c…` 与 musl md5 `f6c41d2d…` 与原镜像字节级一致；`/bin/sh`→`/bin/busybox` 符号链保留）。原始 `rootfs-riscv64.img` 未修改。与 A2 的差异仅镜像容器：/dev 设备节点未提取（第 2 轮 devfs 覆盖 /dev，首轮不触及），板测「1073741824 bytes read」观察相应变为 134217728 bytes read。A2 的文件级事实（解析目标、825088/613016、ELF magic）不受影响，仍以板上直接观察判定。
- 板测根因修复一（boot 页表覆盖缺口）：预留生效后 free 内存被切成三段，axruntime `init_allocator` 在完整页表切换**之前**为每段 free 区起点写分配器元数据；启动期临时页表（boot.rs 仅映射 1 GiB 块 6 + UART 块 3）不含尾段起点 0x280000000 → page fault，且因尚未 `init_scheduler` 被 trap 路径的 `current task is uninitialized` panic 掩盖（与 MS08 001-replan 记录的掩盖模式一致，来源 `.agents` 归档 MS08 Iteration 000）。修复：`ram-rootfs` feature 下 boot.rs 将 K3 全 RAM 窗口（1 GiB 块 5–11 + 高半镜像，本板该区间纯 RAM）补入启动页表——仅多映射、不提前访问，旧模式启动页表逐字节不变。D1 的「入口只在完整 paging 后访问 rootfs」语义保持（映射 ≠ 访问）。
- 板测根因修复二（fdt-parser 路径形式）：`Fdt::find_nodes` 约定为「带 `/` 按路径、不带按别名」，首轮误用 `find_nodes("chosen")` 被当作别名查找而 panic `aliase not found`（fdt.rs:144）。修复为 `find_nodes("/chosen")`，与 axhal chosen.rs 的用法一致。
- 诊断辅助与正式产物：分支内新增 ax_println! 直写 early console 的 stage 标记（不依赖调度器/copier，随任务 2.1 的分支替换一并消失）；诊断期间使用 LOG=info 与 DWARF=y 构建（后者 .bin 24.99 MiB 违反 16 MiB 布局门，未上板，仅确认 DWARF 嵌入即膨胀）；正式交付镜像为 LOG=off/DWARF=n（.bin 458944 B、.fit 611872 B，尺寸门复验通过）。

### Self-Review Findings

- Gate 3（spec compliance）：Task Contract 1.1/1.2 的 Required behavior、Preserve、Forbidden 逐项核对通过——旧模式零预留（`&[]` 分支由 k3-shell 构建走通）、无 init_memory_root/auto-probe/第二驱动、ROOT_FS_CONTEXT 唯一初始化、无 tty/RX 消费者/spawn、mount 结论以实际文件 read 为准。
- Gate 3（code quality）：修复 `Box` 未导入、`header.len()` 借用冲突、一处超长行；遗留 Minor：本 feature 组合下本地 `crates/axfs-ng` 出现一条 `unused import: prelude` 构建形状警告（未改动该 crate 源码，无功能影响，不阻塞）。
- Gate 4：验证命令与决定性输出（每项 ≤20 行、退出码）：
  - host RED→GREEN：RED exit 101（`couldn't read .../ram_layout.rs`）；GREEN `test result: ok. 18 passed`（ram_rootfs）+ `9 passed`（contract），整体 exit 0。
  - `make k3-rootfs` exit 0；`dumpimage -l`：kernel 458944 B @ 0x180000000、FDT 150998 B @ 0x181000000。
  - `make k3-rootfs SMP=2` → `ERROR: k3-rootfs is a single-hart mode...` exit 2。
  - `cargo check -p starry-kernel ... --features k3-rootfs,k3-stdio` → compile_error `k3-rootfs cannot be combined with k3-stdio/k3-shell/k3-uart-port-test` exit 101（其后 UART_IRQ 报错为该裸命令缺 AX_CONFIG_PATH 的既有现象，非本次引入）。
  - `make k3-shell` exit 0（受影响旧入口保留；随后 `make k3-rootfs` 重跑恢复板测 FIT，避免同名输出误用）。
  - `stat -c '%s'`：.bin 458944（<16 MiB）、.fit 611872（<16 MiB）、DTB 150998（<1 MiB）、`rootfs-riscv64.img` 1073741824 未修改。
- A1 达成；A2/A3 的板上观察待执行（见上）。

### Persisted Evidence

None required（计划 Mode: none；上述命令输出与待补的一句话板测结论已足够）。

### Experience Candidates

None。

### Unresolved Issues

- 无阻塞项。坏 FS 板测按用户豁免跳过（见 Board Witness 步骤 5）；首轮 accepted 后由 Iteration 001 承接普通 shell。

## Plan Review

- Review Result: accepted
- Reviewed: 2026-10-08（独立阅读完整未提交 diff——10 文件 +330/−5 与两个新文件——ram_layout/entry/mem/boot/lib/Cargo/Makefile 逐行、18 项 host 测试清点、Cargo.lock 漂移审计、特征传递链与守卫核对、板测 witness 与代码逐字对照；不重复运行 Act 已采信验证）

**Findings**

阻塞项：无。非阻塞项：

1. Minor（Act 偏离，已记录并核证为必要修正）：`boot.rs` 启动页表扩展不在 Task Contract 1.1 Targets 内。核证：`ram-rootfs` cfg 门控、仅写 PTE（1 GiB 块 N 的 frame=N<<28 正确、block 5..=11 覆盖两个载运窗且该区间纯 RAM）、identity+高半双份，无任何 rootfs 访问——D1「入口只在完整 paging 后访问 rootfs」保持（映射 ≠ 访问）；非该 feature 时逐字节不变，Preserve「旧模式」成立。预留生效后 free 区被切段而 `init_allocator` 在完整页表前写尾段元数据会 page fault，此修复是 A1 预留与可启动内核共存的必要条件，非范围扩张。
2. Minor（加固，已记录）：kernel `k3-rootfs` feature 重复传递 `axplat-riscv64-k3/ram-rootfs` 预留 edge——防绕过 root feature 的直接构建漏掉预留。核证于 `kernel/Cargo.toml`，语义与 root edge 一致。
3. Minor（用户裁决，原话与依据已记录）：板测镜像由 1 GiB 原镜像改为 128 MiB `debugfs` 提取+`mkfs.ext4` 重建（看门狗窗口与 e2fsprogs orphan_file 限制）。原始 `rootfs-riscv64.img` 未修改；A2 文件级事实（resolve 目标、825088/613016 B、ELF magic）不受容器替换影响，witness `size=0x8000000` 与之自洽。
4. Minor（用户豁免，原话与风险已留档）：坏 FS 板测跳过；A2 坏 FS 子句当前依据为既有 `axfs::init_filesystems` expect 错误层语义，后续轮需要时可补测，不阻塞本轮。
5. Minor（pre-existing）：本 feature 组合下本地 `crates/axfs-ng` 一条 `unused import: prelude` 构建形状警告——未触碰该 crate 源码，不满足 Issue 准入。
6. Minor（产物一致性，本次修正）：change `tasks.md` 头部「实施未开始」与 proposal「实施：未进入」为计划期遗留 stale 行，与本轮 Act/Review 状态矛盾——本次 Review 已精准修正两行。非代码缺陷。

独立核验通过的关键面（代码 review，非 Act 自检）：

- `ram_layout.rs`：纯 core 单源常量（mem.rs 预留、entry 校验、host 测试同源消费，无第二份边界复制）。`parse_initrdmem` 唯一项、缺失/重复/非法/溢出四类独立错误、K/M/G 后缀经 `to_digit(16)` 拒绝、`splitn(2)` 使第三段随 len 解析失败；`validate_rootfs_extent` 零长/错起点/非 512/checked 越界；DTB 指针窗口含 40 B header 余量，header 校验 magic BE + totalsize≥40 + checked `ptr+totalsize`≤窗口端。
- `entry.rs` k3-rootfs 分支：先起已验收 UART/copier/alarm，再 DTB→extent 校验，四层失败（dtb-pointer/dtb-header/dtb-parse/bootargs）全部在 `from_raw_parts_mut` 之前 panic，`entry.rs:345 MissingInitrdmem` 与板测步骤 4 逐字对齐；唯一 `'static` RamDisk→Box→`from_one`→`axfs::init_filesystems`，无 init_memory_root/自动 probe/第二驱动；resolve `/bin/sh`→absolute_path + `fs.read` BusyBox/musl + ELF magic 硬断言；30 s drain `block_on(timeout)`→wfi park。无 tty/raw RX 消费者/用户态 spawn，loader/exec/tty 零改动。
- 形态面：data-plane 分支补 `not(k3-rootfs)`（无该 feature 时语义不变）；kernel lib.rs 互斥 compile_error（k3-stdio 覆盖 k3-shell 传递闭包）；SMP 链 `k3-rootfs→k3-uart→k3-async-uart`→既有 `+smp` 守卫传递闭合，Makefile SMP>1 前置拒绝；mem.rs 非 ram-rootfs 保持 `&[]`；Makefile target 与兄弟 K3 target 同型（MEM=6400M/SMP=1/LOG=off/DWARF/n）+ `.PHONY` 补齐。
- host 测试：`ram_rootfs.rs` 恰 18 个 `#[test]`，`#[path]` 直引生产模块，覆盖契约见证清单全部类别；allocator 差集 `free_ranges` 为测试内嵌 fixture，非独立判定层，无身份型证据工程。
- Cargo.lock：仅 5 个新增包（dma-api 0.7.3、pin-utils、rd-block 0.1.2、rdif-block 0.7.1、spin_on）+ 一处 virtio-drivers 的 `dma-api` 引用加 `0.5.2` 版本限定（解析版本未漂移）+ starry-kernel 两条 optional 边（axdriver_block/fdt-parser），与 Act 声明一致。

**Acceptance Gaps**

None。A1（host 18/18 GREEN、`make k3-rootfs` exit 0、SMP>1 与模式互斥明确拒绝、旧模式零预留）满足；A2（板测 resolve=/bin/busybox、825088/613016 B ELF magic=true 逐字达成，缺参数范围层失败无 fallback；坏 FS 子句按用户豁免并留档风险）满足；A3（drain done 后 park、`make k3-shell` exit 0 保留）满足。

**Evidence**

- Act Response 验证结论直接采信（来源：本 Cycle Act Response；工作树变更面与 Act 声明清单一致，报告后无改动）：host 18/18、`make k3-rootfs`/SMP=2 拒绝/互斥 compile_error/`make k3-shell`、stat/dumpimage 尺寸门、板测步骤 1–4 串口记录。
- 采信 MS17 已验收底座结论（timer/AIA/UART/copier/物理 drain），本轮未触碰其覆盖面。
- Persisted Evidence 维持 `none`（计划 Mode: none；无新增 `required` 情形）。

**Follow-up Decision**

accepted。任务 1.1/1.2 契约与 Invariants 全部满足；Deviations 中已记录的 boot 页表修复、fdt `/chosen` 路径形式修复、host 测试期望值笔误修正经独立核证均为必要或非实质；6 项非阻塞 finding 记录在案，无需当前 Cycle 修复。首轮 accepted 只证明 RAM block/VFS 路径——普通 shell 交 Iteration 001，不以本轮宣称本次功能或原 MS18（真实存储）完成。

**Iteration Plan Update**

None。

**Next Cycle**

None。

**Next Iteration**

`001-standard-shell`（tasks 2.1–2.2，Map 依赖已满足；本次 Review 未展开，待用户审计后以 plan 指令创建自包含执行契约）。
