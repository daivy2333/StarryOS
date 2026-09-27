# MS07 单 hart QEMU 网络恢复测试与诊断

- Status: active（命令与环境实测可重复；006-rework 已随 DMA 零化修复完整手工 PASS，P9 手工资格完成）
- Last validated: 2026-08-31（含 P8 zero-fd poll 修复后让 probe 越过 pre-reset 等待）；2026-09-02 六 case 全过（Iteration 007 Cycle 006，DMA 零化修复）
- Environment: QEMU RISC-V `virt`；单 hart、单 VirtIO-MMIO NIC；1 GiB；user-net；QEMU 7.0.0；
  Rust nightly-2026-02-25；`/opt/musl/riscv64-linux-musl-cross`；python3
- Source: `ms07-qemu-single-hart-recovery-semantics` Iteration 007 / Cycle 004-replan（P8
  `user_poll_fds` 修复与 `zero_fd_poll_preflight` 探针前置）；2026-09-02 Act Response
  `iterations/007-single-hart-qemu-qualification/006-rework.md`（DMA 零化，`Dma::new` 全 region 清零）+ `evidence/007-single-hart-qemu-qualification/006-rework/`

## 适用与不适用

手工 QEMU 资格流程：在 single-hart QEMU VirtIO-MMIO 上运行 MS07 probe（reset / old-new socket /
HMP link flap），跑受影响回归，并用 validator 离线审计 raw serial；失败时用 LOG=info + 诊断 probe
分层调试。

**不适用**：真板（用 `board-bringup-ladder.md` R40）、SMP/PCI/DWMAC、性能（QEMU 证据不替代真板
DMA/cache/IRQ/SMP）；自动驱动 QEMU/HMP（按 R44 一律手工）。

## 归因方式

MS07 现场以 **raw serial + validator 判定**为准，不使用 hash、revision pin、run-id 或冻结镜像证明
运行归属。命令与环境的版本字段只描述适用范围，不作 Acceptance 证据。

## 前置条件

- 自动产品 Gate 已通过（编译错误不转手工）；`kernel/src/syscall/io_mpx/poll.rs` 的 P8 修复在产物内。
- `StarryOS_riscv64-qemu-virt.bin`、`make/disk.img` 已生成；QEMU 端口未被旧进程占用。
- `riscv64-linux-musl-gcc`；HTTP server 需 `--bind 0.0.0.0`。
- probe 本次行为：`run_probe` 首 case 前执行 `zero_fd_poll_preflight`，先后验证零 timeout、
  有限 timeout、零 `nfds` 忽略无效地址、正 `nfds` NULL→`EFAULT` 四个边界（`DBG:` 输出，validator
  忽略，不改 V4/case schema）。
- **peer 端口 15572 不要加 QEMU hostfwd**（否则与 peer 争用同一 host 端口，报
  `Could not set up host forwarding rule 'udp::15572-:15572'`）。数据面遵循 R56/MS05 模式：
  guest probe 作为 UDP client 出站连 host `10.0.2.2:15572`，host peer 直收。

## 操作步骤

本 Cycle（004-replan）的 Evidence 路径为
`openspec/changes/ms07-qemu-single-hart-recovery-semantics/evidence/007-single-hart-qemu-qualification/004-replan/`。

### 0. 基线（幂等）

```bash
cd /home/daivy/projects/serial/work/StarryOS
export PATH="/opt/musl/riscv64-linux-musl-cross/bin:$PATH"
make ARCH=riscv64 build && make tests/ms07_recovery_probe
file StarryOS_riscv64-qemu-virt.bin      # 应为 RISC-V ELF/binary；失败则重新 build
file make/disk.img
```

### 1. Terminal A — 建证据目录 + 启动 peer

```bash
cd /home/daivy/projects/serial/work/StarryOS
EV=openspec/changes/ms07-qemu-single-hart-recovery-semantics/evidence/007-single-hart-qemu-qualification/004-replan
mkdir -p "$EV"
python3 scripts/ms07-recovery-peer.py --host 0.0.0.0 --port 15572 --deadline-seconds 7200
```

peer 打印每个被接受的阶段（`peer: accepted phase=<p> seq=<n>`），用于确认 guest 是否到达 host。

### 2. Terminal B — 录制串口 + 启动 QEMU（无 15572 hostfwd）

```bash
cd /home/daivy/projects/serial/work/StarryOS
EV=openspec/changes/ms07-qemu-single-hart-recovery-semantics/evidence/007-single-hart-qemu-qualification/004-replan
script -q -e -f "$EV/qemu-serial.log" -c 'qemu-system-riscv64 -m 1G -smp 1 -machine virt -bios default -kernel StarryOS_riscv64-qemu-virt.bin -device virtio-blk-device,drive=disk0 -drive id=disk0,if=none,format=raw,file=make/disk.img -device virtio-net-device,netdev=net0 -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 -nographic'
```

### 3. Terminal C — HTTP（供 guest 下载 probe）

```bash
cd /home/daivy/projects/serial/work/StarryOS/tests && python3 -m http.server 18765 --bind 0.0.0.0
```

### 4. Guest 内（`starry:~#` 后）跑 MS07 probe

```sh
wget -q -O /tmp/ms07 http://10.0.2.2:18765/ms07_recovery_probe && chmod +x /tmp/ms07 && /tmp/ms07 --run; echo "MS07_HARNESS_EXIT: $?"
```

应先看到 `DBG: preflight …` 四行且均符合边界（零 timeout/有限 timeout/零 `nfds` 忽略无效地址
返回 0，正 `nfds` NULL 返回 -1 且 errno==EFAULT），再进入 `pre_reset_traffic`。preflight 任一项失败
即 `FAIL: zero_fd_poll_preflight`。

### 5. HMP link flap（probe 打印 READY 时，全部手工）

- `...MS07_HMP_READY: link=off` → `Ctrl-A c` 进 monitor → `set_link net0 off` → `Ctrl-A c` 回 guest
- `...MS07_HMP_READY: link=on` → `Ctrl-A c` 进 monitor → `set_link net0 on` → `Ctrl-A c` 回 guest

`MS07_HMP_OBSERVED` 才是设备状态证据；READY 只表示操作边界。

### 6. Terminal B — 退出 QEMU（`Ctrl-A x`）后离线审计

```bash
cd /home/daivy/projects/serial/work/StarryOS
EV=openspec/changes/ms07-qemu-single-hart-recovery-semantics/evidence/007-single-hart-qemu-qualification/004-replan
python3 scripts/ms07-qemu-validate.py --expect-environment "qemu-virt-riscv64-single-hart-virtio-mmio-user-net" "$EV/qemu-serial.log"
```

### 7. 同会话受影响回归

MS01 14/14、MS04 四 mode、MS05 六 mode、MS06 12-case，各自最终 marker + exit 必须明确 PASS
（沿用各自 evidence Runbook）；缺终态或非成功 exit 不计 PASS。回归终态写入 `$EV/regressions.txt`。

## 验证

- MS07 六 case 唯一顺序完成，validator exit 0（判据含 V4 epoch/ledger/terminal/fatal、HMP off/on、
  raw serial 无 panic/trap/owner drift/permanent Pending）。
- 四组回归终态明确 PASS。

当前已知状态（2026-08-31）：P8 已修复 `poll(NULL,0,remaining)` 的 `EFAULT` 阻塞——`sys_ppoll`
在 `nfds==0` 时忽略 `fds` 并返回安全空 slice，probe 由 `zero_fd_poll_preflight` 就近见证四个边界。
P8 自动 Gate（kernel/probe build、MS07 host harness、`make host-test`、OpenSpec strict）全绿。

**P9 定位结果（第一轮 resetdiag run，2026-08-31）**：`old_socket_terminal` 失败的 reset 卡点是
**driver reset 阶段超时**。probe `DBG: wait_for_reset` 观测到：lifecycle 从 Active(2) 进入
Resetting(6)，`device_owned=0 quarantined=64`（reset 把 buffer 全隔离进 quarantine 保持 backing），
但 **q/s 停在 0，即 `Transport::reset_confirmed()`（status==0 读回）在 2s deadline 内从未成立**，
recovery_step 一直停在 `Resetting`，最终 `fstage=5 (RECOVER_STAGE_RESET) fcause=1 (TIMEOUT)` 进入
Faulted(3)。epoch 未从 q=0 推进，符合「reset 未完成不推 epoch」。根因待 driver/transport 层进一步
确认（写 status=0 后 QEMU MMIO status 未回零，或 owner 未在 reset deadline 内重新 poll）。

**info 层埋点（2026-08-31 新增）**：`crates/axdriver_virtio/src/net.rs::poll_recovery_step`
Resetting 分支在读到非空 status 未确认 reset 时，打出
`VirtIO reset pending: status=<raw> read_back_nonzero=true`（按残留 bit 值去重，一次/值）。用
`make ARCH=riscv64 LOG=info build` 重建。**目的**：揭示 QEMU 写 status=0 后读回残留的 status 位
（如 `DRIVER_OK=4` / `FAILED=128` / 组合），区分「driver 写 status 值错」vs「MMIO 读回未清零」。
复位点 2s 超时前 owner 反复 poll，此埋点据残留值去重避免 debug 风暴。

**async_rx recovery 状态埋点（新增）**：`crates/axnet/src/async_rx.rs::poll_recovery` 入口打出
`RX recovery state: <Quiescing/Resetting/Reinitializing/None>`（变化才打）。**定位 owner 在 reset
请求后是否真正被持续 poll 驱动 recovery 状态机**，以及它停在哪个 stage。与 driver 层的
`VirtIO reset pending`（写 status 后未清零）区分「owner 未推进状态机」vs「driver reset 未确认」。

**driver recovery step 埋点（2026-08-31 终版）**：`crates/axdriver_virtio/src/net.rs::poll_recovery_step`
入口打出 `VirtIO recovery step: state=<RecoveryState> reset_confirmed=<bool>`（按 (stage,confirmed)
对去重）。**目的**：区分两个互斥根因——(a) owner 在 Resetting 后是否被再次 poll_recovery_step
驱动；若无，则 `state=Resetting reset_confirmed=?` 只出现一次即停产（唤醒/调度丢失）；若有则连续
出现并揭示 `reset_confirmed` 实际值（写 status=0 后 QEMU 读回是否清零）。取代早期基于非 0 status
读回的 `VirtIO reset pending` 埋点（后者无法覆盖 reset_confirmed()==true 但 Reinit 失败的场景）。

**根因端到端确认（2026-08-31 step3 run）**：`RX recovery state` 仅 Quiescing@10.02 →
Resetting@12.02（2s 间隔，即 RESET_STAGE_DEADLINE_NS=2s）；`VirtIO recovery step` 埋点零输出；
probe iter=69 ABORT Faulted(fstage=5 RESET / fcause=1 TIMEOUT)。结论：owner 进 Resetting 后
`arm_recovery_timer` 把唯一唤醒源 arm 到 `now+2s` 到期；期间无任何周期性/事件 wake，owner 睡到
2s 到期那次 poll 时 `recovery_deadline.is_some_and(expired)==true` → 直接走 Resetting/Reinit 分支的
TIMEOUT Faulted，**从未调用 driver `poll_recovery_step`**（故 driver 埋点零输出）。根因是 owner
在 Resetting 阶段缺少自唤醒/重试，一次 `sleep_until(deadline)` 让 QEMU 写 status=0 后无机会被
重新 poll 到 `reset_confirmed()` 而已 2s 超时 Fautted。修法方向：Resetting 阶段每次 Pending 自
`wake_by_ref`（同 Quiescing 的 `reclaimed_at_budget` 模式）或在 reset 未确认时安排周期 wake，不等到
2s deadline 才被 poll。

**调试风暴避免原则**：`wait_for_reset`/`zero_fd_poll_preflight` 的 `DBG:` 只在 **transition-relevant
字段变化时打印**（lifecycle/epoch/owner/fault 任一变化，或首轮/ABORT），不在稳定期逐 poll 刷屏。
再采集时日志应只含初始、reset 阶段进入、fault 三处节点，而非 69 行重复。

## 失败处理

| 现象 | 分类与处理 |
|---|---|
| `-netdev ... hostfwd=udp::15572` 报 `Could not set up host forwarding rule 'udp::15572-:15572'` | peer 已占 15572；去掉该 hostfwd（peer 端口不 hostfwd），按 R56 直连 |
| probe 在 case 前 `FAIL: zero_fd_poll_preflight` | 四个边界中某一项不合预期；对照 `DBG: preflight …` 逐项核对，产品修在 syscall 层 |
| `FAIL: old_socket_terminal reason=reset-terminal` | reset 后 epoch 未推进。对照 `DBG: wait_for_reset iter=… lifecycle=…` 判据：lifecycle 停留在 Quiescing=5/Resetting=6/Reinitializing=7 → reset 卡在对应 driver stage；lifecycle=3(Faulted) 且 `fstage/`fcause=` 非 0 → 读 fault_stage/cause 归因（如 Reset/Reinitialize 失败、OwnerSummary 不守恒）；q/s 未 +1 但 lifecycle 回到 2 → epoch 推进缺失。据此决定是否需 LOG=info 内核 recovery 日志或改 driver |
| `open_peer_socket ... errno=` | 下个 Cycle 据 errno 定位 guest UDP 到 `10.0.2.2:15572` 不可达根因 |
| `DBG: preflight poll(NULL,1,0)!=EFAULT` | 正 `nfds` NULL 地址校验退化；确认 `user_poll_fds` 的 `nfds>0` 分支未绕过 `get_as_mut_slice` |
| `wget` 立即 `Connection refused` | HTTP 未 `--bind 0.0.0.0`；`ss -tlnp | grep 18765` 确认 |
| `wget` 挂起 | 数据面问题；用 LOG=info + 分层诊断（见下）或 R55 debugfs 离线注入 |
| 首次跑 peer 收不到 | 先确认 peer 进程/端口（`pgrep -af ms07-recovery-peer`、`ss -ulnp|grep 15572`），
  deadline 到期则重启（用更长 deadline） |
| `make run` 报 hostfwd::5555 Could not set up | 旧 QEMU 占 5555；`pgrep -af qemu-system` + kill |
| `ifconfig/ping` 报 `/proc/net/dev`/ioctl/socket 错误 | 内核未实现该 surface，不是网络断证据；用 probe DBG + pcap |
| validator 对 hmp_link_down 报 `wrong marker count`、exit 1 | 采集伪影：`-nographic` 下 QEMU monitor 提示符 `(qemu) ` 与 guest `MS07_HMP_OBSERVED` 共用控制台，marker 行以 `(qemu) ` 开头、未以 `MS07_` 开头被解析器丢弃；设备状态与两侧数据面 marker 均真实存在，非产品/探针缺陷。重跑时在 monitor 输入命令后按 `Enter` 或稍等再 `Ctrl-A c`，让 marker 落在新行；或按用户豁免计入通过（006-rework 采用豁免） |

分层诊断（R55）：`make LOG=info build` 后可观测 eth0/IP、`TCP/UDP socket`、恢复相关 info；需要
客观帧证据时加 `-object filter-dump`。诊断用 info 镜像，判据用 probe DBG 与 pcap。诊断重建后不改写
`make/disk.img`；盘/注入用副本。

## 回滚

- 保留 `make/disk.img` 只读；诊断盘/注入只用副本。
- 退出 QEMU `Ctrl-A X`；停 peer/HTTP `Ctrl-C`；残留进程 `pgrep -af` 核对单个 PID 后 `kill`。
- 诊断以 LOG=info 重建的产物如需恢复，`make ARCH=riscv64 build` 重建即可，不需 hash 备份。
- 无 runtime 合格证据时不创建 Evidence 占位目录；本 Cycle 保存与否按 evidence-format 的
  一次性现场/不可复现判据决定。

## 证据

- `openspec/changes/ms07-qemu-single-hart-recovery-semantics/iterations/007-single-hart-qemu-qualification/004-replan.md`
  （Act Response `reported`，P8 自动部分完成）。
- `openspec/.../evidence/007-single-hart-qemu-qualification/004-replan/`：P9 手工执行后再按
  R58 `qemu-evidence-capture.md` 采集 raw serial、validator 结果与 `regressions.txt`；未执行前不创建
  占位目录。
- 2026-09-02（006-rework，最新）：`iterations/007-single-hart-qemu-qualification/006-rework.md`
  （Act Response `reported`，DMA 零化修复 + 六 case + 四组回归）与
  `evidence/007-single-hart-qemu-qualification/006-rework/`（`qemu-serial.log`、`regressions.txt`、
  `ms01/ms04/ms05/ms06-*-serial.log`、`README.md`）。
- Revision：分支 `net-k3`；本文件归档时不使用 hash/revision pin 作运行归属，版本仅描述适用范围。
- 适用限制：结论限定于单 hart QEMU VirtIO-MMIO 软件/设备模型；本文件记录已实测可重复的测试与
  诊断流程，MS07 合格 PASS 以六 case + validator + 四组回归为准（006-rework 已达成，hmp_link_down
  采集伪影按用户豁免）。