# rootfs ext4 镜像压缩精简（K3 MS18 板测输入）

- Status: active
- Last validated: 2026-10-08
- Environment: WSL2 主机（e2fsprogs 1.46.5、fakeroot 可用、无免密 sudo）；StarryOS 仓库根（分支 k3，MS18 Iteration 000 实施）；原始镜像 `rootfs-riscv64.img`（1 GiB ext4，含 orphan_file/FEATURE_C12 新特性）；目标板 CoM260 Kit U-Boot FAT32 U 盘启动
- Source: MS18 Iteration 000 Act Response（`openspec/changes/ms18-com260-kit-standard-shell-rootfs/iterations/000-ram-rootfs/000-initial.md`）Board Witness 与 Deviations；本会话板测串口记录（用户执行）

## 适用范围

- 适用：需要把大 ext4 rootfs 镜像缩小为可快速 `fatload` 的测试镜像，且要求关键文件（如 BusyBox/musl）字节级不变、原始镜像只读不修改的场景。本仓 K3 MS18 板测即此场景：1 GiB 原始镜像 USB 装载 35–100+ s，超出官方启动链遗留看门狗窗口导致自动重启。
- 不适用：需要保留原文件系统容器（UUID/块组布局/journal 状态）逐位一致的场景——本流程产出的是**内容相同的新容器**，不是 resize2fs 收缩；需要写入持久化的生产 rootfs 也不适用（产物为测试输入）。

## 前置条件

- 主机装有 `debugfs`/`e2fsck`/`mkfs.ext4`（e2fsprogs）与 `fakeroot`；`/tmp` 空间 ≥ 镜像内容体积。
- 原始镜像为 ext4 且可在本机被 `debugfs` 读取。注意：新 mkfs 生成的镜像可能带 e2fsprogs 1.47+ 特性（如 orphan_file，e2fsck 报 `FEATURE_C12`），1.46.5 的 `e2fsck`/`resize2fs` 无法处理，但 `debugfs` 读取/导出不受影响——这正是选择"提取重建"而非"原地收缩"的原因。
- U-Boot 侧 FAT32 U 盘可用；目标内核按 `initrdmem=<hexaddr>,<hexlen>` 消费镜像长度，要求长度 512 对齐（mkfs 默认 4096 块天然满足）。
- 安全检查：全程只对**副本/新文件**操作；对原始镜像只使用 debugfs 只读命令（rdump/dump/stat）。

## 操作步骤

1. （可选）确认原地收缩是否可行——对副本试 `e2fsck -f -n`；若报 `unsupported feature(s): FEATURE_C12`（或需 e2fsprogs ≥ 1.47 的其他特性）则放弃 resize2fs，走提取重建。
2. 提取内容树（只读源镜像；目标目录必须**先存在**，否则 rdump 报 `No such file or directory while statting`）：
   ```bash
   mkdir -p /tmp/ms18-root
   debugfs -R 'rdump / /tmp/ms18-root' rootfs-riscv64.img
   ```
   预期告警（非 root，属主问题第 3 步解决）：`Operation not permitted while changing ownership of ...`。`/dev` 设备节点不会被提取（mknod 被拒）——内核侧由 devfs 覆盖 `/dev` 时可接受，纯文件读取场景无影响。检查规模：`du -sh /tmp/ms18-root`、`find /tmp/ms18-root -type f | wc -l` 等（本次实测 7.5 MB、85 文件、335 符号链、100 目录）。
3. 建新镜像并用 fakeroot 归一属主为 0:0（属主归一必要：如 `/root` 为 0700，若属主为主机用户 uid，内核侧 uid 0 进程后续将无写权限）：
   ```bash
   truncate -s 128M rootfs-riscv64-min.img
   fakeroot -s /tmp/ms18-fake.state -- sh -c \
     'chown -R 0:0 /tmp/ms18-root && mkfs.ext4 -F -q -L rootfs -d /tmp/ms18-root rootfs-riscv64-min.img'
   ```
   尺寸按"内容体积 + ext4 元数据/journal + 余量"选择（本次内容 7.5 MB 取 128 MiB，装载 17 s）。`mkfs.ext4 -d` 自暂存目录填充，文件字节原样进入新容器。
4. 镜像验证（见下节），通过后拷贝到 FAT32 U 盘，按既有 U-Boot 流程 `fatload` 到预留窗口地址，`setenv bootargs initrdmem=0x240000000,0x${filesize}` 后 bootm。

## 验证

- 新镜像文件系统干净：`e2fsck -f -y rootfs-riscv64-min.img` → exit 0 且零修复动作（注意：非 tty 下裸 `e2fsck -f` 即使干净也可能 exit 8 `need terminal for interactive repairs`，用 `-y`）。
- 关键文件事实：`debugfs -R 'stat /bin/sh' rootfs-riscv64-min.img`（symlink、Mode 0777、Size=目标路径长度）、`debugfs -R 'stat /bin/busybox' ...` 与 `stat /lib/ld-musl-riscv64.so.1`（User/Group 0/0、Size 825088/613016）。
- 字节级一致性：对原镜像与新镜像分别 `debugfs -R "dump /$f /tmp/..."` 后 `md5sum` 比对——本次 BusyBox `fdb59f1c…`、musl `f6c41d2d…` 两镜像完全一致。
- 板上端到端：`fatload` 字节数等于镜像尺寸（134217728 bytes read，17 s @7.5 MiB/s，看门狗窗口内）；内核 `stage=extent/mount-ok/resolve-ok/file-ok ... elf_magic=true/drain done` 全序列通过（串口记录见 Source）。

## 失败处理

- `rdump: No such file or directory while statting /tmp/...` → 目标目录不存在，先 `mkdir -p`。
- 原镜像 `e2fsck` 报 `unsupported feature(s)` → 预期现象（新特性镜像 + 旧工具），只需 debugfs 可读即可继续；debugfs 也失败时需升级 e2fsprogs ≥ 1.47 再走 resize2fs 路线（未验证）。
- 新镜像 `e2fsck` 不干净 → 停止，不得上板；重建（提取与 mkfs 步骤重跑）。
- md5 比对不一致 → 停止，检查提取是否完整（rdump 告警）与 mkfs `-d` 路径参数。
- 板上 `fatload` 后内核报范围层错误 → 核对 `initrdmem` 地址/长度与镜像实际尺寸、512 对齐；属内核启动契约问题，见 MS18 Act Response 根因记录。

## 回滚

- 主机侧：原始镜像全程只读未动；删除新镜像与 `/tmp/ms18-root`、fakeroot state 后重跑即可。无不可回滚操作。
- 板侧：RAM-only，不 saveenv；外部 RESET 回官方启动链。

## 证据

- MS18 Iteration 000 Act Response（Board Witness、Deviations from Plan：镜像压缩裁决与双根因记录）：`openspec/changes/ms18-com260-kit-standard-shell-rootfs/iterations/000-ram-rootfs/000-initial.md`
- 本会话命令与退出码：提取/mkfs/fakeroot/`e2fsck -f -y` exit 0、md5 双向比对一致、`stat` 尺寸 134217728 B；板上全序列串口输出由执行人提供（2026-10-08）。
- 限制：本 Runbook 在"只读 ext4 挂载 + VFS 文件读取"用途上验证；`resize2fs` 原地收缩路线在 e2fsprogs ≥ 1.47 环境下未经本仓验证。重建容器不保留原 UUID/块组布局；`/dev` 节点为空依赖内核 devfs 覆盖。
