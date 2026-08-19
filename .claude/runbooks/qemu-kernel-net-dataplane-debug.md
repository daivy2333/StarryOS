# QEMU 内核网络数据面分层诊断

- Status: active
- Last validated: 2026-08-17
- Environment: WSL2 x86_64；QEMU 7.0.0；RISC-V `virt`；1 GiB；单 hart；单 VirtIO-MMIO NIC；user-net；Rust nightly-2026-02-25；offline Cargo
- Source: `ms05-qemu-bounded-bidirectional-device-data-plane` Iteration 011 Cycle 000（blocked，Diagnostic Addendum）；证据在 `openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/evidence/011-independent-manual-qemu-runtime-and-closeout/000-initial/`

## 适用范围

当 QEMU 中内核网络行为异常（guest 工具可用但下载/连接挂起、无流量、IRQ 不触发、waker 不醒）且 host 测试全 PASS、需要区分"驱动注册层 / TX 数据面 / IRQ→wake→reap 链路 / slot 交付 / smoltcp 消费 / socket 唤醒"哪一层断时使用。

**适用**：StarryOS 类 ArceOS 内核 + axnet/smoltcp 异步 NIC 数据面；wget 等 guest 网络操作失败；需要客观层间证据的排查。

**不适用**：真板（用 `board-bringup-ladder.md` R40）；纯编译错误；已知功能缺口（如 guest 工具本身缺内核 surface）；性能问题。

## 前置条件

- 冻结镜像已备份：诊断前必须 `cp StarryOS_riscv64-qemu-virt.bin /tmp/<name>-frozen.bin` 并记录 SHA-256；诊断后必须恢复（否则污染证据 hash）。
- 产物已冻结：`make/disk.img`、guest payload 的 size/hash 已记录；诊断盘只用副本。
- 工具可用：`qemu-system-riscv64`、`riscv64-linux-musl-gcc`、`debugfs`（离线写 ext4）、`tcpdump`（读 pcap）。
- QEMU 端口未被占用（上次 QEMU 未退出时 `make run` 会报 `hostfwd ... Could not set up`；先 `pgrep -af qemu-system` + kill）。
- 每个终端先设 `EV=<change>/evidence/<iteration>/<cycle>/` 短变量。

## 操作步骤

### 1. 冻结与备份（不可跳过）

```bash
cd /home/daivy/projects/serial/work/StarryOS
sha256sum StarryOS_riscv64-qemu-virt.bin make/disk.img
cp StarryOS_riscv64-qemu-virt.bin /tmp/ms05-frozen-bin-backup.bin
sha256sum /tmp/ms05-frozen-bin-backup.bin   # 必须与冻结值一致
```

### 2. 分层日志镜像

不要直接用 `LOG=debug` 重跑——axlog 只有全局 `set_max_level`，debug 会把 UART benchmark/驱动全量日志刷屏，无法聚焦网络（实测 157 KB 日志 90% 是噪声）。按需构建：

```bash
# 黄金平衡：info —— 能看到 eth0 注册、IRQ handler、queue task，无噪声
make LOG=info build
sha256sum StarryOS_riscv64-qemu-virt.bin     # 记录诊断镜像 hash（区别于冻结值）

# 仅在需要 axtask 调度/唤醒细节时才上 debug
make LOG=debug build
```

诊断完必须恢复冻结镜像（见回滚）。

### 3. QEMU + 客观抓包

info 镜像 + `-object filter-dump`（R45 已验证的参数）——**pcap 是层间客观证据，不依赖日志级别**：

```bash
cd /home/daivy/projects/serial/work/StarryOS
EV=openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/evidence/011-independent-manual-qemu-runtime-and-closeout/000-initial
script -q -f "$EV/qemu-serial-info.log" -c \
'qemu-system-riscv64 -machine virt -bios default -kernel StarryOS_riscv64-qemu-virt.bin -m 1G -smp 1 -device virtio-blk-device,drive=disk0 -drive id=disk0,if=none,format=raw,file=make/disk.img -device virtio-net-device,netdev=net0 -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 -object filter-dump,id=ms05diag,netdev=net0,file=make/ms05-diag.pcap -nographic'
```

### 4. Guest 手工采集（R44 政策：QEMU 一律手工）

**先确认 host 服务活着**——`wget` 挂起前必须 `ss -tlnp | grep 18765`；服务没起会得 `Connection refused`（环境噪声），服务起着还挂才是产品问题。

```sh
# 网络请求（带输出，不带 -q）
wget -O /tmp/ms05_probe http://10.0.2.2:18765/ms05_data_plane_probe
```

**注意**：`ifconfig`/`ping` 在 StarryOS 里不可靠——内核未实现 `/proc/net/dev`、`SIOCGIF*` ioctl、`SOCK_RAW`，busybox 工具报错**不代表网络断**，不要拿它当判据。判据用 probe snapshot 和 pcap。

### 5. 鸡生蛋破解：离线注入 probe

probe 下载需要网络、诊断需要 probe——用 `debugfs` 把 payload 写进 **disk.img 副本**（原盘不动）：

```bash
cp make/disk.img /tmp/ms05-diag-disk.img
for f in ms05_data_plane_probe ms04_rx_probe; do
  debugfs -w -R "write tests/$f /root/$f" /tmp/ms05-diag-disk.img
done
# 重启 QEMU 时换 file=/tmp/ms05-diag-disk.img，其余 argv 不变
```

guest 里直接离线执行（无需网络）：

```sh
/root/ms05_data_plane_probe snapshot     # 直接 ioctl 读 V3 快照
/root/ms05_data_plane_probe tx-only 96 64
/root/ms04_rx_probe snapshot
```

### 6. 逐层归因

每层一个证据源，按顺序对照：

| 层 | 证据源 | PASS 标记 |
|---|---|---|
| 驱动注册 | serial `eth0:` `mac:` `ip:` `registered a new Net device` | eth0/IP 正确 |
| TX 数据面 | pcap 中 guest 发出的 ARP request | guest MAC→广播 ARP 上线 |
| IRQ→wake→reap | MS04 snapshot `isr_publish` `isr_wake` `reaped` `refilled` | 均 >0，`fault=0` |
| slot 交付 | MS05 snapshot `lifecycle=2 owner=1` | Active + AsyncOwned |
| smoltcp 消费 | pcap 中 SYN/数据帧 | 有后续包 |
| socket 唤醒 | wget 完成 / `MS05 PASS mode=tx-only` | 下载成功 |

**实测案例**（MS05 Iteration 011）：pcap 只有 2 帧（guest ARP request + slirp ARP reply），无 SYN；snapshot 显示 `lifecycle=2 owner=1 fault=0`、`isr_publish=1 isr_wake=1 reaped=1 refilled=1 non_ip=1`。归因：TX 上线、RX 硬件链路工作，断点在"ARP reply 被 reap 之后 → smoltcp 消费 → 发 SYN"之间（slot 交付 / `Service::poll` RX-slot drain / socket-waker 桥接），判定为产品缺陷而非环境。

## 验证

- 串口：`grep -a 'eth0:' "$EV/qemu-serial-info.log"` → 有 `mac:` `ip:`。
- snapshot：`grep -a 'MS05 (PRE|POST)' "$EV/qemu-serial-snapshot.log"` → `lifecycle=2 owner=1`。
- pcap：`tcpdump -r make/ms05-diag.pcap -nn -e` → 能看到 guest ARP/TCP 帧方向；仅 request+reply 而无 SYN = RX 消费断。
- 每次会话结束：`sha256sum -c <evidence>/artifacts.sha256` 恢复后必须仍 OK。

## 失败处理

| 现象 | 分类与处理 |
|---|---|
| `make run` 报 `hostfwd ... Could not set up` | 旧 QEMU 占用 5555；`pgrep -af qemu-system` + kill 后重试 |
| `wget` 立刻 `Connection refused` | host HTTP server 未监听；`ss -tlnp \| grep 18765` 确认后再测 |
| `wget` 挂起（`Connecting to ...` 停住） | 产品数据面问题；进入分层诊断（本 Runbook 第 5-6 步） |
| `tx-only` 报 `reason=handshake` | host 没起 stimulus；先 `python3 scripts/ms05_data_plane_stimulus.py --host 0.0.0.0 --port 15557` |
| `ifconfig`/`ping` 报 `/proc/net/dev` 或 ioctl/socket 错误 | 内核未实现该 surface；不是网络断证据，改用 probe snapshot + pcap |
| debug 日志噪声刷屏 | 降回 info；调度细节用 debug 单独一次并 grep 过滤 |
| pcap 只有 ARP 无 SYN | 锁定 RX-reap 之后；检查 slot 交付/`Service::poll` drain/waker 桥 |

## 回滚

- 恢复冻结镜像：`cp /tmp/ms05-frozen-bin-backup.bin StarryOS_riscv64-qemu-virt.bin` + `sha256sum` 校验（必须回到冻结值）。
- `make/disk.img` 从未被诊断盘覆盖（诊断只用副本），无需恢复。
- 退出 QEMU：`Ctrl-A X`；停止 host server/stimulus：`Ctrl-C`。
- 清理诊断盘/临时文件：`rm /tmp/ms05-diag-disk.img /tmp/ms05-*.log`（保留 evidence 内日志与 pcap）。
- 诊断产物归入 evidence 后，`openspec validate <change> --strict` 必须 exit 0。

## 证据

- 本流程第一次端到端执行：`openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/iterations/011-independent-manual-qemu-runtime-and-closeout-review/000-initial.md`（Act Response Diagnostic Addendum，blocked）。
- 原始日志/pcap：`openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/evidence/011-independent-manual-qemu-runtime-and-closeout/000-initial/`（`qemu-serial-info.log`、`qemu-serial-snapshot.log`、`qemu-serial-debug.log`、`ms05-diag.pcap`、`artifacts.sha256`）。
- 冻结镜像 hash：`fe20b5b2107ddb0ff333c572913a8bb0b52206934a898d27bc2b14f3705007fc`。
- 适用限制：结论限定于单 hart QEMU VirtIO-MMIO 软件/设备模型；本 Runbook 描述诊断方法，不包含未确认根因的修复方案。
