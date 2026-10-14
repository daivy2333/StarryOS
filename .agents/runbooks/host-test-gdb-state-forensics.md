# host 测试二进制 gdb 状态取证（断点取真值）

- Status: active
- Last validated: 2026-10-14
- Environment: WSL2 x86_64；GNU gdb 12.1（Ubuntu 12.1-0ubuntu1~22.04.2，`-batch` 模式）；cargo 集成测试 debug 构建（无 profile 覆盖，debug_assertions 生效）；示例载体 `tests/k3-async-net-host`（`recovery_socket`）
- Source: MS13 Iteration 002 Cycle 000 Plan Review 的定向审计（`openspec/changes/ms13-com260-kit-single-hart-network-recovery/iterations/002-socket-combination-final-acceptance/000-initial.md` › Plan Review › Evidence），revision `d07285c0` 之上的未提交工作树

## 适用范围

何时使用：host 测试失败且**静态读码与观测快照矛盾**（计数器/账本/状态字段值与代码判定链推不出同一结论，或失败打印里的解释性字段疑似误读）时，在断点处取结构体真值仲裁；或需要重构"两个时间点之间发生了什么"的事件史。

不适用：真板现场（无 gdb 附着面，走判别行/计数器观察面）；QEMU guest（用既有 QEMU 取证 Runbook，见 R55/R63）；输出已能直接定位的纯逻辑错误。

## 前置条件

- 失败可在本机原生复现（先跑一次，记录 panic 行与快照原文）。
- 测试二进制为 debug 构建（cargo test 默认；确认包无 profile 覆盖关闭 debug-assertions——见失败处理最后一条）。
- 知道目标符号的近似名（所属 crate、类型、方法名）。

## 操作步骤

已验证的执行顺序（编号即建议次序）：

1. **原生复现**：
   ```sh
   AX_IP=10.0.2.15 AX_GW=10.0.2.2 cargo test --manifest-path tests/k3-async-net-host/Cargo.toml \
     --locked --offline --test recovery_socket -- --test-threads=1
   ```
   记录 panic 快照（本例 `flush-target=21 flush-ok=0 flush-err=1`）。
2. **定位二进制与真实符号名**：
   ```sh
   ls tests/k3-async-net-host/target/debug/deps/recovery_socket-*
   nm -C tests/k3-async-net-host/target/debug/deps/recovery_socket-<hash> | grep -i "<方法名>"
   ```
   **crate 在二进制里的名字可能与源码不同**（本例 axnet 源码 crate 在二进制中是 `axnet_ng`）。断点名必须用 nm 显示的完整 demangled 名，如 `<axnet_ng::service::Service>::flush_begin`。
3. **gdb batch 骨架**（测试参数必须经 `--args` 传递）：
   ```sh
   gdb -batch -q \
     -ex 'set pagination off' \
     -ex 'break <sym>' \
     -ex 'ignore 1 <N-1>' \
     -ex 'run' \
     [-ex 'continue'] \
     -ex 'p <expr>' \
     --args tests/k3-async-net-host/target/debug/deps/recovery_socket-<hash> --test-threads=1
   ```
4. **选定目标相位**：用 `ignore 1 <N-1>` 跳过前 N-1 次命中，直接落到失败相位（本例 `flush_begin` 第 2 次命中 = 失败的尾相 flush）。命中后先 `bt 3` 核对调用栈行号与测试源码行一致，再取值。
5. **深层断点两段式**：先停在外层相位断点，之后再 `-ex 'break <深层符号>'` + `-ex 'continue'`——深层断点只命中该相位内的事件，免于翻越全部早期历史（本例：flush_begin#2 停下后才下 `has_live_at_or_before`，首命中即属尾相判定链）。
6. **取值三板斧**（按优先级）：
   - 函数**体内行**断点处 `p self.<field>`（类型化输出，最可靠）；
   - 数组/切片一次打印：`set print elements 300` + `p *self.<box字段>.data_ptr@<len>`（单次表达式打印整个 backing，勿逐项循环）；
   - 裸字 dump：`x/Ngx <addr>`。注意 Rust 结构体内存布局会重排，裸 dump 的字段次序不可按源码推断，必须与类型化打印交叉核对。
7. **前后对比重构事件史**：同一符号在两个停点（`run` 停第一次、`continue` 停第二次）dump 同一批字段，差集即两次停点间的事件（本例 tracker `next` 18→22 ⇒ 期间发生 4 次 alloc，直接修正 Act 对字段值的静态归因——"21"是新会话 FIN 票而非 pre-reset 票）。
8. **输出过滤**：整条管道接 `grep -E "<标签>|^#[0-9]|panic"` 只留标签行、栈帧与 panic，避免 128 项打印刷屏。

## 验证

成功判据：断点处类型化打印的值与复现快照**互洽且能解释失败**。本例实测输出（同一 gdb 会话）：

```text
PRE-RESET  tracker: next=18  last=17
POST-RESET tracker: next=22  live_len=1  last=Some(21)  first_lost=None
live[0] = Some((QueueEpoch(1), 21, TicketState::DeviceOwned))
```

三者合并解释"flush-target=21 永不 Pending 结算"（票 21 在途未回收），与代码判定链推演一致、与 Act 快照矛盾点闭合。

地址稳定性：gdb 默认关 ASLR——静态量与大块 mmap 堆地址跨运行不变（本例 `SERVICE`=0x555555871d70、tracker=0x7ffff00a4bd0 连续 3+ 次运行一致）。第二次运行可直接复用首次会话记录的地址做 `x` dump。

## 失败处理

本会话实测的死路（勿重复）：

- **`--test-threads` 未走 `--args`**：`gdb … <binary> --test-threads=1` → `unrecognized option`；必须 `--args <binary> <args>`。
- **`commands` 块内 `p self.x`**：报 `No symbol 'self' in current context`，且该错误会**中止命令列表的剩余部分（含 continue）**——程序停在断点、batch 挂起直到超时。commands 块内不要引用 `self`；取状态一律停在断点后用顶层 `-ex 'p …'`。
- **`break *<sym>` 未加引号**：报错并**静默放弃整个 -ex 脚本**——无断点生效、程序直跑到底（表象是"断点从未命中"）。
- **run 之前下 `watch`（地址未映射）**：不触发、无有效设置输出。观察点只能在停点（目标内存已映射）之后设置。
- **`printf` 限制**：自定义 struct 参数、以及格式参数里内嵌 `sizeof(...)` 均报错（后者 `Syntax error near 'sizeof'`）。改用 `p` 或裸字 dump 手工映射。
- **`p *(Type*)addr` 类型 cast**：本环境一次尝试未取得输出——优先"函数体行断点 + `p self.field`"，cast 只作备选并在同轮确认输出非空。
- **debug-assert 前提**：`debug_assert` 未触发要能证明"路径未执行"，前提是构建开着 debug-assertions；若包有 profile 覆盖将其关闭，该推断不成立（本例已确认无覆盖）。
- **成本控制**：每轮 gdb batch 重跑整个测试（本例 ~20 s 到达断点）。单轮内合并打印命令；出现挂起（输出停在断点命中行后无 panic 无退出）优先怀疑 commands 块中止，直接改顶层 -ex 序列重跑。

停止条件：同一断点三轮取值与快照仍无法互洽时，停止盲试，回到静态读码重选断点相位（三次失败规则）。

## 回滚

只读操作：gdb batch 与 cargo test 不修改仓库文件（构建产物目录除外），无需回滚。

## 证据

- 来源与摘录：MS13 Iteration 002 Cycle 000 Plan Review › Evidence（路径见头部 Source；含复现命令与 gdb 真值输出）。
- revision：`d07285c0` + 未提交工作树；环境：WSL2、gdb 12.1。
- 适用限制：断点符号名绑定各 crate 在二进制中的真实名（`axnet_ng` 等），换 crate/换测试需重新 `nm -C`。
