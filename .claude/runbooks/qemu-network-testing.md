# QEMU Network Testing Runbook

> Status: active | Related: K31, K32, R55, R48 | Last updated: 2026-08-17
> Verified: 2026-07-29 MS01 manual QEMU 10/10 PASS; 2026-08-09 build exit/artifact classification checked; 2026-08-17 offline-injection fallback path (R55/R48) added

## Purpose

在 QEMU 中运行 StarryOS 并对其网络栈进行手动功能验证的标准流程。

## 政策：QEMU 测试一律手动执行，禁止自动化

**所有 QEMU 相关测试（网络、串口、块设备、启动签名等）只允许在 guest shell 中
手工输入命令，不得通过脚本、pipe、pexpect 或任何形式的自动化框架驱动。**

这是一条硬性政策，原因有三层独立证据：

1. **OS shell 阻塞**：QEMU `-nographic` 启动完整 OS，内部 shell（`starry:~#`）
   一直等待用户输入，既不退出也不主动输出 EOF。脚本读到空 buffer 后永久阻塞。

2. **Sandbox 环境限制**：`t01-smoltcp-axnet-baseline` iteration 001 中，
   `scripts/ms01-qemu-test.py` 的真实 QEMU 启动路径在 sandbox 中返回
   `EPERM (Operation not permitted)`，无法拉起子进程。

3. **串口分帧不可靠**：`read_until()` 在一次 `recv()` 可能读到退出标记行末换行
   和后续 shell prompt，导致后续等待换行的 `read_until()` 因数据已被消耗而超时。
   这类字节级竞态无法用简单解析逻辑覆盖。

QEMU 的 guest OS shell、sandbox 环境和串口分帧构成三个独立阻塞面，
**当前没有可靠的自动脚本路径**。Zephyr 或裸机程序不受此限制，但 StarryOS
作为完整宏内核不在该类别内。

## Sandbox 阻塞的分类与手工交接

Agent 仍应先执行不需要 QEMU 交互的自动 Gate。只有命令最终失败且最早失败层
明确属于执行环境能力限制时，才把该项标记为 `ENV-BLOCKED`，并移到 iteration
末尾的用户手工批次。可交接的环境限制包括：

- workspace 外路径只读，例如无法写入 Cargo home；
- sandbox 禁止联网或安装缺失工具；
- 子进程因 `EPERM`、`SIGSYS` 或 `Bad system call` 被环境拒绝；
- QEMU guest shell、TAP、mount 或 `sudo` 操作需要用户控制的终端或权限。

下列结果仍是产品 Gate 失败，不得改写为 sandbox 阻塞：Rust/C 编译诊断、链接
错误、测试断言失败、source check 失败、OpenSpec validation 失败或 diff 检查失败。
无法从日志区分环境与产品原因时也不得交接，必须保留原始失败并停止。

判断以命令的最终退出状态和产物为准。2026-08-09 的
`make LOG=info build` 曾在准备阶段报告 Cargo home 只读和联网失败，但随后复用已安装
的 `rust-objcopy` 完成构建并生成目标镜像；该次结果是 PASS，不是 `ENV-BLOCKED`。
中间出现环境警告不能覆盖成功的最终结果。

手工交接必须位于 iteration 最后一批，且其他自动 Gate 已通过。交接记录至少包含：

- 原自动命令、最终退出码和最早环境失败层；
- 用户在 sandbox 外执行的同一命令及环境差异；
- 关键输出、最终退出码和要求的产物（按「证据精简原则」，不缺省必要证据，但不再
  强制记录 hash 值）；
- PASS、FAIL 或中断结论。

手工复跑若出现产品错误，仍按对应产品 Gate 失败处理。中断、缺日志或缺产物不能
计为 PASS，也不能用旧 Evidence 替代本 iteration 的结果。

## 完整命令行流程（每次都给）

### Terminal 1 — HTTP Server（先启动）

```bash
cd /home/daivy/projects/serial/work/StarryOS/tests && python3 -m http.server 18765 --bind 0.0.0.0
```

必须 `--bind 0.0.0.0`，不能 `127.0.0.1`。QEMU user-mode networking 通过网关 `10.0.2.2` 访问 host，只监听 lo 接口会导致 guest `wget` 报 `Connection refused`。

### Terminal 2 — 编译 Payload

```bash
cd /home/daivy/projects/serial/work/StarryOS
riscv64-linux-musl-gcc -static -O2 -o tests/ms01_socket_baseline tests/ms01_socket_baseline.c
```

### Terminal 2 — 启动 QEMU（编译完后）

```bash
qemu-system-riscv64 \
  -machine virt -bios default \
  -kernel /home/daivy/projects/serial/work/StarryOS/StarryOS_riscv64-qemu-virt.bin \
  -m 1G -smp 1 \
  -device virtio-blk-device,drive=disk0 \
  -drive id=disk0,if=none,format=raw,file=/home/daivy/projects/serial/work/StarryOS/make/disk.img \
  -device virtio-net-device,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 \
  -nographic
```

| 参数 | 作用 |
|------|------|
| `-device virtio-net-device` | VirtIO-MMIO 网卡（D22/K32，当前主线） |
| `-netdev user` | user-mode networking，guest 可通过 10.0.2.2 出站 |
| `hostfwd=tcp::5555-:5555` | host 5555 → guest 5555 端口转发 |
| `-nographic` | 串口连 stdio |

### 在 QEMU Guest 中（出现 `starry:~#` 后）

```sh
wget -q -O /tmp/ms01_test http://10.0.2.2:18765/ms01_socket_baseline && chmod +x /tmp/ms01_test && /tmp/ms01_test
```

输出在 `MS01_SOCKET_BASELINE_START` 和 `MS01_SOCKET_BASELINE_END` 之间，每行 `PASS: <name>` 或 `FAIL: <name> <reason>`。

## 备用路径：下载失败 / 网络挂起时直接挂载注入 payload

当 guest `wget` 因网络问题（挂起、下载失败）无法从 `10.0.2.2` 拉取测试 payload，
但 host 测试已确认数据面以外的事实、仍需在 guest 内跑 probe 或回归 payload 做层间/数据面
判断时，可用直接挂载（或离线注入）把 payload 放进 rootfs，绕过网络。**仍须遵循本 Runbook
「QEMU 一律手工」政策——仅注入方式是离线的，guest 命令依旧手工输入。**

两种工具按需选择：

### 方式 A：`debugfs` 离线写入（不需要 sudo；推荐用于诊断）

把 payload 写进 **`make/disk.img` 的副本**（原盘不动，避免污染冻结 hash），重启 QEMU 时
让 virtio-blk 指向该副本：

```bash
cd /home/daivy/projects/serial/work/StarryOS
cp make/disk.img /tmp/ms05-diag-disk.img
for f in ms05_data_plane_probe ms04_rx_probe; do
  debugfs -w -R "write tests/$f /root/$f" /tmp/ms05-diag-disk.img
done
# 重启 QEMU 时把 -drive ...file=make/disk.img 换成 ...file=/tmp/ms05-diag-disk.img，其余参数不变
```

guest 里直接离线执行（无需网络）：

```sh
/root/ms05_data_plane_probe snapshot
/root/ms05_data_plane_probe tx-only 96 64
/root/ms04_rx_probe snapshot
```

### 方式 B：`mount -o loop` 直挂根目录（需要 sudo；MS03 已验证）

`make/disk.img` 是裸 ext4（无分区表），loop 直挂不需 offset：

```bash
sudo mount -o loop make/disk.img /mnt/starry-rootfs
sudo cp tests/ms03_irq_probe /mnt/starry-rootfs/root/
sudo umount /mnt/starry-rootfs
```

### 判据与边界

- 用 `debugfs` 或 mount 注入后，guest 能直接执行的依据是 payload 是**静态链接**的 RISC-V ELF
  （`file tests/<name>` 应为 `statically linked`），不依赖动态库或网络。
- 备用路径能证明「guest 内的探针/程序在数据面上是否工作」，**不能替代**对 wget/HTTP 下载
  主路径本身的验证；若目标是回归网络协议（TCP/UDP/socket），仍要在网络可用时用主路径复跑。
- 所有注入只作用于副本或 rootfs 挂载目录；完成后确认探针可执行、数据面工作即达
  目标，不必保存 hash 值（见「证据精简原则」）；诊断盘/挂载目录用后清理。

## 证据精简原则

自 2026-08-19 起（来源：MS05 Cycle 004 后用户决定，Recorder 登记）：

- **证据不收录过大/过多的日志文件**。几百个 gate 日志、几万行的原始日志属于过度，
  不适合作为普通 Evidence。原始长日志保留在外部或临时文件，Evidence 只存必要摘录
  （每模式终态 marker、host 结果、退出码）并按需引用其来源路径，不复制长正文。
- **不再强制记录 hash 值**。未来证据以保证代码功能正确为准，只保留能证明行为的
  命令、关键输出和退出码；hash/manifest 的细粒度 provenance 除非明确要求，不再
  作为每 Cycle 的必填产物。
- 当某个具体 change（如 MS05 自动 gate 管线）仍需机器可审计的 manifest/日志时，
  由该 change 或对应 Runbook 单独声明，不把此项默认套用到全部手工 QEMU 证据。
- 已被删除的 `ms05-automatic-gate-manifest.md`（R54，数百 logs + hash 冻结）不再
  作为通用证据模板；其 R54 引用标记为悬空，待 `openspec-docs-maintainer` 处理。

## 验证

- QEMU 必须进入 `starry:~#`，payload 命令最终退出，并输出完整 START/END marker。
- 对应 change 要求的每个 case 必须有 PASS/FAIL、完整串口和最终退出或中止状态。
- 自动命令只有最终非零且最早失败层满足上方分类时才能记 `ENV-BLOCKED`；用户在
  sandbox 外复跑成功后，该 Gate 才能改为 PASS。
- 中断、缺 marker、缺日志或只保留摘要均不算通过。

## 回滚

本流程不修改产品源码。HTTP 下载到 guest `/tmp` 的 payload 会在 QEMU 退出后消失。
用 `Ctrl-A X` 退出 QEMU，用 `Ctrl-C` 停止 host HTTP server。若用了「直接挂载注入」备用路径，
清理诊断盘副本与挂载目录：`rm /tmp/ms05-diag-disk.img`、`sudo umount /mnt/starry-rootfs`（若仍挂载）；
原 `make/disk.img` 不受影响。若具体 change 另行要求修改 rootfs，必须按该 change 的手工步骤执行
umount/恢复，不能套用本段。

## 排障

| 症状 | 原因 | 解决 |
|------|------|------|
| `wget: Connection refused` | HTTP server bind `127.0.0.1`，guest 通过 10.0.2.2 连不上 | 改 `--bind 0.0.0.0` |
| `wget` 挂起（`Connecting to ...` 停住） | 产品数据面问题；host 服务正常 | 先用 R55 分层诊断定位；需要离线跑 probe 时用上方「直接挂载注入」备用路径 |
| shell 提示符是 `starry:~# ` 不是 `/ #` | 不同版本 prompt 不同 | 看到可输入光标即可 |
| rebind `Address in use` | fork 版 smoltcp 不立即释放 port | 测试中 `sleep(2)` 临时绕过（迁移后移除） |

## 证据

- 2026-07-29：`t01-smoltcp-axnet-baseline` iteration 001/002 的手工 QEMU 与自动化
  失败记录，支持 guest shell 手工政策和 HTTP 下载路径。
- 2026-08-09：`make LOG=info build` 的同次输出同时出现 Cargo home 只读、禁止联网
  和最终 build/objcopy 成功，支持“按最终退出与产物分类，不按中间警告分类”。
- 2026-08-17：R55 `qemu-kernel-net-dataplane-debug.md`（第5步，debugfs 离线注入）与
  R48 `ms03-virtio-mmio-irq-evidence.md`（步骤2.2，mount -o loop 直挂）各已验证端到端，
  支持「下载失败→直接挂载注入 payload」备用路径。
- 适用限制：该规则只处理执行环境能力边界，不证明任何具体 change 的产品行为；备用路径结论
  限定于单 hart QEMU VirtIO-MMIO 软件/设备模型，不替代网络主路径回归。

## 变更历史

- 2026-08-17：新增「直接挂载注入」备用路径（debugfs 离线写入 + mount -o loop 直挂），
  供 guest `wget` 网络挂起/下载失败时离线跑 probe/回归 payload 做数据面判断；排障表
  增加 `wget` 挂起到 R55 分层诊断 + 备用路径的指引；回滚段补充诊断盘副本/挂载目录清理。
- 2026-08-09：增加 sandbox 阻塞分类与手工交接规则。只有明确的环境能力拒绝可以
  标记 `ENV-BLOCKED`；该项移到 iteration 末尾，并保留命令、退出码、日志和产物
  见证。产品错误不得转为手工边界。
- 2026-07-29：升级为硬性政策声明。新增三层独立证据（OS shell 阻塞、sandbox
  EPERM、串口分帧不可靠），来源 iteration 001 Plan Review + Act Response。
  明确"QEMU 测试一律手动，禁止自动化"。
- 2026-07-28：初始版本。记录 MS01 baseline 测试流程、HTTP 下载法、排障经验。
