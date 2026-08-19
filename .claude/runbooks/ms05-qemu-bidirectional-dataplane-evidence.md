# MS05 QEMU 有界双向数据面证据采集

- Status: active
- Last validated: 2026-08-19
- Environment: QEMU RISC-V `virt`；单 hart、单 VirtIO-MMIO NIC；1 GiB；
  user-net；Rust nightly-2026-02-25；cc 11.4；/opt/musl/riscv64-linux-musl-cross；
  python3。
- Source: `ms05-qemu-bounded-bidirectional-device-data-plane` Iteration 011 / Cycle
  004-rework Act Response（`reported`）+ `evidence/011-independent-manual-qemu-runtime-and-closeout/004-rework/`

## 适用范围

验证 MS05 有界双向设备数据面的六种运行模式：

- `snapshot`：V3 快照一致性。
- `tx-only <count> <payload>`：客机 TX。
- `bidirectional <count> <payload>`：双向 TX/RX。
- `slot-full`：TX slot 满 → Recovery。
- `descriptor-full`：driver/descriptor 满 → Recovery。
- `flush`：C4 ticketed flush 闭合。

**不适用**：

- MS04 异步 RX 核心证据（snapshot/idle/nudge/burst）——用 R51。
- 真板、SMP、PCI、DWMAC、性能优化。
- 自动 QEMU harness——按 R44 硬性政策，QEMU 一律手工。

## 前置条件

- 自动 Gate 已通过（产品编译错误不得转成手工）。
- `StarryOS_riscv64-qemu-virt.bin` 与 `make/disk.img` 已生成；agent 静态验证
  （stimulus self-test/loopback、C harness、strict C）已 GREEN。
- `riscv64-linux-musl-gcc` 可用；HTTP server 需 `--bind 0.0.0.0`。
- host stimulus 脚本 `scripts/ms05_data_plane_stimulus.py` 与 guest 探针
  `tests/ms05_data_plane_probe` 为同一最终源码构建。
- QEMU guest 内 payload 经 wget 下载到 `/tmp`。

## 完整命令行流程

### 1. 启动 HTTP server（Terminal A，保持运行）

```bash
cd /home/daivy/projects/serial/work/StarryOS/tests
python3 -m http.server 18765 --bind 0.0.0.0
```

### 2. 启动 QEMU（Terminal B，录制完整串口）

```bash
cd /home/daivy/projects/serial/work/StarryOS
EV=openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/evidence/011-independent-manual-qemu-runtime-and-closeout/004-rework
script -q -f "$EV/qemu-serial.log" -c \
'qemu-system-riscv64 -machine virt -bios default -kernel StarryOS_riscv64-qemu-virt.bin -m 1G -smp 1 -device virtio-blk-device,drive=disk0 -drive id=disk0,if=none,format=raw,file=make/disk.img -device virtio-net-device,netdev=net0 -netdev user,id=net0 -nographic'
```

> 数据面不需要 hostfwd：guest probe 作为 UDP client 出站连 host `10.0.2.2:15557`，
> host stimulus 是 15557 上的 UDP server。出现 `starry:~#` 后逐条输入。

### 3. Guest 下载探针（一次）

```sh
wget -q -O /tmp/ms05_data_plane_probe http://10.0.2.2:18765/ms05_data_plane_probe
chmod +x /tmp/ms05_data_plane_probe
```

### 4. 六个模式

每个模式：先启动 host stimulus（Terminal C），再在 guest 跑对应命令。host stimulus
每个模式需重启一次（一次服务一个 exchange）。

- snapshot：

  ```bash
  cd /home/daivy/projects/serial/work/StarryOS && python3 scripts/ms05_data_plane_stimulus.py --port 15557 | tee "$EV/ms05-snapshot-host.log"
  ```

  ```sh
  /tmp/ms05_data_plane_probe snapshot
  ```

- tx-only：

  ```bash
  cd /home/daivy/projects/serial/work/StarryOS && python3 scripts/ms05_data_plane_stimulus.py --port 15557 | tee "$EV/ms05-tx-only-host.log"
  ```

  ```sh
  /tmp/ms05_data_plane_probe tx-only 96 64
  ```

- bidirectional：

  ```bash
  cd /home/daivy/projects/serial/work/StarryOS && python3 scripts/ms05_data_plane_stimulus.py --port 15557 | tee "$EV/ms05-bidirectional-host.log"
  ```

  ```sh
  /tmp/ms05_data_plane_probe bidirectional 96 64
  ```

- slot-full：

  ```bash
  cd /home/daivy/projects/serial/work/StarryOS && python3 scripts/ms05_data_plane_stimulus.py --port 15557 | tee "$EV/ms05-slot-full-host.log"
  ```

  ```sh
  /tmp/ms05_data_plane_probe slot-full
  ```

- descriptor-full：

  ```bash
  cd /home/daivy/projects/serial/work/StarryOS && python3 scripts/ms05_data_plane_stimulus.py --port 15557 | tee "$EV/ms05-descriptor-full-host.log"
  ```

  ```sh
  /tmp/ms05_data_plane_probe descriptor-full
  ```

- flush：

  ```bash
  cd /home/daivy/projects/serial/work/StarryOS && python3 scripts/ms05_data_plane_stimulus.py --port 15557 | tee "$EV/ms05-flush-host.log"
  ```

  ```sh
  /tmp/ms05_data_plane_probe flush
  ```

## 验证

每个模式成功判据（缺一不可）：

- 唯一 `MS05 PASS mode=<mode>` 终态 marker，且退出码一致（PASS=0 / FAIL≠0）。
- host stimulus 输出 `ms05 stimulus: PASS mode=<mode> count=… received=…`。
- `fault=0`、`lifecycle_fault=0`、`ownership_invariant=0`、`restore=0`。
- `slot-full`：`FULL tx_occ=64` → `RELEASED` → `POST` 闭合（`tx_occ=0`、
  enq=deq、`buf_avail=64 inflight=0`、`live=0`）。
- `descriptor-full`：`FULL buf_avail=0 inflight=64 desc_avail=0 desc_inflight=64`
  → `POST` 闭合。
- `flush`：`flush_ok` 恰好 +1、`flush_err/busy/cancel=0`、`POST` 闭合。
- 双向 mode：guest `WITNESS host_received=<count>` 与 host 共享计数一致
  （DONE/ACK 对齐）。

Cycle 004 实际结果（`qemu-serial.log`）：六模式全部终态 `MS05 PASS mode=…` + exit 0；
tx-only `sent=96 received=96`；bidirectional `tx_sent=96 rx_received=96 host_received=96`；
slot-full/descriptor-full Full→recovery 闭合；flush `flush_ok=1`。

## 失败处理

| 症状 | 分类与处理 |
|------|-----------|
| `MS05 FAIL mode=<m> reason=handshake`（exit 256） | 多为 host stimulus 未先启动，guest 无对端握手超时。重启 stimulus 再跑该模式。 |
| 任一模式缺终态 PASS、超时、中断、账本不闭合 | 产品失败；保存该模式原始串口与 host 输出，停止后续，不进入兼容/归档。 |
| `wget: Connection refused` | HTTP server 必须 `--bind 0.0.0.0`。 |
| `wget` 挂起 | 数据面问题；用 R55 分层诊断，或 debugfs 离线注入探针。 |
| `Address already in use` 15557 | 上一个 host stimulus 未退出；等待或 `pkill -f ms05_data_plane_stimulus.py`。 |

## 回滚

- Guest `/tmp` payload 随 QEMU 退出丢失，无需回滚。
- 退出 QEMU `Ctrl-A X`；停止 host 命令 `Ctrl-C`。
- 进程残留 `pkill -f qemu-system-riscv64`；host stimulus `pkill -f ms05_data_plane_stimulus.py`。
- 证据精简豁免：不再保存几百个 gate 日志 / 几万行原始日志 / hash 值；只保留必要的
  每模式串口 marker 摘录、host 结果和退出码（见 R44 证据精简原则）。

## 证据

- `openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/evidence/011-independent-manual-qemu-runtime-and-closeout/004-rework/`
- `qemu-serial.log`（六模式串口）、`stimulus-self-test.log`、`probe-harness.log`。
- 适用限制：结论限定于单 hart QEMU VirtIO-MMIO 软件/设备模型；不证明 SMP、DWMAC、
  真板 DMA/cache、性能或 fd readiness。
- Revision：worktree on `2af394e6`（net-k3）。
