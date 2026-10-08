# Iteration 001 / Cycle 001: RAM 布局修复轮（replan）

## Plan Context

- Status: ready
- Cycle Type: replan

**Cycle Scope**

- Change tasks: 2.3（新增，主实施）；2.1/2.2 残余（板测与门复跑）。
- Acceptance gaps: A1（S1 板测）、A2（S2 三变异）——Cycle 000 板测 blocked 遗留。
- Repair items: None（replan 使用更新后的全局 task 2.1–2.3）。
- Inherited scope: D1-r1 修订后的 RAM 布局与 ISS01 收档；Cycle 000 已完成的 2.1 代码（entry.rs 分支重写）与 2.2 自动门结论直接复用，不重复实施；单 hart、临时启动、官方存储与持久环境保持完整。
- Excluded scope: S3–S8 正式验收、S6 RESET 见证与全规格覆盖（Iteration 002）；真实存储、网络、SMP、性能；axruntime/axalloc 通用选区策略修复（Issue 候选保留，归 Recorder）。

**Background**

父 Cycle：`000-initial.md`（板测 blocked，Plan Review replan-required）。板测三轮在 busybox 动态运行期 demand-page OOM（66.28 s populate 失败）；根因经源码证实：axruntime `init_allocator` 以 `.bss` 后首个 FREE 区为页分配器唯一主区，DTB 预留窗 `[0x181000000,0x182000000)` 把该区切成 12.4 MiB，其余空闲区只进 slab。用户 2026-10-08 指示：阅读 Act 报告、定位问题、把 ISS01 纳入、制定计划一起解决、给出新 Cycle。据此 D1 修订为 D1-r1（DTB 窗上移至 RAM 顶部、rootfs 窗收档 256 MiB、k3-rootfs 专用 ITS/独立 FIT 名），本 Cycle 在既有 2.1 实施之上完成布局修复、清理临时诊断并重开板测。

**Investigation Facts**

- Current Baseline: 工作树含 Cycle 000 全部未提交改动（entry.rs 分支重写、四处临时诊断、首轮布局与 host 测试）；2.2 自动门已过（`make k3-rootfs`/`k3-shell`/`k3-stdio`/QEMU build exit 0、host 27/27、尺寸门 kernel 1081536 B、fit 1234464 B）；板测 A1/A2 因 OOM 未达成。
- 根因（源码证实，registry `axruntime-0.3.0-preview.2/src/lib.rs::init_allocator`）：`use_next_free` 在 `.bss` 区后置位，随后**首个** FREE 区立即 break 中选为 `global_init`（页分配器唯一主区，单区间 BitmapPageAllocator），不比较大小；其余 FREE 区仅 `global_add_memory`（slab 字节分配器）。当前 DTB 窗 `[0x181000000,0x182000000)` → 主区 `[0x180390000,0x181000000)` = 0xC70000（12.4 MiB）；COW demand frame、页缓存页、页表页、slab 增长切页全体挤兑 → 动态用户态运行期耗尽。QEMU 单段大区不触发。三轮板测串口证据见 Cycle 000 Blocker Handoff（含 `fault area=[VA:0x1000,VA:0xc6000)` 与三处 file 路径 warn 沉默指向 `alloc_frame`）。
- 新布局区域账本（D1-r1）：`.bss` 端 ~0x180390000；FREE 首段 `[0x180390000,0x240000000)` ≈ 3.21 GiB 连续（页分配器主区，`.bss` 后无任何预留）；rootfs 窗 `[0x240000000,0x250000000)` 256 MiB；FREE `[0x250000000,0x2ef000000)` ≈ 2.16 GiB（slab）；DTB 窗 `[0x2ef000000,0x2ef100000)` 1 MiB；FREE 尾段 `[0x2ef100000,0x2f0000000)` ≈ 0.94 MiB（slab）。
- ITS/构建面：`tools/starry-k3.its`（FDT load=0x181000000）与 `k3-fit` → `StarryOS_riscv64-k3.fit` 为旧模式共用，保持不变；新 `tools/starry-k3-rootfs.its`（FDT load=0x2ef000000，kernel 节点不变）+ 新 `k3-rootfs-fit` 目标 → `StarryOS_riscv64-k3-rootfs.fit`；`k3-rootfs` 改调 k3-rootfs-fit。FDT 新址在 block 11（boot 页表 ram-rootfs 门控下块 5..=11 已映射，无需改 boot.rs）；U-Boot `fdt_high` 流程不动（无搬迁、就地改写行为已有首轮 witness）。
- 临时诊断（本轮全部移除；移除后四文件 diff 相对 HEAD 必须为空）：`kernel/src/mm/loader.rs` argv 读回（cfg k3-rootfs）、`kernel/src/mm/aspace/mod.rs` fault area warn（cfg k3-rootfs）、`kernel/src/mm/aspace/backend/file.rs` pt.map warn（未门控）、`crates/axfs-ng/src/highlevel/file.rs` evict/read warn×2（未门控）——后两处未门控即 Cycle 000 justrun SKIPPED 依据不准确的原因。
- `ram_layout.rs` 常量当前值：`RAM_DTB_BASE=0x181000000`、`RAM_DTB_WINDOW_END=0x182000000`、`RAM_ROOTFS_BASE=0x240000000`、`RAM_ROOTFS_WINDOW_END=0x280000000`；parse/validate/`RAM_ROOTFS_RESERVED` 全部由常量派生，窗口移动不需要改校验逻辑；entry.rs 的 DTB 指针校验同样随常量流动。host 测试 `full_window_extent_is_accepted` 硬断言 `0x4000_0000` 须改 `0x1000_0000`；其余 DTB/差集断言均符号化自动适配。
- 板测输入与流程沿用：min 镜像 128 MiB（≤256 MiB 窗）、R68 启动序、rootfs 装载基址与 `initrdmem` 格式不变；仅 FIT 文件名更换。

**Implementation Guidance**

顺序：2.3（常量 + ITS/Makefile + host RED→GREEN + 诊断清理）→ 构建门（2.2 复跑面）→ 板测 A1/A2（2.1 残余）。诊断移除按 Act Response「临时诊断代码」清单做精确反向编辑，不顺手改动共享文件其他部分。

**Task Contracts**

### 2.3: RAM 布局修复（D1-r1）与诊断清理

- Requirement/Scenario: S1 的达成前置（页分配器主区恢复连续大块）；ISS01 收档；S9（旧 ITS/FIT/模式不变）；D1-r1。
- Depends on: None（本轮内先于板测与门复跑）。
- Targets: `crates/axplat-riscv64-k3/src/ram_layout.rs`（`RAM_DTB_BASE=0x2ef000000`、`RAM_DTB_WINDOW_END=0x2ef100000`、`RAM_ROOTFS_WINDOW_END=0x250000000`）；`tools/starry-k3-rootfs.its`（新）；`Makefile`（新 `k3-rootfs-fit`：沿用既有 dtc model 检查，`mkimage -f tools/starry-k3-rootfs.its StarryOS_riscv64-k3-rootfs.fit` + `dumpimage -l`；`k3-rootfs` 末行改调 k3-rootfs-fit；`.PHONY` 补）；`tests/k3-embedded-user-host/tests/ram_rootfs.rs`（full-window 断言改 `0x1000_0000`；新增布局不变量测试）；四处临时诊断文件（反向移除）。
- Current behavior: DTB 窗位于 0x181000000 使页分配器主区 12.4 MiB，动态用户态 OOM；rootfs 窗 1 GiB 中 896 MiB 未用（ISS01）；k3-rootfs 与旧模式共用 FIT 产物名；四处诊断在工作树。
- Required behavior: 新常量生效且单源派生（parse/validate/预留/boot 页表覆盖逻辑不变）；k3-rootfs 产出独立文件名 FIT，FDT 装载地址 0x2ef000000；旧 `starry-k3.its`/`k3-fit` 与其余模式构建输出逐字节不变；四处诊断移除后四个共享文件 diff 相对 HEAD 为空。host 新增布局不变量：全部预留窗不与 `(内核镜像端, RAM_ROOTFS_BASE)` 开区间相交（即首个 post-`.bss` FREE 区连续到 rootfs 窗基），且 rootfs 窗恰为 256 MiB。
- Preserve: 首轮 range/FS 失败层 stage 语义与解析逻辑；U-Boot 操作结构（仅 FIT 文件名变化）；原始 `rootfs-riscv64.img` 只读；MS17 与其他 K3 模式行为；rootfs 装载基址 0x240000000 不变。
- Forbidden: 修改 axruntime/axalloc registry 源或其选区策略；修改共享 loader/进程/tty 语义；留下任何诊断输出；新增判定器或身份型证据；扩大窗口或改 `initrdmem` 语法。
- Test witness: host 先 RED——新增布局不变量测试在当前常量下失败（DTB 窗 0x181000000 落于 kernel 与 rootfs 窗之间）；full-window 断言在常量未改时失败。改常量与断言后 GREEN。
- GREEN condition: host 套件全过（27+新增）；`make k3-rootfs` 产出 `StarryOS_riscv64-k3-rootfs.fit` 且 `dumpimage -l` 显示 FDT load 0x2ef000000；`make k3-fit` 产出旧名 `StarryOS_riscv64-k3.fit` 不变；四处共享文件 `git diff` 为空。
- Verification: host 套件命令（沿用 Cycle 000 全文）；`make k3-rootfs` + `dumpimage -l StarryOS_riscv64-k3-rootfs.fit`；`make k3-fit`；`make k3-shell`、`make k3-stdio`、`make ARCH=riscv64 SMP=1 build`；`make k3-rootfs SMP=2`（Makefile 已改动，守卫须复证拒绝，exit≠0）；`git diff --stat` 诊断四文件归零核对；互斥 compile_error 守卫文件未触碰，采信 Iteration 000 结论不重跑。
- Stop when: 板上 U-Boot 无法在 0x2ef000000 装载/就地改写 FDT（fdt_high 语义与预期不符）；新布局下仍出现页分配耗尽（根因判断错误，返回调查，不盲改）；实施需要触碰 axruntime/共享语义；三次失败按公共规则停止。

### 2.1（残余）: 板上 S1/S2 见证（新布局 FIT）

- Requirement/Scenario: S1、S2 三变异、S8 收尾 smoke；契约正文见 Cycle 000（其 Plan Context 不改写，语义继续有效）。
- Depends on: 2.3。
- Targets: 无新代码目标；板测按本 Cycle Verification 修订流程执行（FIT 文件名更换）。
- Required behavior: 板测会话在新 FIT 上达成 Cycle 000 契约 2.1 的 GREEN 条件：S1（banner→提示符→`/bin/ls /bin`、`/bin/cat /etc/os-release`→回提示符；会话末 `exit` 观察进程 exit、fs 收尾、drain/park 各 diag 行）；S2 三变异各一次（resolve 层 / load 层×2，无提示符、无 fallback）。
- Preserve / Forbidden / Test witness / GREEN / Stop when: 沿用 Cycle 000 契约 2.1 对应条目；板测不可操作时写 blocked 与所缺行为结果，不把等待当任务。
- Verification: 见本 Cycle Verification 板测步骤 1–6。

### 2.2（复跑）: 门复跑与 justrun 判定

- Requirement/Scenario: S9；契约正文见 Cycle 000。
- Depends on: 2.3。
- Targets: 无产品代码目标。
- Required behavior: 2.3 改动后复跑全部自动门；justrun 判定依据更正为「2.3 后净共享 diff 为零且无共享语义变化 → SKIPPED」；若任一共享文件残留未门控变化或共享 loader/进程/tty/pseudofs/syscall 语义被触碰 → 运行 QEMU 同源 rootfs justrun 并在普通 shell 执行外部命令（`-snapshot`，原始 image 不写）。
- GREEN condition: 全部门 exit 0、host 套件通过、justrun 按条件执行通过或以正确依据记录 SKIPPED。
- Stop when: 任一旧模式构建或 host 测试回归且根因在 2.3 改动；需修改旧模式行为才能通过时返回 Plan。

**Invariants**

沿用 Cycle 000 Invariants 全部，另加：`.bss` 后至 rootfs 窗基之间无任何预留窗（页分配器主区连续性，由 host 布局不变量测试承载）；旧 `starry-k3.its`/`k3-fit` 与其余模式构建输出不变；共享文件诊断残留为零。

**Non-goals**

S3–S8 正式验收、S6 见证与全规格覆盖（Iteration 002）；axruntime/axalloc 通用选区策略修复（Issue 候选保留，Recorder 落账）；rootfs 装载基址或 `initrdmem` 语法变化；真实存储、网络、SMP、性能。

**Acceptance**

- A1（S1，沿用 Cycle 000）：新布局 FIT 板测进入普通交互 `/bin/sh`，`/bin/ls /bin` 与 `/bin/cat /etc/os-release` 输出实际内容并回到提示符；会话末 `exit` 收尾行到位——同时构成布局修复的功能性见证（越过 Cycle 000 的 populate 死亡点）。
- A2（S2，沿用 Cycle 000）：三变异各一次命中对应失败层，无 fallback。
- A3（S9，沿用 Cycle 000 并复跑）：2.3 后全部门与 host 套件通过；justrun 按更正依据判定。
- A4（新增，D1-r1/ISS01）：新常量与 host 布局不变量 GREEN；`dumpimage -l` 显示 k3-rootfs FIT 的 FDT load=0x2ef000000 与独立文件名；`make k3-fit` 旧产物名不变；四处共享诊断 diff 归零。ISS01 关闭由本证据支撑，落账归 Recorder（用户指令）。

**Verification**

自动命令（退出码即判定；host 命令沿用 Cycle 000 Verification 全文）：

- host 套件（含新增布局不变量测试，RED→GREEN）
- `make k3-rootfs`；`dumpimage -l StarryOS_riscv64-k3-rootfs.fit`（FDT 0x2ef000000）
- `make k3-fit`（旧产物名 `StarryOS_riscv64-k3.fit` 不变）
- `make k3-shell`；`make k3-stdio`；`make ARCH=riscv64 SMP=1 build`
- `make k3-rootfs SMP=2`（期望拒绝）
- `git diff --stat`：`crates/axfs-ng/src/highlevel/file.rs`、`kernel/src/mm/aspace/backend/file.rs`、`kernel/src/mm/aspace/mod.rs`、`kernel/src/mm/loader.rs` 四文件归零

板测（外部能力边界，执行人串口直接观察并一句话记录，各一次）：

1. R68 进 U-Boot；`fatload usb 0:0 0x240000000 rootfs-riscv64-min.img`（134217728 bytes read）；`setenv starry_rootfs_size ${filesize}`；`fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3-rootfs.fit`；`setenv bootargs initrdmem=0x240000000,0x${starry_rootfs_size}`；`setenv fdt_high 0xffffffffffffffff`；`bootm 0x2e0000000`。直接观察 `stage=dtb-ok paddr=0x2ef000000` 与既有 stage 序列至 mount-ok。
2. S1 会话：banner→提示符→`/bin/ls /bin`、`/bin/cat /etc/os-release`→回提示符；会话末 `exit`，观察 exit/fs 收尾/drain/park。
3. S2-a：/tmp 副本 `debugfs -w` 删除 `/bin/sh` → 装载该副本 bootm → resolve 层失败 stage，无提示符。
4. S2-b：副本删除 `/lib/ld-musl-riscv64.so.1` → load 层失败 stage。
5. S2-c：副本将 `/bin/busybox` 替换为非 ELF（`/bin/sh` 符号链保留）→ load 层 InvalidExecutable stage。
6. 每次变异后外部 RESET 恢复；原始 image 与官方介质全程不写；不 saveenv。

板上不可操作时反馈明确写 blocked 与所缺行为结果，不把等待作为可执行任务，不展开 Iteration 002。

**Gate 2 Readiness**

PASS：根因经源码与三轮板测证据闭合；D1-r1 设计修订与 2.3 契约自包含；2.1/2.2 残余以 Cycle 000 契约为准且不要求 Act 回读链；无 TBD；无需要 Act 决定的契约语义。计划获批后 Status 由 draft 改 ready。

**Persisted Evidence**

- Mode: none

当轮 Act Response 的原生命令决定性输出/退出码与执行人一句话板测结论足够；不创建 Evidence 占位目录。

**Risks and Notes**

- U-Boot 在 0x2ef000000 的 FDT 装载/就地改写为板上直接观察点（步骤 1 的 dtb-ok 行）；失败按 Stop when 返回。
- `.bss` 端 0x180390000 为当前构建实测值；其漂移只影响区域大小不影响布局结构——host 不变量按窗口相对关系断言，不绑定 `.bss` 具体值。
- ISS01 关闭后的剩余松弛：256 MiB 窗对 128 MiB 镜像留 128 MiB 余量，为档位制设计取舍（D1-r1 记录），非缺陷。
- axruntime 单区间选区策略的通用弱点未修复（Issue 候选保留）；后续任何新增预留窗必须遵守「不落 `.bss` 后至 rootfs 窗基」约束，host 布局不变量长期承载。

## Act Response

- Status: reported

自动面（任务 2.3 全部、2.2 复跑）与板测 S1 会话均已完成；S2 三变异与 `exit` 收尾观察按用户明确豁免跳过（原话留档见 Board Witness）。A4 布局修复证据完整；A1 以等效命令序列达成（`cd /` + `ls ./` 替代契约点名的 `/bin/ls /bin`、`/bin/cat /etc/os-release`，同为外部命令 + 真实文件系统内容 + 回到提示符）。

### Actual Changes

- `crates/axplat-riscv64-k3/src/ram_layout.rs`：D1-r1 常量生效——`RAM_ROOTFS_WINDOW_END=0x2_5000_0000`（256 MiB 窗，ISS01 收档）、`RAM_DTB_BASE=0x2_ef00_0000`、`RAM_DTB_WINDOW_END=0x2_ef10_0000`（真 1 MiB，位于 rootfs 窗上方）；`RAM_ROOTFS_BASE` 不变。parse/validate/`RAM_ROOTFS_RESERVED`/mem.rs 预留/entry DTB 校验全部单源派生，零逻辑改动。
- `tests/k3-embedded-user-host/tests/ram_rootfs.rs`：`full_window_extent_is_accepted` 断言改 `0x1000_0000`；新增三条布局不变量——`first_post_kernel_free_range_is_contiguous_to_rootfs_base`（经既有 free_ranges fixture 证明首个 post-kernel FREE 区连续到 rootfs 窗基，承载 axruntime 选区根因防线）、`rootfs_window_is_exactly_256mib`、`dtb_window_stays_1mib_at_or_above_rootfs_window`；`valid_dtb_handoff_is_accepted` 的陈旧地址注释更新为窗口派生表述。
- `tools/starry-k3-rootfs.its`（新）：k3-rootfs 专用 ITS，kernel 节点与旧 ITS 相同（load/entry 0x180000000），FDT `load = <0x2 0xef000000>`。
- `Makefile`：新增 `k3-rootfs-fit` 目标（沿用 dtc model 检查 + `mkimage -f tools/starry-k3-rootfs.its StarryOS_riscv64-k3-rootfs.fit` + `dumpimage -l`）；`k3-rootfs` 末行改调 `k3-rootfs-fit`；`.PHONY` 补 `k3-rootfs-fit`。旧 `k3-fit`/`starry-k3.its` 未触碰。
- 四处临时诊断移除：`kernel/src/mm/loader.rs`、`kernel/src/mm/aspace/mod.rs`、`kernel/src/mm/aspace/backend/file.rs`、`crates/axfs-ng/src/highlevel/file.rs` 恢复到 HEAD（移除前经完整 diff 核对四文件工作树改动仅含诊断添加，恢复即契约要求的「diff 相对 HEAD 为空」的字节级实现）。

### RED→GREEN（Test Witness）

- RED（常量未改，仅改测试）：`test result: FAILED. 17 passed; 4 failed`——失败恰为 `dtb_window_stays_1mib_at_or_above_rootfs_window`、`first_post_kernel_free_range_is_contiguous_to_rootfs_base`、`full_window_extent_is_accepted`、`rootfs_window_is_exactly_256mib` 四项预期；连续性断言实测首个 FREE 区终点 `0x181000000`（旧 DTB 窗基）≠ `0x240000000`。RED 跑经管道截尾未单独捕获退出码，判定以原生 FAILED 输出与失败集合为准。
- GREEN（常量修订后）：`test result: ok. 21 passed`（ram_rootfs，18 旧 + 3 新），整套（contract 9 + ram_rootfs 21 = 30）exit 0。

### Gate 4 Verification（2.2 复跑面，全部新鲜运行）

- `make k3-fit` → exit 0；产物 `StarryOS_riscv64-k3.fit`（旧名），FDT Load Address `0x181000000` 不变。
- `make k3-shell` → exit 0；`make k3-stdio` → exit 0；`make ARCH=riscv64 SMP=1 build` → exit 0（QEMU 普通共享编译门，12.43s Finished release）。
- `make k3-rootfs` → exit 0；`dumpimage -l StarryOS_riscv64-k3-rootfs.fit`：kernel 1081536 B @ 0x180000000、FDT 150998 B @ **0x2ef000000**；尺寸门：.bin 1081536 < 16 MiB、FIT 1234496 < 16 MiB、DTB 150998 < 1 MiB。
- `make k3-rootfs SMP=2` → `ERROR: k3-rootfs is a single-hart mode...` exit 2（Makefile 本轮有改动，守卫复证拒绝）。
- `git diff --stat`（四诊断文件）→ 空；全仓 `grep ms18-diag`（kernel/crates/tests）→ 零残留；entry.rs 仅剩契约保留的 `[ms18]` stage/fault/begin/extent/mount/exit 观察面。
- 互斥 compile_error 守卫（kernel/src/lib.rs）本轮未触碰，采信 Iteration 000 accepted 结论，不重跑。
- justrun：`SKIPPED: 净共享 diff 为零`——四共享文件已归 HEAD，其余工作树改动全部位于 k3-rootfs feature/cfg 门内或为本模式新文件，共享 loader/进程/tty/pseudofs/syscall 语义未触碰；QEMU SMP=1 构建门以当前 entry.rs 新鲜通过。
- `openspec validate ms18-com260-kit-standard-shell-rootfs` → valid，exit 0。
- 板测 FIT 已在全部门之后由 `make k3-rootfs` 最后产出（本次门序：k3-fit → k3-shell → k3-stdio → QEMU build → k3-rootfs 收尾），`.bin` 为 k3-rootfs 形状，无同名误用。

### Board Witness（001-replan 板测，2026-10-08，执行人串口记录）

- 步骤 1–2（S1，PASS）：`fatload` min 镜像与 `StarryOS_riscv64-k3-rootfs.fit` 后 `bootm`，串口实测 `[ms18] rootfs stage=extent base=0x240000000 size=0x8000000 dtb_paddr=0x2ef000000 dtb_totalsize=155648`——**D1-r1 新 DTB 窗 0x2ef000000 板上落位**，U-Boot 装载/就地改写无异常；`mount fs=ext4` 后 init.sh banner（`Welcome to Starry OS!`、HOME/SHLVL/PWD 环境、apk 提示）→ `starry:~#` 提示符——普通 `/bin/sh`（动态 ET_DYN BusyBox + musl 解释器）经 ordinary loader 真实运行，**越过 Cycle 000 的 demand-page OOM 死亡点，布局修复的功能性见证成立**。
- 交互观察（执行人原始串口记录）：`ls ./`（~ 下为空属正常）→ `cd /` → `ls ./` 输出完整根目录内容（bin dev etc home lib lost+found media mnt opt proc root run sbin srv sys tmp usr var）并回提示符——外部命令 fork/exec/wait 与真实 ext4 内容达成；两次 `^C` 后提示符恢复（SIGINT 对照）。
- 步骤 3–6 豁免：用户原话「很棒，成功进入shell，多余的测试不做，我们核心目的达到就行」。风险留档：S2 失败分层（resolve/load/InvalidExecutable）本轮未获板上直接见证（Cycle 000 的 range 层 `MissingInitrdmem` 板测与 host 契约测试仍有效，覆盖 range/resolve 参数面）；`exit` 收尾的 join/unmount/flush/drain/park diag 行未观察（drain/park 契约本身已在 Iteration 000 板测验收，同栈未变）。
- 本轮未 saveenv，原始镜像与官方介质未写。

### Deviations from Plan

- 诊断移除方式：契约写「按 Act Response 清单做精确反向编辑」，实际以 `git checkout -- <四文件>` 恢复 HEAD。移除前已核对四文件完整 diff 仅含诊断添加（两处为对原行的包裹替换），恢复后终态与反向编辑逐字节相同且直接满足「diff 归零」验收。非实质。
- 现场事实修正：旧 DTB 窗 `[0x181000000,0x182000000)` 实际跨度 16 MiB（源码注释误写 1 MiB）；D1-r1 新窗为真 1 MiB，ISS01 收档因此同时归还该 15 MiB 与 rootfs 窗的 768 MiB。契约只规定新窗边界，本项为注释纠偏（新常量注释与 host 断言 `dtb_window_stays_1mib_at_or_above_rootfs_window` 已承载），非范围变化。
- host 测试同步更新一处陈旧注释（`valid_dtb_handoff_is_accepted` 的「loaded at 0x181000000」→ 窗口派生表述），与常量修订同文件同任务，非实质。

### Self-Review Findings

- Gate 3（spec compliance）：2.3 Targets 五项全部落地且无计划外文件；新常量单源派生（mem.rs 预留、entry DTB 校验、host 测试同源消费，无第二份边界复制）；k3-rootfs FIT 独立命名且 FDT load=0x2ef000000（dumpimage 直接证据）；旧 ITS/`k3-fit`/其余模式构建输出不变；host 新增布局不变量与 full-window 断言 RED→GREEN；boot 页表块 5..=11 已覆盖新 rootfs 窗（块 9）与新 DTB 窗（块 11），`boot.rs` 零改动，契约判断核实。
- Gate 3（code quality）：新 Makefile 目标与兄弟目标同型（dtc 检查/mkimage/dumpimage/注释）；测试仅覆盖可达布局不变量，复用既有 fixture，无判定层、无身份型证据机制；未触碰 axruntime/axalloc、共享 loader/进程/tty 语义、`initrdmem` 语法、rootfs 装载基址 0x240000000；Forbidden 全清（无窗口扩大、无诊断残留、无守卫文件改动）。
- 遗留 Minor（pre-existing，未触碰）：本 feature 组合下本地 axfs-ng `unused import: prelude` 警告（Iteration 000 Review finding 5 沿袭）；Makefile:31 SMP 未设时的 shell 噪声（Cycle 000 沿袭）。均不阻塞。

### Persisted Evidence

None required（计划 Mode: none；上述命令决定性输出/退出码与待补的一句话板测结论足够）。

### Experience Candidates

None（本轮无新的可重复操作路径或 change 范围外实质缺陷；axruntime 选区弱点已按 Cycle 000 候选登记为 ISS01 关联背景，落账归 Recorder）。

### Unresolved Issues

- 无阻塞项。S2 三变异与 `exit` 收尾按用户豁免跳过（原话与风险见 Board Witness）；S1 与 A4 证据完整，Response 已收口 `reported`，交 Plan Review 审计豁免充分性与 Acceptance 判定。

## Plan Review

- Review Result: accepted
- Reviewed: 2026-10-08（独立核对：ram_layout 常量与注释、新 ITS、Makefile `k3-rootfs-fit`、三条新布局不变量测试逐行、诊断四文件 `git diff` 0 行与 `ms18-diag` grep 0 处、工作树变更面对照 Act 清单、板测 witness 采信）

**Findings**

阻塞项：无。非阻塞项：

1. 用户豁免（原话留档于 Board Witness）：S2 三变异（resolve/load/InvalidExecutable 板上层见证）与 `exit` 收尾观察跳过。残余覆盖：range 层 `MissingInitrdmem` 板测与 host 契约测试仍有效；loader 失败层为 QEMU 同源已验收路径。后续轮需要时可补测，不阻塞本轮。
2. 用户现场等效裁决（原话留档）：A1 见证形式由点名的 `/bin/ls /bin`、`/bin/cat /etc/os-release` 等效为 `cd /`+`ls ./`（同为外部命令 + 真实 ext4 内容 + 回提示符）；S1 能力结论（普通 shell + 外部程序）有直接证据，`/bin/cat` 文件读取未逐字执行，记为简化见证形式。
3. Act 现场事实修正（纠偏，采纳）：旧 DTB 窗实际 16 MiB——round-1 源码注释误写 1 MiB，design D1 原文与 ISS01 Issue 文本随之失准；新窗为真 1 MiB。ISS01 关闭记账应按修正数字（原浪费 1 GiB+16 MiB → 现 256 MiB 档位含 128 MiB 余量 + 1 MiB DTB 窗）。
4. Minor（Plan 自身，记录在案）：本 Cycle 区域账本中间/尾段近似值有滑差（实际 ≈2.49 GiB / 15 MiB）；不变量测试按窗口相对关系断言，不受影响。
5. Deviation 核证：诊断以 `git checkout` 恢复 HEAD——移除前经完整 diff 核对四文件仅含诊断添加，终态与反向编辑字节一致；diff 归零/零残留已由 Plan 独立复核。RED 跑经管道未单独捕获退出码，以原生 FAILED 输出与失败集合判定，符合验证规则。

独立核验通过的关键面（代码 review，非 Act 自检）：

- `ram_layout.rs`：`RAM_ROOTFS_WINDOW_END=0x2_5000_0000`、`RAM_DTB_BASE=0x2_ef00_0000`、`RAM_DTB_WINDOW_END=0x2_ef10_0000`；注释承载根因（post-kernel free 连续性）；parse/validate/预留单源派生零逻辑改动。
- 新 `tools/starry-k3-rootfs.its`：kernel load/entry 0x180000000 不变，FDT `load = <0x2 0xef000000>`；旧 `starry-k3.its`/`k3-fit` 未触碰（Makefile diff 仅新增段）。
- Makefile：`k3-rootfs-fit` 与兄弟目标同型（dtc 检查/mkimage/dumpimage）；`k3-rootfs` 改调之、SMP 守卫保留（Act 复证 exit 2）、`.PHONY` 补齐。
- 三条新测试与契约一致：首个 post-kernel FREE 区终点==`RAM_ROOTFS_BASE`（根因防线，旧常量下必 RED）、rootfs 窗恰 256 MiB、DTB 窗恰 1 MiB 且不低于 rootfs 窗端。RED 4 失败集合与 GREEN 21+9=30 吻合。
- 板测 witness：`stage=extent ... dtb_paddr=0x2ef000000`（新窗板上落位）→ mount → banner → `starry:~#` → `cd /`+`ls ./` 全根目录 → 回提示符；两次 `^C` 后提示符恢复（超出本轮要求的 SIGINT 对照）；越过 Cycle 000 demand-page OOM 死亡点——A1 功能面与 A4 布局修复见证成立。

**Acceptance Gaps**

None blocking。A2 与 S8 收尾 smoke 按用户原话豁免并留档风险；A1 以用户现场等效命令序列达成；A3 全部新鲜通过（构建 4 门 + host 30/30 + SMP=2 拒绝 + justrun `SKIPPED` 依据更正为「净共享 diff 为零」，Plan 复核属实）；A4 证据完整。

**Evidence**

- 采信 Act Response：host RED→GREEN（17 passed/4 failed → 21+9=30 exit 0）、构建门/尺寸门/`dumpimage` FDT 0x2ef000000、SMP=2 exit 2、板测 S1 串口记录与豁免原话。
- Plan 独立检查：常量/ITS/Makefile/三新测试逐行、四诊断文件 `git diff` 0、`ms18-diag` grep 0、工作树变更面与 Act 清单一致。

**Follow-up Decision**

accepted。任务 2.1/2.2/2.3 契约与 Invariants 全部满足（豁免项留档）；Iteration 001 完成。布局修复的功能性见证（越过 OOM 死亡点 + 新 DTB 窗板上落位）成立；ISS01 关闭证据已备，落账与数字修正归 Recorder（用户指令）。

**Iteration Plan Update**

None。

**Next Cycle**

None。

**Next Iteration**

`002-shell-behavior-and-reset`（tasks 3.1–3.2：S3–S8、S5 易失写入、S6 官方恢复与全规格覆盖检查；未展开，待用户决策是否实施）。
