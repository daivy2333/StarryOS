# Demo Host–Guest Chat — Design

## 目标行为

host 端 Python 聊天服务器与 QEMU guest 端静态 C 客户端通过 TCP 建立连接后，双方各占一个终端逐行聊天；任一方向的行经 TCP 传到另一端显示；`/quit` 关闭连接；对端断开时存活端提示并退出。

## 当前行为

- 不存在 host 聊天服务器或 guest 聊天客户端。既有 `ms02_guest_service`（`tests/ms02_guest_service.c`）是固定请求/响应回声服务，2 次 TCP 往返后自动退出，不提供持续交互。
- guest 端静态载荷构建模式：Makefile `BENCH_CC ?= riscv64-linux-musl-gcc`，`-static`，已有 ms02/ms03/ms04/ms05 先例。
- guest 载荷注入模式：host `python3 -m http.server 18765 --bind 0.0.0.0`，guest shell `wget http://10.0.2.2:18765/<name>`（MS03/MS04/MS05 runbook 均验证）。
- QEMU user-net：guest 访问 host 用网关地址 `10.0.2.2`；hostfwd 仅 tcp/udp 5555→guest 5555，host 侧 5555 被占用，不能用作 host 服务器监听端口。

## 架构

### host 端：`scripts/demo_chat_server.py`

- 监听 `0.0.0.0:15560`（可用 `--host/--port` 覆盖；D1：端口默认 15560，严格避开 5555/15555/15557）。
- `accept` 一个 guest 连接后打印 `DEMO_CHAT_SERVER_CONNECTED peer=<addr>:<port>`。
- 会话主循环用 `selectors`/`select` 同时监视本地终端 stdin 与连接 socket：
  - stdin 一行 → 去除行尾 `\n` → 空行丢弃 → `/quit` 关闭连接并退出会话 → 其余调用 `sendall(line + "\n")`。
  - socket 可读 → `recv` → EOF 表示对端关闭 → 打印 `DEMO_CHAT_SERVER_PEER_CLOSED` 并退出会话（继续等待下一条连接）→ 非 EOF 数据按行打印到终端。
- 消息行协议：UTF-8/ASCII 文本行，行尾 `\n`；控制命令仅 `/quit`。
- 会话可重复：一个连接结束后回到 `accept`，可接受下一条连接（S4）。

### guest 端：`tests/demo_chat_client.c`

- 静态 musl 可执行：`riscv64-linux-musl-gcc -std=c11 -Wall -Wextra -Werror -static -no-pie -Os`。
- `connect("10.0.2.2", 15560)`（可用 argv 覆盖 host/port，默认 10.0.2.2:15560）。
- 连接成功打印 `DEMO_CHAT_CLIENT_CONNECTED server=<host>:<port>`；失败打印 `DEMO_CHAT_CLIENT_CONNECT_FAILED ...` 并返回非零（S2）。
- 主循环用 `poll()` 同时监视 fd 0（串口 stdin）与 socket fd——`kernel/src/syscall/io_mpx/poll.rs` 提供 `sys_poll`，tty（`kernel/src/pseudofs/dev/tty/mod.rs:279`，IN 由 `ldisc.poll_read()` 决定）与 socket 均实现 `Pollable`，ms02 已用同一 poll 模式：
  - stdin 可读 → 读一行 → 空行丢弃 → `/quit` → 打印 `DEMO_CHAT_CLIENT_QUIT`，关闭连接并退出码 0（本地主动退出为正常，S3）→ 其余 `send(all)`。
  - socket 可读 → `recv` → 0 字节（EOF）→ 打印 `DEMO_CHAT_CLIENT_PEER_CLOSED`，退出码 1（对端断开，S4）→ 非 EOF 数据打印到串口 stdout。
  - `POLLERR/POLLHUP/POLLNVAL` → 打印 `DEMO_CHAT_CLIENT_PEER_CLOSED`，退出码 1。
- `SIGPIPE` 忽略（ms02 先例），`EINTR` 时继续循环。
- 决策核心（行分类：EMPTY / QUIT / TEXT；EOF → peer-closed+exit 1；QUIT → exit 0）作为可 host 测试的纯逻辑封装，测试文件用 `#include` 方式编译进 host 二进制（ms05 `MS05_DATA_PLANE_PROBE_TESTING` 模式先例）。

### 协议语义汇总

| 事件 | host 端 | guest 客户端 |
|---|---|---|
| connect 成功 | `DEMO_CHAT_SERVER_CONNECTED` | `DEMO_CHAT_CLIENT_CONNECTED` |
| 收到非空行 | 打印到终端 | 打印到串口 |
| 收到空行 | 丢弃 | 丢弃 |
| 本地输入 `/quit` | 关闭连接，退出会话（可再 accept） | 关闭连接，exit 0 |
| 对端 EOF/断开 | `DEMO_CHAT_SERVER_PEER_CLOSED`，回到 accept | `DEMO_CHAT_CLIENT_PEER_CLOSED`，exit 1 |
| 连接失败 | — | `DEMO_CHAT_CLIENT_CONNECT_FAILED`，exit 非零 |

## 测试策略

- host 决策核心（行分类：EMPTY/QUIT/TEXT）：`scripts/demo_chat_server_test.py` 中纯单元测试，无需网络。
- guest 决策核心（行分类 + EOF/QUIT 结果）：`tests/demo_chat_client_test.c`（host 编译运行），测试行分类函数与 EOF/QUIT 状态判定，不依赖 guest 环境。
- C 语法/规范检查：`cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/demo_chat_client.c`。
- guest 载荷构建：`make tests/demo_chat_client`。
- QEMU 手工演示：单 hart、单 VirtIO-MMIO NIC、user-net、hostfwd 5555（现有参数）。host 起 `python3 -m http.server 18765 --bind 0.0.0.0` + `python3 scripts/demo_chat_server.py`；guest `wget http://10.0.2.2:18765/demo_chat_client` 后运行。

## 非目标

- 逐字符/raw 模式、TLS、认证、多客户端并发、自动重连、心跳、消息历史、文件传输。
- 修改 `crates/axnet`、`kernel` 任何产品代码。
- 改变既有 Makefile target 行为（只追加）。
- 绑定 MS06 或 MS-T 路线。

## 关键决策与依据

| 决策 | 选择 | 依据 |
|---|---|---|
| host 端口 | 15560 | hostfwd 占用 5555；MS03 用 15555、MS05 用 15557 |
| guest→host 地址 | 10.0.2.2 | QEMU user-net 网关；MS05 已验证 UDP 直连 |
| 载荷注入 | wget HTTP 18765 | MS03/04/05 runbook 验证；用户确认 |
| 交互粒度 | 逐行 | QEMU 串口 ICANON 行缓冲；用户确认 |
| guest 决策核心测试 | host 编译 `#include` | ms05 `MS05_DATA_PLANE_PROBE_TESTING` 模式 |
| 并发 | 单客户端连接 | 演示目的，Non-goal 多客户端 |

## 影响面与兼容性

- 只新增：`scripts/demo_chat_server.py`、`scripts/demo_chat_server_test.py`、`tests/demo_chat_client.c`、`tests/demo_chat_client_test.c`、Makefile 新 target。
- 不触碰：`crates/axnet`、`kernel`、既有 QEMU 参数与 runbook、既有端口。
- 结论只覆盖单 hart QEMU 手工演示。