# Iteration 001 / Cycle 000: 普通动态 ELF shell 与外部命令

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: 2.1、2.2。
- Acceptance gaps: None。
- Repair items: None。
- Inherited scope: 已批准的 RAM rootfs 普通 shell 范围；单 hart、临时启动、官方存储与持久环境保持完整；Iteration 000 accepted 的 range/FS 失败层、RAM 预留与 UART 底座沿用。
- Excluded scope: S3–S7（脚本/管道/重定向、错误/取消恢复、易失写入、空闲/编辑）与 S6 官方 RESET 见证、S8 正式验收与全规格覆盖检查（Iteration 002）；真实存储、网络、SMP、性能；ISS01 预留窗口调整；MS12 范围。

**Objective**

K3 `k3-rootfs` 模式经普通 loader 从 ext4 root 启动动态链接的 BusyBox `/bin/sh`（musl 解释器路径），板上进入可交互 shell 并真实执行文件系统中的外部命令；启动依赖缺失按层报告且无 fallback；受影响旧模式与 QEMU 共享面不回归。

**Investigation Facts**

- Current Baseline: MS18 Iteration 000 accepted（2026-10-08，见 `../000-ram-rootfs/000-initial.md`）：k3-rootfs 分支以 DTB/extent 校验 + `init_filesystems` 挂 ext4 并真实读取 BusyBox/musl（ELF magic 硬断言）后 30 s drain + park；host 18/18、`make k3-rootfs`/`make k3-shell` exit 0、SMP>1 拒绝、k3-rootfs+k3-stdio compile_error、板测步骤 1–4 逐字达成；坏 FS 板测按用户原话豁免（仅限首轮）。MS17 accepted 底座（单 hart timer/AIA、UART copier、tty canonical、物理 drain、用户进程/join）覆盖未变，直接采信。工作树含未提交首轮改动（变更面=首轮 Act Response 清单，本轮在其上继续）。
- Current-State Evidence:
  - 普通启动同源模板为 entry.rs QEMU 分支（`kernel/src/entry.rs:447-558`）：`pseudofs::mount_all()` → `spawn_alarm_task()` → `FS_CONTEXT.resolve(&args[0])`（absolute_path/name）→ `new_user_aspace_empty()`+`copy_from_kernel` → `load_user_app(&mut uspace, None, args, envs)` → `UserContext::new`/`new_user_task`/`set_page_table_root` → `Process::new_init`+`add_thread` → `ASYNC_TTY.bind_to(&proc)` → `ProcessData::new(proc, path, args.to_vec(), uspace, Arc::default(), None)` → scope 内 `crate::file::add_stdio(&mut FD_TABLE…)` → `Thread::new`+`AxTaskExt` → `spawn_task`+`add_task_to_table` → `task.join()` → `root_dir().unmount_all()`+`filesystem().flush()`。
  - `src/main.rs::CMDLINE = ["/bin/sh", "-c", include_str!("init.sh")]`、`envs = []`；`src/init.sh`：export HOME=/root → banner/env → `cd ~` → `sh --login`。K3 分支消费同一组 args 即与 QEMU 启动链同源；首轮分支当前丢弃 args（`let _ = (args, envs)`）。
  - 普通 loader `kernel/src/mm/loader.rs::load_user_app`：`path=None` 时取 args[0]；`.sh` 后缀与 `#!` shebang 头再派发 `/bin/sh`；非 ELF 且无 shebang → `AxError::InvalidExecutable`；PT_INTERP → VFS resolve 解释器（缺失即该层失败）→ map 到 `USER_INTERP_BASE` → entry 取 ldso entry → 构造 auxv；映射 stack/heap 并写入 argv/envs/auxv。`mm::loader` 在 `mm/mod.rs:4` 无 cfg 门控（`pub use loader::*`），K3 特征下已编译——本轮预期零 Cargo/Makefile 变更；有门控的是 `embedded_loader`（本轮不用）。
  - `pseudofs::mount_all()`（`kernel/src/pseudofs/mod.rs:88`）：在已初始化 ROOT_FS_CONTEXT 上挂 `/dev`(devfs)、`/dev/shm`、`/tmp`、`/proc`、`/sys`，缺目录自动 create_dir，`/sys/.../subsystem` 符号链写入 root（RAM 易失，与 QEMU 同源）。devfs `console` 节点持有 `crate::drivers::ASYNC_TTY`（`pseudofs/dev/mod.rs:225-230`）——ASYNC_TTY 惰性构造点即 tty-reader，成为唯一 RX 消费者（MS17 单消费者论证沿用）。`init_memory_root` 的 cfg 门控不含 k3-rootfs，禁止调用。
  - stdio：`crate::file::add_stdio` = publish_stdio + `open_console`（打开 `/dev/console`，read/write 端点，`kernel/src/file/mod.rs:248-273`）。k3-stdio 分支（`entry.rs:129-254`）已在 K3 验收 copier→mount_all→alarm→aspace→Process/ASYNC_TTY/stdio→spawn/join→30 s drain→wfi park 全链。
  - 特征面：k3-rootfs → k3-uart 已开 file/mm/pseudofs/syscall/task 模块与 paging/task-ext；互斥 compile_error（kernel lib.rs）与 Makefile SMP>1 拒绝为守卫文件，本轮不触碰即不重跑。
  - 既有 host 面：`tests/k3-embedded-user-host`（contract.rs 9 项 embedded loader/stdio + ram_rootfs.rs 18 项首轮契约）为 2.2 回归基线；host 直引文件不含 entry.rs，本轮改动不扩大其覆盖面。
  - 板测输入：`rootfs-riscv64-min.img`（128 MiB，内容=原镜像全树提取重建；`/bin/sh`→busybox 符号链、825088/613016 B 文件字节级一致；`/dev` 无设备节点，由 devfs 覆盖）。镜像构建/校验流程见 Runbook `.agents/runbooks/rootfs-image-shrink.md`，U-Boot 启动序见 R68（`fatload` rootfs 到 0x240000000、FIT 到 0x2e0000000、`setenv bootargs initrdmem=0x240000000,0x${starry_rootfs_size}`、`fdt_high`、`bootm`；不 saveenv）。
- Code and Critical Path: 重写 `kernel/src/entry.rs` k3-rootfs 分支：保留 DTB 指针/header/parse/bootargs 校验 + `init_filesystems` 前缀（stage panic 语义不变）→ `mount_all` → `spawn_alarm_task` → resolve(args[0]) → 普通 loader → 进程/tty/stdio → spawn/join → root unmount+flush → 既有 30 s drain → wfi park。失败层：range（bootargs）/FS（mount）/resolve/load（解释器缺失、InvalidExecutable）/stdio/spawn 各层 panic，无 fallback。

**Implementation Guidance**

进程装配逐行镜像 QEMU 分支结构（同源论证的最强形式：布局差异即偏差）；失败 stage panic 命名沿用首轮风格（stage=resolve/stage=load/stage=stdio-open），join/fs 收尾/drain 以 `k3_uart::diag_line` 报告供串口直接观察。先 2.1 分支重写并过 `make k3-rootfs`，再板测 S1/S2，最后执行 2.2 门。板测 FIT 以 `make k3-rootfs` 重新产生，避免同名旧产物误用。

**Task Contracts**

### 2.1: K3 普通 shell 分支（ordinary loader 与 join 收尾）

- Requirement/Scenario: S1、S2（缺 shell/解释器/坏 ELF 子句）、S8 会话收尾 smoke（S8 正式验收含 timeout 语义归 3.2）；D3。
- Depends on: None（本轮内先于 2.2 板测与门检查）。
- Targets: `kernel/src/entry.rs`（k3-rootfs 分支体重写 + k3-rootfs 专用 import 块；`let _ = (args, envs)` 移除）。预期不改其他文件。
- Current behavior: 首轮分支 mount 后 resolve/read witness → 30 s drain → park；无 shell、无 spawn、args/envs 被丢弃。
- Required behavior: 按 QEMU 同源顺序接入：`mount_all()` → `spawn_alarm_task()` → `FS_CONTEXT.resolve(&args[0])`（失败 stage=resolve）→ `new_user_aspace_empty`+`copy_from_kernel` → `load_user_app(None, args, envs)`（失败 stage=load，含解释器缺失与 InvalidExecutable）→ UserContext/`new_user_task`/`set_page_table_root` → `Process::new_init`+`add_thread` → `ASYNC_TTY.bind_to` → `ProcessData::new`（argv=原始 args）→ scope 内 `add_stdio` → `Thread`/`spawn_task`/`add_task_to_table` → `task.join()` → root `unmount_all`+`flush` → 既有 30 s 物理 drain（成功/timeout 如实报告）→ wfi park。移除首轮 resolve/read witness、boot 进度 ax_println 标记与 `entry-enter` 标记；保留 range/FS 失败 stage。不新增 feature edge（确需时记 Deviation 且不改互斥/单 hart 语义）。
- Preserve: 首轮 range/FS 失败层 stage 命名与 RAM 预留窗口；MS17 各模式行为与边界；原始 `rootfs-riscv64.img` 与官方存储只读；30 s drain/park 契约；启动链与 QEMU 同源（CMDLINE/init.sh/普通 loader/pagefault 路径，不走 embedded/eager）。
- Forbidden: 修改共享 loader/exec/tty/process 语义（发现缺口按 Stop when 返回 Plan）；调用 `init_memory_root`/构造第二 driver/第二 RX 消费者；spawn 内嵌载荷或以打印提示符替代真实外部命令执行；补全 S3–S7 行为；身份型证据工程或判定器；调整 ISS01 预留窗口。
- Test witness: RED 基线 = 首轮 accepted 板测行为（file-ok 后 drain+park，无 shell，来源 `../000-ram-rootfs/000-initial.md` Board Witness）；GREEN = 板测 S1（banner→提示符→`/bin/ls /bin`、`/bin/cat /etc/os-release`→回提示符）+ 会话末 `exit` 收尾行 + S2 三变异各一次。
- GREEN condition: `make k3-rootfs` exit 0（尺寸门沿用）；S1 各项板上直接观察达成；S2 三变异命中对应失败层且无提示符、无 fallback。
- Verification: `make k3-rootfs`；板测最短步骤见 Verification 节；首轮已过的 range/互斥/SMP 检查不重跑。
- Stop when: K3 上动态 ELF/COW/exec/auxv 出现需修改共享 loader/process/tty 语义的缺口；spawn/join/unmount/flush 需要新的错误处理设计；argv/auxv 布局与 musl BusyBox 不兼容且根因在共享代码；实际板态与已确认 RAM/FDT 事实矛盾；三次失败按公共规则停止。

### 2.2: 受影响模式隔离与 QEMU 同源兼容

- Requirement/Scenario: S9；D4。
- Depends on: 2.1。
- Targets: 无产品代码目标（验证任务）；受影响面 = entry.rs 头部 imports 与 k3-shell/k3-stdio/QEMU 普通构建、既有 host 套件。
- Current behavior: 首轮后 `make k3-shell`/`make k3-stdio` exit 0、host 套件 27 项通过（覆盖未变，采信首轮）。
- Required behavior: 2.1 改动后以下门全部通过：`make k3-shell`、`make k3-stdio`、`make ARCH=riscv64 SMP=1 build`（QEMU 普通共享编译门）、host 套件。当且仅当共享 loader/进程/tty/pseudofs/syscall 源发生影响 k3-rootfs 之外构建的实际语义变化时，运行 QEMU 同源 rootfs justrun 回归并在普通 shell 执行外部命令（`-snapshot` 保持原始 image 不写）；未触发时记录 SKIPPED 与判定依据。互斥/SMP 守卫文件本轮未触碰，采信首轮结论。
- Preserve: 原始 `rootfs-riscv64.img` 与 `make/disk.img` 不写；旧模式行为边界不变。
- Forbidden: 为兼容性新增脚本、wrapper 或判定器；修改旧模式行为换取通过。
- Test witness: native 构建退出码与 host 套件原生结果即见证。
- GREEN condition: 三个构建 exit 0；host 套件全过；justrun 按条件执行通过或记录 SKIPPED 依据。
- Verification: 见 Verification 节自动命令；任一失败即阻塞并定位到对应构建面。
- Stop when: 任一旧模式构建或 host 测试回归且根因在 2.1 改动；需修改旧模式行为才能通过时返回 Plan。

**Invariants**

单 hart；rootfs/DTB 预留窗口与首轮一致（ISS01 维持 open，本轮不调整）；唯一 block driver 与 ROOT_FS_CONTEXT owner（`init_filesystems` 一次，无 `init_memory_root`、无自动 probe）；唯一 UART hardware/copier，mount_all 后 tty-reader 为唯一 RX 消费者；失败分层无 fallback；只写 RAM rootfs 与临时文件系统，不 saveenv、不刷固件/GPT/官方分区；原始 `rootfs-riscv64.img` 只读；native 验证结果不由外部 wrapper 判定。

**Non-goals**

S3–S7 与 S6 见证、S8 正式验收与全规格覆盖（Iteration 002）；真实存储、分区、网络、SMP、性能优化；ISS01 窗口调整；MS12/MS14 范围；共享 loader/process/tty 的预防性重构。

**Acceptance**

- A1（S1，Requirement: Filesystem-backed ordinary shell）：板测进入普通交互 `/bin/sh`（init.sh banner → 提示符，非内嵌载荷），`/bin/ls /bin` 与 `/bin/cat /etc/os-release` 输出实际文件系统内容并回到提示符——证明动态 BusyBox/musl 经普通 loader、fork/exec/wait 真实运行；会话末 `exit` 观察进程 exit、fs 收尾与 drain/park diag 行到达 park（smoke 级）。
- A2（S2 启动依赖子句）：可丢弃镜像副本上移除 `/bin/sh` → resolve 层失败；移除 musl `ld-musl-riscv64.so.1` → load 层失败；`/bin/busybox` 替换为非 ELF → load 层 InvalidExecutable；均无提示符、无内嵌 shell fallback，原始 image 不动。
- A3（S9，Requirement: Isolated mode and qualification scope）：k3-shell/k3-stdio/QEMU 普通 native build exit 0、host 套件通过、justrun 按条件执行或记录 SKIPPED；RAM rootfs 结论不记为真实存储/SMP 验收。

**Verification**

自动命令（退出码即判定）：

- `make k3-rootfs`
- `make k3-shell`；`make k3-stdio`
- `make ARCH=riscv64 SMP=1 build`
- host 套件：`env RUSTFLAGS='-C linker=/home/daivy/projects/serial/work/StarryOS/scripts/cc-nopie.sh' cargo test --manifest-path tests/k3-embedded-user-host/Cargo.toml --offline -- --test-threads=1`
- 条件回归（仅共享 loader/进程/tty/pseudofs/syscall 语义变化时）：`make ARCH=riscv64 SMP=1 DISK_IMG=/home/daivy/projects/serial/work/StarryOS/rootfs-riscv64.img QEMU_ARGS=-snapshot justrun`，普通 shell 执行外部命令；未触发记录 `SKIPPED: <依据>`。
- 互斥/SMP 拒绝：守卫文件未触碰，采信 Iteration 000 accepted 结论，不重跑。

板测（外部能力边界，执行人串口直接观察并一句话记录，各一次）：

1. 按 R68 进 U-Boot；`fatload usb 0:0 0x240000000 rootfs-riscv64-min.img`（预期 134217728 bytes read）；`setenv starry_rootfs_size ${filesize}`；`fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit`；`setenv bootargs initrdmem=0x240000000,0x${starry_rootfs_size}`；`setenv fdt_high 0xffffffffffffffff`；`bootm 0x2e0000000`。
2. 观察启动链（沿用首轮 stage 序列至 mount-ok）后 init.sh banner 与 `/bin/sh` 提示符；依次执行 `/bin/ls /bin`、`/bin/cat /etc/os-release`，确认输出实际内容并回到提示符；会话末 `exit`，观察进程 exit、fs 收尾与 drain done/timeout、park 各 diag 行。
3. S2-a：在 /tmp 用 debugfs -w 于镜像副本删除 `/bin/sh`，按步骤 1 装载该副本 bootm → 观察 resolve 层失败 stage，无提示符。
4. S2-b：副本删除 `/lib/ld-musl-riscv64.so.1` → load 层失败 stage。
5. S2-c：副本将 `/bin/busybox` 内容替换为非 ELF（`/bin/sh` 符号链保留）→ load 层 InvalidExecutable stage。
6. 每次变异后外部 RESET 恢复；原始 image 与官方介质全程不写；不 saveenv。

板上不可操作时反馈明确写 blocked 与所缺行为结果，不把等待作为可执行任务，不展开 Iteration 002。

**Gate 2 Readiness**

PASS：调查、设计、任务契约、验证设计与自包含性齐备，无 TBD，无需要 Act 决定的契约语义；S2 三变异为本轮必做（首轮坏 FS 豁免不延续）。计划获批后 Status 由 draft 改 ready。

**Persisted Evidence**

- Mode: none

当轮 Act Response 的原生命令决定性输出/退出码与执行人一句话板测结论足够；不创建 Evidence 占位目录。

**Risks and Notes**

- K3 上动态 ELF/COW/exec/auxv 此前未验收——这正是本轮目标行为；出现共享语义缺口按 Stop when 返回 Plan，不盲改共享面。
- min 镜像 `/dev` 无设备节点：devfs 覆盖 `/dev`，无功能影响；`mount_all` 的 `/sys` 符号链写入 RAM rootfs 属易失行为（与 QEMU 同源）。
- 首轮遗留的本地 `axfs-ng` unused-import 构建形状警告在本 feature 组合可能仍出现（pre-existing Minor，不阻塞）。
- ISS01（1 GiB 固定预留 vs 128 MiB 实际镜像，896 MiB 未用）维持 open，本轮不调整；MS14 前需独立 Plan 决策。
- 板测会话中 stray 输入或超长行由既有 MS17 tty 契约处理；S7 的正式观察归 002。

## Act Response

- Status: blocked

### Blocker Handoff

- **偏差位置**：Task 2.1 板测（Verification 步骤 1–2，S1 交互 shell）；Gate 4 板测 GREEN 无法达成。
- **Plan 预期 vs 实际**：Plan 预期 QEMU 同源装配链在 K3 上运行动态 shell；实际 busybox 运行中 demand-page fault，COW `alloc_new_at` → `alloc_frame` → `global_allocator().alloc_pages(1, 4K, VirtMem)` 返回 `Err(NoMemory)` → SIGSEGV exit 139（`join=0` 为 MS17 已裁决的 API 行为，掩盖真实码）。
- **根因（证据完整）**：`axruntime::init_allocator`（registry，lib.rs:297–312）选 `.bss` 之后**紧邻的 FREE 区**作为页分配器（`BitmapPageAllocator`，单连续区间）主区。K3 上 MS18 DTB 预留窗 `[0x181000000,0x182000000)` 把该区切成 `[0x180390000,0x181000000)` = **12.4 MiB**；其余空闲区（192 MiB + 1.67 GiB）经 `global_add_memory` 只进 slab 字节分配器（`balloc`）。**所有页粒度分配**——COW demand-paging 帧（`alloc_frame`）、页缓存页（`PageCache::new`）、页表页（`PagingHandler`）、以及 slab 增长（两级设计从 palloc 切页）——**全部挤 12.4 MiB 单池**，历史依赖地耗尽 → 死亡点非确定（release 轮出 banner 后死，diag 轮更早死，LOG=info 增大 heap 占用）。QEMU 的 `.bss` 后空闲区 = 其余全部 RAM（~1 GB）→ 正常。
- **证据**：三轮板测串口日志（66.28s populate 失败 + `fault area=[VA:0x1000,VA:0xc6000) Size4K`；三处 file 路径 warn 全沉默 → 失败点在 `alloc_frame`）；argv/栈读回 `match=true` 排除装配链；axruntime/axalloc/cow 源码静态核对与日志区域清单逐项对齐。
- **已完成**：2.1 代码（entry.rs 重写）+ 2.2 全部自动化门（构建/host 27/27，justrun 由 QEMU 实跑交互成功补强）。**受阻**：板测 A1/A2；2.1 完成勾选。
- **临时诊断代码**（修复轮移除）：`kernel/src/mm/loader.rs`（argv 读回校验，cfg k3-rootfs）、`kernel/src/mm/aspace/mod.rs`（fault area 输出，cfg k3-rootfs）、`kernel/src/mm/aspace/backend/file.rs`（pt.map 失败 warn）、`crates/axfs-ng/src/highlevel/file.rs`（evict/read 失败 warn，仅错误路径打印）。
- **恢复条件**：用户经 `openspec-plan` 裁决修复路径后创建对应执行契约。候选：(a) 把 DTB/rootfs 预留窗移到 RAM 顶部使 `.bss` 后空闲区恢复大块连续（boot 命令随 ram_layout 常量变化；与 ISS01 窗口调整联动）——推荐先评估；(b) workspace patch axruntime 选区策略（改选最大 FREE 区）——通用但引入 registry patch 面；(c) 更换 axalloc 页分配器——最重。

### Automated Verification（Gate 4）

- `make k3-rootfs` → exit 0；`dumpimage -l`：kernel 1081536 B（<16 MiB）、FDT 150998 B（<1 MiB）；`StarryOS_riscv64-k3.fit` 1234464 B。尺寸门沿用首轮标准。
- `make k3-shell` → exit 0；`make k3-stdio` → exit 0（受影响旧入口保留）。
- `make ARCH=riscv64 SMP=1 build` → exit 0（QEMU 普通共享编译门）。
- host 套件（`env RUSTFLAGS='-C linker=.../cc-nopie.sh' cargo test --manifest-path tests/k3-embedded-user-host/Cargo.toml --offline -- --test-threads=1`）→ contract 9 passed + ram_rootfs 18 passed，exit 0（27/27）。
- 定向 warning 审计（Gate 3）：QEMU 与 k3-rootfs 两形状 touch entry.rs 重建后，entry.rs 零 warning/error；QEMU 形状 lib 的 14 条 warning 均为其他 crate pre-existing（virtio-drivers 1、smoltcp 9、axtask 2、axfs-ng 1、cargo config 弃用提示）；k3-rootfs 形状下 starry-kernel 本体零 warning。
- justrun 条件回归：`SKIPPED: 本轮改动全部位于 cfg(all(target_arch = "riscv64", feature = "k3-rootfs"))` 之内（含被移除的 cfg'd entry-enter 标记），QEMU/旧 K3 形状编译面零变化，QEMU SMP=1 构建门已过；共享 loader/进程/tty/pseudofs/syscall 源未触碰。
- 互斥/SMP 守卫：kernel/src/lib.rs 与 Makefile 守卫本回合未触碰，采信 Iteration 000 accepted 结论（SMP>1 拒绝、k3-rootfs+k3-stdio compile_error），不重跑。
- RED 基线采信：首轮 accepted 板测行为（file-ok → drain+park、无 shell），来源 `../000-ram-rootfs/000-initial.md` Board Witness，覆盖未变；本轮 GREEN 由板测 A1/A2 建立。
- 板测 FIT 已在全部构建门之后由 `make k3-rootfs` 重新产出（2026-10-08 17:07，kernel 1081536 B），避免同名输出误用。

### Actual Changes

- 本回合仅改 `kernel/src/entry.rs`（三处编辑；契约"预期不改其他文件"达成）：
  1. 移除 init() 顶部的 `#[cfg(feature = "k3-rootfs")] ax_println!("[ms18] entry-enter")` 及其 MS18 诊断注释。
  2. 新增 k3-rootfs 专用 import 块（逐项镜像 QEMU 块：ASYNC_TTY、FD_TABLE、copy_from_kernel/load_user_app/new_user_aspace_empty、pseudofs、ProcessData/Thread/add_task_to_table/new_user_task/spawn_alarm_task、FS_CONTEXT、UserContext、Mutex、AxTaskExt/spawn_task、Pid/Process）。
  3. k3-rootfs 分支体重写：保留 UART/copier 启动 + DTB pointer/header/parse/bootargs 四层 stage panic + extent 校验 + 唯一 `'static` RamDisk + `init_filesystems` 前缀（range/FS 失败层语义不变）；随后按 QEMU 同源顺序接入普通链：`mount_all()` → `spawn_alarm_task()` → `FS_CONTEXT.resolve(&args[0])`（失败 stage=resolve / resolve-path）→ `new_user_aspace_empty`+`copy_from_kernel` → `load_user_app(None, args, envs)`（失败 stage=load，含解释器缺失与 InvalidExecutable）→ UserContext/`new_user_task`/`set_page_table_root` → `Process::new_init`+`add_thread` → `ASYNC_TTY.bind_to` → `ProcessData::new`（argv=原始 args）→ scope 内 `add_stdio`（失败 stage=stdio-open）→ `Thread`/`AxTaskExt` → `spawn_task`+`add_task_to_table` → `task.join()` → root `unmount_all`+`flush` → diag 上报 → 既有 30 s 物理 drain（done/deadline 如实报告）→ wfi park。移除首轮 resolve/read witness、全部 boot 进度 ax_println 标记与 `let _ = (args, envs)`；保留 diag begin/extent/mount 三条进度行（Verification 步骤 2 的"首轮 stage 序列至 mount-ok"观察面）。

### Deviations from Plan

- 执行顺序：契约 Implementation Guidance 为"先 2.1 构建门 → 板测 S1/S2 → 最后 2.2 门"；本回合把 2.2 门提前到板测交接之前执行，并以 `make k3-rootfs` 收尾重产出板测 FIT。原因：板测由用户异步执行，工作树必须留存正确形态的 FIT（同名输出会被旧模式构建覆盖；与首轮 1.2 契约"更优先把旧模式检查排在新模式最终打包前"一致）。非实质，契约语义与验证面不变。
- diag 进度行保留范围：契约仅要求移除"boot 进度 ax_println 标记"；begin/extent/mount 三条 diag_line 为首轮已验收观察面、Verification 步骤 2 的板测依赖，予以保留。非实质。

### Self-Review Findings

- Gate 3（spec compliance）：分支装配顺序与 QEMU 分支逐行镜像；失败层 stage 命名沿用首轮风格；range（dtb-pointer/dtb-header/dtb-parse/bootargs）与 FS 失败层保留；Forbidden 全清（无 init_memory_root、无第二 driver/RX 消费者、无内嵌载荷 spawn、无共享 loader/process/tty 语义修改、无新增 feature edge、ISS01 未触碰）；Invariants 保持（单 hart、预留窗口、唯一 block/ROOT_FS owner、唯一 UART/copier、mount_all 后 tty-reader 为唯一 RX 消费者、失败分层无 fallback）。
- Gate 3（code quality）：两形状定向审计零新增 warning；无死代码/重复实现/身份型证据机制；join 退出码与 fs 收尾经 diag 上报。
- Minor（pre-existing，不属本回合变更面）：Makefile:31 全局 `ifeq ($(shell test $(SMP) -gt 1 ...))` 在 SMP 未设时输出 `test: -gt: unary operator expected` shell 噪声；构建路径不受影响，首轮已存在。
- Minor（pre-existing，Iteration 000 Review finding 5 沿袭）：k3-rootfs 形状下本地 axfs-ng `unused import: prelude` 警告。

### Persisted Evidence

None required（计划 Mode: none；上述命令决定性输出/退出码与待补的一句话板测结论足够）。

### Experience Candidates

- **Issue 候选**（当前 change 任务范围外的实质平台缺陷，证据见 Blocker Handoff）：多段空闲内存平台上，`axruntime::init_allocator` 以 `.bss` 后紧邻 FREE 区（而非最大区）初始化单区间 `BitmapPageAllocator`，其余空闲区只进 slab 字节分配器；页粒度分配（COW 帧/页缓存/页表/slab 增长）全部挤该小区，在预留窗口切割 RAM 的平台（K3 MS18 起）上耗尽为历史依赖的运行期 OOM。QEMU 单段大区不触发。由用户指示后经 `openspec-experience-recorder` 落账。

### Unresolved Issues

- 无阻塞项。板测步骤 1–6（S1 交互 + `exit` 收尾 + S2 三变异）待执行人按 Verification 节操作并一句话记录；A1/A2 的 GREEN 判定依赖该记录。change `tasks.md` 中 2.1/2.2 勾选随板测见证与本 Response 收口一并更新。

## Plan Review

- Review Result: replan-required
- Reviewed: 2026-10-08（独立阅读 Act Response、工作树全量 diff、`axruntime` `init_allocator` 源码、entry.rs 分支实施、四处临时诊断的 cfg 门控核查）

**Findings**

1. 阻塞（基线变化/新证据，Act 根因定位经源码独立证实）：`axruntime-0.3.0-preview.2/src/lib.rs::init_allocator` 对 `.bss` 之后出现的**首个** FREE 区无条件中选为页分配器唯一主区（立即 break，不比较大小），其余 FREE 区仅经 `global_add_memory` 进入 slab 字节分配器。K3 上 DTB 预留窗 `[0x181000000,0x182000000)` 使该区仅 `[0x180390000,0x181000000)` = 0xC70000 = 12.4 MiB；页粒度分配（COW demand frame、页缓存页、页表页、slab 增长切页）全体挤兑，动态用户态运行期耗尽（Act 三轮板测串口证据：66.28 s populate 失败、`fault area=[VA:0x1000,VA:0xc6000)`、三处 file 路径 warn 沉默指向 `alloc_frame`）。QEMU 单段大区不触发。A1/A2 被阻塞，属设计级布局问题，且用户指示与 ISS01（预留窗收档）联动解决——超出当前执行契约（rework 不得扩大范围），需修订设计 D1 并重建执行契约 → replan。
2. Minor（Act 偏离，记录在案）：justrun `SKIPPED` 记录依据「本轮改动全部位于 cfg(k3-rootfs) 之内」不准确——`crates/axfs-ng/src/highlevel/file.rs`（两处）与 `kernel/src/mm/aspace/backend/file.rs`（一处）的 warn 诊断**未加 cfg**，QEMU 形状编译面实际可见（warn-only 错误路径打印；QEMU build 门已过，无验收影响）。修复轮移除全部四处诊断、共享文件 diff 归零后，justrun 判定以「净共享 diff 为零」重立。
3. Act 实施独立核证通过：entry.rs k3-rootfs 分支装配与声明一致（mount_all→alarm→resolve→load→stdio→spawn/join→unmount_all+flush→30 s drain→wfi park；stage=resolve/resolve-path/load/stdio-open 齐备；无残留 boot ax_println 标记；range/FS 失败层保留）；契约「预期不改其他文件」在产品面达成（四处诊断改动为 blocker 调查期产物且已在 Blocker Handoff 单列）。
4. 观察项（非缺陷，候选保留）：axruntime 单区间选区策略对多段 FREE 平台是通用弱点；本轮修复为平台局部布局调整，不解决通用策略。Act 已列为 Issue 候选（含证据），落账归 `openspec-experience-recorder`（用户指令）。

**Acceptance Gaps**

A1（S1 板测）与 A2（S2 三变异）未达成——板测因 demand-page OOM 阻塞，需布局修复后在新 FIT 上执行。A3 自动门已通过（来源 Act Response，采信），但布局修复改动后须复跑。

**Evidence**

- 采信 Act Response：构建/尺寸门（k3-rootfs 1081536 B、fit 1234464 B）、host 27/27、`make k3-shell`/`k3-stdio`/QEMU build exit 0、三轮板测串口证据与根因链。
- Plan 独立检查：axruntime 源码选区逻辑、四处诊断 diff 的 cfg 门控、entry.rs 分支逐行、R75/R76 登记（references.md）。

**Follow-up Decision**

replan-required：修复路径（DTB 承载窗上移 + rootfs 窗收档 + 专用 ITS）修订设计 D1，并按用户指示纳入 ISS01（窗口 1 GiB→256 MiB 收档），需要新的自包含执行契约与 Gate 2；已完成的 2.1 代码与 2.2 自动门结论未失效，直接复用为基线，不重复实施。

**Iteration Plan Update**

Iteration 001 任务由 2.1–2.2 扩为 2.1–2.3：新增 2.3「RAM 布局修复与诊断清理」（DTB 窗 `[0x2ef000000,0x2ef100000)`、rootfs 窗 `[0x240000000,0x250000000)`、k3-rootfs 专用 ITS/FIT、移除四处临时诊断）；Acceptance 增补 A4（布局修复与 ISS01 证据）。Map 依赖与独立验收边界相应更新。

**Next Cycle**

`001-replan.md`（同一 Iteration 目录）。

**Next Iteration**

None。
