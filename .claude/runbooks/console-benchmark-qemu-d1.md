# Console benchmark 的 QEMU 与 D1 部署

## 适用范围

本 Runbook 用于在 `console-polling-baseline` 分支复跑与 async UART 基线相同的 benchmark：

- QEMU：编译 musl payload、注入 ext4 rootfs、交互运行并保存日志。
- D1：构建 command-entry Android boot image、复制到 TF 卡、备份与写入 boot 分区、采集串口并恢复官方 Linux。

通用构建说明见 `qemu-build.md` 和 `d1-build-and-flash.md`，测试判据见 `benchmark-guide.md`。本文以当前 Makefile 为准，不使用已删除的 `lichee-kbench` 目标。

## 前置条件

在项目根目录执行 host 命令。先记录版本和工具链：

```bash
git branch --show-current
git rev-parse HEAD
export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH
riscv64-linux-musl-gcc --version
qemu-system-riscv64 --version
command -v readelf rg script picocom /sbin/debugfs
```

正式对比必须使用上述 musl 工具链。若受限沙箱对编译器报 `Bad system call`，停止生成正式 payload，改在普通 host shell 执行；不要用 glibc 产物替代。

保留以下证据边界：

| 用途 | 文件 | 规则 |
|---|---|---|
| 冻结 async QEMU 基线 | `docs/qemu_out.md` | 不覆盖、不追加 |
| 新 Console QEMU 日志 | `docs/qemu_console_out.md` | 本轮写入 |
| 冻结 async D1 基线 | `docs/d1_out.md` | 不覆盖、不追加 |
| 新 Console D1 日志 | `docs/d1_console_out.md` | 本轮写入 |

当前冻结输入的 SHA256 是：

```text
d2f2486aa1f4df452ae14880c22ad3d08467561ae5f7799affc768b972ae15d2  docs/qemu_out.md
b98af673ca56ab983c55f3ddaf4f7f39228f7a4ec69f88b6b1f0a907731947cc  docs/d1_out.md
```

D1 写盘会覆盖 boot 分区。开始前必须具备：

- 可正常启动的 D1 官方 Linux。
- TF 卡 `exUDISK` 分区和足够的备份空间。
- 115200 8N1 串口连接。
- 已验证的 attach-only Console 实现：不得重写 U-Boot 留下的 divisor、LCR、FCR 或 MCR。

## 操作步骤

### 1. 构建和检查 QEMU payload

```bash
make tests/benchmark
file tests/benchmark
readelf -h tests/benchmark
readelf -l tests/benchmark
sha256sum tests/benchmark
strings tests/benchmark | rg 'polling-console|S05|Blocking Transmit|UNSUPPORTED'
```

检查结果应为静态 RISC-V 64-bit ELF；manifest 和 section 标签必须来自当前 Console 源码。

### 2. 准备 QEMU rootfs

`make rootfs` 会用干净的 `rootfs-riscv64.img` 覆盖 `make/disk.img`。需要重置镜像时先执行一次，之后再注入 payload：

```bash
pgrep -af qemu-system-riscv64
make rootfs
/sbin/debugfs -R 'stat /bin/benchmark' make/disk.img
/sbin/debugfs -w -R 'rm /bin/benchmark' make/disk.img
/sbin/debugfs -w -R 'write tests/benchmark /bin/benchmark' make/disk.img
/sbin/debugfs -R 'stat /bin/benchmark' make/disk.img
```

如果 `rm` 报文件不存在，表示镜像中没有旧 payload，可以继续执行 `write`。不要在注入后再次运行 `make rootfs`。

用导出副本核对镜像内 payload，防止运行到旧二进制：

```bash
verify_dir=$(mktemp -d)
/sbin/debugfs -R "dump /bin/benchmark ${verify_dir}/benchmark" make/disk.img
sha256sum tests/benchmark "${verify_dir}/benchmark"
strings "${verify_dir}/benchmark" | rg 'polling-console|S05|Blocking Transmit|UNSUPPORTED'
```

两个 SHA256 必须相同。

若 `debugfs` 不可用，可以用旧 learned 中的 loop mount 方式。挂载成功后才复制，结束后必须卸载：

```bash
sudo mkdir -p /mnt/starry-rootfs
sudo mount -o loop make/disk.img /mnt/starry-rootfs
findmnt /mnt/starry-rootfs
sudo cp tests/benchmark /mnt/starry-rootfs/bin/benchmark
sync
sudo umount /mnt/starry-rootfs
```

### 3. 运行 QEMU 并留证

先构建内核，再用终端记录完整交互。benchmark 不需要网络；显式关闭网络可避免 host UDP 端口冲突。

```bash
make NET=n build
script -q -f -c 'make NET=n justrun' docs/qemu_console_out.md
```

进入 shell 后执行：

```sh
printf 'x\n'
/bin/benchmark
echo $?
```

确认 benchmark 结束后，用 QEMU 的 `Ctrl-A X` 退出。不要把输出重定向到 `docs/qemu_out.md`。

### 4. 构建和检查 D1 image

正式横向对比使用 command-entry，它会在启动后自动运行 `/bin/benchmark`：

```bash
make lichee-fullbench-command
file kernel/resources/benchmark.elf
readelf -h kernel/resources/benchmark.elf
strings kernel/resources/benchmark.elf | rg 'polling-console|S05|Blocking Transmit|UNSUPPORTED'
python3 tools/android_boot_image.py inspect starry-lichee-fullbench-command-boot.img
sha256sum kernel/resources/benchmark.elf starry-lichee-fullbench-command-boot.img
ls -lh starry-lichee-fullbench-command-boot.img
```

inspect 必须显示 `ANDROID!`、`kernel_addr=0x40200000`、`page_size=2048`。image 必须明显小于约 10 MiB 的 boot 分区限制。任一项不符就停止，不烧录。

### 5. 将 D1 image 复制到 TF 卡

在 PC 上先识别可移动介质，禁止凭 `/dev/sdX` 猜设备：

```bash
lsblk -o NAME,PATH,SIZE,MODEL,RM,FSTYPE,LABEL,MOUNTPOINTS
ls -l /dev/disk/by-label/exUDISK
sudo mkdir -p /mnt/exUDISK
sudo mount /dev/disk/by-label/exUDISK /mnt/exUDISK
findmnt /mnt/exUDISK
```

只有 `findmnt` 指向已确认的 TF 卡分区时才继续：

```bash
sudo cp starry-lichee-fullbench-command-boot.img /mnt/exUDISK/
sha256sum starry-lichee-fullbench-command-boot.img /mnt/exUDISK/starry-lichee-fullbench-command-boot.img
sync
sudo umount /mnt/exUDISK
```

两个 SHA256 必须相同。若 D1 官方 Linux 已直接挂载同一 `exUDISK` 分区，可跳过 PC 手工挂载，但仍要核对 image hash。
PC 卸载完成后，将 TF 卡插回 D1 并先启动官方 Linux，再执行后续 boot 分区操作。

### 6. 连接串口并记录 D1 输出

在 PC 上确认串口设备后启动记录；以下以 `/dev/ttyUSB0` 为例：

```bash
ls -l /dev/serial/by-id/ /dev/ttyUSB* 2>/dev/null
script -q -f -c 'picocom -b 115200 /dev/ttyUSB0' docs/d1_console_out.md
```

不要在 StarryOS 启动后才开始记录，日志必须包含 U-Boot、kernel、benchmark 和进程退出状态。若串口设备不是 `/dev/ttyUSB0`，把命令中的路径改为实际设备。

### 7. 在 D1 官方 Linux 中备份并烧录

先验证 TF 卡挂载点和稳定的 boot 别名：

```bash
findmnt /mnt/exUDISK
ls -l /dev/by-name/boot
readlink -f /dev/by-name/boot
sha256sum /mnt/exUDISK/starry-lichee-fullbench-command-boot.img
```

首次烧录前备份官方 boot；已有可信备份时先核对其非空和 hash，不要覆盖：

```bash
test ! -e /mnt/exUDISK/boot-official-backup.img && \
    dd if=/dev/by-name/boot of=/mnt/exUDISK/boot-official-backup.img bs=1M
sync
ls -lh /mnt/exUDISK/boot-official-backup.img
sha256sum /mnt/exUDISK/boot-official-backup.img
```

确认备份存在后写入 Console image：

```bash
dd if=/mnt/exUDISK/starry-lichee-fullbench-command-boot.img of=/dev/by-name/boot bs=1M conv=fsync
sync
reboot -f
```

只能写 `/dev/by-name/boot`。不要把底层 `/dev/mmcblk0p4` 或 PC 上的整张 TF 卡设备写进命令。

## 验证

### QEMU

```bash
rg -n 'backend=polling-console|S05|Blocking Transmit|S40|Done\.|drain_errors|exit' docs/qemu_console_out.md
sha256sum docs/qemu_out.md
```

通过条件：

- shell 输入、回显和 blocking read 正常。
- manifest 含 `backend=polling-console`。
- S05 为 `SKIPPED`，S11 为 `Blocking Transmit`，S40 为 `UNSUPPORTED`。
- 运行到 `Done.`，进程退出码为 0，所有 `drain_errors=0`。
- 冻结 async QEMU 日志 hash 未变化。

QEMU 只提供功能与同环境相对开销证据，不提供物理串口线速结论。

### D1

```bash
rg -n 'backend=polling-console|S05|Blocking Transmit|S30|S31|S40|Done\.|drain_errors|exit' docs/d1_console_out.md
sha256sum docs/d1_out.md
```

通过条件：

- 日志包含 D1 启动链、stdio 绑定和完整 S00-S40 顺序。
- 运行到 `Done.`，进程退出码为 0，所有 `drain_errors=0`。
- S30、S31、S40 按 Console 实际能力标记 `UNSUPPORTED` 或 `SKIPPED`，不得伪装为 PASS。
- 冻结 async D1 日志 hash 未变化。

只有 D1 真板结果可以形成物理线速结论。对比时逐项核对 workload、总字节、iterations、timer 和 drain policy；不把 blocking S11 称为 enqueue。

## 失败处理

| 症状 | 处理 |
|---|---|
| musl 编译器找不到 | 重新设置本文 PATH，确认 `riscv64-linux-musl-gcc --version` 成功 |
| 编译器在沙箱报 `Bad system call` | 转到普通 host shell 构建，保留 `ENV BLOCK`；不更换 libc |
| QEMU 仍打印旧 manifest | 重新编译、重新注入，用 dump 后 SHA256 和 strings 双重核对 |
| `make rootfs` 后 payload 消失 | 这是镜像重置的预期行为，重新注入 `/bin/benchmark` |
| QEMU 报磁盘锁定 | 退出占用 `make/disk.img` 的旧 QEMU 进程；不要删除镜像 |
| QEMU 报 host 网络端口占用 | 使用 `make NET=n build` 和 `make NET=n justrun` |
| D1 image header、地址或尺寸不符 | 停止烧录，回到构建与 inspect Gate |
| D1 烧录后无输出或乱码 | 停止重复写盘，核对 115200 8N1、串口设备、attach-only UART 和完整启动日志；随后恢复官方 boot |
| D1 benchmark 未到 `Done.` | 保存完整串口日志，记录最后 section 和 fault；不生成性能对比结论 |

## 回滚

在 D1 上恢复首次烧录前保存的官方 boot：

```bash
ls -lh /mnt/exUDISK/boot-official-backup.img
sha256sum /mnt/exUDISK/boot-official-backup.img
dd if=/mnt/exUDISK/boot-official-backup.img of=/dev/by-name/boot bs=1M conv=fsync
sync
reboot -f
```

恢复后应重新看到官方 Linux 启动日志。若首次烧录前没有生成有效备份，本流程无法本地回滚，必须使用已验证的官方镜像或另一块同型号板的备份恢复。
