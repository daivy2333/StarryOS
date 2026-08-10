# QEMU Network Testing Runbook

> Status: active | Related: K31, K32 | Last updated: 2026-08-09
> Verified: 2026-07-29 MS01 manual QEMU 10/10 PASS; 2026-08-09 build exit/artifact classification checked

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
- 完整输出、最终退出码和要求的产物或 hash；
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

## 验证

- QEMU 必须进入 `starry:~#`，payload 命令最终退出，并输出完整 START/END marker。
- 对应 change 要求的每个 case 必须有 PASS/FAIL、完整串口和最终退出或中止状态。
- 自动命令只有最终非零且最早失败层满足上方分类时才能记 `ENV-BLOCKED`；用户在
  sandbox 外复跑成功后，该 Gate 才能改为 PASS。
- 中断、缺 marker、缺日志或只保留摘要均不算通过。

## 回滚

本流程不修改产品源码。HTTP 下载到 guest `/tmp` 的 payload 会在 QEMU 退出后消失。
用 `Ctrl-A X` 退出 QEMU，用 `Ctrl-C` 停止 host HTTP server。若具体 change 另行要求
mount 或修改 rootfs，必须按该 change 的手工步骤执行 umount/恢复，不能套用本段。

## 排障

| 症状 | 原因 | 解决 |
|------|------|------|
| `wget: Connection refused` | HTTP server bind `127.0.0.1`，guest 通过 10.0.2.2 连不上 | 改 `--bind 0.0.0.0` |
| shell 提示符是 `starry:~# ` 不是 `/ #` | 不同版本 prompt 不同 | 看到可输入光标即可 |
| rebind `Address in use` | fork 版 smoltcp 不立即释放 port | 测试中 `sleep(2)` 临时绕过（迁移后移除） |

## 证据

- 2026-07-29：`t01-smoltcp-axnet-baseline` iteration 001/002 的手工 QEMU 与自动化
  失败记录，支持 guest shell 手工政策和 HTTP 下载路径。
- 2026-08-09：`make LOG=info build` 的同次输出同时出现 Cargo home 只读、禁止联网
  和最终 build/objcopy 成功，支持“按最终退出与产物分类，不按中间警告分类”。
- 适用限制：该规则只处理执行环境能力边界，不证明任何具体 change 的产品行为。

## 变更历史

- 2026-08-09：增加 sandbox 阻塞分类与手工交接规则。只有明确的环境能力拒绝可以
  标记 `ENV-BLOCKED`；该项移到 iteration 末尾，并保留命令、退出码、日志和产物
  见证。产品错误不得转为手工边界。
- 2026-07-29：升级为硬性政策声明。新增三层独立证据（OS shell 阻塞、sandbox
  EPERM、串口分帧不可靠），来源 iteration 001 Plan Review + Act Response。
  明确"QEMU 测试一律手动，禁止自动化"。
- 2026-07-28：初始版本。记录 MS01 baseline 测试流程、HTTP 下载法、排障经验。
