# MS03 VirtIO-MMIO 可诊断中断基线 — 证据采集

- Status: active
- Last validated: 2026-08-27（命令行更新为 EV+script/tee 采集模式；依据 `qemu-evidence-capture.md`、R44 证据精简原则）
- Environment: WSL2 x86_64; Rust nightly-2026-02-25; QEMU riscv64; single-hart
- Source: `openspec/changes/ms03-virtio-mmio-diagnostic-irq-baseline/iterations/000-initial.md` (Act Response: reported)

## 适用范围

MS03 可诊断中断基线的完整证据采集路径：

- 启动签名验证（UART IRQ 10 + NET IRQ 7 设备 handler 注册）
- Guest C probe（idle / uart-only / rx2 / tx2 / both / repeat rx2）
- MS02 TCP/UDP 5555 回归
- MS01 14/14 socket baseline 回归
- 纯逻辑 host harness（20 tests）

**不适用**：

- 异步 RX/TX（MS04+）——本路径只覆盖中断诊断控制面，不含 waker 或 queue task
- 真板（VF2）、PCI
- TAP 网络、无 hostfwd 启动（已有 MS02 runbook R45 覆盖）
- 自动 QEMU harness——按 R44 硬性政策，QEMU 测试一律手工

## 前置条件

- 工具链：`riscv64-linux-musl-gcc` 已安装
- QEMU：`qemu-system-riscv64` 已安装
- 已构建内核：`make LOG=info build`
- 磁盘镜像：`make/disk.img` 存在（ext4，可直接 mount loop）
- 分支：`net-k3`
- Guest probe 源码：`tests/ms03_irq_probe.c` 存在

## 操作步骤

### 阶段 1：Agent 静态验证

```bash
# 1.1 纯逻辑 host harness
rustc --edition=2024 --test tests/ms03-irq-host-harness.rs \
  -o /tmp/ms03-irq-host-test && /tmp/ms03-irq-host-test
# 通过条件：20 passed; 0 failed

# 1.2 axnet service tests
cargo test --manifest-path crates/axnet/Cargo.toml \
  --locked --offline --lib service::tests -- --nocapture
# 通过条件：8 passed; 0 failed

# 1.3 UART async tests
cargo test --manifest-path crates/uart_16550/Cargo.toml \
  --offline --features async
# 通过条件：62 unit + 18 doc passed

# 1.4 C probe host syntax
cc -Wall -Wextra -Werror -fsyntax-only tests/ms03_irq_probe.c
# 通过条件：exit 0，无输出

# 1.5 target build
make LOG=info build
# 通过条件：exit 0，StarryOS_riscv64-qemu-virt.bin 生成
```

### 阶段 2：QEMU 手工验证

按 R44 硬性政策，QEMU 测试一律手工。需要 2 个终端（1 个 QEMU + 1 个 echo server）。

#### 步骤 2.1：编译 Guest Probe

```bash
cd /home/daivy/projects/serial/work/StarryOS
make tests/ms03_irq_probe ARCH=riscv64
file tests/ms03_irq_probe
# 预期: ELF 64-bit LSB ..., UCB RISC-V, ..., statically linked
```

#### 步骤 2.2：将 Probe 放入 Rootfs

```bash
# disk.img 是裸 ext4（无分区表），直接用 loop mount
sudo mount -o loop make/disk.img /mnt/starry-rootfs
sudo cp tests/ms03_irq_probe /mnt/starry-rootfs/root/
sudo umount /mnt/starry-rootfs
```

#### 步骤 2.3：启动 QEMU 并验证启动签名

先建立 evidence 目标目录与短变量，再从启动开始录制完整串口：

```bash
cd /home/daivy/projects/serial/work/StarryOS
EV=openspec/changes/<change>/evidence/<iteration>/<cycle>
```

Terminal A（QEMU）：

```bash
script -q -e -f "$EV/qemu-serial.log" -c 'make ARCH=riscv64 run'
```

等 `starry:~#` 出现，检查内核日志（前 ~50 行）**必须**包含以下三行：

```
[UART INIT] QEMU UART IRQ 10 registered as device handler, buffers=64KBx2
[NET IRQ] VirtIO-MMIO net validated: magic=0x74726976 version=1 device_id=1 at 0x10007000
[NET IRQ] Diagnostic IRQ 7 handler registered; polling fallback active
```

串口即保存在 `$EV/qemu-serial.log`（`script` 只录制，不从启动开始录制则不能补证 boot）。

> 任一行缺失或显示 `magic mismatch` / `version too old` / `Not a network device` → 立即停止，保存串口日志。

#### 步骤 2.4：运行 Guest Probe

Terminal B（echo server，先启动）：

```bash
# 注意：端口 15555，不是 5555。5555 被 QEMU hostfwd 占用会导致 nc 绑定失败。
nc -l -p 15555 -k
```

Terminal A（QEMU shell，一条一条执行）：

```sh
# 2.4a 空闲窗口（验证无 IRQ storm，2000ms 窗口内 total delta ≤ 100）
/root/ms03_irq_probe idle

# 2.4b UART-only（验证 UART 不触发 net IRQ）
/root/ms03_irq_probe uart

# 2.4c RX2（在 echo server 终端输入两行任意内容作为 TCP 回应）
/root/ms03_irq_probe rx2

# 2.4d TX2
/root/ms03_irq_probe tx2

# 2.4e concurrent（UART + net 同时）
/root/ms03_irq_probe both

# 2.4f 重复投递验证（第二次 RX2，证明 IRQ 可重复触发）
/root/ms03_irq_probe rx2
```

成功判据（每条 probe 输出最后一行）：

| 模式 | 通过条件 |
|------|----------|
| idle | `PASS idle`，total delta ≤ 100 |
| uart | `PASS uart`，net used_ring delta=0 |
| rx2 | `PASS rx2`，used_delta ≥ 1，ack_delta ≥ 1 |
| tx2 | `PASS tx2`，used_delta ≥ 1，ack_delta ≥ 1 |
| both | `PASS both` |
| rx2 (repeat) | 再次 `PASS rx2` |

#### 步骤 2.5：MS02 回归

Terminal C（HTTP server，同一次或新开终端）：

```bash
cd /home/daivy/projects/serial/work/StarryOS/tests
python3 -m http.server 18765 --bind 0.0.0.0
```

编译 guest service 并启动 QEMU（或复用当前 QEMU session；若新开 session 用 EV+script 录制到 `$EV/regression-ms02.log`）：

```bash
cd /home/daivy/projects/serial/work/StarryOS
EV=openspec/changes/<change>/evidence/<iteration>/<cycle>
riscv64-linux-musl-gcc -static -O2 \
  -o tests/ms02_guest_service tests/ms02_guest_service.c

script -q -e -f "$EV/regression-ms02.log" -c \
'qemu-system-riscv64 -machine virt -bios default -kernel StarryOS_riscv64-qemu-virt.bin -m 1G -smp 1 -device virtio-blk-device,drive=disk0 -drive id=disk0,if=none,format=raw,file=make/disk.img -device virtio-net-device,netdev=net0 -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 -nographic'
```

QEMU shell：

```sh
wget -q -O /tmp/ms02_service http://10.0.2.2:18765/ms02_guest_service
chmod +x /tmp/ms02_service
/tmp/ms02_service
```

看到 `MS02_READY tcp=5555 udp=5555` 后：

```bash
# TCP（另开终端）
nc 127.0.0.1 5555
# 输入 MS02_TCP_REQUEST → 预期收到 MS02_TCP_RESPONSE

# UDP
echo "MS02_UDP_REQUEST" | nc -u -w1 127.0.0.1 5555
# 预期收到 MS02_UDP_RESPONSE
```

通过条件：guest 串口含 `MS02_TCP_PASS` + `MS02_UDP_PASS` + `MS02_COMPLETE`。

#### 步骤 2.6：MS01 14/14 Socket 回归

QEMU shell（同一次启动）：

```sh
wget -q -O /tmp/ms01_test http://10.0.2.2:18765/ms01_socket_baseline
chmod +x /tmp/ms01_test
/tmp/ms01_test
```

通过条件：14 个 `PASS:` 标记，0 个 `FAIL:`。

### 阶段 3：证据归档

```bash
EV=openspec/changes/<change>/evidence/<iteration>/<cycle>
mkdir -p "$EV"
cp make/build.log "$EV/build.log"
```

`script` 已把串口写入 `$EV/qemu-serial.log`、`$EV/regression-ms02.log`、`$EV/ms01-regression.log`（MS01 回归也应在录制 session 内完成；若新开 session，用同样 EV+script 命令录制）。

evidence 目录必须包含：
- `README.md` — Gate 映射和结果判定
- `build.log` — target build 输出
- `regression-ms02.log` — MS02 TCP/UDP 结果
- `ms01-regression.log` — MS01 14/14 PASS
- `qemu-serial.log` — 完整串口日志（启动签名 + 全部 probe + 回归）

## 验证

| 验证项 | 命令 / 操作 | 通过条件 |
|--------|-------------|----------|
| host harness | `rustc --edition=2024 --test ...` | 20/20 PASS |
| axnet tests | `cargo test ... service::tests` | 8/8 PASS |
| UART tests | `cargo test ... --features async` | 80/80 PASS |
| C syntax | `cc -Wall -Wextra -Werror -fsyntax-only` | exit 0 |
| target build | `make LOG=info build` | exit 0，.bin 生成 |
| 启动签名 | QEMU 内核日志 | UART IRQ 10 + NET validated + IRQ 7 registered |
| idle probe | `/root/ms03_irq_probe idle` | PASS idle，delta ≤ 100 |
| uart probe | `/root/ms03_irq_probe uart` | PASS uart，net delta=0 |
| rx2 probe | `/root/ms03_irq_probe rx2` | PASS rx2，used≥1，ack≥1 |
| tx2 probe | `/root/ms03_irq_probe tx2` | PASS tx2，used≥1，ack≥1 |
| both probe | `/root/ms03_irq_probe both` | PASS both |
| repeat rx2 | 第二次 `rx2` | 再次 PASS rx2 |
| MS02 回归 | ms02_guest_service | MS02_TCP_PASS + MS02_UDP_PASS + MS02_COMPLETE |
| MS01 回归 | ms01_socket_baseline | 14/14 PASS |

## 失败处理

| 症状 | 原因 | 解决 |
|------|------|------|
| `Not a network device (device_id=1)` | `device_id` 校验写成了 `!= 2`（块设备）而非 `!= 1`（网卡）。VirtIO device_id 1=net, 2=block | 修改 `virtio_net_irq.rs` 中校验条件为 `device_id != 1` |
| 所有 IRQ 事件 `used_ring=0`，全部被分类为 `spurious` | VirtIO MMIO `InterruptStatus`（0x60）和 `InterruptACK`（0x64）是 **32-bit** 寄存器，用 `u8` 读写导致读到错误字节 | 改用 `read_volatile::<u32>()` / `write_volatile::<u32>()`，取低 2 bits |
| `used_ring` 正确递增但 `ack_count=0` | handler 中只有 ACK write 没有 `ack_count.fetch_add` | 在 ACK write 之后增加 `TELEMETRY.ack_count.fetch_add(1, Relaxed)` |
| `nc -l -p 5555` 报 `Address already in use` | QEMU `hostfwd=tcp::5555-:5555` 占用了宿主机 5555 端口。probe 用 `connect(10.0.2.2, 5555)` 出站不需要 hostfwd，但 host 端 `nc -l -p 5555` 与 QEMU hostfwd 冲突 | 换用其他端口（如 15555）：probe `SERVER_PORT=15555`，host 端 `nc -l -p 15555 -k` |
| `mount: wrong fs type` | disk.img 是裸 ext4（无分区表），`mount -o loop,offset=...` 指定了错误偏移 | 去掉 offset 参数：`sudo mount -o loop make/disk.img /mnt/starry-rootfs` |
| `MS02_FAIL stage=tcp-close-before-payload` | `timeout 5 nc ...` 在用户输入前就 kill 了 nc | 用裸 `nc 127.0.0.1 5555`，手动输入后 Ctrl+C |
| 编译报 `error: cannot find type Ordering` | `virtio_net_irq.rs` 中 `ack_count.fetch_add` 使用了 `Ordering::Relaxed` 但未 import | 添加 `use core::sync::atomic::Ordering;` |

## 回滚

- QEMU guest 内命令无持久化，关闭 QEMU 即丢失；不需回滚
- Rootfs 挂载写入的 probe 在下一次 `make rootfs` 时覆盖
- 进程残留：先用`pgrep -af qemu-system-riscv64`核对命令，再对确认的单个PID执行`kill <PID>`
- TAP 设备（如果使用了 MS02 的 TAP 测试）：`sudo ip link delete tap-ms02`

## 证据

来源：`openspec/changes/ms03-virtio-mmio-diagnostic-irq-baseline/iterations/000-initial.md` Act Response (reported, 2026-08-03)

Revision：`b35fcafa5fb7388d9e22b448a1e30b1e77cfbdd8`

阶段 1 证据（agent）：
- `openspec/changes/ms03-virtio-mmio-diagnostic-irq-baseline/evidence/000-initial/build.log` — target build

阶段 2 证据（user）：
- `openspec/changes/ms03-virtio-mmio-diagnostic-irq-baseline/evidence/000-initial/qemu-serial.log` — 启动签名 + 全部 probe + 回归
- `openspec/changes/ms03-virtio-mmio-diagnostic-irq-baseline/evidence/000-initial/regression-ms02.log` — MS02 TCP/UDP
- `openspec/changes/ms03-virtio-mmio-diagnostic-irq-baseline/evidence/000-initial/ms01-regression.log` — MS01 14/14

适用范围：MS03 中断诊断控制面。MS04 引入 waker/queue task 后 IRQ handler 行为会变化，本 runbook 的诊断 probe 模式仍可复用。
