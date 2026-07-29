# QEMU Network Testing Runbook

> Status: active | Related: K31, K32 | Last updated: 2026-07-29
> Verified: 2026-07-29 MS01 baseline iteration 001 (manual QEMU, 10/10 PASS on local smoltcp/axnet)

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

综上所述，QEMU 的 guest OS shell + sandbox 环境 + 串口特性构成三重阻塞面，
**不存在可靠的自脚本路径**。Zephyr 或裸机程序不受此限制，但 StarryOS 作为完整
宏内核不在该类别内。

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

## 排障

| 症状 | 原因 | 解决 |
|------|------|------|
| `wget: Connection refused` | HTTP server bind `127.0.0.1`，guest 通过 10.0.2.2 连不上 | 改 `--bind 0.0.0.0` |
| shell 提示符是 `starry:~# ` 不是 `/ #` | 不同版本 prompt 不同 | 看到可输入光标即可 |
| rebind `Address in use` | fork 版 smoltcp 不立即释放 port | 测试中 `sleep(2)` 临时绕过（迁移后移除） |

## 变更历史

- 2026-07-29：升级为硬性政策声明。新增三层独立证据（OS shell 阻塞、sandbox
  EPERM、串口分帧不可靠），来源 iteration 001 Plan Review + Act Response。
  明确"QEMU 测试一律手动，禁止自动化"。
- 2026-07-28：初始版本。记录 MS01 baseline 测试流程、HTTP 下载法、排障经验。
