# Blocker: stage12 TX 本地 terminal 不推进（设备不回写 TX descriptor OWN）

- 发现位置: Iteration 001 / Cycle 000-initial，task 2.2 板测 V3（第 2 轮，链路正常）；命中的契约条款：
  2.2「Stop when: 板上 terminal 不推进（doorbell 语义与板不符）……保留现场交 Plan，不试写替代语义」。
  Gate 5 阻塞类型：实际硬件行为与契约的 terminal 语义存在实质分歧。
- Plan 预期（2.2 GREEN）: 20 个提交、terminals=20、tx-errors=0、对端完整抓包 ≥1 reply、无本地 error。
- 实际情况（串口原文见 board-stage12-tx-timeout.txt）:
  - `sent=16 terminals=0 tx-errors=0 tx-timeouts=16 not-sent=4 rx-frames=15 rx-drops=0 arp-reply=1`。
  - 16 个提交（slot 0..15）逐一在各自独立 1s 窗超时，raw 一律 `0xb000003c` —— 与 CPU 发布的
    `OWN|FD|LD|60` 逐位相同：描述符被设备以某种方式消费，但 OWN 从未（可见地）被清零。
  - 对端证据：`arp-reply slot=0` 帧为真实 is-at 应答（dst=02:00:00:00:26:01、src=00:e0:4c:60:2f:60、
    oper=2、spa=192.168.137.1、tpa=192.168.137.2）——PC 只会对线上收到的 who-has 应答，故请求确实
    上线传输；另有 14 个 60B RX 帧（样本帽内未打印内容），≈15/16 请求到达对端。
  - 时序异常：对 submit i=0 的应答在 submit i=1 发布之后才被 RX reap（i=0 的整个 1s terminal 窗内
    service_rx 每 1ms 轮询未见 reply）——传输疑似延迟了一个提交间隔。
  - 有界止损全部按设计工作：超时不软件清 OWN、slot 不复用（16 槽耗尽后 not_sent=4 有界放弃）、
    无 FBE、无 ES、backing 保留、10s RX 窗有界结束。
- 影响:
  - A1（TX 发布/terminal/reclaim 契约）host 部分已 GREEN，但板上 terminal 分支**未达成**——本地
    completion 观察从未发生，单次回收语义无真板见证。
  - A2（对端交付）有强间接支持（应答证明帧上线），但需 Plan 裁决后才能定级。
  - A3（错误与 owner 安全）板上行为符合契约（bounded stop、owner 保留）。
  - A4/A5：gate（第 1 轮 link-down 零访问）、init（两次 SWR 后 RX 仍完全正常）、boot 正常；
    操作者 reset 恢复结论未回报。
- 部分工作: tasks 2.1/2.2 host 部分 done（V1 19 passed、V2 17 passed + make k3 exit 0）；板测 RX
  数据面（stage11+stage12 两个 arena、两次 SWR）完全正常；仅 TX completion/write-back 语义与板不符。
- 工作区状态: 本轮产品改动限于 `kernel/src/platform/k3_gmac/{mod,desc,regs}.rs`、
  `kernel/src/platform/k3_network_probe.rs`、`kernel/src/platform/k3.rs`、`tests/k3-gmac-host-harness.rs`；
  无计划外修改；已通过 Gate 见证见 Act Response V1/V2。
- 候选假设（供 Plan 裁决；Act 按契约未试写任何替代语义）:
  - H1 tail doorbell 语义: 契约采用「本次提交 slot 自身地址」（与已裁决 RX 同构），R69 参考用
    next-slot。若 K3 TX tail 为「排他 watermark」，每个提交要到下一次 doorbell 才被取走——解释
    reply 延迟一个间隔与 ≈15/16 帧（slot 15 永不被取），但不解释 OWN 不回写。
  - H2 IOC=0 抑制 write-back: 契约关闭 TDES2 IOC（轮询设计），R69 参考置位 IOC(31)。本板 DWMAC
    变体可能仅在 IOC 置位时执行描述符回写——解释「发送成功但 OWN 不清零」。
  - H3 TX 描述符 cache 可见性: 弱假设——RX reap 使用同一 inval→fence→volatile read 序列且 15 帧
    全部正确；TX 描述符行仅多一次 clean（发布序）。
- 恢复条件: 用户调用 openspec-plan 审查本 blocker 与 Evidence；Plan 对照 R69 参考裁决 H1/H2（或提出
  新假设），创建 replan Cycle 修订 2.1/2.2 契约与验证（建议把 IOC 置位与 tail=next 作为可分别裁决的
  板测变量），Act 按新契约继续当前 Iteration。操作者 reset 一句结论可在下一轮板测一并回报。
