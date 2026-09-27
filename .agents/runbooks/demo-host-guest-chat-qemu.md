# Demo Host–Guest Chat QEMU 手测

- Status: active
- Last validated: 2026-08-22（host 侧自动化 Gate；QEMU 手测段由操作者手动执行）
- Environment: QEMU RISC-V `virt`；单 hart、单 VirtIO-MMIO NIC；1 GiB；user-net + hostfwd
  5555；`riscv64-linux-musl-gcc`；Python 3.10；branch `linshi`。
- Source: `demo-host-guest-chat` Iteration 000 / Cycle 000 Task 3.2（Task Contract 与
  Invariants）+ design.md；host 侧命令已在同 Cycle Act 中运行 GREEN。

## 适用范围

验证 host 聊天服务器与 QEMU guest 静态客户端之间的逐行双向 TCP 聊天：连接提示、
双向多行交换、空行丢弃、`/quit` 关闭、对端断开提示。只证明单 hart QEMU VirtIO-MMIO
数据面；不覆盖 SMP、真板、性能或 fd readiness。

**不适用**：

- 逐字符聊天、多客户端、TLS、自动重连（Non-goal）。
- 修改产品代码；改动既有 Makefile target、端口或 QEMU 参数。
- 替代 MS01/MS04/MS05 runbook 的网卡数据面验收。

## 前置条件

- `StarryOS_riscv64-qemu-virt.bin` 与 `make/disk.img` 已生成（MS05 产物即可；本 change
  不触碰产品代码，无需为 QEMU 手测重建内核）。
- `tests/demo_chat_client` RISC-V 静态二进制已构建：

  ```bash
  make tests/demo_chat_client
  ```

- host 侧自动化 Gate 已全绿（操作者可在手测前快速复核）：

  ```bash
  python3 scripts/demo_chat_server_test.py
  cc -std=c11 -Wall -Wextra -Werror tests/demo_chat_client_test.c \
    -o /tmp/demo-chat-client-test && /tmp/demo-chat-client-test
  cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/demo_chat_client.c
  ```

- 端口 15560（聊天服务器）与 18765（HTTP 注入）空闲；QEMU hostfwd 5555 不被占用。

## 操作步骤

三个终端（A/B/C）+ QEMU 串口交互。

### 1. 启动 HTTP 注入服务（Terminal A，保持运行）

```bash
cd /home/daivy/projects/serial/work/StarryOS/tests
python3 -m http.server 18765 --bind 0.0.0.0
```

### 2. 启动 host 聊天服务器（Terminal B，保持运行）

```bash
cd /home/daivy/projects/serial/work/StarryOS
python3 scripts/demo_chat_server.py            # 默认 0.0.0.0:15560
```

连接建立后该终端即是 host 侧的"聊天输入窗口"。

### 3. 启动 QEMU（Terminal C，录制完整串口）

```bash
cd /home/daivy/projects/serial/work/StarryOS
script -q -f /tmp/demo-chat-qemu-serial.log -c \
'qemu-system-riscv64 -machine virt -bios default \
 -kernel StarryOS_riscv64-qemu-virt.bin -m 1G -smp 1 \
 -device virtio-blk-device,drive=disk0 \
 -drive id=disk0,if=none,format=raw,file=make/disk.img \
 -device virtio-net-device,netdev=net0 \
 -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 \
 -nographic'
```

等待串口出现 `starry:~#`。退出 QEMU 用 `Ctrl-A X`。

### 4. Guest 下载并运行客户端

在 `starry:~#` 串口输入：

```sh
wget -q -O /tmp/demo_chat_client http://10.0.2.2:18765/demo_chat_client
chmod +x /tmp/demo_chat_client
/tmp/demo_chat_client
```

成功时串口打印 `DEMO_CHAT_CLIENT_CONNECTED server=10.0.2.2:15560`，host 聊天
终端打印 `DEMO_CHAT_SERVER_CONNECTED peer=10.0.2.2:<port>`。

### 5. 逐场景演示

| 场景 | 操作 | 预期 |
|---|---|---|
| S1 双向聊天 | guest 串口输入一行文本；host 聊天终端输入一行文本 | 两端各自显示对方的行 |
| S3a 空行丢弃 | 任一端输入一个空行 | 空行不转发、对端不显示 |
| S3b `/quit`（guest 主动） | guest 串口输入 `/quit` | guest 打印 `DEMO_CHAT_CLIENT_QUIT` 并退出；host 打印 `DEMO_CHAT_SERVER_PEER_CLOSED` 并回到 accept |
| S4 对端断开（host 主动） | 重新运行 `/tmp/demo_chat_client` 连接后，host 聊天终端输入 `/quit` | guest 打印 `DEMO_CHAT_CLIENT_PEER_CLOSED` 并退出（exit 1） |
| S2 连接失败 | 不启 host 聊天服务器时运行 `/tmp/demo_chat_client` | guest 打印 `DEMO_CHAT_CLIENT_CONNECT_FAILED … errno=<n>` 并非零退出，不悬挂 |

## 验证

成功判据（缺一不可）：

- 两端出现 `DEMO_CHAT_SERVER_CONNECTED` 与 `DEMO_CHAT_CLIENT_CONNECTED`。
- S1 交换的行在另一端逐行完整显示。
- S3a 空行不显示；S3b `/quit` 语义与 S4 对端断开提示正确，退出码符合设计（guest
  quit=0、peer-closed=1）。
- 全程无 `wget` 失败、无客户端悬挂、无半行/乱码。

在 Act Response 或执行记录中保留：host 终端与 guest 串口完整输出、QEMU 命令、
revision（`git rev-parse HEAD`）、环境。结论只限单 hart QEMU。

## 失败处理

| 症状 | 处理 |
|---|---|
| `wget: Connection refused` | HTTP server 必须 `--bind 0.0.0.0` 且在 `tests/` 目录启动；确认 Terminal A 运行 |
| `wget` 挂起或下载慢 | 既有数据面问题；按 R55 分层诊断或 debugfs 离线注入，不归因本 change |
| guest connect refused | 聊天服务器未启动或端口被占；检查 Terminal B，`ss -ltn` 确认 15560 监听 |
| host 聊天终端无 `CONNECTED` | 服务器 accept 了但提示未刷新；确认串口交互正常后重试 |
| `Address already in use` 15560/18765 | 上轮进程残留；`pkill -f demo_chat_server.py` / `pkill -f http.server` 后重试 |
| 两端已连接但行不显示 | 记录两端原始输出，判定产品数据面或 socket 唤醒问题；超时/缺 marker 记为未完成而非通过 |

## 回滚

- Guest `/tmp` payload 随 QEMU 退出消失，无需回滚。
- 退出 QEMU `Ctrl-A X`；停止 Terminal A/B 用 `Ctrl-C`。
- 进程残留：`pkill -f qemu-system-riscv64`；`pkill -f demo_chat_server.py`；
  `pkill -f 'http.server 18765'`。
- 本 change 不修改产品代码；如需清除产物删除新增文件与 Makefile 追加段即可
  （`tests/demo_chat_client`、`scripts/demo_chat_server*.py`、
  `tests/demo_chat_client*.c`）。

## 证据

- change: `openspec/changes/demo-host-guest-chat/`（Iteration 000 / Cycle 000）。
- host 侧：Act 已验证命令与输出（决策单测、`-fsyntax-only`、`make tests/demo_chat_client`、
  host E2E 冒烟）；QEMU 启动到 `starry:~#` 已由 Agent 侧试运行见到位于
  `/tmp/demo-qemu-serial.log`。
- QEMU 手测段：由操作者按本 Runbook 手动执行，结果记录于 Cycle 000 Act Response。
- 适用限制：结论限定于单 hart QEMU VirtIO-MMIO 软件/设备模型；不覆盖 SMP、真板
  DMA/cache、性能或 fd readiness。
- Revision: branch `linshi`，HEAD `2079bb96`（本 change 未提交前）。