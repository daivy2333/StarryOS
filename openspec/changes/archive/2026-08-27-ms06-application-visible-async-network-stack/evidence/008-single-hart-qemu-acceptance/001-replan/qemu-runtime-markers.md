# MS06/MS01/MS04/MS05 runtime markers（raw 行号）

来源：`/tmp/ms06-iteration-008-cycle-001-qemu-serial.log`（264 行，86,723 B）。

## boot / 命令

| raw 行 | 内容 |
|---|---|
| 133 | `starry:~# wget -q -O /tmp/ms06 … ms06_stack_readiness_probe` |
| 152 | `starry:~# wget -q -O /tmp/ms01 … ms01_socket_baseline` |
| 182 | `starry:~# wget -q -O /tmp/ms04 … ms04_rx_probe` |
| 202 | `starry:~# wget -q -O /tmp/ms05 … ms05_data_plane_probe` |

## MS06（12/12，exit 0）

| raw 行 | marker |
|---|---|
| 135 | `MS06_STACK_READINESS_START` |
| 136 | `MS06_REVISION: 1d0313ad8d0f36d918d1a101dd0ceda5c2ba336b` |
| 137 | `MS06_ENVIRONMENT: qemu-virt-riscv64-single-hart` |
| 138–149 | `PASS: tcp-timer / udp-progress / listener / nonblock-connect-error / quiet / continuous-traffic / close-error / poll-multiwaiter / select-multiwaiter / epoll-multiwaiter / waiter-64 / waiter-65-reregister`（固定顺序，各唯一） |
| 150 | `MS06_STACK_READINESS_END` |
| 151 | `MS06_HARNESS_EXIT: 0` |

## MS01（14/14，exit 0）

| raw 行 | marker |
|---|---|
| 154 | `MS01_SOCKET_BASELINE_START` |
| 161–179 | 14 个唯一 `PASS: tcp-accept / tcp-adjacent / tcp-512cap / tcp-512-recovery / tcp-relisten / udp-bidi / tcp-nonblock-accept / udp-nonblock / poll-readiness / udp-source / bind-getsockname / bind-ephemeral / bind-conflict / bind-close-cleanup` |
| 180 | `MS01_SOCKET_BASELINE_END` |
| 181 | `MS01_HARNESS_EXIT: 0` |

## MS04（4/4，exit 0）

| raw 行 | marker / 判据 |
|---|---|
| 188 | `MS04 PASS mode=snapshot`（lifecycle=2 owner=1 fault=0） |
| 194 | `MS04 PASS mode=idle`（DELTA 全 0） |
| 200 | `MS04 PASS mode=nudge`（DELTA nudge=1 task=1 empty=1） |
| 207–208 | `MS04 DELTA … reaped=96 refilled=96 delivered=96 … budget=3 yield=2 fault=0` → `MS04 PASS mode=burst` |
| 189/195/201/209 | `MS04_EXIT_snapshot/idle/nudge/burst: 0` |

## MS05（6/6，exit 0）

| raw 行 | marker / 判据 |
|---|---|
| 214 | `MS05 PASS mode=snapshot`（fault=0 lc_fault=0 owner_inv=0） |
| 225–227 | `MS05 WITNESS mode=tx-only sent=96 received=96` → `MS05 PASS mode=tx-only` |
| 232–234 | `MS05 WITNESS mode=bidirectional tx_sent=96 rx_received=96 host_received=96` → `MS05 PASS mode=bidirectional` |
| 239–244 | `MS05 FULL … tx_occ=64 tx_full=1` → `RELEASED` → `POST … tx_occ=0` → `WITNESS mode=slot-full sent=96 host_received=96` → `MS05 PASS mode=slot-full` |
| 249–254 | `MS05 FULL … buf_avail=0 inflight=64 desc_avail=0 desc_inflight=64` → `POST` → `WITNESS mode=descriptor-full sent=96 host_received=96` → `MS05 PASS mode=descriptor-full` |
| 258–261 | `POST … flush_ok=1 flush_err=0` → `WITNESS mode=flush sent=96 host_received=96` → `MS05 PASS mode=flush` |
| 215/221/228/235/245/255/262 | `MS05_EXIT_snapshot(×2)/tx-only/bidirectional/slot-full/descriptor-full/flush: 0` |

注：MS05 snapshot 在 raw 中运行两次（214 与 220 各一次 PASS），六种唯一 mode 全部 PASS；重复 snapshot 不影响验收。

## 无异常

完整 raw 串口 grep `FAIL|panic|trap|fatal|illegal|page fault` 为空，无内核致命输出。文件结束于
`make[1]: Leaving`，没有保存 `script -e`/QEMU 命令退出码；该进程级结果不由 guest exit 推断。
