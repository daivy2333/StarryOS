# MS16 手工 QEMU 校准

- Policy: R44
- Topology: R45、R48
- Evidence: `evidence/005-runtime-readiness-closure-and-manual-handoff/`
- Boundary: 所有 QEMU、guest shell 和 TAP 操作由用户手工执行。

本文只覆盖 N00-N03 和 user-net/TAP smoke。它不生成 B0 性能结论。

## 构建与目录

Terminal A：

```bash
cd /home/daivy/projects/serial/work/StarryOS
export BENCH_CC=/opt/musl/riscv64-linux-musl-cross/bin/riscv64-linux-musl-gcc
make tests/network_benchmark-host tests/network_benchmark
make LOG=info build
make network-benchmark-calibration-preflight | tee /tmp/ms16-preflight.log

mkdir -p openspec/changes/ms16-qemu-polling-network-performance-baseline/evidence/005-runtime-readiness-closure-and-manual-handoff
sha256sum tests/network_benchmark-host tests/network_benchmark \
  StarryOS_riscv64-qemu-virt.bin make/disk.img
```

后文用以下路径简称 Evidence 目录：

```bash
cd /home/daivy/projects/serial/work/StarryOS
MS16_EVIDENCE="$PWD/openspec/changes/ms16-qemu-polling-network-performance-baseline/evidence/005-runtime-readiness-closure-and-manual-handoff"
test -d "$MS16_EVIDENCE" && printf '%s\n' "$MS16_EVIDENCE"
printf '%s\n' '# MS16 calibration Evidence' > "$MS16_EVIDENCE/README.md"
```

每次启动 QEMU 前，把实际命令追加到 `qemu-command.txt`。完整交互终端输出保存为 `qemu-serial.log`。guest 输入与输出从同一记录复制为 `guest-console.log`。不得只保存成功标记。

## User-net 启动

Terminal A，先启动 HTTP server：

```bash
cd /home/daivy/projects/serial/work/StarryOS/tests
python3 -m http.server 18765 --bind 0.0.0.0
```

Terminal B，启动 user-net QEMU：

先开启终端录制。命令返回新 shell 后，再运行下面的 QEMU 命令。QEMU 退出后输入 `exit` 结束录制。

```bash
script -q -f /tmp/ms16-usernet-terminal.log
```

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
  -object filter-dump,id=ms16user,netdev=net0,file=/tmp/ms16-usernet.pcap \
  -nographic
```

执行前把上述命令原样保存：

```bash
MS16_EVIDENCE="$PWD/openspec/changes/ms16-qemu-polling-network-performance-baseline/evidence/005-runtime-readiness-closure-and-manual-handoff"
printf '%s\n' 'user-net: qemu-system-riscv64 -machine virt -bios default -kernel StarryOS_riscv64-qemu-virt.bin -m 1G -smp 1 -device virtio-blk-device,drive=disk0 -drive id=disk0,if=none,format=raw,file=make/disk.img -device virtio-net-device,netdev=net0 -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 -object filter-dump,id=ms16user,netdev=net0,file=/tmp/ms16-usernet.pcap -nographic' >> "$MS16_EVIDENCE/qemu-command.txt"
```

出现 `starry:~#` 后，Terminal B 每次只输入一条命令：

```sh
wget -q -O /tmp/network_benchmark http://10.0.2.2:18765/network_benchmark
chmod +x /tmp/network_benchmark
sha256sum /tmp/network_benchmark
/tmp/network_benchmark --self-test
```

R44/R45 已验证 `/tmp` payload 路径。不要在 guest 使用 `mkdir -p /root/ms16`；当前 BusyBox 会尝试创建 `/` 并返回 `Invalid argument`。

## N00-N03

N00 使用 preflight 的四个 SHA-256、QEMU 版本、machine、SMP、内存和 `icount=n`。

Terminal B，N01 和 N02：

```sh
/tmp/network_benchmark --calibrate
/tmp/network_benchmark loopback --protocol tcp --direction bidi --flows 2 --profile smoke --duration 1
/tmp/network_benchmark loopback --protocol udp --direction bidi --flows 2 --profile smoke --duration 1
```

N01/N02 原始输出由 `script` 会话保存。不要批量粘贴多行；每条命令结束并再次出现 `starry:~#` 后再输入下一条。

N03 先记录 guest 接口，再从 host 检查路径。Terminal B：

```sh
ip addr show eth0
```

Terminal C：

```bash
ping -c 3 -W 2 10.0.2.15
```

User-net 的 ping 只作诊断。TAP pcap 才是 N03 的 ARP/ICMP 见证。

## User-net RX

RX 表示 host 发送、guest 接收。hostfwd 占用 host 5555，并转发到 guest 5555。

TCP：先在 Terminal B 输入 server，再在 Terminal C 输入 client。

```sh
/tmp/network_benchmark server --side guest --protocol tcp --direction rx --port 5555 --profile smoke --duration 1 --run-id 4001 --test-id 201 --round-id 1
```

```bash
cd /home/daivy/projects/serial/work/StarryOS
./tests/network_benchmark-host client --side host --protocol tcp --direction rx --addr 127.0.0.1:5555 --profile smoke --duration 1 --run-id 4001 --test-id 201 --round-id 1 | tee /tmp/usernet-tcp-rx-host.ndjson
```

UDP：

```sh
/tmp/network_benchmark server --side guest --protocol udp --direction rx --port 5555 --profile smoke --duration 1 --run-id 4001 --test-id 202 --round-id 1
```

```bash
cd /home/daivy/projects/serial/work/StarryOS
./tests/network_benchmark-host client --side host --protocol udp --direction rx --addr 127.0.0.1:5555 --profile smoke --duration 1 --run-id 4001 --test-id 202 --round-id 1 | tee /tmp/usernet-udp-rx-host.ndjson
```

## User-net TX 和 bidi

hostfwd 已绑定 host 5555。guest 出站必须连接 host `10.0.2.2:15555`。

TCP TX：先在 Terminal C 输入 server，再在 Terminal B 输入 client。

```bash
cd /home/daivy/projects/serial/work/StarryOS
./tests/network_benchmark-host server --side host --protocol tcp --direction tx --port 15555 --profile smoke --duration 1 --run-id 4001 --test-id 203 --round-id 1 | tee /tmp/usernet-tcp-tx-host.ndjson
```

```sh
/tmp/network_benchmark client --side guest --protocol tcp --direction tx --addr 10.0.2.2:15555 --profile smoke --duration 1 --run-id 4001 --test-id 203 --round-id 1
```

UDP TX：

```bash
cd /home/daivy/projects/serial/work/StarryOS
./tests/network_benchmark-host server --side host --protocol udp --direction tx --port 15555 --profile smoke --duration 1 --run-id 4001 --test-id 204 --round-id 1 | tee /tmp/usernet-udp-tx-host.ndjson
```

```sh
/tmp/network_benchmark client --side guest --protocol udp --direction tx --addr 10.0.2.2:15555 --profile smoke --duration 1 --run-id 4001 --test-id 204 --round-id 1
```

TCP bidi：

```bash
cd /home/daivy/projects/serial/work/StarryOS
./tests/network_benchmark-host server --side host --protocol tcp --direction bidi --port 15555 --profile smoke --duration 1 --run-id 4001 --test-id 205 --round-id 1 | tee /tmp/usernet-tcp-bidi-host.ndjson
```

```sh
/tmp/network_benchmark client --side guest --protocol tcp --direction bidi --addr 10.0.2.2:15555 --profile smoke --duration 1 --run-id 4001 --test-id 205 --round-id 1
```

UDP bidi：

```bash
cd /home/daivy/projects/serial/work/StarryOS
./tests/network_benchmark-host server --side host --protocol udp --direction bidi --port 15555 --profile smoke --duration 1 --run-id 4001 --test-id 206 --round-id 1 | tee /tmp/usernet-udp-bidi-host.ndjson
```

```sh
/tmp/network_benchmark client --side guest --protocol udp --direction bidi --addr 10.0.2.2:15555 --profile smoke --duration 1 --run-id 4001 --test-id 206 --round-id 1
```

如 StarryOS 报 rebind `Address in use`，等待两秒后重试。重试使用新的 `round-id`，不得覆盖旧输出。

## CPU 和 IRQ 采样

任一 smoke 正在运行时，Terminal D 采集 QEMU、peer 和 collector。先检查 PID 非空且唯一：

```bash
cd /home/daivy/projects/serial/work/StarryOS
QEMU_PID="$(pgrep -n -f qemu-system-riscv64)"
PEER_PID="$(pgrep -n -f '/tests/network_benchmark-host')"
COLLECTOR_PID="$$"
test -n "$QEMU_PID" && test -n "$PEER_PID" && kill -0 "$QEMU_PID" && kill -0 "$PEER_PID"
python3 scripts/network_benchmark_collect.py \
  --pid "$QEMU_PID" "$PEER_PID" "$COLLECTOR_PID" \
  --scope qemu peer collector --interval 0.1 --duration 1.5 \
  | tee /tmp/ms16-host-cpu.ndjson
```

IRQ 前后快照使用 MS03 probe。若 `/root/ms03_irq_probe` 不存在，先按 R48 放入 rootfs。Terminal B：

```sh
/root/ms03_irq_probe idle
/root/ms03_irq_probe idle
```

这两个 idle probe 只校验 snapshot ABI 和 IRQ storm。流量窗口的增量由两次原始快照人工对照。

## User-net 文件保存

退出 QEMU 前保存完整 Terminal B 输出。用 `Ctrl+A`、`X` 退出后执行：

```bash
cd /home/daivy/projects/serial/work/StarryOS
MS16_EVIDENCE="$PWD/openspec/changes/ms16-qemu-polling-network-performance-baseline/evidence/005-runtime-readiness-closure-and-manual-handoff"
cp /tmp/ms16-usernet.pcap "$MS16_EVIDENCE/usernet.pcap"
cp /tmp/usernet-*-host.ndjson "$MS16_EVIDENCE/"
cp /tmp/ms16-host-cpu.ndjson "$MS16_EVIDENCE/host-cpu.ndjson"
```

把手工保存的 Terminal B 完整文本放入以下两个文件：

```bash
cp /tmp/ms16-usernet-terminal.log "$MS16_EVIDENCE/qemu-serial.log"
cp /tmp/ms16-usernet-terminal.log "$MS16_EVIDENCE/guest-console.log"
```

若没有 `/tmp/ms16-usernet-terminal.log`，先从终端保存完整会话。缺少该原始记录时停止，不能用 README 代替。

## TAP 启动

先确认路由不冲突：

```bash
ip route get 10.0.2.2
```

Terminal A：

```bash
sudo ip tuntap add dev tap-ms16 mode tap user "$(id -un)"
sudo ip addr add 10.0.2.2/24 dev tap-ms16
sudo ip link set tap-ms16 up
sudo tcpdump -i tap-ms16 -nn -e -w /tmp/ms16-tap.pcap
```

Terminal B：

先开启 TAP 会话录制。命令返回新 shell 后，再运行下面的 QEMU 命令。QEMU 退出后输入 `exit`。

```bash
script -q -f /tmp/ms16-tap-terminal.log
```

```bash
cd /home/daivy/projects/serial/work/StarryOS
qemu-system-riscv64 \
  -machine virt -bios default \
  -kernel StarryOS_riscv64-qemu-virt.bin \
  -m 1G -smp 1 \
  -device virtio-blk-device,drive=disk0 \
  -drive id=disk0,if=none,format=raw,file=make/disk.img \
  -device virtio-net-device,netdev=net0 \
  -netdev tap,id=net0,ifname=tap-ms16,script=no,downscript=no \
  -nographic
```

启动前记录 TAP 命令：

```bash
MS16_EVIDENCE="$PWD/openspec/changes/ms16-qemu-polling-network-performance-baseline/evidence/005-runtime-readiness-closure-and-manual-handoff"
printf '%s\n' 'tap: qemu-system-riscv64 -machine virt -bios default -kernel StarryOS_riscv64-qemu-virt.bin -m 1G -smp 1 -device virtio-blk-device,drive=disk0 -drive id=disk0,if=none,format=raw,file=make/disk.img -device virtio-net-device,netdev=net0 -netdev tap,id=net0,ifname=tap-ms16,script=no,downscript=no -nographic' >> "$MS16_EVIDENCE/qemu-command.txt"
```

N03 host 检查：

```bash
ping -c 3 -W 2 10.0.2.15
```

pcap 必须含 ARP request/reply 和 ICMP request/reply。

## TAP smoke

TAP guest TX/bidi：host 监听 `10.0.2.2:5555`，guest 连接该地址。不得连接 guest 自身 `10.0.2.15`。

六组逐对执行。每组 listener 或 guest server 会阻塞；启动一端后，只运行紧随其后的匹配端，等双方退出，再开始下一组。顺序为 TCP TX、UDP TX、TCP RX、UDP RX、TCP bidi、UDP bidi。不得一次启动六个阻塞命令。

```bash
./tests/network_benchmark-host server --side host --protocol tcp --direction tx --port 5555 --profile smoke --duration 1 --run-id 4002 --test-id 301 --round-id 1 | tee /tmp/tap-tcp-tx-host.ndjson
./tests/network_benchmark-host server --side host --protocol udp --direction tx --port 5555 --profile smoke --duration 1 --run-id 4002 --test-id 302 --round-id 1 | tee /tmp/tap-udp-tx-host.ndjson
./tests/network_benchmark-host client --side host --protocol tcp --direction rx --addr 10.0.2.15:5555 --profile smoke --duration 1 --run-id 4002 --test-id 303 --round-id 1 | tee /tmp/tap-tcp-rx-host.ndjson
./tests/network_benchmark-host client --side host --protocol udp --direction rx --addr 10.0.2.15:5555 --profile smoke --duration 1 --run-id 4002 --test-id 304 --round-id 1 | tee /tmp/tap-udp-rx-host.ndjson
./tests/network_benchmark-host server --side host --protocol tcp --direction bidi --port 5555 --profile smoke --duration 1 --run-id 4002 --test-id 305 --round-id 1 | tee /tmp/tap-tcp-bidi-host.ndjson
./tests/network_benchmark-host server --side host --protocol udp --direction bidi --port 5555 --profile smoke --duration 1 --run-id 4002 --test-id 306 --round-id 1 | tee /tmp/tap-udp-bidi-host.ndjson
```

对应 Terminal B guest 命令：

```sh
/tmp/network_benchmark client --side guest --protocol tcp --direction tx --addr 10.0.2.2:5555 --profile smoke --duration 1 --run-id 4002 --test-id 301 --round-id 1
/tmp/network_benchmark client --side guest --protocol udp --direction tx --addr 10.0.2.2:5555 --profile smoke --duration 1 --run-id 4002 --test-id 302 --round-id 1
/tmp/network_benchmark server --side guest --protocol tcp --direction rx --port 5555 --profile smoke --duration 1 --run-id 4002 --test-id 303 --round-id 1
/tmp/network_benchmark server --side guest --protocol udp --direction rx --port 5555 --profile smoke --duration 1 --run-id 4002 --test-id 304 --round-id 1
/tmp/network_benchmark client --side guest --protocol tcp --direction bidi --addr 10.0.2.2:5555 --profile smoke --duration 1 --run-id 4002 --test-id 305 --round-id 1
/tmp/network_benchmark client --side guest --protocol udp --direction bidi --addr 10.0.2.2:5555 --profile smoke --duration 1 --run-id 4002 --test-id 306 --round-id 1
```

停止 tcpdump 后复制 pcap，并清理 TAP：

```bash
cd /home/daivy/projects/serial/work/StarryOS
MS16_EVIDENCE="$PWD/openspec/changes/ms16-qemu-polling-network-performance-baseline/evidence/005-runtime-readiness-closure-and-manual-handoff"
cp /tmp/ms16-tap.pcap "$MS16_EVIDENCE/capture.pcap"
sudo ip link delete tap-ms16
```

退出 TAP QEMU 后，追加完整终端记录：

```bash
cat /tmp/ms16-tap-terminal.log >> "$MS16_EVIDENCE/qemu-serial.log"
cat /tmp/ms16-tap-terminal.log >> "$MS16_EVIDENCE/guest-console.log"
```

## 汇总与检查

把 guest 与 host 的每个 NDJSON 文件按执行顺序合并。必须保留失败和补跑 round：

```bash
cd /home/daivy/projects/serial/work/StarryOS
MS16_EVIDENCE="$PWD/openspec/changes/ms16-qemu-polling-network-performance-baseline/evidence/005-runtime-readiness-closure-and-manual-handoff"
export MS16_EVIDENCE
python3 - <<'PY'
import json
import os
from pathlib import Path

base = Path(os.environ["MS16_EVIDENCE"])
source = base / "qemu-serial.log"
with source.open(encoding="utf-8", errors="replace") as src, \
        (base / "guest-netbench.ndjson").open("w", encoding="utf-8") as dst:
    for line in src:
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (record.get("type") in {"manifest", "round"}
                and record.get("side") == "guest"
                and record.get("run_id") in {4001, 4002}):
            dst.write(json.dumps(record, separators=(",", ":")) + "\n")
PY
cat /tmp/usernet-*-host.ndjson /tmp/tap-*-host.ndjson > "$MS16_EVIDENCE/host-netbench.ndjson"
tr -d '\r' < "$MS16_EVIDENCE/guest-console.log" \
  | grep -E '^(READY|PRE|MID|POST|DELTA|PASS|FAIL)( |$)' \
  > "$MS16_EVIDENCE/irq-snapshots.log"
test -s "$MS16_EVIDENCE/guest-netbench.ndjson"
test -s "$MS16_EVIDENCE/irq-snapshots.log"
python3 scripts/network_benchmark_report.py \
  --guest "$MS16_EVIDENCE/guest-netbench.ndjson" \
  --host "$MS16_EVIDENCE/host-netbench.ndjson" \
  --cpu "$MS16_EVIDENCE/host-cpu.ndjson" \
  --output-csv "$MS16_EVIDENCE/results.csv" \
  --output-summary "$MS16_EVIDENCE/summary.json"
BENCHMARK_HASH="$(sha256sum tests/network_benchmark | awk '{print $1}')"
KERNEL_HASH="$(sha256sum StarryOS_riscv64-qemu-virt.bin | awk '{print $1}')"
ROOTFS_HASH="$(sha256sum make/disk.img | awk '{print $1}')"
export MS16_EVIDENCE BENCHMARK_HASH KERNEL_HASH ROOTFS_HASH
python3 - <<'PY'
import hashlib
import json
import os
from pathlib import Path

base = Path(os.environ["MS16_EVIDENCE"])
hashed = [
    "qemu-command.txt", "qemu-serial.log", "guest-console.log",
    "guest-netbench.ndjson", "host-netbench.ndjson", "host-cpu.ndjson",
    "irq-snapshots.log", "capture.pcap", "results.csv", "summary.json",
]
manifest = {
    "schema_version": 1,
    "side": "paired",
    "platform": "qemu-virt-riscv64",
    "driver_mode": "polling",
    "profile": "calibration",
    "protocol": "TCP+UDP",
    "payload_size": 1400,
    "flow_count": 1,
    "duration_s": 1,
    "benchmark_hash": os.environ["BENCHMARK_HASH"],
    "kernel_hash": os.environ["KERNEL_HASH"],
    "rootfs_hash": os.environ["ROOTFS_HASH"],
    "backend": "user+tap",
    "machine": "virt",
    "smp": 1,
    "memory_mb": 1024,
    "icount": "n",
    "file_hashes": {
        name: hashlib.sha256((base / name).read_bytes()).hexdigest()
        for name in hashed
    },
}
(base / "manifest.json").write_text(
    json.dumps(manifest, sort_keys=True, indent=2) + "\n",
    encoding="utf-8",
)
PY
python3 scripts/network_benchmark_evidence.py \
  --dir "$MS16_EVIDENCE" --profile calibration \
  | tee "$MS16_EVIDENCE/evidence-check.json"
```

checker 只有 exit 0 才算校准通过。user-net 结果只算兼容 smoke，TAP 结果只算校准，不算 standard B0。
