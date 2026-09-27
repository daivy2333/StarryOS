# MS08 QEMU SMP=16 资格运行与诊断

- Status: active
- Last validated: 2026-09-26
- Environment: QEMU RISC-V `virt`（**必须显式 `-machine virt`**）；ARCH=riscv64；SMP=16；VirtIO-MMIO；user 模式 net；Rust nightly-2026-02-25；`riscv64-linux-musl-gcc`；`debugfs`；`socat`；`script`；`tcpdump`（读 pcap）
- Source: MS08 Iteration 005 Cycle 002 Act Response（blocked，B1/B2）；Cycle 001 replan 手动运行；2026-09-26 多轮 SMP=16 运行（runs D/E/F）

## 适用范围

在 `smoke/net` 开发阶段以手动方式在 QEMU `SMP=16` 上运行 MS08 net probe（或 UART probe），覆盖两类用途：

1. **正式资格流程**：peer + `script` 捕获 + HMP 操作手 + validator 判定的完整编排（各步骤已执行验证；最终 validator 收编受已知间歇竞态 B1/B2 阻塞，见失败处理）。
2. **分层诊断**：竞态/冻结定位（warn 级分层打印、`[NET-ROUND]` 阶段追踪、冻结时 HMP 寄存器取证）。

**不适用**：真板（用 `board-bringup-ladder.md`）；竞态根因修复本身（属 change 工作）；`LOG=info` 时序下的行为结论（日志洪水扰动时序，不作判据）。

## 前置条件

- 已注入 probe 的 disk 副本（见下）；若尚无，先 `cp make/disk.img /tmp/ms08-net-disk.img`（UART 用独立副本 `/tmp/ms08-uart-disk.img`，两驱动不得共用 session）。
- 宿主机可用：`qemu-system-riscv64`、`riscv64-linux-musl-gcc`、`debugfs`、`socat`、`script`、`python3`。
- **QEMU 机器类型必须显式 `-machine virt`**：qemu-system-riscv64 7.0 默认机器是 `spike`（最多 8 CPU），缺省直接跑 SMP=16 会以 `Invalid SMP CPUs 16. The max CPUs supported by machine 'spike' is 8` 退出。
- 无残留 QEMU 占用端口；先 `pgrep -af qemu-system` + kill。

## 操作步骤

### 1. 编译 probe 与内核

```bash
cd /home/daivy/projects/serial/work/StarryOS
make tests/ms08_network_smp_probe          # 或 ms08_uart_smp_probe
make ARCH=riscv64 SMP=16 build             # 默认 LOG=warn；诊断打印已挂在 warn 级
```

### 2. 准备 disk 副本并注入 probe

```bash
cp make/disk.img /tmp/ms08-net-disk.img
debugfs -w -R 'write tests/ms08_network_smp_probe /root/ms08_network_smp_probe' /tmp/ms08-net-disk.img
```

### 3. 起 peer（先于 QEMU）

```bash
python3 scripts/ms08-network-peer.py --host 0.0.0.0 --port 15578 --deadline-seconds 420 2>&1 | tee /tmp/ms08-net-peer.log
```

### 4. 运行 QEMU（transcript 捕获 + pcap 客观证据 + HMP socket）

```bash
rm -f /tmp/ms08-net-hmp.sock /tmp/ms08-diag.pcap
script -q -e -f /tmp/ms08-net-transcript.log -c \
  'qemu-system-riscv64 -m 1G -smp 16 -machine virt -bios default \
    -kernel StarryOS_riscv64-qemu-virt.bin \
    -device virtio-blk-device,drive=disk0 \
    -drive id=disk0,if=none,format=raw,file=/tmp/ms08-net-disk.img -snapshot \
    -device virtio-net-device,netdev=net0 -netdev user,id=net0 \
    -object filter-dump,id=ms08diag,netdev=net0,file=/tmp/ms08-diag.pcap \
    -serial stdio -monitor unix:/tmp/ms08-net-hmp.sock,server=on,wait=off \
    -nographic'
```

**开机即看 boot smoke**（前 2 秒）：`[NET-MIG-SMOKE] PASS/FAIL` 与每角色 `[NET-MIG-STIM] iters=.. wakes=.. rollback-exit=..`。`FAIL` 预示 boot 命中迁移竞态（B1），后续 owner-migration 会报 `migration-not-idle`，不必跑完。

### 5. 进 guest 运行 probe

```sh
stty -echo; chmod +x /root/ms08_network_smp_probe && /root/ms08_network_smp_probe --run; echo "MS08_NET_HARNESS_EXIT: $?"; stty echo
```

### 6. HMP 操作手（严格按 READY marker 现发，勿预发）

| guest 打印 | 操作 |
|---|---|
| `MS08_NET_HMP_READY: link=off` | `printf 'set_link net0 off\n' \| socat - UNIX-CONNECT:/tmp/ms08-net-hmp.sock` |
| `MS08_NET_HMP_OBSERVED: link=off` 且 `MS08_NET_HMP_READY: link=on` | `printf 'set_link net0 on\n' \| socat - UNIX-CONNECT:/tmp/ms08-net-hmp.sock` |
| 上条后发 1-2 秒 | **回 guest 控制台**输入 `MS08_NET_HMP_DONE link=on`（输到串口，不是 HMP socket） |

注意：打印 `HMP_READY: link=on` 后探针阻塞等待控制台完成行是**协议步骤**，不是故障。预发 HMP 命令会导致 guest 漏观测（如 on 在 down 处理期到达被合并），必须现发现打。

### 7. 收尾判定

probe 退出、QEMU `Ctrl-A X` 后：

```bash
grep 'MS08_NET_PEER_RESULT' /tmp/ms08-net-peer.log >> /tmp/ms08-net-transcript.log
python3 scripts/ms08-network-validate.py /tmp/ms08-net-transcript.log; echo "validator exit=$?"
```

### 8. 冻结取证（发生整机冻结时，杀 QEMU 之前执行）

```bash
# 单 hart（当前 CPU）：
(echo 'info registers'; sleep 3) | socat - UNIX-CONNECT:/tmp/ms08-net-hmp.sock | tee /tmp/ms08-frozen-regs.txt
# 全部 16 hart：
for i in $(seq 0 15); do echo "cpu $i"; echo "info registers"; sleep 0.6; done | socat - UNIX-CONNECT:/tmp/ms08-net-hmp.sock | tee /tmp/ms08-frozen-allregs.txt
```

## 验证

- guest shell 出现 `starry:~#`；boot smoke `[NET-SMP-SMOKE] PASS`、`[NET-MIG-SMOKE] PASS`（竞态 boot 除外）。
- 自动 case：`PASS: placement → timer-disabled-wake → tcp/udp-bidirectional → full-recovery → readiness-quiet → owner/runner-migration → reset-io`，`reset-io` 可见若干 `DBG: reset-io open-attempt=..` 后收敛（见失败处理：恢复窗口行为）。
- 2026-09-26 运行 F：10/10 case 全部推进至 link-up 之后（冻结发生在探针观测段，竞态 B2）。
- validator：`validator exit=0`（受 B1/B2 阻塞期间未达成；流程各步骤本身已执行验证）。
- pcap：`tcpdump -r /tmp/ms08-diag.pcap -nn -e` 可查链路事件对应的客观流量。
- 串口双写者交织（探针 marker 与内核日志逐字节交错）是诊断模式正常现象，以 transcript 文件与 validator 解析为准。

## 失败处理

| 现象 | 分类与处理 |
|---|---|
| `Invalid SMP CPUs 16 ... machine 'spike'` | 缺 `-machine virt`（见前置条件） |
| `make justrun` hostfwd 占用 | 旧 QEMU 未退；`pgrep -af qemu-system` + kill 后重试 |
| boot 时 `[NET-MIG-SMOKE] FAIL` / 探针 `owner-migration reason=migration-not-idle` | **已知间歇竞态 B1**（迁移 wake→schedule 丢失）：看 `[NET-MIG-STIM] wakes=`（0=自然迁移路径未走、>0=唤醒发了但没被调度）；属产品缺陷，不靠重跑规避 |
| `[NET-MIG-owner] TIMEOUT-widened ... parked=true` | 同上 B1；`parked=true` 表示目标真实 park 但未被调度 |
| link-up 后整机冻结（控制台无响应） | **已知间歇竞态 B2**（lost-wakeup 型）：先按步骤 8 取全 hart 寄存器再杀 QEMU；签名=全部 hart 在 `current_run_queue` 附近 wfi↔tick 空转；`[NET-ROUND]` 阶段行给出冻结前最后阶段 |
| `reset-io` 多个 `DBG: open-attempt=.. connect_errno=104` | **预期行为**：恢复窗口内新 socket 生于已终结 epoch（kernel 设计：入口即发布 socket 终端）；循环在恢复完成后自动收敛，不是故障 |
| 探针在 `HMP_READY: link=on` 后"卡住" | 协议等待 guest 控制台完成行（步骤 6），输 `MS08_NET_HMP_DONE link=on` |
| 手工预发 HMP 命令后 guest 漏观测 | 严格按 READY 现发；漏发 on 时补一次 off→on 可能引入额外 l 计数，必要时重跑 |
| validator 报 `malformed marker`（如 `rejects=0own_mig`） | 多为复制粘贴丢空格；以 `/tmp/ms08-net-transcript.log` 文件原文为准 |
| `timer-disabled-wake affinity-read FAIL` / `readiness-quiet progress-drift FAIL` | 旧 probe 缺陷；重建最新 probe（见 Cycle 001 记录） |
| `readiness-quiet quiet-progress` / 探针无后续 | peer 未起或提前退出；确认 peer 先于 QEMU 且未 Ctrl-C |

## 回滚

- 本流程不改产品源码（仅构建与盘副本）。QEMU 退出：`Ctrl-A X`；停 peer：`Ctrl-C`。
- 诊断盘副本 `/tmp/ms08-*.img` 用完可删；`make/disk.img` 不受影响。
- 诊断镜像若曾用 `LOG=info` 构建，收尾用 `make ARCH=riscv64 SMP=16 build`（默认 warn）恢复正式时序。
- 清理临时文件：`rm -f /tmp/ms08-*.img /tmp/ms08-*.log /tmp/ms08-*.txt /tmp/ms08-diag.pcap`（保留 change 内 Evidence 要求的部分）。

## 证据

- Act Response：`openspec/changes/ms08-qemu-multi-hart-correctness-baseline/iterations/005-per-driver-smp16-fixed-qualification/002-rework.md`（B1/B2 证据链、分层打印、寄存器分析）。
- 2026-09-26 runs D/E/F 原始输出：会话 transcript 与 `/tmp/ms08-frozen-allregs.txt`（all-harts 签名）；pcap `/tmp/ms08-diag.pcap`。
- 分层诊断打印均为 `qemu-diagnostics` 门控 + warn 级（默认镜像可见；`LOG=info` 会因普通日志洪水扰动时序，不作竞态判据）。
- 限制：正式 validator 收编尚未达成（B1/B2 未修）；本 Runbook 描述已验证的操作与诊断方法，不包含竞态修复方案。
