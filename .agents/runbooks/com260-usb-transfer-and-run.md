# CoM260 Kit 官方 Linux U 盘传输与静态 ELF 运行

- Status: active
- Last validated: 2026-10-01
- Environment: CoM260 Kit（SpacemiT K3）官方 Linux（Bianbu，kernel 6.18.3-generic，hostname k3）；host 侧 WSL2（Windows 盘符挂载于 /mnt/<盘符>）
- Source: MS09 `ms09-com260-kit-observable-link-baseline` Iteration 001 Cycle 000 现场实跑；证据路径见文末

## 适用范围

把 host 构建的静态 Linux ELF（如 `ms09-board-facts`）传入 CoM260 Kit 官方 Linux 并运行取回输出。适用于 musl `-static` 产物；动态链接产物不适用（官方 rootfs 的 libc 与 host 工具链不保证匹配）。

## 前置条件

- U 盘为 FAT32/exFAT（NTFS 官方 rootfs 未必带 ntfs-3g）。
- 板上有 root 权限 shell（mount 需要）；host 侧产物已构建。
- 板 USB-A 口可用（xHCI + usb-storage，官方内核已启用）。

## 操作步骤

1. host 侧拷入 U 盘（U 盘插 Windows，WSL2 直接读写盘符）：
   ```sh
   cp target/ms09-board-facts /mnt/<盘符>/
   sync
   ```
2. Windows 安全弹出 U 盘，插入 K3 板 USB-A 口。
3. 板上确认设备节点：
   ```sh
   lsblk
   ```
   板载 UFS 为 `/dev/sda`；U 盘为 `/dev/sdb`。实测 U 盘无分区表（FAT32 直接在整盘）。
4. 挂载：
   ```sh
   mkdir -p /mnt/usb
   mount /dev/sdb /mnt/usb      # lsblk 显示 sdb1 时改挂 /dev/sdb1
   ls /mnt/usb
   ```
5. 拷到板子本地再执行（FAT 上 chmod 不可靠）：
   ```sh
   cp /mnt/usb/ms09-board-facts /tmp/
   chmod +x /tmp/ms09-board-facts
   cd /tmp && ./ms09-board-facts > board-facts.txt
   echo $?
   ```
6. 取回输出并卸载：
   ```sh
   cp /tmp/board-facts.txt /mnt/usb/
   sync
   umount /mnt/usb
   ```
7. U 盘拔回 Windows，从 `/mnt/<盘符>/board-facts.txt` 取走输出。

## 验证

- 成功判据：步骤 5 `echo $?` 为 0；输出含完整 `MS09_FACTS_BEGIN`…`MS09_FACTS_END` 序列。
- 2026-10-01 实跑：完整序列取得、shell 回到提示符（当日未单独回显 `$?`，以序列完整性与提示符返回判定）。

## 失败处理

- `lsblk` 无 sdb：U 盘未识别。换 USB-A 口；`dmesg | tail` 看枚举；确认 host 侧已拷入文件。
- mount 报 invalid argument：设备节点选错（有分区表时应挂 sdb1 而非整盘）。
- 板上执行 Permission denied：先 cp 到 /tmp 再 chmod（FAT 的 exec 位由挂载选项决定）。
- NTFS 盘无法挂载：换 FAT32/exFAT U 盘。

## 回滚

`umount` 后拔盘即可。mount 为临时挂载、/tmp 位于 tmpfs 重启即清、板上无持久写入；U 盘新增文件可按需删除。

## 证据

- `openspec/changes/ms09-com260-kit-observable-link-baseline/evidence/001-board-ram-boundary/000-initial/board-facts.txt`（2026-10-01 实跑输出）
- k3 参考仓库 runbook《K3 CoM260 首次真机接口、通信与固定网络配置报告.md》§4：/dev/sdb 整盘挂载先例（首次验证）
