# Iteration 002 / Cycle 000: ARP/ICMP/UDP/TCP 轮询交换

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: 3.1, 3.2（含用户 2026-10-04 批准并入的 stage9+ 输出换行修复，归入 3.2 契约）。
- Acceptance gaps: None。
- Repair items: None。
- Inherited scope: 已批准 R1–R6、S1–S11 全量保留；本轮承担 R4（四协议双向交换）为主，及 R1/R3/R5/R6 的协议侧。
- Excluded scope: 异步 queue task/wake、运行时 reset/link-flap 恢复、SMP、offload、零拷贝、吞吐/soak 性能、交互 shell、存储/rootfs、MAC DMA IRQ、证据工具。

**Objective**

在已验收 raw 双向基线上接入真实协议栈：生产 smoltcp Device 与有界 session 的 host 测试全绿；stage13 真板 60 秒会话完成 ARP、ICMP、UDP、TCP 四协议双向交换，PC 原生工具与串口分列证据互相印证；reset 恢复原系统。host GREEN 与操作者板测分别成立才交 Review。

**Investigation Facts**

Current Baseline（引用 Iteration 001 最终 Act Response 与 accepted Review，2026-10-04）：

- raw 双向基线成立：stage12 板上 `sent=20 terminals=20 tx-errors=0 tx-timeouts=0 not-sent=0 rx-frames=20 rx-drops=0`、20 条提交跨 slot 0..15 回绕复用 0..3、真实 ARP is-at 应答且不再延迟。V1 21 passed/0 failed、device-irq 17 passed、make k3 exit0 均新鲜有效。
- 已裁决语义事实（本轮回填 002 契约，Act 不回读）：**TX tail(+0x1120) 为排他 fetch 上界**——提交 slot i 后写 `addr((i+1) mod 16)` 低 32 位（i=15→addr(0)）；**RX tail(+0x1128) 写为唤醒 doorbell、fetch 由 OWN 驱动**——own-slot refill 含整环回绕已板测（20 帧零 drop），RX 侧无待修复项。Zicbom cache/fence 序列、MAC/MTL/DMA 配置、APMU RMW、bounded stop/fault/cancel 语义均沿 000/001 交付。
- 豁免与用户指令（原话留存于 001-rework Act Response/Review）：Wireshark 永久豁免（"Wireshark不做，后续也不用做"）——本轮板测验证不依赖 Wireshark，对端证据改用 PC 原生工具输出；stage9+ 输出换行修复并入本轮（"同意下一批优化输出"）。

Current-State Evidence 与 Code and Critical Path（新表面）：

| 入口/符号 | 实际边界 |
|---|---|
| `crates/smoltcp`（0.13.1） | workspace exclude 中的本地 crate；所需 feature 全部存在（Cargo.toml 已核对）：medium-ethernet、proto-ipv4、socket-icmp、socket-udp、socket-tcp、socket-tcp-reno、auto-icmp-echo-reply；不启用 test-seeds、async、proto-dhcpv4、offload；不修改其协议实现 |
| root `Cargo.toml` | `members=["kernel"]`；exclude 已含 `crates/smoltcp`。kernel 引用方式仿 `axnet`（第 43 行 path + workspace.dependencies 先例：path 指向 exclude crate 合法）；新 `tests/k3-polling-net-host/` 包需追加进 exclude（Map 预告的必要调整） |
| `kernel/Cargo.toml` | 可选依赖 + feature 门控先例：`dep:axnet`/`axnet/qemu-diagnostics`（第 27/64 行）；smoltcp 依赖以 k3 feature 门控，default 构建不引入 |
| smoltcp 0.13 Device 契约 | `phy/mod.rs::Device`：`receive` 需同时交付 RX+TX token；token `consume` 无错误返回——D5 已裁决：容量先保留、驱动错误存入设备并由前台 poll 消费 |
| `k3_gmac::Gmac` | `try_submit/poll_tx/poll_rx/enable_tx` 已交付且语义已板测；`GmacEnv` seam 复用 |
| `k3_network_probe.rs` | stage11/12 序列与 `stage11_line!/stage12_line!` 宏（经 `aia::LineBuf`，无尾随 `\n`——本轮修）；stage13 新增第三 arena（前两个 arena 由已发布 guard 保留） |
| `k3_device_irq_probe.rs` | `stage9_line!/stage10_line!` 宏同样无换行——本轮修，触及后 V2 需重跑 device-irq harness |
| `platform/k3.rs::run_first_byte_park` | stage10→11→12 已链接；stage13 在此链接 stage12 返回值 |
| `tests/` | 既有 rustc 直编 harness 模式（21 tests）；本轮新增 Cargo integration package（真实 smoltcp 依赖超出单文件直编能力，Map/D5 已裁决） |

**Implementation Guidance**

按 3.1→3.2 推进：先建 `tests/k3-polling-net-host/` 骨架与 RED，再实现 `polling_net.rs` 至 GREEN；stage13 集成与换行修复随后。Device 与 session 的 borrow 结构、静态 socket 布局等非实质选择由 Act 决定。

**Task Contracts**

### 3.1: K3 smoltcp Device 与有界 session

- Requirement/Scenario: R1/R3/R4/R5/R6；S4/S5/S6/S7/S8/S9/S10/S11。
- Depends on: Iteration 001 accepted（已满足）。
- Targets: 新 `kernel/src/platform/k3_gmac/polling_net.rs`；root `Cargo.toml`（workspace.dependencies smoltcp path 依赖 + exclude 追加）、`kernel/Cargo.toml`（可选 smoltcp 依赖、k3 feature 门控）、`Cargo.lock`；新 `tests/k3-polling-net-host/`（Cargo 包与 integration tests）。
- Current behavior: raw 双向后端可用；无网络栈 owner、无协议 session、无 PC 侧对端工具约定。
- Required behavior:
  - smoltcp 0.13.1 以 path 依赖引入，`default-features = false`，只启用：medium-ethernet、proto-ipv4、socket-icmp、socket-udp、socket-tcp、socket-tcp-reno、auto-icmp-echo-reply；不修改 smoltcp 协议实现；default/其他平台构建不引入该依赖。
  - Device：MTU 1514（IP MTU 1500）、checksum 全软件；每次 poll 先 reclaim TX；无 TX free slot 不交付 RX token 对（保留 RX 至下一 poll）；TX token 预留一个 CPU-owned slot，consume 写完整帧后按已验收发布序提交，未 consume 的 token 释放预留槽；RX token 先拷贝到独立 CPU scratch 并 refill，不同时借用设备仍拥有的 buffer；consume 无法返回的驱动错误记录为设备 fault，前台 poll 后立即读取并结束 session，不把失败当协议成功。
  - 有界 session：静态 socket 存储（无动态分配）；UDP4 socket 4096B 双向 buffer；TCP socket 4096B 双向 buffer、单连接；UDP/TCP echo 累计上限 1024 字节，partial send/recv 以 offset 推进；协议操作等待/取消/close 各 5 秒有界；Full 期间不丢合法 owner。
  - 板侧网络标识：IP 192.168.137.2/24、软件构建源 MAC 02:00:00:00:26:01、无网关/DNS。
- Preserve: 既有 21 个 witness GREEN、default/D1/lichee 构建不回归、Iteration 000/001 已验收语义与输出（除 3.2 的换行修复）。
- Forbidden: 修改 smoltcp 实现、启用 test-seeds 作产品 feature、第二 executor、无限/动态 socket 队列、scope 外公开 API、身份型证据。
- Test witness: `cargo test --manifest-path tests/k3-polling-net-host/Cargo.toml` 先 RED：生产 Device token 预留/背压/错误语义与 session 有界性测试；真实 smoltcp + 内存 peer 覆盖 ARP、ICMP、UDP、TCP 四协议、满队列、partial、timeout/cancel/close；不使用替代 session 算法。
- GREEN condition: 原生 `0 failed` + exit0；buffer 内容与协议关闭正确；Full 无 owner 丢失、fault 不伪成功。
- Verification: V1/V2（下述）；真板裁决在 3.2。
- Stop when: smoltcp 0.13.1 本地接口无法表达已批准行为（与 D5 事实矛盾），或需扩展协议/异步/恢复范围。

### 3.2: stage13 四协议真板验收（含输出换行修复）

- Requirement/Scenario: R4/R5/R6；S4/S6/S7/S9/S10/S11。
- Depends on: 3.1 GREEN + `make k3` exit0。
- Targets: `kernel/src/platform/k3_network_probe.rs`（stage13 session/输出与新宏）、`kernel/src/platform/k3_device_irq_probe.rs`（stage9/10 宏换行）、`kernel/src/platform/k3.rs`（stage12→13 链接）、`tests/k3-polling-net-host/` 协议阶段 integration tests。
- Current behavior: stage12 任一 outcome 后 park；stage9+ 输出行首尾相连无换行。
- Required behavior:
  - 换行修复（用户 2026-10-04 批准并入）：`stage9_line!` 到 stage13 新宏统一补尾随 `\n`（对齐 stage≤8 的既有形态）；stage1–8 输出与既有测试断言不回归。
  - gate：仅 stage12 `WindowEnded` 进入 stage13；其余 outcome 保持 park、stage13 零 GMAC 访问。
  - stage13：分配第三 arena 完整有界 init → `enable_tx` → smoltcp Interface + 静态 socket（3.1 Device）→ 60 秒有界会话窗：PC `ping -n 20 192.168.137.2`（auto-icmp-echo-reply）、UDP 26001 回显 ASCII `ms11-udp`、TCP 26002 回显 `ms11-tcp` 并正常 close；窗内持续 poll（RX 收包、栈 ingress、TX 提交与 terminal 回收）。
  - 串口分列输出：各协议计数（ICMP 应答数、UDP/TCP 回显字节数、TCP close 状态）、rx frames/drops、tx terminals/errors、协议故障行（含阶段与 owner）；样本上限沿既有约定；不以协议栈成功冒充 DMA 层结论，也不以本地 terminal 冒充对端交付。
  - 操作者板测对端证据（Wireshark 按用户原话永久豁免，不作为条件）：PC 原生工具输出——ping 原生输出（20/20 应答）、UDP 客户端发送 `ms11-udp` 后回显内容、TCP 客户端连接/发送 `ms11-tcp`/回显/关闭结果；PC 客户端用系统自带能力（如 Python 标准库单行）产生流量并以原生输出为判定，不新增判定脚本。
  - reset 恢复原系统，操作者一句结论。
- Preserve: stage1–12 行为（除换行格式）、单前台 owner、masks、RAM-only、旧平台。
- Forbidden: Wireshark 依赖、持久网络配置/固件写入、只用栈 host 测试宣称真板结果、吞吐/soak 门槛、自定义帧身份字段。
- Test witness: k3-polling-net-host integration tests 先 RED stage13 gate 与失败/close 输出语义；既有 21 witness 与 device-irq harness GREEN 为兼容见证（换行修复触及后者表面）。
- GREEN condition: host/build 通过 + 操作者四协议真实结果满足 R4/R6（ping 20/20、UDP/TCP 回显内容一致、TCP 正常 close、reset 恢复）；partial 或未执行人工项不计完成。
- Verification: V3'（下述）。
- Stop when: 任一协议缺失、链路或 owner 错误、需要用户改变拓扑/验收范围。

**Invariants**

单 hart 唯一前台 owner；TX terminal ≠ peer delivery 维持到协议层（回显/应答才是对端证据）；OWN 不软件清除；所有等待有界；发布后 backing 不 Drop；W1C 只清已消费位；smoltcp 不被修改；新能力仅 K3 cfg；TDD/三次失败/验证边界生效。

**Non-goals**

异步 wake、运行时恢复、多 hart、offload/零拷贝、性能与 soak、shell/存储、MAC DMA IRQ、不重做 stage1–12（除换行格式）。

**Acceptance**

| Acceptance | Requirement/Scenario/Task | 直接通过条件 |
|---|---|---|
| A1 Device/session 契约（host） | R4；S4/S5/S6；3.1 | token 预留/背压/错误语义、四协议内存 peer、满队列/partial/timeout/cancel/close 全绿 |
| A2 四协议真板交换 | R4；S6；3.2 | PC 原生工具 + 串口分列：ping 20/20、UDP/TCP 回显一致、TCP 正常 close |
| A3 错误/超时/取消有界 | R5；S8/S9/S10；3.1/3.2 | session 超时/cancel/close 测试 GREEN；板上如出现故障行须含阶段/owner |
| A4 入口与兼容 | R6；S7/S11；3.2 | stage13 gate 次序、换行修复无回归、受影响 harness/构建 GREEN |
| A5 RAM boot/reset 边界 | R6；S10；3.2 | reset 一句结论；无数据不计完成 |

**Verification**

V1：新 Cargo 包原生命令 + 既有 harness 回归：

~~~sh
cargo test --manifest-path tests/k3-polling-net-host/Cargo.toml
rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/k3-gmac-host-test
/tmp/k3-gmac-host-test
~~~

判据：Cargo 测试编译 exit0、`0 failed`、exit0；既有 21 witness 保持 GREEN。

V2：触及共享 cfg（root/kernel Cargo.toml、workspace exclude）与 `k3_device_irq_probe.rs`：`make k3`、`make build`、`make lichee` 各一次 exit0；device-irq harness 重跑 0 failed。phy/mac/aia 未变采信；既有 VF2/ms04 失败只报告。

V3'（操作者，能力边界；Wireshark 豁免，不作为条件）：

1. 同一 RAM boot 拓扑（PC 192.168.137.1/24 直连）；stage12 窗口结束自动进入 stage13 60 秒会话。
2. 会话窗内 PC 依次执行：`ping -n 20 192.168.137.2`（记录原生输出 20/20）；UDP 客户端向 192.168.137.2:26001 发送 ASCII `ms11-udp` 并读回显；TCP 客户端连接 192.168.137.2:26002、发送 `ms11-tcp`、读回显并观察关闭。
3. 串口 stage13 summary 各协议计数与 PC 侧原生输出逐项对应；每步一句话结论记入 Act Response。
4. reset 后原系统恢复，一句实际结论。此操作结束本轮。

**Gate 2 Readiness**

PASS — 2026-10-04 用户计划审计原话"批准"。调查闭合（001 accepted 基线 + smoltcp/依赖表面已核实）、契约自包含、验证为最简直接判定（PC 原生工具 + 串口分列，无 Wireshark 依赖、无判定层）；无 BLOCKED/WAIVED、无待 Act 决定的实质未知项。Plan Context 离开 draft 改为 ready；下一边界为 Act 指令。

**Persisted Evidence**

- Mode: none

Act Response 承载各协议计数、决定性行与人工结论；板测输出超限时按用户明确要求沿先例另存，不预创建目录。

**Risks and Notes**

- smoltcp `consume` 无错误返回的适配已由 D5 裁决（容量保留 + 设备侧 fault 前台消费）；实现若与 0.13.1 实际接口矛盾，Stop when 交 Plan。
- RX 容量：RX own-slot 已板测支持整环回绕（001 轮 20 帧零 drop）；60 秒会话 RX 帧数预计更高，若出现 RBU/drop 按 bounded 语义报告并计入 summary（观察项，不判协议失败）。
- TCP close 判定以"回显一致 + close 完成"为准，关闭发起方向非实质。
- PC 客户端形态（系统自带工具/Python 标准库单行）与样本行数上限为非实质选择，留 Act 并记录。
- Wireshark 豁免的剩余风险：UDP/TCP 的对端帧级细节（重传/FIN 时序）不可见——由回显内容一致性与串口协议计数共同覆盖，用户已明示接受。

## Act Response

- Status: reported（2026-10-06；host V1/V2 全绿 + 真板 V3' 四协议交换成立，用户裁决 ping 形态
  与 reset 已回填，全部 Acceptance 闭合，等待 Plan Review）

### Actual changes

**3.1 K3 smoltcp Device 与有界 session**

- 新增 `kernel/src/platform/k3_gmac/polling_net.rs`（k3_gmac 子模块，`#[cfg(feature = "k3-polling-net")]`）：
  `K3PollDevice`（smoltcp 0.13.1 token Device，MTU 1514、软件校验和、poll 入口先 TX reclaim、
  无空闲 TX slot 保留 RX stash 不交付 token 对、驱动错误记入 `SessionFault{stage, owner}`）、
  `K3RxToken/K3TxToken`（RX token 持 CPU 拷贝；TX token 容量预检后经已验收 `try_submit` 发布，
  未 consume 的 token 零预留可释放；consume 内 LengthInvalid/Full/Driver 一律落 fault 不伪成功）、
  `K3Session`（静态 socket 存储 `SessionStorage`、UDP 26001/TCP 26002 echo 各 4096B 双向、
  echo 累计上限 1024B 且 partial send/recv 按 offset 推进、等待/close 5 秒有界、cancel 走驱动
  bounded stop、TCP close 以"回显全部 ACK/对端半闭"触发、完成有界）、板侧标识
  192.168.137.2/24 + MAC 02:00:00:00:26:01。
- `kernel/src/platform/k3_gmac/mod.rs`：新增子模块声明与只读访问器 `Gmac::tx_free_capacity()`
  （Device 容量预检所需，不改变任何 submit/发布语义）。
- `kernel/Cargo.toml`：`k3 = ["dep:axplat-riscv64-k3", "k3-polling-net"]`、
  `k3-polling-net = ["dep:smoltcp"]`、可选 `smoltcp = { workspace = true }`。
- 根 `Cargo.toml`：`[workspace.dependencies]` 新增本地 smoltcp（path，default-features=false，
  仅 medium-ethernet/proto-ipv4/socket-icmp/socket-udp/socket-tcp/socket-tcp-reno/
  auto-icmp-echo-reply）；`exclude` 追加 `tests/k3-polling-net-host`。`Cargo.lock`：kernel 增
  smoltcp 边（1 行；lock 中 smoltcp 条目无 registry checksum，即本地 path crate）。
- 新增 `tests/k3-polling-net-host/`（Cargo 包，root workspace exclude）：`src/lib.rs`（#[path]
  生产模块接线，与 rustc 直编 harness 同模式）、`src/fake.rs`（FakeEnv 假板 + FakeWire 设备侧
  DMA 半环：RX 取指游标按环序注入、TX 按序完成出线；PeerHost 真实 smoltcp 内存对端）、
  `tests/device_contract.rs`（6 测试）、`tests/session_protocols.rs`（6 测试）。

**3.2 stage13 与输出换行修复**

- `kernel/src/platform/k3_network_probe.rs`：新增 stage13 section（`STAGE13_WINDOW_NANOS` 60 s、
  `Stage13Summary/Stage13Outcome`、`stage13_line!` 宏、`stage13_fault_outcome`（fault 行必含
  stage+owner，FBE → `Fault{raw_status}`，其余 → `Stopped{session-fault}`）、`run_stage13`
  （仅 stage12 `WindowEnded` 过 gate，零 GMAC 访问；第三 arena 全量 init → enable_tx →
  Interface+静态 socket → 60 s 连续 poll 会话窗，result 行分列 icmp-replies/udp-echo/
  udp-over-limit/tcp-echo/tcp-close/rx-frames/rx-drops/tx-terminals/tx-errors/tx-timeouts）；
  board glue `run_k3_stage13`（第三 GlobalPage arena，会话存储置于 256 KiB boot 栈）。
- 换行修复（用户 2026-10-04 批准并入）：`aia.rs probe_line!`、`aia_probe.rs stage9_line!`、
  `k3_device_irq_probe.rs stage10_line!`、`k3_network_probe.rs stage11_line!/stage12_line!/
  print_frame` 统一补尾随 `\n`；stage1–8 console 输出未触及。
- `kernel/src/platform/k3.rs`：`run_first_byte_park` 捕获 stage12 outcome 并链接
  `run_k3_stage13`。
- `tests/k3-device-irq-host-harness.rs`、`tests/k3-aia-probe-host-harness.rs`：各追加 1 个
  stage9/10 wire 行换行见证测试。

### Deviations from Plan

1. `Gmac::tx_free_capacity()` 只读访问器为契约 Targets 之外的最小新增（3.1 契约的"容量先保留"
   语义需要可见的空闲容量；不改变发布/回收语义）。
2. TX token 预留实现为"容量预检 + consume 时提交"：未 consume 的 token 天然零预留（契约
   "未 consume 的 token 释放预留槽"的等价实现；单一前台 owner 下预检后容量只增不减，提交不可能
   Full）。等价局部实现，未改变 D5 裁决语义。
3. `SessionStorage<'s>` 带存储生命周期参数（`SocketStorage` 在 `&mut` 下不协变，编译所需）。
4. `rx_frames`/`icmp_echo_replies` 在交付给栈时计数而非 reap 时（暂存未交付的帧不计入）；
   drop 计数仍在 reap 处。
5. 换行修复同时覆盖 `aia.rs probe_line!`（`[k3:irq]` 芯片观察行与 stage9 同一串口线、同一
   无换行缺陷）——属"stage9+ 输出换行修复"的同一表面。
6. stage13 会话窗内 TCP 连接等待不设 5 s 独立上限（由 60 s 窗口有界）；5 s 上限适用于连接
   建立后的 echo 无进展与 close 完成（契约为"协议操作等待/取消/close 各 5 秒"的操作级有界；
   连接等待按窗口有界保证操作者先 ping 后 TCP 的流程不被截断）。

### Self-review findings（已修复）与遗留 Minor

- 已修复：rx 计数时点（暂存→交付）；`is_faulted` 反映驱动 Faulted 态而非一次性记录；
  `run_k3_stage13` 多余 `mut` 警告；FakeWire RX 注入改为按设备取指游标环序（初版"最老 OWN 槽"
  与驱动读取序错位，为测试假件缺陷，非产品缺陷）。
- 遗留 Minor：本地 smoltcp crate 自身既有 9 个编译警告（pre-existing，未触碰其实现）；
  `SESSION_IP_MTU` 常量为契约文档值（pub，无警告）。

### Verification

V1（host，新鲜运行，exit0）：

- `cargo test --manifest-path tests/k3-polling-net-host/Cargo.toml`：
  `test result: ok. 6 passed`（device_contract）+ `6 passed`（session_protocols）+ `5 passed`
  （stage13_gate）+ lib `0 passed`，全部 `0 failed`，exit0。
  RED 见证：实现前同命令三个测试二进制编译失败（E0432 unresolved `k3_gmac::polling_net`、
  `run_stage13`/`Stage13Outcome`）。
- `rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/k3-gmac-host-test &&
  /tmp/k3-gmac-host-test`：`test result: ok. 21 passed; 0 failed`，exit0（既有 witness 保持）。

V2（触及共享 cfg 与换行表面）：

- `make k3`：exit0（kernel+FIT 产出 `StarryOS_riscv64-k3.fit` 398880 B；触碰文件定向重建后
  kernel 代码零新增警告）。
- `make build`：exit0；`make lichee`：exit0（均在最后一次 k3-only 文件修改后表面未变的结论）。
- device-irq harness 重跑：`test result: ok. 18 passed; 0 failed`，exit0（含新增换行见证）。
- aia-probe harness（aia.rs/aia_probe.rs 的直接目标测试）：`test result: ok. 11 passed;
  0 failed`，exit0（含新增换行见证）。
- `openspec validate ms11-com260-kit-polling-bidirectional-network-baseline --strict
  --no-interactive`：`Change ... is valid`，exit0。
- Cargo.lock 核对：kernel 的 smoltcp 解析到无 registry checksum 的本地 path 条目。

change 结构自检：Iteration/Cycle 文件齐全；本 Cycle 的 Map checkbox（3.1/3.2）维持 planned，
待 Review accepted 后由 Plan 更新；specs/design 与实现一致（D5 裁决语义按契约落位）；
Persisted Evidence 计划为 `none`，未创建 Evidence 目录（Act Response 承载全部输出）。

### Persisted Evidence

- None required（模式 `none`；板测串口输出超限时按用户要求沿先例另存，不预创建目录）。

### Experience Candidates

- None。

### V3' 真板结果（2026-10-06 操作者实跑，Windows PC 192.168.137.1/24 直连）

PC 原生工具输出（操作者粘贴，逐字关键行）：

- ping 两轮共 10 个请求：第一轮 5 发 4 收（首请求 1 次超时，会话开始邻居解析未热），
  第二轮 5/5（0% 丢失，平均 1 ms）；操作者以 Ctrl-C 结束两轮，未跑 `ping -n 20` 整型。
- UDP：`UDP echo: ms11-udp`（与发送载荷逐字节一致）。
- TCP：`TCP echo: ms11-tcp`（一致）、`FIN: b''`（板侧主动关闭的 FIN 到达，干净 EOF）、
  `TCP closed`。

串口决定性行（操作者粘贴）：

~~~text
[starry-k3] stage=13 begin window-ms=60000 udp-port=26001 tcp-port=26002
[starry-k3] stage=13 init LinkConfig { speed: S1000, full_duplex: true } rx-running
[starry-k3] stage=13 tx-enabled
[starry-k3] stage=13 result icmp-replies=10 udp-echo=8 udp-over-limit=0 tcp-echo=8 tcp-close=Closed rx-frames=23 rx-drops=0 tx-terminals=16 tx-errors=0 tx-timeouts=0 window-elapsed-ms=60000
~~~

交叉判读：板侧收到 10 个 echo 请求、10 次应答，PC 可见 9 个（缺席的 1 个即首请求超时，
邻居缓存热后不复现）；UDP/TCP 回显字节级一致；`tcp-close=Closed` 与 PC 观察到的 FIN 互证；
60 秒满窗运行、零 drop、16 个栈发出帧全部 terminal（tx-errors=tx-timeouts=0）、无任何
op-timeout/fault 行（A3）。换行修复在真板线上成立（粘贴中每行独立成行，A4 操作者侧证据）；
stage12 同轮复证 `sent=20 terminals=20 tx-errors=0 tx-timeouts=0`，与 Iteration 001 已验收
基线一致。

用户裁决（原话）：

- ping 形态（契约文本为 `ping -n 20` 20/20，实际执行 5+5 手动 Ctrl-C）：**"接受"**。
- reset 恢复原系统（A5，操作者一句结论）：**"reset顺手就做了，不用管"**（reset 已执行，
  板子回原系统）。

Acceptance 对照：A1（host，V1）✓；A2（真板四协议，PC 原生 + 串口分列互证）✓（ping 形态按
用户原话豁免）；A3（无 op-timeout、无 fault 行）✓；A4（gate 链路 stage12→13、换行无回归、
V2 构建全绿）✓；A5（reset 回原系统）✓（用户原话豁免逐项描述）。本 Cycle 全部 Acceptance
闭合，等待 Plan Review。

## Plan Review

- Review Result: accepted（2026-10-06）

**Findings**

独立审查（Plan 直接读代码与 diff，非复核 Act 自检）确认 3.1/3.2 全部声明与代码一致：

- `polling_net.rs`：token Device（peek 预检容量、RX stash 保留、consume 错误落 `SessionFault{stage,owner}`）、静态 socket 存储（UDP/TCP 各 4096B 双向、echo 上限 1024B、partial 按 offset 推进、5 s 操作界）、板侧标识 192.168.137.2/24 + `02:00:00:00:26:01` 均按 D5 裁决落位。smoltcp MTU 语义已向本地 0.13.1 源码核实：`max_transmission_unit` 含以太网头（`ip_mtu = mtu - 14`），1514 正确对应 IP MTU 1500，栈出帧不越 `SESSION_MTU`。
- stage13：仅 stage12 `WindowEnded` 过 gate（gate 前零 GMAC 访问，有测试见证）、第三 arena 全量 init→`enable_tx`→60 s 连续 poll 会话、result 行分列字段与板上输出逐字段一致、fault/op-timeout 行均含 stage+owner。
- 换行修复：`aia.rs probe_line!`、`aia_probe.rs stage9_line!`、`k3_device_irq_probe.rs stage10_line!`、`k3_network_probe.rs stage11/12/13_line!` 与 `print_frame` 统一补尾随 `\n`；stage1–8 输出未触及。
- Cargo 接线：smoltcp 以本地 path 依赖、`default-features = false`、恰为契约 7 个 feature（无 test-seeds/async/dhcpv4/offload）；`k3` feature 门控 `k3-polling-net = ["dep:smoltcp"]`，default 构建不引入；`tests/k3-polling-net-host` 已入 workspace exclude。
- 测试：`tests/{device_contract,session_protocols,stage13_gate}.rs` 共 17 测试全部驱动生产模块（`#[path]` 接线）与真实 smoltcp 内存对端；名字与计数和 V1 声明一致（6+6+5）；既有 harness 各 +1 个换行见证，与 V2 的 18/11 passed 对应。

非阻塞 Minor findings（分类：Act 偏离-文档性 / 新证据，均不阻塞 Acceptance）：

1. Act Response "Actual changes" 未在文件清单中列出 `tests/stage13_gate.rs`（5 测试）；其结果已在 V1 完整报告（"5 passed (stage13_gate)"）且 RED 见证覆盖。文档性遗漏。
2. `K3TxToken::consume` 的 `write_len = len.min(SESSION_MTU)` 对假设性超长帧会静默截断并提交而非记 `LengthInvalid`——在 smoltcp 0.13.1 自身 MTU 约束下该分支不可达（栈出帧 ≤1514），无当前行为影响；防御性写法的稳健性备注。
3. UDP echo 在 socket TX buffer Full 分支会丢弃已 `recv` 的数据报——实际不可达（TX buffer 4096B > echo 总预算 1024B，预算先于 Full 耗尽）；设备级 Full 背压（slot owner 不丢）由 `tx_capacity_full_refuses...`/`rx_frame_retained...` 覆盖，符合 "Full 期间不丢合法 owner" 语义。

Deviations 审计：6 项偏差确认为非实质等价或有据收紧——#2 容量预检在单前台 owner 下与"预留槽"语义等价（预检后容量只增不减，且 `unconsumed_tx_token_releases_the_reservation` 见证）；#6 TCP 连接等待由 60 s 窗口有界、5 s 界限于 echo 无进展与 close，与操作者流程一致。其余为编译所需/同表面扩展/计数时点，均已在 Act Response 记录。

范围外观测（不属本 Cycle 处置）：工作树存在未提交的 R68 Runbook 与 references 更新（board 全流程版，来源标注 002 Act Response），内容与 V3' 证据一致；属 Recorder/Maintainer 收尾领域，不影响本 Review。

**Acceptance Gaps**

None。A1（V1 17 测试 0 failed）、A2（真板四协议：ping 9/10 互证 + UDP/TCP 回显字节一致 + FIN 干净关闭；ping 形态按用户原话"接受"）、A3（板上无 op-timeout/fault 行 + host timeout/cancel GREEN）、A4（gate 见证 + V2 make k3/build/lichee exit0 + 18/11/21 harness GREEN）、A5（reset 回原系统，用户原话豁免逐项描述）全部闭合。

**Evidence**

- 独立阅读：`kernel/src/platform/k3_gmac/polling_net.rs`（全文）、`kernel/src/platform/k3_network_probe.rs`（stage13/宏/板侧 glue）、`kernel/src/platform/k3.rs`（stage12→13 链接）、`crates/axplat-riscv64-k3/src/aia.rs` 与 `kernel/src/platform/{aia_probe,k3_device_irq_probe}.rs` 换行 diff、root/`kernel` `Cargo.toml` 与 `Cargo.lock` diff、`kernel/src/platform/k3_gmac/mod.rs`（`tx_free_capacity` 只读）、`tests/k3-polling-net-host/`（lib/fake/三测试文件全文）、两个既有 harness 新增见证 diff、`crates/smoltcp/src/phy/mod.rs`（MTU 语义）。
- 采信 Act Response（2026-10-06 新鲜运行）：V1（Cargo 6+6+5 passed、gmac harness 21 passed）、V2（make k3/build/lichee exit0、device-irq 18、aia-probe 11、validate 通过）。产品代码自 Act 运行后未变化（工作树与 HEAD `2e4efc5f` 一致，仅 .agents 与本 Cycle 文件有未提交修改），结论未失效。
- V3' 真板（操作者粘贴 + 用户裁决原话，见 Act Response）。

**Follow-up Decision**

全部 Acceptance 满足且无阻塞项，2 项 Minor finding 与 1 项防御性备注仅记录不返工（不影响已验收行为，修复无对应验收收益）。Iteration 002 完成；无剩余 Iteration，change 全部 7 任务闭合，下一步为用户授权后的 docs-maintainer 收尾。

**Iteration Plan Update**

None。

**Next Cycle**

None。

**Next Iteration**

None。
