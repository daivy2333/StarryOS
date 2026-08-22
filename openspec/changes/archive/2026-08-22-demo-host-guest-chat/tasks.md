## 1. Host 聊天服务器

- [x] 1.1 在 `scripts/demo_chat_server.py`（新）实现 host 聊天服务器：`--host/--port` 参数（默认 0.0.0.0:15560）、阻塞 accept 单连接、`DEMO_CHAT_SERVER_CONNECTED` 提示、select 监视终端 stdin + socket 的逐行双向转发、空行丢弃、`/quit` 关闭、EOF 打印 `DEMO_CHAT_SERVER_PEER_CLOSED` 后回到 accept；行分类决策核心（EMPTY/QUIT/TEXT）为可单测纯函数。WHY 是 host 端需要既被 guest 加载载荷依赖、又可演示双向聊天；HOW 是先写 `scripts/demo_chat_server_test.py` 测行分类与 EOF/QUIT 判定 RED，再实现服务器。EXPECTED 是行分类单元测试 GREEN、服务器可启动监听 15560 且 accept 连接后逐行转发。

## 2. Guest 聊天客户端

- [x] 2.1 在 `tests/demo_chat_client.c`（新）实现 guest 静态 C 客户端：默认 connect `10.0.2.2:15560`（argv 覆盖）、成功打印 `DEMO_CHAT_CLIENT_CONNECTED`、失败打印 `DEMO_CHAT_CLIENT_CONNECT_FAILED` 并非零退出、`poll()` 同时监视串口 stdin 与 socket、空行丢弃、`/quit` exit 0、EOF/POLLERR/POLLHUP 打印 `DEMO_CHAT_CLIENT_PEER_CLOSED` 并 exit 1、行分类决策核心函数可 host 测试。WHY 是 guest 需要在使用既有 wget 注入的前提下与 host 聊天；HOW 是先写 `tests/demo_chat_client_test.c`（host 编译，`#include` 决策核心，测 EMPTY/QUIT/TEXT 与 EOF/QUIT 结果）RED，再实现客户端。EXPECTED 是 host 决策测试 GREEN、`make tests/demo_chat_client` 产出 RISC-V 静态二进制、`cc -fsyntax-only` 通过。

## 3. 集成与验证

- [x] 3.1 在 `Makefile` 追加 `tests/demo_chat_client` 与 host 决策测试 target（沿用 `BENCH_CC -static -no-pie -Os`），不改动任何既有 target；依次运行 host 决策测试、`cc -fsyntax-only`、guest 载荷构建、python 服务器测试，记录命令与退出码到 Act Response。WHY 是演示载荷需可重复构建且不污染既有构建；HOW 是只追加新 target 并跑全部验证。EXPECTED 是所有验证命令 exit 0 且既有 Makefile target 行为不变。
- [x] 3.2 在单 hart、单 VirtIO-MMIO NIC、user-net、hostfwd 5555 的 QEMU 中手工演示：host 起 18765 HTTP 注入服务与 15560 聊天服务器，guest `wget` 下载并运行客户端；验证双方连接提示、双向多行聊天、空行、`/quit`、对端断开提示。记录完整串口/终端输出到 Act Response。WHY 是应用可见链路只有真实 QEMU 能证明；HOW 是按 runbook 串口交互模式逐步执行并记录证据。EXPECTED 是两端提示与全部交换行出现、`/quit` 与断开语义正确，结论只限单 hart QEMU。

## Iteration Plan

### Iteration 000: demo-chat

- Tasks: 1.1, 2.1, 3.1, 3.2
- Depends on: None（独立于 MS06；linshi 分支演示产物）
- Stable baseline: host 服务器 + guest 客户端在单 hart QEMU 下可完成双向逐行聊天，`/quit` 与断开语义正确。
- Verification boundary: host 决策单测、guest 决策 host 测试、C 语法检查、guest 静态构建、单 hart QEMU 手工演示全部通过。
- Diagnostic boundary: 失败限制在 demo_chat_server.py、demo_chat_client.c、Makefile 新 target、QEMU 环境或注入链路。
- Non-goals: 逐字符聊天、多客户端、TLS、产品代码修改、绑定 MS06。

## Requirement Traceability Matrix

| Requirement | Scenario | Design | Task | Iteration | Code surface | Test witness | Simplification | Status |
|---|---|---|---|---|---|---|---|---|
| R1 host 服务器 | S1,S3,S4 | 架构/host 端 | 1.1 | 000 | `scripts/demo_chat_server.py` | `demo_chat_server_test.py` 行分类/EOF/QUIT | None | Covered |
| R2 guest 客户端 | S1,S2,S3,S4 | 架构/guest 端 | 2.1 | 000 | `tests/demo_chat_client.c` | `demo_chat_client_test.c` 决策核心 | None | Covered |
| R3 空行/quit/EOF | S3,S4 | 协议语义 | 1.1,2.1 | 000 | 两端决策核心 | 两端决策单测 | None | Covered |
| R4 连接失败 | S2 | 架构/guest 端 | 2.1 | 000 | `demo_chat_client.c` | `demo_chat_client_test.c` + QEMU S2 手测 | None | Covered |
| R5 演示集成 | S1,S5 | 测试策略 | 3.1,3.2 | 000 | Makefile、QEMU 流程 | 构建 + 手测 | None | Covered |
| R6 回归无害 | S5 | 影响面 | 3.1 | 000 | 只追加 Makefile target | `make` 既有 target 不变 | None | Covered |

没有 Missing、未批准 Simplified 或依赖 MS07/MS08 才能满足的当前 Requirement。