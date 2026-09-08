# QEMU guest 经 host 代理访问百度（出公网可达性）

- Status: active
- Last validated: 2026-09-06
- Environment: QEMU RISC-V `virt`；单 hart、单 VirtIO-MMIO NIC；1 GiB；
  user-net（SLIRP）+ hostfwd 5555；`riscv64-linux-musl-gcc`；Python 3.10+；branch `linshi`。
- Source: 本会话端到端实测（guest 串口 + host 侧 12/12 loopback 验证）+ 代码内
  `scripts/baidu_ping_server.py`、`tests/baidu_probe_client.c`。

## 适用范围

在 QEMU user-net（SLIRP）下，用 host 上的 Python server 代表 QEMU guest 访问真实
外网（百度），验证「guest 网卡 → host → 百度」全链路可达（HTTP 200 + ICMP RTT）。
适用于把 async NIC demo 展示为可到公网的应用层验证。

**不适用**/限制：

- **guest 自身发 ICMP 出网**：SLIRP 不转发 guest 对外 ICMP，且当前内核 `socket()`
  syscall（`kernel/src/syscall/net/socket.rs`）未实现 `SOCK_RAW`（`AF_INET, SOCK_RAW`
  → `ESOCKTNOSUPPORT`），故 guest 内 `ping baidu.com` 无法工作。本流程是替代该路径的
  可达性证明，不等价于 guest 内 ICMP。
- SMP、真板 DMA/cache、性能指标；非 ad-hoc 演示（不在任何已归档 change 的
  Acceptance 内）。
- 替换 MS01/MS04/MS05/MS06/MS07 runbook 的网卡数据面验收。

## 前置条件

- 宿主能出公网且可访问百度（ICMP + HTTP）：
  ```bash
  ping -c 1 www.baidu.com
  curl -s -m 8 -o /dev/null -w "%{http_code}\n" https://www.baidu.com   # 期望 200
  ```
- `StarryOS_riscv64-qemu-virt.bin` 与 `make/disk.img` 已生成。
- guest 载荷 RISC-V 静态二进制已构建：
  ```bash
  BENCH_CC=/opt/musl/riscv64-linux-musl-cross/bin/riscv64-linux-musl-gcc \
    make tests/baidu_probe_client
  ```
- 端口空闲：18765（HTTP 注入）、15561（百度代理 server）、5555（QEMU hostfwd）。

## 操作步骤

三个终端（A/B/C）+ QEMU 串口交互。

### 1. 启动 HTTP 注入服务（Terminal A，保持运行）

```bash
cd /home/daivy/projects/serial/work/StarryOS/tests
python3 -m http.server 18765 --bind 0.0.0.0
```

### 2. 启动百度代理 server（Terminal B，保持运行）

```bash
cd /home/daivy/projects/serial/work/StarryOS
python3 scripts/baidu_ping_server.py            # 默认 0.0.0.0:15561
```

应打印 `BAIDU_PING_SERVER_LISTENING 0.0.0.0:15561 target=https://www.baidu.com`。

### 3. 启动 QEMU（Terminal C，录制完整串口）

```bash
cd /home/daivy/projects/serial/work/StarryOS
qemu-system-riscv64 -machine virt -bios default \
  -kernel StarryOS_riscv64-qemu-virt.bin -m 1G -smp 1 \
  -device virtio-blk-device,drive=disk0 \
  -drive id=disk0,if=none,format=raw,file=make/disk.img \
  -device virtio-net-device,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 \
  -nographic
```

等待串口出现 `starry:~#`。退出 QEMU 用 `Ctrl-A X`。

### 4. Guest 下载并运行载荷

在 `starry:~#` 串口输入：

```sh
wget -q -O /tmp/baidu_probe_client http://10.0.2.2:18765/baidu_probe_client
chmod +x /tmp/baidu_probe_client
/tmp/baidu_probe_client
```

可反复运行多遍以验证稳定性。

## 验证

成功判据：

- guest 串口打印：
  ```
  BAIDU_PROBE_CLIENT_CONNECTED server=10.0.2.2:15561
  BAIDU_PROBE_OK http=200 ping_rtt_ms=<RTT> total_ms=<耗时>
  ```
- `BAIDU_PROBE_OK` 的 `http=200` 表示 host 实际访问百度成功；`ping_rtt_ms` 为 host
  ICMP 到百度的 RTT，与宿主直测一致（本会话实测 32–35ms）。
- **稳定性**：连续多次运行应全部 `BAIDU_PROBE_OK`，**不得**出现
  `BAIDU_PROBE_CLIENT_PEER_CLOSED recv errno=107`。

结果分类：

| marker | 含义 |
|---|---|
| `BAIDU_PROBE_OK` | 全链路可达（HTTP 200 + ICMP RTT）→ 通过 |
| `BAIDU_PROBE_ICMP_FAIL http=200` | HTTP 通、ICMP 拿不到 RTT → 部分可达，算通 |
| `BAIDU_PROBE_HTTP_FAIL ping_rtt_ms=…` | ICMP 通、HTTP 失败 → 部分可达，算通 |
| `BAIDU_PROBE_UNREACHABLE` | host 到百度不通 → 检查宿主机出网 |
| `BAIDU_PROBE_CLIENT_CONNECT_FAILED … errno=<n>` | guest 连不上 host 代理 |
| `BAIDU_PROBE_CLIENT_PEER_CLOSED recv errno=107` | 见失败处理（已修复的 server 缺陷） |

## 失败处理

| 症状 | 处理 |
|---|---|
| `wget: Connection refused` | HTTP server 须 `--bind 0.0.0.0` 且在 `tests/` 目录启动；确认 Terminal A 运行 |
| guest connect refused / `CONNECT_FAILED` | 百度代理 server 未启动或端口被占；确认 Terminal B，`ss -ltn` 查 15561 |
| `Addr already in use` 15561/18765 | 上轮进程残留；`pkill -f baidu_ping_server.py` / `pkill -f 'http.server 18765'` 后重试 |
| 间歇 `PEER_CLOSED recv errno=107`（ECONNRESET） | **已知 + 已修复**：server 未读取 guest 请求字节，close 时残留未读数据触发 TCP RST，与 guest `recv` 竞态 → 间歇 ECONNRESET。修法：`serve_once` 先 `conn.recv(64)` drain 请求，写结果后 `conn.shutdown(SHUT_WR)` 再 close。若仍出现，确认运行的是修复后版本并重启 Terminal B。修复后 12/12 loopback、0 RST |
| guest 内 `ping baidu.com` 报 raw socket 不支持 | 非本流程能力：SLIRP 不转发对外 ICMP + 内核无 `SOCK_RAW`。按适用范围改用本代理流程或 TAP 网桥，勿视为网卡 async 缺陷 |

## 回滚

- Guest `/tmp` payload 随 QEMU 退出消失，无需回滚。
- 退出 QEMU `Ctrl-A X`；停止 Terminal A/B 用 `Ctrl-C`。
- 进程残留：`pkill -f qemu-system-riscv64`；`pkill -f baidu_ping_server.py`；
  `pkill -f 'http.server 18765'`。
- 如需清除产物：删除 `scripts/baidu_ping_server.py`、
  `tests/baidu_probe_client{,_test}.c`、`tests/baidu_probe_client` 及 Makefile 追加段。

## 证据

- guest 串口实测（成功）：`BAIDU_PROBE_CLIENT_CONNECTED …` +
  `BAIDU_PROBE_OK http=200 ping_rtt_ms=33.3 total_ms=220.7`。
- host 侧 loopback 验证（修复后）：连续 12 次连接，`OK=12 RST=0/12`。
- 修复前间歇：同调用偶发 `PEER_CLOSED recv errno=107`。
- 适用限制：结论限定于单 hart QEMU VirtIO-MMIO 软件/设备模型；不覆盖 SMP、真板、
  性能或 guest 内原始 ICMP。
- Revision: branch `linshi`，HEAD `6fcc602d`；产物文件为新增（`git status` 见
  `A scripts/baidu_ping_server.py`、`A tests/baidu_probe_client*`），演示提交或已归档
  状态视当时仓库现场而定。