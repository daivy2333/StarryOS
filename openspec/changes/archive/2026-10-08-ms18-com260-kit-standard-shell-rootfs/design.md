## Context

需求见 `proposal.md`。调查时 HEAD 为 `9a75c9f3`，分支 k3，开始时工作树 clean。MS17 已验收单 hart timer/AIA、UART copier、tty 和静态用户进程；普通 ELF/exec 路径仍仅有其他平台经验。

参考 tgoskits 通过 UFS、`root=PARTLABEL=rootfs`、普通 VFS loader 启动 `/bin/sh`。本仓 `axfs::init_filesystems` 仅消费单个设备，没有分区选择器；本次 RAM 原始 ext4 镜像不需要 GPT。

已只读观察：`rootfs-riscv64.img` 为 1073741824 字节 clean ext4，块大小 4096，`/bin/sh` 指向 BusyBox；BusyBox 为 RISC-V ET_DYN，解释器 `/lib/ld-musl-riscv64.so.1`。现有 `make/disk.img` 带 errors 标记，不作为 K3 默认输入。原始 image 不被挂载写入。

## Goals / Non-Goals

设计复用已有块设备、ext4、VFS、普通 loader 和用户进程能力；新增范围集中于 RAM 交接与 K3 入口。真实存储、分区、网络、多 hart和持久安装边界见 proposal。

## Decisions

### D1 Separate rootfs load and native boot argument

使用既有小 FIT（kernel + DTB），另行 `fatload` 原始 rootfs 到 RAM。rootfs 固定承载窗口为 `[0x240000000, 0x280000000)`，容量 1 GiB；地址不变，实际 image 大小通过临时 `bootargs` 的 `initrdmem=<address>,<size>` 传递。

该参数只传产品启动所需的物理范围，不含运行身份或指纹。它借用 Linux 已有地址/长度语法，由本模式消费，不声称已支持完整 Linux 命令行或 K/M/G 后缀。当前操作使用十六进制数字。出处：[Linux kernel parameters](https://docs.kernel.org/admin-guide/kernel-parameters.html)。FIT 仍走现有 `bootm`，不依赖 U-Boot raw-initrd 支持或新增 FIT ramdisk：[U-Boot bootm](https://docs.u-boot.org/en/latest/usage/cmd/bootm.html)。

本仓 K3 `phys_ram_ranges` 从 0x180000000 到 0x2f0000000。新模式在 `reserved_phys_ram_ranges` 固定排除上述 rootfs 窗口，以及 DTB 窗口 `[0x181000000,0x182000000)`。DTB 当前 150998 字节，在 1 MiB 窗口内。预留在 allocator 取 free ranges 前生效；axhal 后续从保存的 bootarg 读取 DTB 时仍有效。其他模式不增加这些预留。

| 用途 | 物理范围/地址 | 边界 |
|---|---|---|
| kernel | 0x180000000 起 | `.bin` 必须小于 16 MiB，不碰 DTB |
| DTB | `[0x181000000,0x182000000)` | 仅新模式预留；指针/实际 totalsize 均在窗口内 |
| rootfs | `[0x240000000,0x280000000)` | 起始地址固定；长度 >0、512 对齐且不超过容量 |
| FIT staging | 0x2e0000000 起 | FIT 必须小于 16 MiB，不越 0x2f0000000 |

无需在初始页表映射 rootfs：入口只在 axruntime 建立完整 RAM 映射后访问它。DTB 在现有 boot 1 GiB 页映射内。

排除的方案：将 1 GiB image 嵌入当前 FIT 会超出既有 staging 窗口；使用 UFS 会重新引入官方存储写入决策；USB mass-storage 需要独立驱动。本次不引入这些工作。

#### D1-r1 2026-10-08 replan 修订（Iteration 001 板测根因 + ISS01 收档）

`axruntime::init_allocator` 以 `.bss` 之后**首个** FREE 区为页分配器（单区间 `BitmapPageAllocator`）唯一主区——无条件立即中选、不比较大小；其余 FREE 区仅进 slab 字节分配器。DTB 承载窗位于 0x181000000 时该主区仅 12.4 MiB（`[0x180390000,0x181000000)`），页粒度分配（COW demand frame、页缓存页、页表页、slab 增长切页）全体挤兑，动态用户态运行期耗尽（Iteration 001 三轮板测串口证据，见其 Blocker Handoff）。修订布局，保证 `[.bss 端, rootfs 窗基)` 无任何预留：

| 用途 | 物理范围/地址 | 边界 |
|---|---|---|
| kernel | 0x180000000 起 | 不变；`.bin` < 16 MiB |
| FIT staging | 0x2e0000000 起 | 不变；< 16 MiB，与 DTB 窗无重叠（≥15 MiB 余量） |
| rootfs | `[0x240000000,0x250000000)` | 起点与 U-Boot 装载流程不变；容量收档 256 MiB（ISS01：与实际镜像档位匹配，128 MiB 镜像 2× 余量） |
| DTB | `[0x2ef000000,0x2ef100000)` | RAM 顶部 16 MiB 下界起 1 MiB；k3-rootfs 专用 ITS 的 FDT load 地址与之一致；boot 页表 block 11 覆盖 |

排除方案：patch axruntime 选区策略（改选最大 FREE 区）可通用修复，但引入 registry patch 面、影响所有平台共享 runtime 并强制 QEMU 运行时回归；本仓优先平台局部布局修复，通用策略弱点保留为 Issue 候选（落账归 Recorder）。旧 `tools/starry-k3.its`、`k3-fit` 目标与其余模式输出不变；k3-rootfs 使用专用 ITS（`tools/starry-k3-rootfs.its`）与独立 FIT 文件名（`StarryOS_riscv64-k3-rootfs.fit`）。

### D2 Existing static-backed RAM block device and ext4

使用已存在的 `axdriver_block::ramdisk_static::RamDisk`，由预留窗口建立唯一 `'static mut [u8]`，整个 rootfs 生命周期保持该 owner。它要求基址及长度按 512 字节对齐。入口在构造 slice 前完成 checked bounds 校验，且只构造一次。

直接 kernel feature 启用 `axfs/ext4`、`axdriver/block` 和 `axdriver/dyn`，以及现有 `axdriver_block` 的 `ramdisk-static`；把该 driver 放入 `AxDeviceContainer::from_one` 交给 `axfs::init_filesystems`。`dyn` 采用依赖自身的 Box 设备接口，不调用参考 rdrive/RDIF 运行时。不要启用 `axfeat/fs-ng-ext4` 自动 probe/mount 路径，否则 root 初始化会在 entry 前发生。

选择 static-backed 实现以避免对 1 GiB 镜像再做一次堆分配/复制；不新写 RAM block driver，不为此修改 registry 源码。`ROOT_FS_CONTEXT` 只由 ext4 初始化一次，不调用 `pseudofs::init_memory_root`。随后挂 `/dev`、`/tmp`、`/proc`、`/sys`，由既有 devfs 构造唯一 tty。

缺启动范围参数、无效范围、坏 FS 在相应层报错并停止，不 fallback。ext4 当前初始化失败会 panic，沿用其现有分层结果；不为本次新增通用 filesystem 错误恢复框架。

### D3 Independent K3 boot mode with the ordinary loader

新增 `k3-rootfs` 模式/`make k3-rootfs`，依赖 `k3-uart` 并传递平台 RAM 预留 feature；与 `k3-stdio`/`k3-shell`/raw-port test 互斥，拒绝 SMP。K3 copier qualification 分支明确排除新模式，避免先跑 raw reader。

最终顺序：UART hardware → copier → RAM block/ext4 root → pseudofs → alarm task → `FS_CONTEXT.resolve(args[0])` → `new_user_aspace_empty` + `copy_from_kernel` → `load_user_app` → UserContext/task → process/tty/stdio → spawn/join。

复用 `src/main.rs::CMDLINE` 和 `src/init.sh`，使启动链与本仓 QEMU 同源。普通 loader 读取 PT_INTERP、映射 BusyBox/musl 并生成 auxv；用户 pagefault 走现有 `aspace.handle_page_fault`。不走 embedded ET_EXEC loader，也不照搬 D1 eager 路径来绕过正常行为。

退出后沿既有 root unmount/flush 及 K3 30 s 物理 drain/park 收尾。前台子进程行为使用现有 clone/execve/wait、process group 和 tty SIGINT。后续实测发现的 loader/COW/tty 缺口属于目标行为调查点；影响接口、架构或状态所有权时返回规划，不让实施者自行设计替代路线。

### D4 Layered implementation and native verification

按三个独立诊断边界分轮：RAM rootfs 挂载/文件读取；普通 BusyBox/musl 启动和外部命令；命令组合、取消、退出与复位。只展开首轮执行契约。

首轮建立 extent/ownership 边界的 host 测试，并用板上实际 ext4 resolve/read 判定挂载。第二轮验证普通动态用户态；第三轮完成全部规格场景。QEMU 检查只支持共享用户态回归，不代替 K3 板测。适用 `qemu-kernel-debugging` 的 image/mutation 规则：共享代码变化时使用原始 image 的临时副本或 QEMU `-snapshot`，不覆盖现有工作盘。

不用 shell 提示符替代外部执行/管道/文件读写测试，不新增结果判定器或证据协议。Persisted Evidence 为 none，命令结果和用户一句话板测结论进入当轮反馈即可。

2026-10-08 收口修订：第三轮（命令组合、取消、退出与复位）按用户裁决取消。手动补充板测已见证错误报告（`man: not found`、`netstat` 缺文件报错）、`sleep 60`+Ctrl-C 前台取消恢复与外部 RESET 回官方 Linux；脚本/管道/重定向、易失写入、空闲编辑与 shell exit 收尾不再正式验证。场景级见证状态权威记录见 change `tasks.md`「见证状态」节。

## Risks / Trade-offs

- 1 GiB rootfs 占用固定 RAM，启动 USB 装载较慢 → 先沿用已检查的原始镜像，性能不作为当前验收；不得顺带压缩/裁剪用户空间。
- RAM window/FDT 可能被 allocator 重用 → 新模式固定预留，两者保留生命周期；native host 检查 free range 不包含预留窗口。
- 普通 ELF/COW/exec 在 K3 未验收 → 每轮先建立测试见证；新实质差异返回规划，保留层次诊断。
- U-Boot 实际传递参数是板上行为 → 首轮直接观察解析出的 rootfs 范围、ext4 挂载和文件读，不凭文档推断通过；未传参数时明确拒绝。
- RESET 不撤销持久写入 → 当前无物理存储 driver 接入，不 saveenv，不写官方系统；最终仍需外部 RESET 见官方登录。

## Migration Plan

仅打包新模式的 FIT 并复制 FIT、原始 rootfs 到现有 FAT32 U 盘。先在既有 R68 路径进入 U-Boot，所有 env 修改只在 RAM，不保存。依次加载：

```text
usb start
fatls usb 0:0
fatload usb 0:0 0x240000000 rootfs-riscv64.img
setenv starry_rootfs_size ${filesize}
fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit
setenv bootargs initrdmem=0x240000000,0x${starry_rootfs_size}
setenv fdt_high 0xffffffffffffffff
bootm 0x2e0000000
```

每次 fatload 必须成功，rootfs 原生读取字节数应为 1073741824（0x40000000）；FIT 装载小于 16 MiB。失败不继续 bootm。`starry_rootfs_size` 只是避免第二次 fatload 覆盖 U-Boot 的 filesize；不 saveenv。

外部 RESET 后沿原启动链进入官方 Linux。下一次 StarryOS 会话重新从原始 image 加载；无需磁盘恢复。最终部署操作以当轮 accepted 构建结果为输入，不在计划阶段执行。
