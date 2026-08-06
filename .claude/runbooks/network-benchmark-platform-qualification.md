# 网卡基准平台资格扫描

- Status: active
- Last validated: 2026-08-05
- Last procedure review: 2026-08-06
- Environment: QEMU 7.0.0、`virt`、VirtIO-MMIO、1 hart、1 GiB、user-net、轮询驱动
- Source: [MS16 analysis](../analysis/starryos-virtio-mmio-network-benchmark-baseline.md)
- Runtime source: [EV-005-07](../../openspec/changes/archive/2026-08-06-ms16-qemu-polling-network-performance-baseline/evidence/005-runtime-readiness-closure-and-manual-handoff/qemu-usernet-smoke-attempt-1.md)
- Change source: [iteration 005](../../openspec/changes/archive/2026-08-06-ms16-qemu-polling-network-performance-baseline/iterations/005-runtime-readiness-closure-and-manual-handoff.md)

## 适用范围

本 Runbook 判断一个网卡环境和驱动实现能否运行统一测试。QEMU、真板属于环境；polling、async 属于被比较的 driver treatment。协议、方向、payload、flow 和指标才是测试项目。

资格扫描允许得到 invalid round。只要握手、传输、采集和失败分类完成，该测试方向便可进入后续修复。invalid round 不能生成性能结论。

以下步骤尚未完成 MS16 运行验证：TAP 六方向、真板、standard B0、多流、payload 阶梯、UDP pacing 和长期稳定性。RTT、精确 burst、背压指标和部分机制遥测尚无完整测试基础设施。两类缺口不能混记。

## 测试目录

测试 ID、公式和完成点由 [R47 analysis](../analysis/starryos-virtio-mmio-network-benchmark-baseline.md) 定义。换平台或驱动时保留这些语义。

| 组 | 项目 | 用途 |
|---|---|---|
| N00-N03 | manifest、时钟、loopback、路径 | 校准环境和上层对照 |
| N10-N14 | TCP 单向、写尺寸、双向、多流、稳态 | 吞吐、批量和公平性 |
| N20-N24 | TCP RTT、UDP 吞吐、RTT、burst、负载延迟 | 延迟、抖动和丢包 |
| N30-N35 | 背压、队列、连接、缓冲、复制 | 边界与恢复成本 |
| N40-N46 | idle、CPU、IRQ、调度、内存、唤醒、descriptor | 资源和机制效率 |
| N50-N54 | 损伤、稳定、过载、SMP、真板机制 | 扩展与平台特性 |

每个平台至少支持以下 smoke 矩阵：

```text
protocol:  TCP, UDP
direction: TX, RX, BIDI
flows:     1
payload:   1400 B
duration:  1 s
seed:      12345
```

通过 smoke 后再扩展到 2、4、8 flows，以及 quick、standard、soak/board profile。

## 不变量

- `send()` 返回只代表 C1 enqueue。
- goodput 使用 C6 receiver 校验字节。
- RTT 只在同一时钟域测量。
- 缺失 capability 写 `unavailable`，不写零。
- guest、host 必须使用同一 benchmark hash。
- 无效 round 原样保留。补跑使用新 round ID。
- QEMU user-net 只做兼容 smoke。
- QEMU TAP 才能生成 QEMU 性能基线。
- QEMU 与真板建立不同基线。
- A/B 只允许 `treatment` 不同。
- QEMU 和 guest shell 按 [R44](qemu-network-testing.md) 手工操作。

## 前置条件

Host 需要 C11 编译器、RISC-V musl 交叉编译器、Python 3 和 QEMU。TAP 还需要 `ip`、`tcpdump` 与 sudo。

产品与工具入口：

- [workload](../../tests/network_benchmark.c)
- [protocol](../../tests/network_benchmark_protocol.h)
- [platform adapter](../../tests/network_benchmark_platform.h)
- [collector](../../scripts/network_benchmark_collect.py)
- [report](../../scripts/network_benchmark_report.py)
- [evidence checker](../../scripts/network_benchmark_evidence.py)

构建并固定 hash：

```bash
cd /home/daivy/projects/serial/work/StarryOS
export BENCH_CC=/opt/musl/riscv64-linux-musl-cross/bin/riscv64-linux-musl-gcc
make -B tests/network_benchmark tests/network_benchmark-host
file tests/network_benchmark tests/network_benchmark-host
sha256sum tests/network_benchmark tests/network_benchmark-host \
  StarryOS_riscv64-qemu-virt.bin make/disk.img
```

`tests/network_benchmark` 必须是 RISC-V static-pie。每次运行保存以下事实：

- source revision 与 dirty 状态；
- toolchain 与 QEMU；
- kernel、rootfs 和 workload hash。

## 资格扫描顺序

按依赖顺序执行。后一级不能替代前一级。

| Gate | 操作 | 通过或分类条件 |
|---|---|---|
| P0 | build、self-test、hash | 工具可执行，双端 hash 一致 |
| P1 | calibration、TCP/UDP loopback | 时钟可用，协议路径输出记录 |
| P2 | boot、ARP/ICMP、设备签名 | 平台网络路径可定位 |
| P3 | TCP/UDP × TX/RX/BIDI | 六方向产生 manifest 和 round |
| P4 | CPU、instret、IRQ、pcap | capability 有记录或明确 unavailable |
| P5 | C6 账本和异常分类 | valid 或可诊断 invalid |
| P6 | quick/standard | 只消费 valid round |
| P7 | A/B compare | comparison key 仅 treatment 不同 |

P0-P3 完成表示 workload 可在该平台执行。P4-P5 完成表示测量设施就绪。只有 P5 valid 后才能进入性能统计。

## QEMU user-net 操作

已验证拓扑：host 入站使用 hostfwd 5555；guest 出站连接 host `10.0.2.2:15555`。HTTP server 必须在 `tests/` 目录监听 `0.0.0.0:18765`。

Host Terminal A：

```bash
cd /home/daivy/projects/serial/work/StarryOS/tests
python3 -m http.server 18765 --bind 0.0.0.0
```

Host Terminal B：

```bash
cd /home/daivy/projects/serial/work/StarryOS
qemu-system-riscv64 \
  -machine virt -bios default \
  -kernel StarryOS_riscv64-qemu-virt.bin \
  -m 1G -smp 1 \
  -device virtio-blk-device,drive=disk0 \
  -drive id=disk0,if=none,format=raw,file=make/disk.img \
  -device virtio-net-device,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 \
  -object filter-dump,id=netbench,netdev=net0,file=/tmp/network-usernet.pcap \
  -nographic
```

Guest `starry:~#`，每次只输入一条：

```sh
wget -q -O /tmp/network_benchmark http://10.0.2.2:18765/network_benchmark
chmod +x /tmp/network_benchmark
sha256sum /tmp/network_benchmark
/tmp/network_benchmark --self-test
/tmp/network_benchmark --calibrate
```

不要使用 `/root/ms16`。本次环境只能可靠写入 `/tmp`。

## QEMU TAP 待执行操作

本节命令已按 R44、R45 和现有 benchmark CLI 核对，但 MS16 workload 尚未在 TAP 上端到端执行。执行后必须以新 Evidence 更新验证状态。

OS/bash Terminal 1 启动 HTTP server：

```bash
cd /home/daivy/projects/serial/work/StarryOS/tests
python3 -m http.server 18765 --bind 0.0.0.0
```

OS/bash Terminal 2 创建 TAP 并抓包。`ip link show tap-ms16` 已存在时不得重复创建：

```bash
cd /home/daivy/projects/serial/work/StarryOS
EVIDENCE=/tmp/ms16-qemu-tap-qualification
mkdir -p "$EVIDENCE"
ip link show tap-ms16
sudo ip tuntap add dev tap-ms16 mode tap user "$(id -un)"
sudo ip addr add 10.0.2.2/24 dev tap-ms16
sudo ip link set tap-ms16 up
ip addr show tap-ms16
sudo tcpdump -i tap-ms16 -nn -e -w "$EVIDENCE/capture.pcap"
```

OS/bash Terminal 3 启动 QEMU：

```bash
cd /home/daivy/projects/serial/work/StarryOS
qemu-system-riscv64 -machine virt -bios default -kernel StarryOS_riscv64-qemu-virt.bin -m 1G -smp 1 -device virtio-blk-device,drive=disk0 -drive id=disk0,if=none,format=raw,file=make/disk.img -device virtio-net-device,netdev=net0 -netdev tap,id=net0,ifname=tap-ms16,script=no,downscript=no -nographic
```

TAP 不使用 hostfwd。Host 是 `10.0.2.2`，guest 是 `10.0.2.15`，benchmark 端口统一为 5555。

StarryOS/sh 出现提示符后逐条执行：

```sh
wget -q -O /tmp/network_benchmark http://10.0.2.2:18765/network_benchmark
chmod +x /tmp/network_benchmark
sha256sum /tmp/network_benchmark
/tmp/network_benchmark --self-test
/tmp/network_benchmark --calibrate
```

OS/bash Terminal 4 验证路径：

```bash
ping -c 3 -W 2 10.0.2.15
```

StarryOS 当前不支持 AF_NETLINK。不要用 guest `ip addr` 代替 boot IP、host ping 和 pcap 见证。

## 六方向命令模板

每个 server 启动后，在 10 秒内启动 client。两个 endpoint 的 protocol、direction、run/test/round 必须一致。

RX 使用 guest server 和 host client：

```sh
# StarryOS/sh
PROTO=udp RUN_ID=4001 TEST_ID=202 ROUND_ID=1
/tmp/network_benchmark server --side guest --protocol "$PROTO" \
  --direction rx --port 5555 --profile smoke --duration 1 \
  --run-id "$RUN_ID" --test-id "$TEST_ID" --round-id "$ROUND_ID"
```

```bash
# OS/bash
PROTO=udp RUN_ID=4001 TEST_ID=202 ROUND_ID=1
./tests/network_benchmark-host client --side host --protocol "$PROTO" \
  --direction rx --addr 127.0.0.1:5555 --profile smoke --duration 1 \
  --run-id "$RUN_ID" --test-id "$TEST_ID" --round-id "$ROUND_ID" \
  | tee "/tmp/usernet-${PROTO}-rx-host-r${ROUND_ID}.ndjson"
```

TX 与 BIDI 使用 host server 和 guest client：

```bash
# OS/bash
PROTO=tcp DIRECTION=tx RUN_ID=4001 TEST_ID=203 ROUND_ID=1
./tests/network_benchmark-host server --side host --protocol "$PROTO" \
  --direction "$DIRECTION" --port 15555 --profile smoke --duration 1 \
  --run-id "$RUN_ID" --test-id "$TEST_ID" --round-id "$ROUND_ID" \
  | tee "/tmp/usernet-${PROTO}-${DIRECTION}-host-r${ROUND_ID}.ndjson"
```

```sh
# StarryOS/sh
PROTO=tcp DIRECTION=tx RUN_ID=4001 TEST_ID=203 ROUND_ID=1
/tmp/network_benchmark client --side guest --protocol "$PROTO" \
  --direction "$DIRECTION" --addr 10.0.2.2:15555 \
  --profile smoke --duration 1 --run-id "$RUN_ID" \
  --test-id "$TEST_ID" --round-id "$ROUND_ID"
```

变量只在对应 shell 设置。guest 和 host 不共享 shell 状态。

| test_id | PROTO | DIRECTION |
|---:|---|---|
| 201 | tcp | rx |
| 202 | udp | rx |
| 203 | tcp | tx |
| 204 | udp | tx |
| 205 | tcp | bidi |
| 206 | udp | bidi |

每组结束后等待 2 秒。rebind 失败时使用新 round ID，不能覆盖旧输出。

## TAP 参数化命令

每轮先在两端分别设置同一组变量。RX 先启动 StarryOS server，再启动 OS client：

```sh
# StarryOS/sh
PROTO=tcp; DIRECTION=rx; FLOWS=1; PAYLOAD=1400; PROFILE=smoke; DURATION=1; WARMUP=0; LOAD=0; RUN_ID=4200; TEST_ID=301; ROUND_ID=1
/tmp/network_benchmark server --side guest --protocol "$PROTO" --direction "$DIRECTION" --flows "$FLOWS" --payload "$PAYLOAD" --port 5555 --profile "$PROFILE" --duration "$DURATION" --warmup "$WARMUP" --offered-load "$LOAD" --run-id "$RUN_ID" --test-id "$TEST_ID" --round-id "$ROUND_ID"
GUEST_EXIT=$?
printf 'guest_exit=%s\n' "$GUEST_EXIT"
```

```bash
# OS/bash
cd /home/daivy/projects/serial/work/StarryOS
PROTO=tcp; DIRECTION=rx; FLOWS=1; PAYLOAD=1400; PROFILE=smoke; DURATION=1; WARMUP=0; LOAD=0; RUN_ID=4200; TEST_ID=301; ROUND_ID=1
./tests/network_benchmark-host client --side host --protocol "$PROTO" --direction "$DIRECTION" --flows "$FLOWS" --payload "$PAYLOAD" --addr 10.0.2.15:5555 --profile "$PROFILE" --duration "$DURATION" --warmup "$WARMUP" --offered-load "$LOAD" --run-id "$RUN_ID" --test-id "$TEST_ID" --round-id "$ROUND_ID" | tee "/tmp/tap-${TEST_ID}-host-r${ROUND_ID}.ndjson"
HOST_EXIT=${PIPESTATUS[0]}
printf 'host_exit=%s\n' "$HOST_EXIT"
```

TX 和 BIDI 先启动 OS server，再启动 StarryOS client：

```bash
# OS/bash
cd /home/daivy/projects/serial/work/StarryOS
PROTO=tcp; DIRECTION=tx; FLOWS=1; PAYLOAD=1400; PROFILE=smoke; DURATION=1; WARMUP=0; LOAD=0; RUN_ID=4200; TEST_ID=303; ROUND_ID=1
./tests/network_benchmark-host server --side host --protocol "$PROTO" --direction "$DIRECTION" --flows "$FLOWS" --payload "$PAYLOAD" --port 5555 --profile "$PROFILE" --duration "$DURATION" --warmup "$WARMUP" --offered-load "$LOAD" --run-id "$RUN_ID" --test-id "$TEST_ID" --round-id "$ROUND_ID" | tee "/tmp/tap-${TEST_ID}-host-r${ROUND_ID}.ndjson"
HOST_EXIT=${PIPESTATUS[0]}
printf 'host_exit=%s\n' "$HOST_EXIT"
```

```sh
# StarryOS/sh
PROTO=tcp; DIRECTION=tx; FLOWS=1; PAYLOAD=1400; PROFILE=smoke; DURATION=1; WARMUP=0; LOAD=0; RUN_ID=4200; TEST_ID=303; ROUND_ID=1
/tmp/network_benchmark client --side guest --protocol "$PROTO" --direction "$DIRECTION" --flows "$FLOWS" --payload "$PAYLOAD" --addr 10.0.2.2:5555 --profile "$PROFILE" --duration "$DURATION" --warmup "$WARMUP" --offered-load "$LOAD" --run-id "$RUN_ID" --test-id "$TEST_ID" --round-id "$ROUND_ID"
GUEST_EXIT=$?
printf 'guest_exit=%s\n' "$GUEST_EXIT"
```

server 启动后 10 秒内启动 client。变量必须在两端各自设置。补跑增加 `ROUND_ID`。

### 待执行矩阵

TAP 六方向 smoke：

| test_id | protocol | direction | flows | payload | profile | duration | warm-up | load |
|---:|---|---|---:|---:|---|---:|---:|---:|
| 301 | tcp | rx | 1 | 1400 | smoke | 1 | 0 | 0 |
| 302 | udp | rx | 1 | 1400 | smoke | 1 | 0 | 0 |
| 303 | tcp | tx | 1 | 1400 | smoke | 1 | 0 | 0 |
| 304 | udp | tx | 1 | 1400 | smoke | 1 | 0 | 0 |
| 305 | tcp | bidi | 1 | 1400 | smoke | 1 | 0 | 0 |
| 306 | udp | bidi | 1 | 1400 | smoke | 1 | 0 | 0 |

多流使用 1400 B、smoke、1 秒、warm-up 0、load 0：

| protocol | direction | flows → test_id |
|---|---|---|
| tcp | tx | 2→1302，4→1304，8→1308 |
| tcp | rx | 2→1322，4→1324，8→1328 |
| tcp | bidi | 2→1342，4→1344，8→1348 |
| udp | tx | 2→2302，4→2304，8→2308 |
| udp | rx | 2→2322，4→2324，8→2328 |
| udp | bidi | 2→2342，4→2344，8→2348 |

payload 阶梯使用 1 flow、smoke、1 秒、warm-up 0、load 0：

| protocol | direction | payload → test_id |
|---|---|---|
| tcp | tx | 1→1101，64→1102，256→1103，512→1104，1024→1105，1460→1106，2012→1107 |
| tcp | rx | 1→1121，64→1122，256→1123，512→1124，1024→1125，1460→1126，2012→1127 |
| udp | tx | 1→2101，64→2102，256→2103，512→2104，1024→2105，1400→2106，1436→2107 |
| udp | rx | 1→2121，64→2122，256→2123，512→2124，1024→2125，1400→2126，1436→2127 |

profile 阶梯使用 1 flow、1400 B、load 0：

| test_id | protocol | direction | profile | duration | warm-up |
|---:|---|---|---|---:|---:|
| 1401 | tcp | tx | quick | 5 | 1 |
| 1402 | udp | tx | quick | 5 | 1 |
| 1403 | tcp | tx | standard | 10 | 2 |
| 1404 | udp | tx | standard | 10 | 2 |
| 1405 | tcp | rx | standard | 10 | 2 |
| 1406 | udp | rx | standard | 10 | 2 |
| 1407 | tcp | bidi | standard | 10 | 2 |
| 1408 | udp | bidi | standard | 10 | 2 |

UDP pacing 探针使用 1 flow、1400 B、smoke、2 秒：

| direction | offered load → test_id |
|---|---|
| tx | 25→2151，50→2152，75→2153，90→2154，100→2155 |
| rx | 25→2171，50→2172，75→2173，90→2174，100→2175 |

当前 `--offered-load` 相对程序内 1 Gbit/s 名义速率，不相对同环境 pilot。该组只验证 pacing 参数和失败分类，不能完成 N21/N24 的正式口径。

## 可观测性

CPU collector 同时采样 QEMU、peer 和 collector。采样必须覆盖流量窗口。

```bash
QEMU_PID="$(pgrep -n -f qemu-system-riscv64)"
PEER_PID="$(pgrep -n -f '/tests/network_benchmark-host')"
COLLECTOR_PID="$$"
test -n "$QEMU_PID" && test -n "$PEER_PID"
kill -0 "$QEMU_PID" && kill -0 "$PEER_PID"
python3 scripts/network_benchmark_collect.py \
  --pid "$QEMU_PID" "$PEER_PID" "$COLLECTOR_PID" \
  --scope qemu peer collector --interval 0.1 --duration 1.5 \
  | tee /tmp/network-host-cpu.ndjson
```

`collector_exit=0` 只证明采集管线可用。peer 全程 0 tick 表示采样未覆盖负载，不能计算 CPU/GiB。

在 10 秒 standard round 上把采样扩为 12 秒。先启动 host server，再启动 collector，最后启动 guest client：

```bash
cd /home/daivy/projects/serial/work/StarryOS
QEMU_PID="$(pgrep -n -f qemu-system-riscv64)"
PEER_PID="$(pgrep -n -f 'tests/network_benchmark-host server')"
COLLECTOR_PID="$$"
test -n "$QEMU_PID" && test -n "$PEER_PID"
kill -0 "$QEMU_PID" && kill -0 "$PEER_PID"
python3 scripts/network_benchmark_collect.py --pid "$QEMU_PID" "$PEER_PID" "$COLLECTOR_PID" --scope qemu peer collector --interval 0.1 --duration 12 | tee /tmp/tap-1403-host-cpu.ndjson
CPU_EXIT=${PIPESTATUS[0]}
printf 'cpu_collector_exit=%s\n' "$CPU_EXIT"
```

有效窗口要求 QEMU 和 peer 样本覆盖流量，且 peer 的 user 或 system tick 发生变化。

N40 无 benchmark socket 对照：

```bash
cd /home/daivy/projects/serial/work/StarryOS
QEMU_PID="$(pgrep -n -f qemu-system-riscv64)"
COLLECTOR_PID="$$"
python3 scripts/network_benchmark_collect.py --pid "$QEMU_PID" "$COLLECTOR_PID" --scope qemu collector --interval 0.5 --duration 30 | tee /tmp/n40-no-socket-cpu.ndjson
printf 'n40_no_socket_exit=%s\n' "${PIPESTATUS[0]}"
```

N40 idle socket 对照先在 StarryOS/sh 启动监听：

```sh
nc -l -p 5555 >/tmp/n40-idle-socket.log 2>&1 &
echo "idle_socket_pid=$!"
```

再在 OS/bash 采样：

```bash
cd /home/daivy/projects/serial/work/StarryOS
QEMU_PID="$(pgrep -n -f qemu-system-riscv64)"
COLLECTOR_PID="$$"
python3 scripts/network_benchmark_collect.py --pid "$QEMU_PID" "$COLLECTOR_PID" --scope qemu collector --interval 0.5 --duration 30 | tee /tmp/n40-idle-socket-cpu.ndjson
printf 'n40_idle_socket_exit=%s\n' "${PIPESTATUS[0]}"
```

结束时只 kill 上一步打印的明确 PID。

IRQ 使用 [R48](ms03-virtio-mmio-irq-evidence.md) probe：

```sh
test -x /root/ms03_irq_probe
/root/ms03_irq_probe idle
```

idle 成功判据是 `PASS idle` 且 total delta 不超过 100。流量 IRQ 效率需要负载前后 snapshot，不能用 idle 结果替代。

## 平台适配

公共 workload、测试 ID、payload、seed、完成点和 Evidence 字段保持不变。平台只替换下表内容。

| 平台 | peer 与网络 | CPU/指令 | 机制证据 |
|---|---|---|---|
| guest loopback | 单进程双 endpoint | guest instret | 无设备结论 |
| QEMU user-net | hostfwd 5555；出站 15555 | QEMU PID、guest instret | 兼容 smoke |
| QEMU TAP | host `10.0.2.2`、guest `10.0.2.15` | QEMU PID、guest instret | TAP pcap、IRQ snapshot |
| 真板 | 独立外部 peer | hart cycle/instret | DMA、cache、PLIC、PHY |
| 异步驱动 | 与同平台 polling 相同 | 同一采集方式 | IRQ、wake、task、queue |

TAP 操作沿用 [R45](ms02-virtio-mmio-evidence.md)。MS16 TAP 六方向尚未验证，不能产生 B0。

真板先按 [R40](board-bringup-ladder.md) 完成启动、MMIO、IRQ、网络和外部 peer 阶梯。QEMU 与真板不比较绝对吞吐或 RTT。

## 覆盖状态与缺口分类

环境、treatment 和测试项目分别记录：

```text
environment: qemu-usernet | qemu-tap | board
treatment:   polling | async
test:        N00-N54 + protocol/direction/payload/flow/profile
```

### 基础设施已支持，本 change 未执行

这些项目已有 CLI 或外部采集入口。本 change 没有取得对应运行 Evidence：

| 项目 | 已有入口 | 本 change 状态 |
|---|---|---|
| TAP TCP/UDP 六方向 | server/client、TAP、pcap | 未执行 |
| TCP/UDP 2/4/8 flows | `--flows` | 未执行 |
| TCP payload 1-2012 B | `--payload` | 未执行 |
| UDP payload 1-1436 B | `--payload` | 未执行 |
| quick/standard 时长 | `--profile`、`--duration`、`--warmup` | 未执行 |
| UDP 名义速率 pacing | `--offered-load` | 未执行；不满足 pilot-relative 口径 |
| N40 host CPU/RSS | host collector、idle listener | collector 已校准；两组对照未执行 |
| TAP pcap | `tcpdump` | 未执行 |
| N50 netem | Linux TAP/netem | MS16 明确不执行 |
| N51 固定时长运行 | `--duration 300` | MS16 明确不执行；资源稳定指标不完整 |
| 真板同协议运行 | portable C workload | 环境适配与运行均未执行 |

“支持”只表示命令可表达。没有运行 Evidence 时不得标记 execution、correctness 或 performance PASS。

### 基础设施不足，当前无法按设计测试

| 项目 | 缺失能力 |
|---|---|
| N11 4096/16384/65536 B | TCP payload 上限为 2012 B |
| N20 TCP RTT | 无 RTT request/reply 模式和原始样本 |
| N22 UDP RTT/间隔误差 | 无匹配 reply、发送计划和接收间隔样本 |
| N23 exact burst | 无精确 datagram count 参数 |
| N24 负载下延迟 | 无并行 RTT 流；offered load 不是 pilot-relative |
| N30 背压恢复 | 有 EAGAIN 状态处理，无 fill-to-EAGAIN 模式和等待/恢复指标 |
| N31/N34 边界 | 无 packet count、socket buffer、ARP 或 metadata 容量控制 |
| N32 connect churn | 无连接次数、并发度和 churn 结果 |
| N33 单流公平性 | 可运行多流，只输出聚合账本 |
| N35 copy/allocation | 无 copy byte 和 allocation counter |
| N41 guest inst/byte | calibration 可读 instret，workload round 未集成 delta |
| N42 benchmark IRQ/packet | MS03 probe 可自测，benchmark round 未集成前后 snapshot |
| N43 timer interference | 无 wake overshoot 原始样本 |
| N44-N46 | 无 allocator、wake 和 descriptor 内部遥测 |
| N52 overload recovery | 单个 round 不能动态改变 offered load |

这类结果记为 `infrastructure-unavailable`，不能记为网卡失败或“未跑”。新增支持需要独立 change，不属于重复执行命令。

## 验证

资格扫描按三层判定：

| 层 | PASS | FAIL |
|---|---|---|
| 执行 | 双端启动并输出 manifest/round | hang、crash、无法建连 |
| 正确性 | fingerprint 一致，C6 账本闭合 | invalid、异常计数或账本不符 |
| 性能 | valid round、采样覆盖负载 | invalid round 或 capability 缺失 |

2026-08-05 的 QEMU user-net 结果：

| 场景 | 执行 | 正确性 | 证据 |
|---|---|---|---|
| TCP RX | PASS | invalid partial | host 9964 TX；guest 7788 RX |
| UDP RX | PASS | invalid；pending buffer full | host 60822 TX；guest 27 late |
| TCP TX | PASS | valid | 双端 4702 packets、6582800 B |
| UDP TX | PASS | invalid | guest 4819 TX；host 4812 RX、7 late |
| TCP BIDI | PASS | invalid | 一方向闭合，反向未闭合 |
| UDP BIDI | PASS | invalid | 双向流量存在，账本未闭合 |
| IRQ idle | PASS | PASS | delta 0、2000 ms |
| CPU collector | PASS | window invalid | 15 samples；peer 0 tick |

这些 invalid 结果不能进入吞吐或 CPU 对比。它们证明六方向命令、协议记录和失败分类可运行。

## 失败处理

| 症状 | 分类与处理 |
|---|---|
| hash 不一致 | 停止；重新分发同一 artifact |
| HTTP 404 | server 必须从 `tests/` 启动 |
| `/root/ms16` 创建失败 | 使用 `/tmp/network_benchmark` |
| `ip addr` 报 AF_NETLINK unsupported | 使用 boot IP 和 host ping 见证 |
| `Address in use` | 等待 2 秒；新 round ID 重跑 |
| `invalid_reason=4` | 保留双端账本；归类 workload 收口或 partial |
| pending packet buffer full | 保留 warning、pcap 和 UDP 序号；不可生成性能结果 |
| UDP late 非零 | 保留 offered/accepted/received；不可折叠为 loss |
| peer CPU 全程 0 tick | 采样窗口无效；重新同步 collector 与 workload |
| guest hang 或网络失联 | 停止后续矩阵，保存完整串口日志 |

先记录最早失败层：console、artifact、topology、control、data、timer、ledger、IRQ 或 collector。不要因为后层失败改写前层 PASS。

## Evidence

正式目录至少包含：

```text
README.md
manifest.json
qemu-command.txt
qemu-serial.log
guest-console.log
guest-netbench.ndjson
host-netbench.ndjson
host-cpu.ndjson
irq-snapshots.log
capture.pcap
results.csv
summary.json
evidence-check.json
```

原始串口是 guest NDJSON 的来源。guest `/tmp` 文件会随 QEMU 退出消失，不能依赖 rootfs 提取。

离线报告与 checker：

```bash
python3 scripts/network_benchmark_report.py \
  --guest "$EVIDENCE/guest-netbench.ndjson" \
  --host "$EVIDENCE/host-netbench.ndjson" \
  --cpu "$EVIDENCE/host-cpu.ndjson" \
  --output-csv "$EVIDENCE/results.csv" \
  --output-summary "$EVIDENCE/summary.json"
python3 scripts/network_benchmark_evidence.py \
  --dir "$EVIDENCE" --profile calibration \
  | tee "$EVIDENCE/evidence-check.json"
```

checker exit 0 才表示该 profile 具备比较资格。user-net 数据不替代 TAP calibration 或 standard B0。

来源限制：

- EV-005-07 raw log 是 2026-08-05 手工记录；
- revision 为 `2a9319a946dbe9c07cb0f448d82c0b7c14069015`；
- worktree 非干净；
- guest artifact SHA-256 为 `b863b060500c3a0977102e840d2d7160d75d7ea899567ab38cc72891ad5f1eb3`。

## 回滚

guest payload 位于 `/tmp`，退出 QEMU 后自动丢失。user-net 无 host 网络配置需要回滚。

TAP 测试结束后停止 tcpdump，再删除明确设备：

```bash
ip link show tap-ms16
sudo ip link delete tap-ms16
```

删除失败时停止，不改用更宽泛的清理命令。原始日志和 invalid round 不回滚、不覆盖。
