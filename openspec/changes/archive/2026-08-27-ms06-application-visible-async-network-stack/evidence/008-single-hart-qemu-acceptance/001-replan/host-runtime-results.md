# MS04 / MS05 host 双边结果

## MS04 host stimulus（R51）

- 命令：`python3 scripts/ms04_rx_stimulus.py --host 0.0.0.0 --port 15556`（burst 前在 Terminal C 启动）。
- guest 侧守恒（raw 行 207）：`MS04 DELTA … reaped=96 refilled=96 delivered=96 … budget=3 yield=2 fault=0`，
  满足 R51「reaped_delta == refilled_delta == delivered_delta == 96」且 budget_exhausted>0、self_yield>0、fault=0。
- host 侧 tee 与 pipeline exit 未单独留档；guest DELTA 只证明 guest 侧守恒，不替代 host 进程证据。

## MS05 host stimulus（R56）

- 命令：`python3 scripts/ms05_data_plane_stimulus.py --port 15557`（每 mode 一次，Terminal C `set -o pipefail` 后启动）。
- guest 侧共享计数（raw `MS05 WITNESS` 行）：

| mode | WITNESS | guest 判定 |
|---|---|---|
| tx-only | sent=96 received=96 | PASS |
| bidirectional | tx_sent=96 rx_received=96 host_received=96 | PASS |
| slot-full | sent=96 host_received=96 | PASS（FULL tx_occ=64 → RELEASED → POST tx_occ=0） |
| descriptor-full | sent=96 host_received=96 | PASS（FULL buf_avail=0 inflight=64 → POST 闭合） |
| flush | sent=96 host_received=96 | PASS（flush_ok=1 flush_err=0） |

- 已留档的 host stimulus 输出：`/tmp/ms05-snapshot-host.log`（63 B）内容为
  `ms05 stimulus: PASS mode=flush count=96 payload=64 received=96`（文件名为 snapshot 但内容是 flush mode，
  由用户采集命名所致；该记录与 guest `WITNESS mode=flush host_received=96` 对齐）。
- host 侧其余 mode 的 stimulus 输出和 pipeline exit 未单独留档；guest `MS05 WITNESS host_received=96`
  证明 guest 观察到交换进度，但不替代各 host 进程结果。

## pipeline exit

- MS05 每个 guest mode 显式 `MS05_EXIT_<mode>: 0`（raw 行 215/221/228/235/245/255/262）。
- MS04 每个 guest mode 显式 `MS04_EXIT_<mode>: 0`（raw 行 189/195/201/209）。
- 无 FAIL、无 handshake 重试、无 panic/trap/fatal。

## 用户验收决定

用户 2026-08-27 明确声明："证据就不重复采集了，我逐步手动验证的全过，只是没采集完整而已，没必要重复工作"，
并要求改为接收、正式结束 change。因此不补跑 QEMU；缺失的 MS04/MS05 host transcript/pipeline exit 与 QEMU
命令退出码作为用户接受的证据完整性风险保留，不将 guest marker 误写成未采集的 host 证据。
