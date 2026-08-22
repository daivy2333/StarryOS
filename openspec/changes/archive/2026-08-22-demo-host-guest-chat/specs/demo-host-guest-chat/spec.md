## ADDED Requirements

### Requirement: Host 聊天服务器

`scripts/demo_chat_server.py` MUST 监听 TCP `0.0.0.0:15560`，接受一个 guest 客户端连接，并在连接建立后打印明确的"连接成功"提示。服务器 MUST 逐行读取本地终端输入并经 TCP 转发给 guest，同时逐行读取 guest 发来的数据并打印到本地终端；对空白行 MUST NOT 转发；收到本地 `/quit` 或对端 EOF 时 MUST 关闭连接，且对端断开时 MUST 打印断开提示。

#### Scenario: 服务器拒绝本地 bind 非法端口

- **WHEN** 端口不是 `15560`（默认）
- **THEN** 服务器 MUST 使用用户通过命令行参数指定或环境变量覆盖的端口
- **AND** bind 失败时 MUST 打印明确错误并以非零退出码结束

#### Scenario: 服务器接受 guest 连接并显示连接成功

- **WHEN** guest 客户端完成 TCP 握手
- **THEN** 服务器 MUST 打印包含对端地址的"连接成功"提示
- **AND** MUST 进入逐行双向转发状态

#### Scenario: 服务器转发一方输入到另一方

- **WHEN** 任一端输入一行非空文本（回车结束）
- **THEN** 服务器 MUST 将该行（含行结束符）经 TCP 发送到另一端，且另一端 MUST 显示该行

#### Scenario: 服务器处理 `/quit` 与对端 EOF

- **WHEN** 本地输入 `/quit`，或对端关闭连接（EOF）
- **THEN** 服务器 MUST 关闭连接并退出转发循环
- **AND** 对端先断开时 MUST 打印断开提示，再等待或接受下一条连接

#### Scenario: 空行不转发

- **WHEN** 任一端输入仅含空白/空的行
- **THEN** 服务器 MUST 不将其作为消息转发或显示

### Requirement: Guest 聊天客户端

`tests/demo_chat_client.c` MUST 作为 RISC-V 静态可执行文件（`riscv64-linux-musl-gcc -static`）连接 `10.0.2.2:15560`，连接成功后 MUST 打印明确的"连接成功"提示。客户端 MUST 逐行将本地（QEMU 串口）终端输入经 TCP 发送给 host 服务器，逐行显示 host 发来的数据；空行不发送；本地输入 `/quit` 或对端 EOF 时关闭连接并退出；对端断开时 MUST 打印断开提示后退回非零退出码。

#### Scenario: 客户端连接失败退出

- **WHEN** host 服务器未监听或网络不可达
- **THEN** 客户端 MUST 打印包含错误原因/码的连接失败消息
- **AND** 以非零退出码结束，不无限重试、不悬挂

#### Scenario: 客户端连接成功并显示提示

- **WHEN** TCP 握手完成
- **THEN** 客户端 MUST 打印连接成功提示
- **AND** 进入逐行双向转发状态

#### Scenario: 客户端逐行收发

- **WHEN** 本地输入非空行
- **THEN** 客户端 MUST 经 TCP 发送该行给 host，且 host 端显示该行
- **WHEN** host 端发来一行
- **THEN** 客户端 MUST 在本地串口显示该行

#### Scenario: 客户端处理 `/quit` 与对端 EOF

- **WHEN** 本地输入 `/quit`
- **THEN** 客户端 MUST 关闭连接并以零退出码结束（本地主动退出视为正常）
- **WHEN** host 端关闭连接（EOF）
- **THEN** 客户端 MUST 打印断开提示并以非零退出码结束

### Requirement: 演示集成与验证边界

本 capability MUST 支持在单 hart、单 VirtIO-MMIO NIC 的 QEMU user-net 环境中完整演示：host 起聊天服务器，guest 经 wget HTTP 注入载荷后运行客户端，两端显示连接成功，双向逐行聊天，空行与 `/quit` 语义正确，任一方断开时对端可见。验证结论 MUST 只覆盖单 hart QEMU 手工演示，MUST NOT 扩大到 SMP、真板、性能或产品代码语义。

#### Scenario: 载荷构建与注入

- **WHEN** 构建 guest 客户端
- **THEN** 使用 `riscv64-linux-musl-gcc -static`（Makefile `BENCH_CC`）生成静态可执行文件
- **AND** guest 通过 `wget http://10.0.2.2:18765/demo_chat_client` 下载后运行

#### Scenario: 演示完成

- **WHEN** 演示流程完整走通（连接成功、双向多行聊天、空行、`/quit`）
- **THEN** 两端输出中出现连接成功提示与全部交换的行
- **AND** 结论只覆盖单 hart QEMU 环境，不声明产品代码或真实硬件行为

#### Scenario: 回归无害

- **WHEN** 演示载荷与 host 脚本相对既有 QEMU 参数、Makefile target 与产品代码共存
- **THEN** 不修改任何既有 target 行为、不占用 5555/15555/15557、不触碰 `crates/axnet` 与 `kernel` 产品代码