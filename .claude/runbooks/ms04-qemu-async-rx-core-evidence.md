# MS04 QEMU 异步 RX 核心证据采集

- Status: active
- Last validated: 2026-08-12
- Environment: WSL2 x86_64；QEMU 7.0.0；RISC-V `virt`；1 GiB；单 hart；单 VirtIO-MMIO NIC；user-net
- Source: 已归档的 `2026-08-12-ms04-qemu-async-rx-queue-baseline` iteration 009 与
  `evidence/009-final-sandbox-rerun-and-qemu-runtime/`

## 适用范围

本 Runbook 验证 MS04 的核心异步 RX 路径：唯一 queue task、IRQ/软件唤醒、空闲无忙轮询、
96 包有界 burst、descriptor 回收守恒、budget/self-yield 和 Router 满后恢复。

它不证明完整 MS01/MS02/MS03 compatibility、SMP、PCI、真板 DMA/coherency、物理时序或性能。
完整同步网络与 IRQ 回归分别使用 R45 和 R48；QEMU guest 始终遵守 R44 的手工输入政策。

## 前置条件

- 当前 change 或后继 change 的自动 Gate 已通过；产品编译错误不得转成手工测试。
- `StarryOS_riscv64-qemu-virt.bin` 和 `make/disk.img` 已生成。
- `riscv64-linux-musl-gcc` 可在普通 host shell 中执行。受限沙箱若返回 `SIGSYS` 或
  `Bad system call`，转到普通 shell 复跑同一命令并保留两次输出。
- `tests/ms04_rx_probe.c` 必须显式包含 `<sys/time.h>`；宿主 libc 的间接包含不能作为
  musl payload 的编译见证。
- 先为 kernel 和 payload 记录 byte size、mtime 与 SHA-256。启动 QEMU 后不得重建它们；
  否则 exact-binary reproducibility 声明失效，需要全新会话和证据。

为避免终端自动换行拆断长路径，每个终端先设置：

```bash
cd /home/daivy/projects/serial/work/StarryOS
EV=openspec/changes/<change>/evidence/<iteration>
```

## 操作步骤

### 1. 构建并限定 payload

```bash
make -B tests/ms03_irq_probe tests/ms04_rx_probe
riscv64-linux-musl-gcc -static -O2 \
  -o tests/ms02_guest_service tests/ms02_guest_service.c
riscv64-linux-musl-gcc -static -O2 \
  -o tests/ms01_socket_baseline tests/ms01_socket_baseline.c

file tests/ms03_irq_probe tests/ms04_rx_probe \
  tests/ms02_guest_service tests/ms01_socket_baseline
stat -c '%y %s %n' StarryOS_riscv64-qemu-virt.bin \
  tests/ms03_irq_probe tests/ms04_rx_probe \
  tests/ms02_guest_service tests/ms01_socket_baseline
sha256sum StarryOS_riscv64-qemu-virt.bin \
  tests/ms03_irq_probe tests/ms04_rx_probe \
  tests/ms02_guest_service tests/ms01_socket_baseline
```

四个 payload 必须是 fresh、static RISC-V ELF。任一编译诊断、缺文件或非零退出都停止。

### 2. 启动 HTTP 服务和 QEMU

Terminal A：

```bash
cd /home/daivy/projects/serial/work/StarryOS/tests
python3 -m http.server 18765 --bind 0.0.0.0
```

Terminal B：

```bash
cd /home/daivy/projects/serial/work/StarryOS
EV=openspec/changes/<change>/evidence/<iteration>
script -q -f "$EV/qemu-serial.log" -c \
'qemu-system-riscv64 -machine virt -bios default -kernel StarryOS_riscv64-qemu-virt.bin -m 1G -smp 1 -device virtio-blk-device,drive=disk0 -drive id=disk0,if=none,format=raw,file=make/disk.img -device virtio-net-device,netdev=net0 -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 -nographic'
```

`script` 只录制；用户在每次出现 `starry:~#` 后逐条输入 guest 命令。若当前 Gate 要求 boot
签名，必须从启动开始录制；只保留 workload 摘要不能补证 boot。

### 3. 下载并运行 quiet/wake 模式

Guest：

```sh
wget -q -O /tmp/ms04_rx_probe http://10.0.2.2:18765/ms04_rx_probe
chmod +x /tmp/ms04_rx_probe
/tmp/ms04_rx_probe snapshot
/tmp/ms04_rx_probe idle
/tmp/ms04_rx_probe nudge
```

每个模式必须产生唯一 `MS04 PASS mode=<mode>`：

- snapshot：`lifecycle=2`、`owner=1`，`fault/restore/irq_entry=0`。
- idle：IRQ、软件事件、task、descriptor、budget 和 fault delta 全为 0。
- nudge：`nudge=1 task=1 empty=1`，descriptor delta 为 0。

### 4. 运行 96 包 burst

Terminal C 先启动：

```bash
cd /home/daivy/projects/serial/work/StarryOS
python3 scripts/ms04_rx_stimulus.py --host 0.0.0.0 --port 15556
```

Guest：

```sh
/tmp/ms04_rx_probe burst
```

成功判据：

- 唯一 `MS04 PASS mode=burst`。
- `reaped_delta == refilled_delta == delivered_delta == 96`。
- `isr_publish` 和 `isr_wake` 均推进。
- `budget_exhausted > 0` 且 `self_yield > 0`。
- `fault/restore/irq_entry=0`。

MS04 iteration 009 的见证值为 budget/yield 各 2、Router full/space wake 各 1；这些是一次
运行结果，不是未来运行必须精确复现的固定阈值。

## 验证

```bash
rg -n 'MS04 (PASS|FAIL)|fault=|restore=|irq_entry=' "$EV/qemu-serial.log"
sha256sum "$EV/qemu-serial.log"
```

每个执行模式只能有一个终态 marker。原始串口、命令、环境、artifact hashes 和派生日志
必须能互相追溯。若还要声明完整 compatibility，继续执行对应 change 明列的 R45/R48
回归；未运行项写 `SKIPPED/WAIVED`，不得写 PASS。

## 失败处理

| 现象 | 分类与处理 |
|---|---|
| musl 报 `struct timeval` incomplete | 产品 payload 错误；确认显式 `<sys/time.h>`，修复后以真实 musl RED/GREEN 复跑 |
| compiler `SIGSYS` / `Bad system call` | 只有最早失败层确认为沙箱能力拒绝时才记 `ENV-BLOCKED`；普通 host shell 复跑同一命令 |
| idle 有持续 task/descriptor 进度 | 产品失败；保存 PRE/POST/DELTA，停止 burst |
| nudge 修改 descriptor 或 IRQ | 产品失败；检查软件唤醒与 register-recheck 边界 |
| burst 少于 96、回收不守恒或无 yield | 产品失败；保存 host/guest 原始日志，不进入 compatibility |
| 任一 safety/fault 非零 | 产品失败；记录第一失败层，不用后续 PASS 覆盖 |
| artifact 在 hash 后被重建 | 当前 session 不再支持 exact-binary 声明；重新 hash 并启动全新 QEMU session |
| 长路径被终端换行拆断 | 使用短变量 `$EV`；保留意外文件，核对内容后合并，避免重跑覆盖首次失败 |

## 回滚

本流程只生成 payload、`/tmp` guest 文件和 change-local Evidence。退出 QEMU 使用
`Ctrl-A X`，停止 host server 使用 `Ctrl-C`。Guest `/tmp` 随 QEMU 退出丢失。若需要清理
host payload，应先确认 change Evidence 已记录 size/hash；Runbook 不自动删除文件。

## 证据

- `openspec/changes/archive/2026-08-12-ms04-qemu-async-rx-queue-baseline/iterations/009-final-sandbox-rerun-and-qemu-runtime.md`
- `openspec/changes/archive/2026-08-12-ms04-qemu-async-rx-queue-baseline/evidence/009-final-sandbox-rerun-and-qemu-runtime/`
- 验证 revision：`8f5b5228747dc817a5a9de7a3461dccdf06e0c24` 加 iteration 009 staged/worktree diff
- 适用限制：本次 raw serial 未含 boot 与 termination，完整 compatibility 和 post-regression
  safety 由用户显式 waiver；这些缺口未写入本 Runbook 的核心成功结论。
