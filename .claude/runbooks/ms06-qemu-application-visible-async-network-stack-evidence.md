# MS06 QEMU 应用可见异步网络栈验收

- Status: active
- Last validated: 2026-08-27；2026-09-02 MS07 Cycle 006 兼容回归 12/12 + validator exit 0（证据 `006-rework/ms06-qemu-serial.log`）
- Environment: QEMU 7.0.0；RISC-V `virt`；1 GiB；单 hart；单 VirtIO-MMIO NIC；user-net；`LOG=warn`；Rust nightly-2026-02-25
- Source: `ms06-application-visible-async-network-stack` Iteration 008 / Cycle `001-replan` Act Response（`reported`）+ `evidence/008-single-hart-qemu-acceptance/001-replan/`；2026-09-02 回归来源 MS07 Iteration 007 Cycle 006 `evidence/007-single-hart-qemu-qualification/006-rework/ms06-qemu-serial.log`

## 适用范围

在 single-hart QEMU VirtIO-MMIO 上手工验收 MS06 应用可见异步网络栈：MS06 12-case readiness
probe 必须先 12/12 + exit 0，随后同一 session 跑 MS01/MS04/MS05 兼容回归。不覆盖 reset、SMP、
PCI/DWMAC、真板、性能；guest shell 一律手工输入（R44 硬性政策）。

## 前置条件

- `StarryOS_riscv64-qemu-virt.bin` 与四个 probe（`tests/ms06_stack_readiness_probe`、
  `tests/ms01_socket_baseline`、`tests/ms04_rx_probe`、`tests/ms05_data_plane_probe`）已构建为
  static RISC-V ELF；启动前记录 size/mtime，session 中不得重建。
- 自动 Gate（host-test、probe seam、validator self-test）已通过。
- `riscv64-linux-musl-gcc`、`script`、`tee` 可用；HTTP server 需 `--bind 0.0.0.0`。

## 操作步骤

Terminal A — HTTP server（先启动，保持运行）：

```bash
cd /home/daivy/projects/serial/work/StarryOS/tests
python3 -m http.server 18765 --bind 0.0.0.0
```

Terminal B — 启动 QEMU（已冻结镜像，不重建；从 boot 开始录完整串口到 `$EV/ms06-qemu-serial.log`）：

先设短变量，避免长路径被终端换行拆断：

```bash
cd /home/daivy/projects/serial/work/StarryOS
EV=openspec/changes/<change>/evidence/<iteration>/<cycle>
mkdir -p "$EV"
```

```bash
cd /home/daivy/projects/serial/work/StarryOS
script -q -e -f "$EV/ms06-qemu-serial.log" -c 'qemu-system-riscv64 -m 1G -smp 1 -machine virt -bios default -kernel StarryOS_riscv64-qemu-virt.bin -device virtio-blk-device,drive=disk0 -drive id=disk0,if=none,format=raw,file=make/disk.img -device virtio-net-device,netdev=net0 -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 -nographic'
```

> 与 `make ARCH=riscv64 justrun` 等价但不重建产物；需先 `make ARCH=riscv64 build` 冻结
> 镜像。作为其他 cycle 的兼容回归时 `$EV` 指该 cycle 的 evidence 目录（见 R58）。

出现 `starry:~#` 后按失败即停顺序逐条输入，每个 workload 后显式发布 exit：

MS06（必须先跑，12/12 才继续）：

```sh
wget -q -O /tmp/ms06 http://10.0.2.2:18765/ms06_stack_readiness_probe && chmod +x /tmp/ms06 && /tmp/ms06; rc=$?; echo "MS06_HARNESS_EXIT: $rc"
```

MS01：

```sh
wget -q -O /tmp/ms01 http://10.0.2.2:18765/ms01_socket_baseline && chmod +x /tmp/ms01 && /tmp/ms01; rc=$?; echo "MS01_HARNESS_EXIT: $rc"
```

MS04（snapshot/idle/nudge 无需 host；burst 需 Terminal C 先起 stimulus）：

```bash
# Terminal C（仅 burst 前启动）
cd /home/daivy/projects/serial/work/StarryOS
python3 scripts/ms04_rx_stimulus.py --host 0.0.0.0 --port 15556
```

```sh
wget -q -O /tmp/ms04 http://10.0.2.2:18765/ms04_rx_probe && chmod +x /tmp/ms04
/tmp/ms04 snapshot; echo "MS04_EXIT_snapshot: $?"
/tmp/ms04 idle;      echo "MS04_EXIT_idle: $?"
/tmp/ms04 nudge;     echo "MS04_EXIT_nudge: $?"
/tmp/ms04 burst;     echo "MS04_EXIT_burst: $?"
```

MS05（六 mode，每 mode 一个 host stimulus 15557，Terminal C 先 `set -o pipefail`）：

```bash
# Terminal C（每 mode 一次，跑完 Ctrl-C 停掉再换下一个）
cd /home/daivy/projects/serial/work/StarryOS
set -o pipefail
python3 scripts/ms05_data_plane_stimulus.py --port 15557 | tee /tmp/ms05-<mode>-host.log
```

```sh
wget -q -O /tmp/ms05 http://10.0.2.2:18765/ms05_data_plane_probe && chmod +x /tmp/ms05
/tmp/ms05 snapshot;            echo "MS05_EXIT_snapshot: $?"
/tmp/ms05 tx-only 96 64;       echo "MS05_EXIT_tx-only: $?"
/tmp/ms05 bidirectional 96 64; echo "MS05_EXIT_bidirectional: $?"
/tmp/ms05 slot-full;           echo "MS05_EXIT_slot-full: $?"
/tmp/ms05 descriptor-full;     echo "MS05_EXIT_descriptor-full: $?"
/tmp/ms05 flush;               echo "MS05_EXIT_flush: $?"
```

## 验证

MS06 用 validator 对完整 raw 串口判定（不得人工摘录）：

```bash
python3 scripts/ms06-qemu-validate.py --expect-environment qemu-virt-riscv64-single-hart "$EV/ms06-qemu-serial.log"
```

（validator 已移除 `--expect-revision` 身份层；按项目 identity 清理不再传 revision。）

- MS06：12 个唯一 `PASS:`（固定顺序）、`MS06_STACK_READINESS_END`、`MS06_HARNESS_EXIT: 0`；
  validator exit 0。
- MS01：14 个唯一 PASS + `MS01_HARNESS_EXIT: 0`。
- MS04：snapshot/idle/nudge/burst 各唯一 `MS04 PASS mode=…`；burst `reaped_delta ==
  refilled_delta == delivered_delta == 96`、`budget_exhausted>0`、`self_yield>0`、`fault=0`。
- MS05：六 mode 各唯一 `MS05 PASS mode=…`；`fault=0`；slot-full/descriptor-full Full→recovery
  闭合；flush `flush_ok=1`；`MS05 WITNESS host_received=96` 与 host 计数一致。
- 完整 raw 串口 grep `FAIL|panic|trap|fatal|illegal|page fault` 为空。

## 失败处理

| 症状 | 处理 |
|---|---|
| 任一 FAIL / 缺 marker / 非 0 exit | 停止后续，保留首次失败现场，不用重跑 PASS 覆盖 |
| `wget: Connection refused` | HTTP server 必须 `--bind 0.0.0.0` |
| `wget` 挂起 | 数据面问题；用 R55 分层诊断，或 debugfs 离线注入 probe |
| MS05 `reason=handshake` | host stimulus 未先启动；重启 stimulus 再跑该 mode |
| artifact 被 `make run` 重建 | 先 `make build` 冻结镜像，再用终端 B 的直连 qemu 命令（不复用 `make run`）拉起新 session |
| 长路径换行拆断 | 用短变量 `$EV`；保留意外文件，核对后合并 |

## 回滚

本流程不修改产品源码；guest `/tmp` payload 随 QEMU 退出丢失。退出 QEMU `Ctrl-A X`；停 host
server `Ctrl-C`。进程残留先 `pgrep -af` 核对，再对确认的单个 PID `kill <PID>`，不用宽泛 `pkill -f`。

## 证据

- 来源：`ms06-application-visible-async-network-stack` Iteration 008 / Cycle `001-replan`
  Act Response（`reported`）+ `evidence/008-single-hart-qemu-acceptance/001-replan/`
  （README、qemu-runtime-markers.md、host-runtime-results.md）。
- 完整 raw 串口：`/tmp/ms06-iteration-008-cycle-001-qemu-serial.log`（262 行 / 86,643 B）；
  2026-09-02 回归串口 `evidence/007-single-hart-qemu-qualification/006-rework/ms06-qemu-serial.log`。
- Revision：HEAD `1d0313ad8d0f36d918d1a101dd0ceda5c2ba336b`（net-k3）。
- 适用限制：结论只覆盖 single-hart QEMU VirtIO-MMIO 软件/设备模型；不扩大到 reset、SMP、
  PCI/DWMAC、真板或性能。
