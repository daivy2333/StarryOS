# Demo: Host 与 QEMU Guest 的 TCP 聊天回显

## Why

MS04/MS05 已完成硬件端（IRQ、queue task、有界 slot）的异步化，但协议栈侧仍为同步 `poll_interfaces()`。周会演示需要直观证明"已完成一半异步化的网卡仍然能正常收发"。相比口头报告，一个 host 与 QEMU guest 之间通过 TCP 互相聊天（host 输入 → guest 显示，guest 输入 → host 显示）的交互式演示能直接展示网卡 RX/TX 都在工作。

## What Changes

- 在 `linshi` 分支（临时演示分支，head `2079bb96`）新增 host 端 Python 聊天服务器与 guest 端静态 C 客户端。
- host 服务器监听 TCP `0.0.0.0:15560`；guest 客户端 connect `10.0.2.2:15560`（QEMU user-net 网关，10.0.2.2 访问 host，避开被 hostfwd 占用的 5555 及既有测试端口 15555/15557）。
- 连接建立后双方各打印"连接成功"提示；之后逐行聊天：任一方在终端输入一行文本（回车），经 TCP 传到另一端并显示；空行直接跳过；`/quit` 使发送方关闭连接，另一端收到 EOF 后提示对方断开并退出。
- guest 载荷经既有 wget HTTP 注入方式进入 OS：host 起 `python3 -m http.server 18765 --bind 0.0.0.0`，guest shell 内 `wget http://10.0.2.2:18765/demo_chat_client` 下载后运行。
- host 服务器逻辑提供 host 可测试的决策核心（协议解析、会话状态机），guest 客户端同样提供 host 可测试的决策核心（消息帧处理、EOF/quit 判定），README 头部给出编译与运行命令。

## Scenario Sketch

### S1 Happy Path：双方聊起来

- **前置状态**：QEMU 以 user-net + hostfwd 5555 启动（现有参数不改），host 已起 HTTP 注入服务（18765）和聊天服务器（15560）。
- **触发动作**：guest shell 中 `wget` 下载并运行 `demo_chat_client`；客户端 connect `10.0.2.2:15560` 成功；host 终端输入 "hello guest"，guest 串口显示；guest 串口输入 "hello host"，host 终端显示。
- **可观察结果**：两端均打印连接成功提示；host 与 guest 各自输入的行在另一端逐行显示；RX 与 TX 均有实际 TCP 流量（可用 pcap 佐证）。
- **失败边界**：任一方向收不到行、连接后无提示、字符乱码/半行显示均失败。

### S2 Sad Path：服务器未启动

- **前置状态**：host 聊天服务器未运行。
- **触发动作**：guest 客户端启动 connect。
- **可观察结果**：客户端打印明确的连接失败错误（如 connect refused）并退出并返回非零；不悬挂。
- **失败边界**：无限重试、静默失败、悬挂不退出均失败。

### S3 Edge Case：空行与 `/quit`

- **前置状态**：连接已建立。
- **触发动作**：任一端输入空行；随后任一端输入 `/quit`。
- **可观察结果**：空行不转发不显示；`/quit` 发送方正常关闭连接并退出，另一端显示断开提示并退出。
- **失败边界**：空行被当作消息、`/quit` 被当作普通文本转发、对端无断开提示或永久悬挂均失败。

### S4 Error/Timeout：半途断开

- **前置状态**：连接已建立，任一方中途强制关闭（如 Ctrl+C / 杀进程 / QEMU 重启）。
- **触发动作**：对端 TCP 关闭。
- **可观察结果**：存活端 recv 返回 EOF/0，提示"对端断开"并退出；服务器侧可继续 accept 下一条连接。
- **失败边界**：EOF 被当成错误崩溃、recv 阻塞不返回、已死连接继续尝试发送并悬挂均失败。

### S5 Compatibility：不干扰既有基线

- **前置状态**：MS01/MS02/MS04/MS05 全部回归可见。
- **触发动作**：本 change 引入的 host server + guest client 与既有测试并存运行。
- **可观察结果**：不改动既有 socket/queue/runbook 语义；不占用 5555/15555/15557；guest payload 构建沿用既有 `BENCH_CC` 静态 musl 模式。
- **失败边界**：改动 `tcp.rs`/`async_rx.rs` 等产品代码、占用既有端口、改变 Makefile 既有 target 行为均失败。

## BDD 缺口与已接受的默认假设

- 用户确认 guest 载荷经 **wget HTTP 注入**（非固化 rootfs、非内核资源嵌入）。
- 用户确认交互粒度采用**逐行聊天**（QEMU 串口 ICANON 行缓冲，回车提交；不做逐字符 raw 模式）。
- 默认使用既有 `riscv64-linux-musl-gcc -static`（Makefile `BENCH_CC`）构建 guest 客户端；host 服务器仅依赖 Python 标准库。
- 默认端口 `15560`（避开 5555 hostfwd / 15555 MS03 / 15557 MS05）。
- 聊天协议为简单行协议：UTF-8/ASCII 文本行 + `\n`；控制命令仅 `/quit`；不做加密、认证、心跳或自动重连。

## Capabilities

### New Capabilities

- `demo-host-guest-chat`: 定义 host 聊天服务器与 QEMU guest 聊天客户端的行协议、连接/断开语义与单 hart QEMU 手工演示验收。

### Modified Capabilities

- None（不改动任何既有 capability 的 spec 级行为）。

## Impact

- `scripts/demo_chat_server.py`（新增）：host 端聊天服务器，监听 `0.0.0.0:15560`，逐行双向转发。
- `scripts/demo_chat_server_test.py`（新增）：host 决策核心的 host 单元测试。
- `tests/demo_chat_client.c`（新增）：guest 端静态 C 客户端 + host 可测试决策核心。
- `Makefile`：新增 `tests/demo_chat_client` 构建 target（沿用 `BENCH_CC`）与 host test target（可选）——严格只追加，不改既有 target。
- QEMU 运行参数、既有 runbook、`tcp.rs`/`async_rx.rs`/`service.rs` 等产品代码**不动**。

## Non-goals

- 逐字符聊天、TLS、认证、多客户端并发、自动重连、消息历史、文件传输。
- 修改任何产品代码（`crates/axnet`、`kernel`）；本次演示完全在用户态载荷与 host 脚本层面。
- 绑定 MS06 change 或 MS/T-milestone 路线；本次为 `linshi` 分支独立演示产物。
- 修改全局 tasks、SNAPSHOT、M/D/K/R/I，或归档 change。

## Gate 1

- Status: approved。
- 用户于 2026-08-22 审计需求、BDD 草图和范围后回复“同意”，正式批准 Requirements and Scope。
- 已确认缺口：wget HTTP 注入 + 逐行聊天（用户选择）；端口 15560、`/quit` 控制、静态 musl guest 载荷、纯 Python stdlib、不改产品代码、只追加 Makefile target 作为默认假设一并批准。