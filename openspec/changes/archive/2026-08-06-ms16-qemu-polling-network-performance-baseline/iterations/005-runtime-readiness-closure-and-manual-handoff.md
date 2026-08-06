# Iteration 005: Runtime readiness closure and manual handoff

## Plan Context

- Status: ready
- Round: 005
- Parent: 004-runtime-readiness-and-manual-qemu-calibration

**Objective**

接收 fresh RISC-V guest artifact，补齐 004 Review 的 Important 问题，完成 T6 Runtime Readiness Gate，并交付可逐项执行的 user-net/TAP 人工命令。T7 仍由用户执行。

**Background**

004 因 sandbox SIGSYS 无法生成 guest binary，已按 Gate 6 标记 `blocked`。用户随后在 sandbox 外成功构建，并明确要求“自己按照plan创建下一iter，自己再继续实施吧”。该授权覆盖本轮 Plan 与后续 Act，但不改变 004 的历史状态。

独立 Review 还发现测试矩阵、Evidence 重建、不可用遥测和手册文件闭合问题。它们属于 004 已批准范围，合并到本轮处理。

**Current Baseline**

- Revision: `2a9319a946dbe9c07cb0f448d82c0b7c14069015`，工作区含未提交的 MS16 改动。
- Host/local: protocol 26、platform 20、tools 19、integration 6 已在 004 通过。
- Guest artifact: `tests/network_benchmark` 为 RISC-V static-pie，SHA-256 `68a628b0431cfa01a37810d46c9231b7ae29b283910895aaa31a453a52da82d3`，mtime 晚于 `tests/network_benchmark.c`。
- Runtime boundary: agent 不启动 QEMU，不输入 guest 命令，不操作 TAP、sudo 或抓包。

**Current-State Evidence**

- Workload entry: `tests/network_benchmark.c::main` 分派 self-test、calibrate、loopback、server 和 client。
- Formal path: `run_endpoint` 建立 control/data path，运行 workload，交换 SUMMARY，再由 `summaries_close` 判定双端 ledger。
- Local path: `run_loopback` 直接生成 record 并调用 `accept_record`，未经过正式 control lifecycle。
- Tests: `tests/test_network_benchmark_integration.py` 只跑 TCP/UDP × TX/RX/bidi × 2-flow；`--self-test` 只覆盖 fragmented TCP record。
- Evidence: `scripts/network_benchmark_evidence.py::check_evidence` 校验文件、manifest/hash 和 ledger，但不重建 `summary.json`。
- Report: `scripts/network_benchmark_report.py::generate_summary` 是 deterministic summary 的权威生成入口。
- Telemetry: `run_calibration` 在不可用时输出 `instret_status=unavailable` 和三个数值 0。
- Manual package: `manual-calibration.md` 的 5555/15555/`10.0.2.2` 拓扑正确，但 required files 与逐对执行步骤不完整。

**Relevant Code**

| File | Symbol | Responsibility |
|---|---|---|
| `tests/network_benchmark.c` | self-test helpers、`run_loopback`、`run_calibration` | deterministic workload witnesses、local matrix、timer/instret calibration |
| `tests/test_network_benchmark_integration.py` | `WorkloadIntegration` | subprocess CLI、flow/direction matrix 和 failure witnesses |
| `scripts/network_benchmark_evidence.py` | `check_evidence` | calibration/B0 文件、ledger、hash 和 summary closure |
| `tests/test_network_benchmark_tools.py` | `TestEvidence` | checker golden/failure fixtures |
| `Makefile` | MS16 targets | dependency-correct build、aggregate 和 preflight |
| `manual-calibration.md` | Terminal A/B/C/D commands | R44 用户能力边界交接 |

**Critical Path**

fresh artifact acceptance → permanent RED witnesses → minimal workload/checker fixes → focused GREEN → full T6 regression → preflight/manual package Review → user T7 handoff。

任一 Critical/Important finding或非 QEMU Gate 失败时停止，不进入人工 handoff。QEMU runtime 失败属于下一能力边界，不在本轮伪造结论。

**Implementation Guidance**

1. 先扩展 deterministic tests，观察新增 witness 在当前实现失败或缺失。
2. local matrix 覆盖 1/2/4/8 flows。故障 witness 通过纯状态/codec 注入覆盖 mismatch、EOF、timeout、cancel、TCP partial state 和 UDP classification，不依赖 AF_INET 或 sleep。
3. 不把 local simulator 宣称为正式网络证据。正式 endpoint 只由后续 user-net/TAP smoke 证明。
4. Evidence checker 调用 report 的 deterministic summary 逻辑，从 guest、host、CPU 原始输入重建结果，并与 `summary.json` 做结构化精确核对。缺失或漂移返回非零。
5. 不可用 instret 数值字段输出 JSON `null`，report 保持 `unavailable`，禁止用 0 代替。
6. 手册切换到 005 Evidence，逐对列出 listener/connector，先建提取目录，并给出全部 required Evidence 的保存命令和 checker Gate。

**Behavioral Change**

- 自动化 Gate 从单一 2-flow happy path扩展为 1/2/4/8-flow 与 deterministic failure matrix。
- calibration telemetry 用 `null` 表示 unavailable。
- calibration checker 能发现 `summary.json` 与原始记录不一致。
- manual package 生成的文件集合与 calibration profile checker 一致。
- benchmark wire protocol、默认端口和正式 endpoint ownership 不变。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| T1 | R1/R2/R4；flow 和 failure matrix | `network_benchmark.c` self-test；integration tests | partial record + 2-flow happy path | 补 1/4/8-flow 与 mismatch/EOF/timeout/cancel/UDP anomaly witness |
| T2 | R3/R6；unavailable telemetry | `run_calibration`；report tests | unavailable 仍带零值 | 改为 `null` 并验证 report 不计算零成本 |
| T3 | R7/R8；summary reconstruction | Evidence checker、tool tests | 文件/hash/ledger closure | 从 raw inputs 重建并精确核对 summary |
| T4 | R5/R7；manual package | Makefile、manual guide | 正确端口但文件/顺序不闭合 | 完整 005 Evidence 命令与逐对手测步骤 |
| T5 | R1-R8；Runtime Readiness | 全部相关 diff 和 Gate | T6 未完成 | 跑完整回归、preflight、strict validate 和 diff Review |

**Task Contracts**

T1 — permanent workload witnesses：

- Dependency: fresh host binary can be rebuilt.
- RED: 新测试必须证明当前缺少 1/4/8-flow 和各 failure classification，而不是因路径或权限失败。
- GREEN: TCP/UDP × TX/RX/bidi × 1/2/4/8 flows 均生成 nonzero exact dual-endpoint ledgers；mismatch、EOF、timeout、cancel、partial state 和 UDP anomalies 都产生确定的 invalid/classification 结果。
- Preserve: CLI schema、port table、C1/C6 payload ledger 和 no-network local substitute。
- Stop: witness 需要 AF_INET、QEMU、sleep timing 或新的 wire protocol 设计。

T2 — unavailable telemetry：

- Dependency: T1 GREEN.
- RED: calibration output 不得在 unavailable 状态携带可被解释为真实值的 0。
- GREEN: 三个 instret numeric fields 为 JSON `null`；available fixture 仍计算 instructions/bit，unavailable fixture 输出字符串 `unavailable`。
- Preserve: `/proc/instret` ABI 和 report 字段名。
- Stop: 修复需要改变 kernel telemetry ABI。

T3 — Evidence summary closure：

- Dependency: report golden tests GREEN.
- RED: 修改 `summary.json` 后当前 checker 错误地 PASS。
- GREEN: checker 从 guest、host、CPU 重建 summary；缺失、漂移、单端、extra round、hash mismatch 和 incomparable input 均非零。
- Preserve: deterministic JSON/CSV、invalid round retention 和 comparison treatment 语义。
- Stop: raw schema不足以重建现有 report 输出。

T4 — manual package closure：

- Dependency: T1-T3 GREEN.
- RED witness: 静态测试或 review 检查 required filenames、005 path、port tuples 和 listener/connector ordering。
- GREEN: guide 给出 `manifest.json`、README、QEMU command、serial、guest console、endpoint NDJSON、CPU、IRQ、pcap、CSV、summary 和 checker output 的产生/保存路径；preflight 展示相同信息。
- Preserve: R44 手工政策；user-net inbound 5555、outbound 15555；TAP guest→host `10.0.2.2:5555`。
- Stop: 任一命令自动启动或驱动 QEMU，或使用 guest 自身地址作为 guest connector。

T5 — T6 full Gate：

- Dependency: T1-T4 GREEN.
- Run 004 定义的完整验证集。cross guest artifact 使用已提交 hash，除非源码又被修改；若源码变化，必须重新生成并重新核对 freshness/hash。
- Full diff Review 依次做 spec compliance、code quality。Critical/Important 必须修复并重跑受影响 Gate。
- Stop: guest artifact stale、回归失败、preflight mismatch、需要新的设计，或到达 QEMU/TAP 用户能力边界。

**Invariants**

- 004 保持 `blocked`，其 Plan Context 和 Act Response 不改写。
- R44：QEMU、guest console、TAP、sudo 和 pcap 只由用户手工执行。
- QEMU 与真板结果分证据类别；user-net 不作为 TAP 性能结论。
- 不修改异步网卡、队列容量、socket 容量、10ms polling fallback 或驱动行为。
- 不覆盖用户无关工作区改动。

**Non-goals**

- 执行 T7、standard B0、性能优化、异步 RX/TX 或真板测试。
- 自动化 QEMU runner、TAP lifecycle 或 guest command injection。
- 为本轮建立新 wire protocol、kernel telemetry ABI 或 driver instrumentation。

**Acceptance**

- Fresh guest artifact 的类型、hash 和 freshness 已记录。
- T1-T4 的 permanent witnesses 全部 GREEN，无未解决 Critical/Important。
- Full T6 验证全部 exit 0，或在明确能力边界前交接。
- 手册和 preflight 与 R44/R45/R48 及 checker required files 一致。
- 交付完整人工命令、expected markers、Evidence 005 路径和停止条件。

**Requirements Traceability Matrix**

| Requirement | Scenario | Design | Task | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|
| R1/R2 | lifecycle、partial、failure | pure injection | T1 | workload self-test | deterministic failure matrix | None | Covered |
| R3/R6 | instret available/unavailable | null semantics | T2 | calibration/report | telemetry fixtures | None | Covered |
| R4 | 1/2/4/8-flow matrix | loopback matrix | T1 | integration tests | protocol×direction×flow | None | Covered |
| R5 | manual user-net/TAP | R44 boundary | T4 | guide/preflight | static command review | None | Covered |
| R7 | Evidence closure | raw reconstruction | T3/T4 | checker/guide | drifted summary fixture | None | Covered |
| R8 | comparison | exact manifest key | T3 | checker | incomparable fixture | None | Covered |

**Verification**

```bash
make network-benchmark-test
make network-benchmark-workload-test
make network-benchmark-local-test
python3 -m unittest tests.test_network_benchmark_integration -v
python3 -m unittest tests.test_network_benchmark_tools -v
make tests/network_benchmark-host-asan
ASAN_OPTIONS=detect_leaks=0 ./tests/network_benchmark-host-asan --self-test
file tests/network_benchmark-host tests/network_benchmark
sha256sum tests/network_benchmark-host tests/network_benchmark
make network-benchmark-calibration-preflight
make host-test
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib service::tests -- --nocapture
python3 scripts/ms01-qemu-test.py --self-test
make LOG=info build
openspec validate ms16-qemu-polling-network-performance-baseline --strict
git diff --check
```

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 004、实际代码、Evidence、R44/R45/R48 和 fresh artifact 已独立检查 |
| Design | PASS | pure failure injection、null telemetry、summary reconstruction 和 manual boundary 已固定 |
| Task Contracts | PASS | T1-T5 含依赖、RED/GREEN、保持项和停止条件 |
| Traceability | PASS | R1-R8 映射到代码与 witness，无 Missing |
| Verification | PASS | focused、sanitizer、regression、build、preflight、validate 和 diff Gate 已列出 |
| User Approval | PASS | 用户明确要求创建下一 iteration 并继续实施 |

**Persisted Evidence**

- Mode: required
- Directory: `openspec/changes/ms16-qemu-polling-network-performance-baseline/evidence/005-runtime-readiness-closure-and-manual-handoff/`
- T1-T5: fresh artifact facts、RED/GREEN tests、sanitizer、regression、preflight、strict validation 和 full diff Review。
- T7 文件只在用户实际提交后加入该目录；本轮不得创建伪运行文件。

**Risks and Notes**

- sandbox 仍可能阻止 cross compiler。若本轮修改 `tests/network_benchmark.c`，旧 hash 会立即失效；必须再次由用户生成 guest artifact，不能以 host build 替代。
- formal AF_INET path 在 T7 前没有 runtime witness。T6 只能声明 runtime-ready，不声明 QEMU smoke 已通过。
- Plan Review 所列问题均属于原 004 范围，没有新增 benchmark 指标或驱动范围。

## Act Response

- Status: pending

## Plan Review

- Status: pending
