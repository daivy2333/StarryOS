# Iteration 004: Runtime Readiness and Manual QEMU Calibration

## Plan Context

- Status: pending-approval
- Round: 004
- Parent: `iterations/003-workload-correction-and-calibration-readiness.md`

**Objective**

在同一轮完成 portable workload 的阻塞修正、运行就绪验收和 user-net/TAP 手工校准。先用可复现的本地 Gate 证明双端协议与指标可信，再由用户按 R44、R45、R48 手动进入 QEMU；不拆出只修复不测试的独立 iteration。

**Why This Round Exists**

003 Act Response 声称 T0-T7 全部完成，但独立 Review 的新鲜构建得到空 valid round，两个 Makefile workload Gate 失败，Python integration module 不存在。正式路径还有握手闭锁、RX/bidi 未实现、UDP 无发送路径、partial write 丢状态和 metric ledger 污染。

这些问题会让 QEMU 测试无法开始或生成错误基线。004 把修正和手测放在同一轮，但设置不可绕过的 Runtime Readiness Gate：Gate 失败时只记录 source failure，不启动 QEMU；QEMU smoke 失败时保留 invalid Evidence，不进入 standard B0。

**Current Reproducible Baseline**

| Surface | Fresh result | Required state before QEMU |
|---|---|---|
| foundation aggregate | 25 protocol、20 platform、15 Python PASS | 增加 workload/control/tool failure witnesses |
| host rebuild | strict C11/O2 build PASS | sanitizer run 和 binary hash 可复现 |
| loopback smoke | valid round，TX/RX 为 0 | 完整双端 lifecycle，非零且账本闭合 |
| workload/local Gates | 均 exit 2 | 每条子测试新鲜构建并 exit 0 |
| integration module | 不存在，exit 1 | fault-injected integration matrix 存在并通过 |
| formal TCP path | START/data setup 顺序闭锁 | HELLO→READY→data-ready→START→SUMMARY→DONE |
| RX/bidi | config-only | 两端 ownership 与 C1/C6 ledger 可见证 |
| UDP | 无可达 send path | connected/peer-addressed datagram flow + pacing |
| preflight | TAP 指向 guest 自身；信息不全 | 与 R44/R45/R48 一致的完整人工包 |

**Port and Endpoint Contract**

| Run | QEMU network | Listener | Connector | Benchmark port |
|---|---|---|---|---:|
| user-net RX | `hostfwd=tcp/udp::5555-:5555` | guest `0.0.0.0:5555` | host `127.0.0.1:5555` | 5555 |
| user-net TX | same QEMU instance | host `0.0.0.0:15555` | guest `10.0.2.2:15555` | 15555 |
| user-net bidi | same QEMU instance | host `0.0.0.0:15555` | guest `10.0.2.2:15555` | 15555 |
| TAP guest TX | no hostfwd | host `10.0.2.2:5555` | guest `10.0.2.2:5555` | 5555 |
| TAP guest RX | no hostfwd | guest `10.0.2.15:5555` | host `10.0.2.15:5555` | 5555 |

TCP 与 UDP 分开运行。hostfwd 可以同时声明 TCP/UDP 5555，但各自日志、round 和 packet witness 不得合并。HTTP payload distribution 固定为 host `0.0.0.0:18765`、guest `10.0.2.2:18765`，HTTP server 从 `tests/` 启动。

**Execution Order**

```text
T0 RED witnesses
  -> T1 CLI/schema/protocol safety
  -> T2 lifecycle and topology
  -> T3 TCP directions and ledgers
  -> T4 UDP correctness and pacing
  -> T5 tools, local substitute and Evidence
  -> T6 Runtime Readiness Gate
  -> T7 user manual QEMU calibration
```

T0-T6 属于后续 Act。T7 属于用户能力边界，不允许 Act 或脚本代跑。T6 未通过时禁止进入 T7。

**Task Contracts**

T0 — permanent RED witnesses:

- Add workload C unit tests and `tests/test_network_benchmark_integration.py` before behavioral repair.
- RED must reproduce empty-valid loopback, START/data setup deadlock, direction ownership, partial control/TCP writes, UDP unreachable send, missing SUMMARY, profile override, peer-missing report and incomparable Evidence exit.
- Tests inject fd readiness, short I/O, EAGAIN, EOF, deadline, sequence and counter samples. They must not require AF_INET, QEMU, TAP, sudo or timing sleeps for correctness.
- Make aggregate must rebuild its exact binary dependencies. A stale artifact cannot satisfy RED or GREEN.
- Stop if a Critical finding lacks a deterministic witness or needs QEMU to reproduce.

T1 — CLI, schema and protocol safety:

- Parse all numerics with full-string, range and overflow validation. Reject duplicate/conflicting options and missing values before socket creation.
- Define precedence: profile supplies defaults, then explicit duration/warmup/seed overrides remain effective. Add explicit run/test/round identity or a documented deterministic allocation rule.
- Emit one canonical manifest per endpoint with platform, driver mode, profile, topology, protocol, direction, flow/payload, timing, seed, endpoint and fingerprint facts. Round `side` must match report/Evidence join semantics.
- Decode control frames into a temporary object and commit only on success. Bound body length before buffer reads. Cover HELLO/READY/START/CANCEL/ERROR/SUMMARY exact lengths and invalid variants.
- Stop on caller mutation after failed decode, ambiguous endpoint identity or config fingerprints that ignore effective options.

T2 — control lifecycle and endpoint topology:

- Use a single nonblocking `poll()` state machine with persistent RX/TX frame offsets and absolute deadlines. EAGAIN keeps state; it is not success and cannot busy-wait through `poll(NULL, 0, 1)`.
- Establish and identify required data flows before START. Server/client must reach HELLO→READY→data-ready→START without circular waiting.
- Exchange SUMMARY from both endpoints, verify matching run/test/round/config and ledgers, then close through DONE. EOF, timeout, CANCEL and ERROR produce reason-coded invalid rounds.
- Check every `accept`, `connect`, `fcntl`, `setsockopt(TCP_NODELAY)` and poll error. Listener readiness drives accept; accepted-flow count, not configured count, drives phase transition.
- Topology tests cover default 5555, user-net inbound 5555 and guest-outbound 15555 without hard-coding one direction into client/server roles.
- Stop if either endpoint can emit valid before peer SUMMARY or if no-progress reaches normal completion.

T3 — TCP directions, partial I/O and metrics:

- Implement TX, RX and bidi ownership explicitly for both endpoint roles. Bidi must have independent per-direction sequence and ledger state.
- Keep encoded record and offset until the full record is sent. Decode stream records across arbitrary fragmentation/coalescing, validate bounds, CRC, sequence and deterministic payload before C6.
- C1/C6 bytes count verified payload bytes; record headers are reported separately if wire-overhead metrics are needed. Sender C1 must equal peer receiver C6 for every valid TCP direction.
- Cover 1/2/4/8 flows, payload boundaries, EAGAIN recovery, peer EOF, sequence gap/duplicate, corruption and bounded no-progress. Implement RTT request/response only where its measurement contract is unambiguous.
- A round with zero expected-direction bytes, ledger mismatch, corrupt payload or incomplete flow is invalid.
- Stop if flow state can advance after a partial record is discarded or if report must infer which endpoint was receiver.

T4 — UDP path, pacing and classification:

- Create a connected UDP socket or store and use an explicit peer sockaddr for every sending flow. Do not route UDP through TCP data-flow fds.
- Datagram carries bounded record metadata, sequence, timestamp and CRC. Validate deterministic payload before acceptance.
- Distinguish loss, duplicate, reorder, corrupt and late without classifying the first sequence as duplicate. Define end-of-round loss from offered/accepted sequence space.
- Pace against absolute deadlines. After lateness, resynchronize without catch-up burst or zero-sleep loop; record offered load and pacing slips.
- Cover TX/RX/bidi, 1/2/4/8 flow, EAGAIN, truncation, source mismatch and injected anomaly matrices.
- Stop if any UDP smoke has zero offered/accepted data, unconnected `send()` or combined error buckets.

T5 — local substitute, collector, report and Evidence:

- Local mode must exercise the same control codec, lifecycle, data records, direction ownership and summaries as formal endpoints. Use `SOCK_STREAM` for TCP and datagram-preserving transport for UDP; do not count raw recv bytes as C6.
- Integration matrix covers TCP/UDP × TX/RX/bidi, smoke/quick, 1/2/4/8 flows plus mismatch, EOF, timeout and cancel. Fixed-seed normalized outputs must be deterministic.
- Collector uses an absolute sample schedule, validates positive interval/duration and target PID identity before start, and rejects PID reuse/counter regression. Keep QEMU, peer and collector scopes separate.
- Report requires both endpoint records for headline, joins exact IDs/fingerprint/status, retains invalid rounds, derives C6 goodput/PPS/RTT/jitter/UDP/CPU/instructions-per-bit, and writes deterministic JSON plus CSV.
- Evidence checker validates calibration/local profiles, mandatory file hashes, exact round sets including host extras, endpoint status/ledgers, UDP ledgers, summary reconstruction and comparison fields. Missing/incomparable input exits nonzero.
- Stop if unavailable telemetry becomes zero, README text substitutes raw Evidence or a single-sided round becomes valid headline.

T6 — Runtime Readiness Gate and manual package:

- Add a change-local calibration guide that references R44/R45/R48 and gives complete Terminal A/B/C commands. Each guest command is entered manually after `starry:~#`; no automation starts QEMU or drives the console.
- Correct preflight to print benchmark/kernel/rootfs hashes, compiler/file facts, QEMU/machine/SMP/memory/icount placeholders, exact endpoint commands, HTTP distribution, collector PID scopes, MS03 IRQ snapshots, pcap paths and Evidence checklist.
- TAP commands must connect guest-to-host at `10.0.2.2:5555`, never `10.0.2.15:5555`. User-net guest-to-host uses 15555 while hostfwd retains 5555.
- Required GREEN: focused tests, workload/local aggregates, integration tests, ASan/UBSan smoke, host/guest artifacts, report/Evidence golden tests, host-test, axnet 8/8, MS01 self-test, target build, strict OpenSpec validation and diff check.
- Guest artifact requires `file` and SHA-256. A genuine toolchain restriction is ENV BLOCK and blocks T7; it cannot be converted to PASS.
- Remove generated binaries from the diff or add a narrow ignore rule after proving ownership. Do not delete unrelated user files.
- Stop before T7 on any failed command, missing artifact, Critical/Important Review finding or preflight topology mismatch.

T7 — manual user-net and TAP calibration:

- User first runs N00-N03 only: manifest/hash capture, timing/instret calibration, local loopback and ARP/ICMP/MTU path checks. No standard B0 matrix in this round.
- Run user-net TCP/UDP RX smoke through hostfwd 5555. Separately run TCP/UDP TX and bidi with host listener 15555 and guest connector `10.0.2.2:15555`.
- Run TAP TCP/UDP TX/RX/bidi smoke at port 5555 without hostfwd. Save QEMU command, serial, both endpoint NDJSON, QEMU/peer/collector CPU samples, IRQ snapshots and pcap.
- Each case records command, start/end timestamps, exit or termination method, endpoint hashes, valid/invalid status and earliest failing layer. Rebind tests wait two seconds per R44 when the current smoltcp fork retains a port.
- A smoke failure remains Evidence and may be rerun only with a new round ID. Do not overwrite or omit failed rounds.
- Run report and calibration Evidence checker. Only a checker-clean calibration permits planning the standard B0 runtime matrix.
- Stop on console/payload/topology/protocol/timer/instret/IRQ/collector failure. Do not reinterpret user-net as TAP performance or calibration as B0 headline.

**Acceptance**

- All 003 Critical and Important findings have permanent tests and no unresolved Critical/Important Review issue.
- Fresh `network-benchmark-workload-test` and `network-benchmark-local-test` pass with nonzero exact dual-endpoint ledgers.
- Formal TCP and UDP paths cover TX/RX/bidi and multiflow without stale artifact, external network or QEMU dependency.
- Collector, report and Evidence tools fail closed on missing, drifted or incomparable data and reconstruct deterministic summaries.
- Preflight and guide implement the R44/R45/R48 port table exactly and contain no QEMU automation.
- User-submitted calibration Evidence covers user-net and TAP smoke, preserves invalid attempts and passes the calibration checker.
- Tasks 1.6-1.8 and 2.1-2.7 may be checked only after T0-T6 Review PASS. Task 2.8 requires T7 Evidence and remains unchecked before then.

**Requirements Traceability Matrix**

| Requirement | Scenarios | Tasks | Witness | Status |
|---|---|---|---|---|
| R1 protocol | exact control, mismatch, EOF, timeout | T0-T2 | codec + lifecycle tests | Covered |
| R2 integrity | partial TCP, UDP anomaly, payload/CRC | T0,T3,T4 | injected record/datagram matrix | Covered |
| R3 metrics | C1/C6, RTT/jitter, CPU, instret | T3-T5 | ledger + golden report | Covered |
| R4 profiles | override, smoke/quick, flow matrix | T0,T1,T5 | CLI + integration tests | Covered |
| R5 QEMU boundary | manual console and TAP | T6,T7 | guide + user raw Evidence | Covered |
| R6 CPU/IRQ | scope, regression, unavailable | T5-T7 | collector fixtures + snapshots | Covered |
| R7 Evidence | files, hashes, reruns, reconstruction | T5,T7 | strict checker | Covered |
| R8 comparison | topology/platform/treatment drift | T5,T7 | comparison fixtures | Covered |

**Verification**

Runtime Readiness, all commands require exit 0:

```bash
make network-benchmark-test
make network-benchmark-workload-test
make network-benchmark-local-test
python3 -m unittest tests.test_network_benchmark_integration -v
python3 -m unittest tests.test_network_benchmark_tools -v
make tests/network_benchmark-host-asan
ASAN_OPTIONS=detect_leaks=0 ./tests/network_benchmark-host-asan loopback --profile smoke
make tests/network_benchmark-host
make tests/network_benchmark
make network-benchmark-calibration-preflight
file tests/network_benchmark-host tests/network_benchmark
sha256sum tests/network_benchmark-host tests/network_benchmark
make host-test
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib service::tests -- --nocapture
python3 scripts/ms01-qemu-test.py --self-test
make LOG=info build
openspec validate ms16-qemu-polling-network-performance-baseline --strict
git diff --check
```

Manual calibration commands live in the T6 guide. T7 acceptance additionally requires the calibration-profile checker and generated report to exit 0 against the submitted Evidence directory.

**Manual Capability Boundary**

The user alone performs QEMU start/stop, guest shell input, TAP/sudo, host process coordination and packet capture. Act may build artifacts, generate commands and validate already submitted files, but must not automate these operations.

If T6 is GREEN, Act Response must hand off exact hashes, commands, expected markers and Evidence paths. It reports T7 as waiting for user Evidence, not as blocked or complete. The same 004 document remains the iteration context when the user returns with results.

**Persisted Evidence**

- Mode: required
- Directory: `openspec/changes/ms16-qemu-polling-network-performance-baseline/evidence/004-runtime-readiness-and-manual-qemu-calibration/`
- T0-T6: RED/GREEN logs, test counts, sanitizer output, compiler/file/hash facts, regression logs and full diff Review.
- T7: README index, manifest, QEMU command, guest serial, guest/host NDJSON, CPU samples, IRQ snapshots, pcap, results CSV, summary JSON and checker output.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 003 diff、new fresh probes、R44/R45/R48 与 tools 已检查 |
| Design | PASS | fix→readiness→manual calibration 顺序和 port table 已固定 |
| Task Contracts | PASS | T0-T7 有依赖、witness、stop condition 和能力边界 |
| Traceability | PASS | R1-R8 映射到 tasks 和 witnesses |
| Verification | PASS | local、sanitizer、guest build、regression、preflight 与 manual checker 已定义 |
| User Approval | BLOCKED | iteration 004 尚待用户批准 |

**Risks and Notes**

- 004 仍然较大，但用户明确要求不为可合并问题单开修复轮。T6 是防止范围合并污染 QEMU 结果的硬 Gate。
- local transport 只能证明 userspace protocol/state correctness，不能替代 AF_INET、VirtIO、user-net 或 TAP Evidence。
- user-net 只做可运行性 smoke；性能校准以 TAP 为准，standard B0 另行规划。
- 任何 003 生成的二进制都不能作为新鲜 Gate 的默认可信输入。

## Act Response

- Status: blocked

**Implemented**

T0-T5 的 host/local 路径已部分实施。新增 permanent integration tests，重写 portable workload 的 CLI、双端账本和正式 TCP/UDP 路径。protocol decoder 改为成功后提交 caller output。collector、report 和 Evidence checker 改为 fail-closed。

新增手工校准 guide 和 preflight 输出。未启动 QEMU，未操作 TAP。

用户授权原话为“开始修复吧，修复完之后到了手动测试的部分就把测试的那些命令行完整给我”。该授权豁免 Plan Context 中尚未同步的 `pending-approval`，但不豁免 T6 guest artifact Gate。

**Changed Files and Symbols**

| File | Symbols | Change |
|---|---|---|
| `tests/network_benchmark.c` | CLI、control/data path、local simulator、calibration | 严格参数、side ownership、双端 summary、TCP partial state、UDP registration/pacing、C6 payload ledger |
| `tests/network_benchmark_protocol.c` | `nb_frame_decode` | 临时对象 decode，成功后提交 |
| `tests/network_benchmark_protocol_test.c` | failure atomic test | 新增第 26 个 protocol witness |
| `tests/test_network_benchmark_integration.py` | 6 integration tests | CLI、local matrix、missing peer、comparison exit 和 internal fault witness |
| `tests/test_network_benchmark_tools.py` | collector/report/Evidence tests | PID reuse、counter regression、CSV/JSON、extra round 和 missing peer |
| `scripts/network_benchmark_collect.py` | `sample_continues`、`collect_loop` | 绝对采样、正参数、PID identity 和 counter monotonicity |
| `scripts/network_benchmark_report.py` | dual-endpoint report | exact join、ledger、invalid retention、CPU/instret/UDP、CSV/JSON |
| `scripts/network_benchmark_evidence.py` | closure、profiles、CLI | host extra round、status/fingerprint、mandatory files/hashes、nonzero compare |
| `Makefile` | MS16 targets | dependency-correct aggregates、integration Gate、preflight |
| `.gitignore` | benchmark artifacts | 精确忽略三个生成 binary |
| `manual-calibration.md` | R44 manual commands | user-net 5555、guest-to-host 15555、TAP 10.0.2.2:5555 |

**Deviations from Plan**

- T0 的 fd fault injection 受 sandbox socket policy 限制。internal test 使用 partial-record state injection，不把 socket permission failure当作 workload failure。
- T6 在 guest cross-build 停止。未继续 product regression、preflight 或 T7。

**Blocker Handoff**

- Discovered at: T6 / guest static build / Gate 5
- Expected: 当前源码生成 RISC-V static binary，记录 `file` 和 SHA-256
- Actual: cross compiler 被 sandbox SIGSYS 终止；make 输出 `Bad system call`
- Impact: 现有 guest binary 早于当前源码，不能进入 QEMU
- Completed work: host/local RED→GREEN、protocol 26/26、platform 20/20、tools 19/19、integration 6/6、ASan build/self-test
- Partial work: T0-T5 implementation；formal TCP/UDP path 尚无 guest runtime witness
- Unstarted work: fresh guest artifact、preflight PASS、host-test/axnet/MS01/target full batch、T7 manual QEMU
- Worktree state: 保留用户既有未提交 change；本轮文件列于 Changed Files；生成 binaries 被精确 ignore
- Gates: Gate 3 host baseline PASS；Gate 4 partial；Gate 5 host/local PASS；T6 guest build BLOCKED
- Evidence: EV-004-01 至 EV-004-03
- Plan decision needed: 新 iteration 接收用户侧 fresh guest build Evidence，再完成 T6 和手测交接
- Resume condition: 用户执行下方 guest build handoff，提交 exit 0、`file` 和 SHA-256

**Self-Review**

- Plan compliance: BLOCKED
- Full diff reviewed: BLOCKED
- Critical findings unresolved: 0 on verified host/local scope
- Important findings unresolved: 1
- Minor findings unresolved: 0

Important：正式 network path 未取得 fresh guest build/runtime witness，不能声明可校准。未继续修改或用旧 binary 绕过。

**Verification Evidence**

| Verification | Result | Exit | Evidence |
|---|---|---:|---|
| foundation aggregate | protocol 26、platform 20、tools 19、integration 6 PASS | 0 | EV-004-01 |
| workload aggregate | internal state + integration 6 PASS | 0 | EV-004-01 |
| local matrix | TCP/UDP × TX/RX/bidi × 2-flow PASS | 0 | EV-004-01 |
| ASan/UBSan build/self-test | PASS | 0 | EV-004-01 |
| guest static build | `Bad system call` | nonzero | EV-004-02 |
| OpenSpec strict validation | valid | 0 | EV-004-01 |
| diff whitespace | no output | 0 | EV-004-01 |

**Persisted Evidence**

`../evidence/004-runtime-readiness-and-manual-qemu-calibration/README.md`，EV-004-01 至 EV-004-03。

T7 required files are absent because T6 blocked before the manual boundary.

**Experience Candidates**

None。该 sandbox failure 已在 Plan 中预期，未形成新 Runbook 或 Incident 候选。

**Remaining Issues**

- fresh RISC-V guest binary 缺失。
- T6 full regression 和 preflight 未运行。
- T7 user-net/TAP 手测未开始。

**Commit or Diff Reference**

Base revision `2a9319a946dbe9c07cb0f448d82c0b7c14069015`；工作区未提交。

## Plan Review

- Status: reviewed

**Review Result**

follow-up-required

**Findings**

1. 用户在 sandbox 外完成了 fresh guest build。当前 `tests/network_benchmark` 晚于源码，`file` 确认为 RISC-V static-pie，SHA-256 为 `68a628b0431cfa01a37810d46c9231b7ae29b283910895aaa31a453a52da82d3`。原 ENV BLOCK 的恢复条件已满足。
2. `tests/test_network_benchmark_integration.py` 只覆盖 2-flow happy path。004 要求的 1/4/8-flow、mismatch、EOF、timeout、cancel 和 UDP anomaly permanent witnesses 尚不存在。
3. local loopback 使用内存 simulator，没有覆盖正式 endpoint 的 control lifecycle。它可保留为 deterministic substitute，但不能单独证明正式 socket path。
4. `scripts/network_benchmark_evidence.py` 检查文件和双端 ledger，但不根据原始 NDJSON、CPU 样本重建并核对 `summary.json`。
5. `--calibrate` 在 `instret_status=unavailable` 时仍输出数值 0。该表示会把不可用遥测混同为真实零值。
6. `manual-calibration.md` 的端口表符合 R44/R45/R48，但 Evidence 操作不闭合：guest 文件目录创建顺序错误，TAP 六个阻塞命令未逐对展开，且缺少 `manifest.json`、`README.md`、QEMU 命令、完整串口和 guest console 的明确保存步骤。路径仍指向已阻塞的 004 Evidence。
7. T6 full regression、preflight 和完整 diff Review 尚未执行。

**Deviation Classification**

NEW-EVIDENCE、ACT-DEVIATION

**Evidence**

- Fresh artifact: `file tests/network_benchmark`、`sha256sum tests/network_benchmark`、`stat -c '%y %s %n' tests/network_benchmark.c tests/network_benchmark`
- Code review: `tests/test_network_benchmark_integration.py`、`tests/network_benchmark.c`、`scripts/network_benchmark_evidence.py`
- Manual review: `manual-calibration.md`、R44、R45、R48
- 004 persisted Evidence: `../evidence/004-runtime-readiness-and-manual-qemu-calibration/README.md`

**Follow-up Decision**

保持 004 `blocked`。005 接收 fresh guest artifact，补齐原计划内 permanent witnesses、Evidence 重建、不可用遥测和手册闭合问题，然后运行 T6 全量 Gate。QEMU、guest shell、TAP 和抓包仍由用户手工执行。

**Next Iteration**

`005-runtime-readiness-closure-and-manual-handoff.md`
