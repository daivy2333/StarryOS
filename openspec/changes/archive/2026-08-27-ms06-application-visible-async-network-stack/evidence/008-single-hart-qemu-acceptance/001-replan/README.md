# Evidence: 008-single-hart-qemu-acceptance / 001-replan

- Change: ms06-application-visible-async-network-stack
- Iteration: 008-single-hart-qemu-acceptance
- Cycle: 001-replan
- Captured at: 2026-08-27
- Revision: HEAD `1d0313ad8d0f36d918d1a101dd0ceda5c2ba336b`（net-k3）；工作树含本轮 probe/validator/test 编辑与重建产物，未 commit
- Environment: 用户 sandbox 外手工 QEMU；QEMU 7.0.0，RISC-V `virt`，1 GiB，单 hart，单 VirtIO-MMIO NIC，user-net，`LOG=warn`

| ID | Origin | Acceptance | Claim | Artifact | Result |
|---|---|---|---|---|---|
| EV-008-001-01 | plan-required | A1 | MS06 修订 probe 12/12、END、`MS06_HARNESS_EXIT: 0`，validator 对完整 raw 串口 exit 0 | [qemu-runtime-markers.md](qemu-runtime-markers.md) | PASS |
| EV-008-001-02 | plan-required | A2-A3 | MS01 14/14、MS04 四 mode、MS05 六 mode 均 PASS 且 guest exit 0；snapshot 重复一次 | [qemu-runtime-markers.md](qemu-runtime-markers.md) | PASS |
| EV-008-001-03 | user-required | A4-A5 | guest 双边计数闭合；host/QEMU 进程级输出未完整采集，由用户以手工逐步全过声明接受该缺口 | [host-runtime-results.md](host-runtime-results.md) | PASS |

## Task 7.3 RED→GREEN 摘要

三项 witness 修复（只改 `tests/ms06_stack_readiness_probe.c`、`tests/ms06_stack_readiness_probe_test.c`、
`scripts/ms06-qemu-validate.py`，不触碰 axnet/smoltcp/kernel）：

1. listener 单字节 echo：`ms06_listener_reply_matches(ident, echo)` 按 unsigned-char 语义比较，消除 `~ident`
   整数提升造成的必然假阴性。RED=`implicit declaration`；GREEN=28/28 seam。
2. close-error peer-FIN：`ms06_peer_fin_eof_valid(events, recv1, recv2)` 只要求 IN|RDHUP 且无 ERR 且两次零读，
   删除无来源的 send→EPIPE 收敛要求。RED=`implicit declaration`；GREEN=28/28 seam。
3. validator ANSI/CSI + 有界外来 workload：`_normalize()` 只剥离 ESC 控制序列；只有成功的 MS06 exit 后
   出现精确 `MS01_SOCKET_BASELINE_START` 才允许外来 `PASS:`，尾随未知 `PASS:`、MS06 case、`FAIL:` 和
   `MS06_*` 仍拒绝。
   RED=`start marker is missing`（ANSI）与 `protocol marker after the end marker: 'PASS: tcp-accept'`（tail）；
   本次补充 RED=`invalid synthetic output was accepted`（未知尾随 PASS）；GREEN=validator self-test PASS 且
   对完整 raw 串口 exit 0。

## 冻结 Artifact 身份

| 文件 | size | mtime |
|---|---|---|
| StarryOS_riscv64-qemu-virt.bin | 40,763,584 | 2026-08-27 22:30:12 |
| tests/ms06_stack_readiness_probe | 147,128 | 2026-08-27 22:28:17（内嵌 1d0313ad） |
| tests/ms01_socket_baseline | 155,272 | 2026-08-27 20:08:30 |
| tests/ms04_rx_probe | 134,232 | 2026-08-27 20:08:30 |
| tests/ms05_data_plane_probe | 149,528 | 2026-08-27 20:08:31 |

session 后复核：上述 size/mtime 与启动前一致，无重建漂移（`make justrun` 未触发 build）。

## QEMU / guest / host exit 汇总

| workload | 结果 | guest exit |
|---|---|---|
| MS06 12-case | 12/12 PASS | `MS06_HARNESS_EXIT: 0` |
| MS01 14-case | 14/14 PASS | `MS01_HARNESS_EXIT: 0` |
| MS04 snapshot/idle/nudge/burst | 4/4 PASS | 各 `MS04_EXIT_*: 0` |
| MS05 六 mode | 6/6 PASS | 各 `MS05_EXIT_*: 0` |

完整 raw 串口无任何 `FAIL` / `panic` / `trap` / `fatal` / `illegal` / `page fault`。
validator 命令：`python3 scripts/ms06-qemu-validate.py --expect-revision 1d0313ad8d0f36d918d1a101dd0ceda5c2ba336b
--expect-environment qemu-virt-riscv64-single-hart /tmp/ms06-iteration-008-cycle-001-qemu-serial.log` → exit 0。

## 文件映射与限制

- 完整 raw 串口：`/tmp/ms06-iteration-008-cycle-001-qemu-serial.log`（264 行 / 86,723 B；因含用户手工 session
  现场，按 Plan 保存在 /tmp 供 Act/Plan 审查，不入库）。
- 决定性 marker/exit 摘录：`qemu-runtime-markers.md`（含 raw 行号）。
- host 双边结果：`host-runtime-results.md`。
- 适用限制：结论只覆盖 single-hart QEMU VirtIO-MMIO 软件/设备模型；不扩大到 reset、SMP、PCI/DWMAC、真板或性能。
- host 侧 tee/pipeline exit 仅留一份 MS05 flush 输出，QEMU 命令退出码也未留档；guest marker 不替代这些
  进程级证据。用户 2026-08-27 明确决定："证据就不重复采集了，我逐步手动验证的全过，只是没采集完整而已，
  没必要重复工作"，并要求改为接收、正式结束 change。结论因此包含用户接受的证据完整性残余风险。
