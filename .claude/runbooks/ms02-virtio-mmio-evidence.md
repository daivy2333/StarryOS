# MS02 VirtIO-MMIO 证据采集

- Status: active
- Last validated: 2026-07-29
- Environment: WSL2 x86_64; Rust nightly-2026-02-25; offline Cargo; QEMU manual
- Source: `openspec/changes/ms02-virtio-mmio-polling-baseline/iterations/003-policy-coverage-and-runtime-evidence.md` (Act Response: reported)

## 适用范围

MS02 VirtIO-MMIO 轮询网络基线（T02-T03）的完整证据采集路径：

- axnet `register_waker` 策略测试（mask×polling eligibility）
- Agent 静态验证（fmt / unit / feature tree / build / MS01 self-test / openspec validate）
- QEMU 手工验证（无 hostfwd 启动、user-net TCP/UDP 5555、TAP ARP/ICMP、空闲 CPU、MS01 runtime regression）

适用于所有需要验证 VirtIO-MMIO 同步轮询路径的 change。MS03 及之后若继续沿用 MMIO 同步路径，本 runbook 可复用。

**不适用**：

- 异步 RX/TX（MS04+）——需要 IRQ 注册，本路径只覆盖同步轮询
- 真板（VF2）——参考 `board-bringup-ladder.md` (R40)
- 自动 QEMU harness——按 `.claude/runbooks/qemu-network-testing.md` (R44) 硬性政策，QEMU 测试一律手工

## 前置条件

- 工具链：`riscv64-linux-musl-gcc` 已安装（用于编译 guest payload）
- QEMU：`qemu-system-riscv64` 已安装
- 当前分支：`net-k3`（或继续维护 MS02 同步路径的分支）
- 已构建内核：`make LOG=info build` 产生 `StarryOS_riscv64-qemu-virt.bin`
- 磁盘镜像：`make/disk.img` 存在
- Guest payload 源码：`tests/ms02_guest_service.c` 存在

## 操作步骤

### 阶段 1：Agent 静态验证（必须先通过）

按顺序执行，每步失败时停止并写 Blocker Handoff：

```bash
# 1.1 fmt
cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check

# 1.2 unit tests（含 8 个策略测试）
cargo test --manifest-path crates/axnet/Cargo.toml \
  --locked --offline --lib service::tests -- --nocapture

# 1.3 feature tree（确认 auto-icmp-echo-reply）
cargo tree --offline -e features \
  -p starryos --features qemu -i smoltcp | grep auto-icmp-echo-reply

# 1.4 target build
make LOG=info build

# 1.5 MS01 harness self-test（不启动 QEMU）
python3 scripts/ms01-qemu-test.py --self-test

# 1.6 OpenSpec strict validation
openspec validate ms02-virtio-mmio-polling-baseline --strict

# 1.7 diff whitespace check
git diff --check
```

### 阶段 2：QEMU 手工验证（用户能力边界）

按 `.claude/runbooks/qemu-network-testing.md` (R44) 政策，QEMU 测试一律手工。需要 3 个终端。

#### 步骤 2.1：编译 Guest Payload

```bash
cd /home/daivy/projects/serial/work/StarryOS
riscv64-linux-musl-gcc -static -O2 \
  -o tests/ms02_guest_service tests/ms02_guest_service.c
sha256sum tests/ms02_guest_service
```

保存命令、exit、文件路径和 SHA-256 为 `payload-build.log`。

#### 步骤 2.2：无 Hostfwd QEMU（证明串口+MMIO probe 不依赖 hostfwd）

Terminal A（QEMU）：

```bash
cd /home/daivy/projects/serial/work/StarryOS
qemu-system-riscv64 \
  -machine virt -bios default \
  -kernel StarryOS_riscv64-qemu-virt.bin \
  -m 1G -smp 1 \
  -device virtio-blk-device,drive=disk0 \
  -drive id=disk0,if=none,format=raw,file=make/disk.img \
  -device virtio-net-device,netdev=net0 \
  -netdev user,id=net0 \
  -nographic
```

等 `starry:~#` 出现后，验证串口日志含：

- `registered a new Net device ... "virtio-net"`
- `registered a new Block device ... "virtio-blk"`
- `eth0:` 段含 `mac:` 和 `ip: 10.0.2.15/24`

保存完整串口输出为 `qemu-no-hostfwd.log`。

#### 步骤 2.3：User-net QEMU（TCP/UDP 5555 + pcap）

Terminal A（HTTP server，先启动并保持运行）：

```bash
cd /home/daivy/projects/serial/work/StarryOS/tests
python3 -m http.server 18765 --bind 0.0.0.0
```

Terminal B（QEMU）：

```bash
cd /home/daivy/projects/serial/work/StarryOS
qemu-system-riscv64 \
  -machine virt -bios default \
  -kernel StarryOS_riscv64-qemu-virt.bin \
  -m 1G -smp 1 \
  -device virtio-blk-device,drive=disk0 \
  -drive id=disk0,if=none,format=raw,file=make/disk.img \
  -device virtio-net-device,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 \
  -object filter-dump,id=ms02user,netdev=net0,file=ms02-usernet.pcap \
  -nographic
```

Terminal B（QEMU shell，等 `starry:~#` 后**一条一条**输入）：

```sh
wget -q -O /tmp/ms02_service http://10.0.2.2:18765/ms02_guest_service
chmod +x /tmp/ms02_service
/tmp/ms02_service
```

Terminal C（host nc 测试，**必须看到 `MS02_READY` 之后再执行**）：

```bash
# TCP 测试两次（每次输入 MS02_TCP_REQUEST 后回车）
timeout 5 nc 127.0.0.1 5555

# UDP 测试一次
timeout 5 nc -u 127.0.0.1 5555
```

成功判据：

- Guest 串口：`MS02_READY tcp=5555 udp=5555`、`MS02_TCP_PASS`、`MS02_UDP_PASS`、`MS02_COMPLETE`
- Host nc TCP 收到 `MS02_TCP_RESPONSE`
- Host nc UDP 收到 `MS02_UDP_RESPONSE`
- pcap 含 ARP request/reply、5555/TCP 握手和数据

保存 Terminal B 串口输出为 `qemu-usernet.log`，pcap 为 `qemu-usernet.pcap`。

#### 步骤 2.4：TAP QEMU（ARP/ICMP 独立见证）

Terminal A（TAP 设备 + tcpdump）：

```bash
ip route get 10.0.2.2
sudo ip tuntap add dev tap-ms02 mode tap user "$(id -un)"
sudo ip addr add 10.0.2.2/24 dev tap-ms02
sudo ip link set tap-ms02 up
sudo tcpdump -i tap-ms02 -nn -e -w ms02-tap.pcap
```

Terminal B（TAP QEMU）：

```bash
cd /home/daivy/projects/serial/work/StarryOS
qemu-system-riscv64 \
  -machine virt -bios default \
  -kernel StarryOS_riscv64-qemu-virt.bin \
  -m 1G -smp 1 \
  -device virtio-blk-device,drive=disk0 \
  -drive id=disk0,if=none,format=raw,file=make/disk.img \
  -device virtio-net-device,netdev=net0 \
  -netdev tap,id=net0,ifname=tap-ms02,script=no,downscript=no \
  -nographic
```

Terminal B（QEMU shell）：

```sh
nc -u -l -p 5555 >/tmp/ms02-icmp-wait.log 2>&1 &
echo MS02_ICMP_WAITER_READY
```

Terminal C（host ping）：

```bash
ping -c 3 -W 2 10.0.2.15
```

成功判据：

- `ping` 输出 `3 packets transmitted, 3 received, 0% packet loss`
- pcap 含 ARP request/reply 和 3 组 ICMP echo request/reply

清理：

```bash
sudo ip link delete tap-ms02
```

保存 Terminal B 串口输出为 `qemu-tap.log`，pcap 为 `qemu-tap.pcap`。

#### 步骤 2.5：空闲 CPU 采样

在步骤 2.3 或 2.4 的 QEMU 运行期间（guest 服务已启动但无流量），找到 QEMU PID：

```bash
pgrep -f qemu-system-riscv64
```

采样 30 秒：

```bash
top -b -d 1 -n 30 -p <QEMU_PID> > idle-cpu.txt
```

保存为 `idle-cpu.txt`。**不设通过阈值**，只记录环境、方法和原始输出。

#### 步骤 2.6：MS01 Runtime Regression

证明 MS02 改动未退化 MS01 的 socket 行为。

Terminal A（HTTP server）：

```bash
cd /home/daivy/projects/serial/work/StarryOS/tests
python3 -m http.server 18765 --bind 0.0.0.0
```

Terminal B（编译 + QEMU）：

```bash
cd /home/daivy/projects/serial/work/StarryOS
riscv64-linux-musl-gcc -static -O2 \
  -o tests/ms01_socket_baseline tests/ms01_socket_baseline.c

qemu-system-riscv64 \
  -machine virt -bios default \
  -kernel StarryOS_riscv64-qemu-virt.bin \
  -m 1G -smp 1 \
  -device virtio-blk-device,drive=disk0 \
  -drive id=disk0,if=none,format=raw,file=make/disk.img \
  -device virtio-net-device,netdev=net0 \
  -netdev user,id=net0 \
  -nographic
```

Terminal B（QEMU shell，一条一条）：

```sh
wget -q -O /tmp/ms01_test http://10.0.2.2:18765/ms01_socket_baseline
chmod +x /tmp/ms01_test
/tmp/ms01_test
```

成功判据：14 个 `PASS:` 标记（tcp-accept、tcp-adjacent、tcp-512cap、tcp-512-recovery、tcp-relisten、udp-bidi、tcp-nonblock-accept、udp-nonblock、poll-readiness、udp-source、bind-getsockname、bind-ephemeral、bind-conflict、bind-close-cleanup），无 `FAIL:`。

保存 Terminal B 串口输出为 `ms01-regression.log`。

## 验证

阶段 1 成功判据：

| 验证项 | 命令 | 通过条件 |
|---|---|---|
| axnet fmt | `cargo fmt ... -- --check` | exit 0 |
| policy tests | `cargo test ... service::tests` | 8 passed; 0 failed |
| feature tree | `cargo tree ... -i smoltcp` | 含 `auto-icmp-echo-reply` |
| target build | `make LOG=info build` | `.bin` produced |
| MS01 self-test | `python3 scripts/ms01-qemu-test.py --self-test` | `PASS: harness-self-test` |
| openspec validate | `openspec validate ... --strict` | `Change '...' is valid` |
| diff check | `git diff --check` | exit 0 |

阶段 2 成功判据：

| 步骤 | 通过条件 |
|---|---|
| 2.1 | `tests/ms02_guest_service` 存在，SHA-256 已记录 |
| 2.2 | 串口含 `virtio-net`、`virtio-blk`、`eth0` |
| 2.3 | Guest `MS02_COMPLETE`，host nc 收到 `MS02_TCP_RESPONSE`/`MS02_UDP_RESPONSE`，pcap 含 ARP + TCP 5555 |
| 2.4 | `ping` 0% loss，pcap 含 ARP + 3 组 ICMP echo |
| 2.5 | `top` 输出 30 行（QEMU PID 存在） |
| 2.6 | 14 个 `PASS:` 标记 |

## 失败处理

| 症状 | 原因 | 解决 |
|------|------|------|
| `wget: Connection refused` | HTTP server bind `127.0.0.1`，guest 通过 10.0.2.2 连不上 | 改 `--bind 0.0.0.0` |
| `HTTP/1.0 404 File not found` | HTTP server 启动目录错误 | 必须在 `tests/` 目录启动 |
| 串口截断命令（`/tmp/ms` 而非 `/tmp/ms02_service`） | kernel 日志插入打断串口输入 | 一条一条输入，每条等 `starry:~#` 再输下一条 |
| `MS02_FAIL stage=tcp-close-before-payload` | `nc` 连接后未在 5 秒内输入数据 | 重启服务，看到 `MS02_READY` 后立即输入 `MS02_TCP_REQUEST` |
| `chmod: ... No such file or directory` | 上一步命令未完成或文件名截断 | 重做 wget，检查文件存在性 |
| TAP 路由冲突 | `10.0.2.2/24` 已被占用 | 检查 `ip route get 10.0.2.2`，必要时删除冲突路由 |
| TAP 设备残留 | 上次未清理 | `sudo ip link delete tap-ms02` |
| cargo test 编译失败 | 测试引用不存在的 helper | 检查 `any_masked_device_requires_polling` 是否定义 |
| MS01 self-test 失败 | harness 逻辑或解析变化 | 重新跑 `python3 scripts/ms01-qemu-test.py --self-test` |

## 回滚

- QEMU guest 内命令无持久化，关闭 QEMU 即丢失；不需回滚
- TAP 设备：`sudo ip link delete tap-ms02`
- 进程残留：`pkill -f qemu-system-riscv64`
- Guest payload 删除：`rm tests/ms02_guest_service tests/ms01_socket_baseline`
- 测试代码改动：保留 8/8 PASS 作为 refactor witness，不得回滚到 4/4 baseline

## 证据

来源：`openspec/changes/ms02-virtio-mmio-polling-baseline/iterations/003-policy-coverage-and-runtime-evidence.md` Act Response

Revision：`efcf08124294d523ccab4d3569ea97fe31ed96c1`

阶段 1 证据（agent）：

- `openspec/changes/ms02-virtio-mmio-polling-baseline/evidence/003-policy-coverage-and-runtime-evidence/policy-tests.log` — 8/8 unit tests
- `openspec/changes/ms02-virtio-mmio-polling-baseline/evidence/003-policy-coverage-and-runtime-evidence/build.log` — 全部静态验证
- `openspec/changes/ms02-virtio-mmio-polling-baseline/evidence/003-policy-coverage-and-runtime-evidence/review.md` — spec/code review

阶段 2 证据（user）：

- `payload-build.log` — guest payload 编译 + SHA-256
- `qemu-no-hostfwd.log` — 串口+MMIO probe+eth0
- `qemu-usernet.log` + `qemu-usernet.pcap` — TCP/UDP 5555 + ARP
- `qemu-tap.log` + `qemu-tap.pcap` — ARP/ICMP 独立见证
- `idle-cpu.txt` — 30 秒空闲 CPU 采样
- `ms01-regression.log` — 14/14 MS01 PASS

适用范围：MS02 同步轮询路径。MS04+ 引入 IRQ 后，本 runbook 的 `register_waker` 策略和 QEMU 流程需要相应调整。