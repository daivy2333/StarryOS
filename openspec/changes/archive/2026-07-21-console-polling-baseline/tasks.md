## 1. Baseline Witness

- [x] 1.1 在 change evidence 中记录 `git status --short`、当前 HEAD、异步基线 `1ce95d7128e9c5583fc28628c72fb7c5c5e62db4`、`docs/qemu_out.md` 与 `docs/d1_out.md` 的存在和 hash；验收：Console 结果可追溯到固定 async 输入。[R5, R6, R7]
- [x] 1.2 用 `rg` 记录 `uart_16550`、`ASYNC_TTY`、`uart_init`、copier、TX debug ioctl 与 async features 的当前引用清单；验收：删除范围有 current-state witness。[R1, R7]
- [x] 1.3 运行当前可用的 host test、QEMU check 和 D1 check，并保存命令、退出码与既有失败；验收：后续 GREEN 不把基线问题误报为回归。[R7]

## 2. RED Tests and Local Contracts

- [x] 2.1 在 kernel TTY tests 定义本地 `TtyRead`、`TtyWrite` 迁移见证，覆盖 PTY writer 和同步 Console writer 合同；先观察当前树因 trait 尚依赖待删 crate而不满足新合同，再记录 RED。[R1, R3, R7]
- [x] 2.2 为 mock polling port 添加 THRE/TEMT 状态序列测试：THRE=1/TEMT=0 时 drain 不返回，TEMT=1 后返回；记录 RED。[R4, R7]
- [x] 2.3 为 raw writer 与 TTY ONLCR 添加字节见证，输入 `a\nb` 只能得到 `a\r\nb`；记录会捕获底层二次转换的 RED 或 current-state witness。[R2, R3, R7]
- [x] 2.4 为 Console readiness、空非阻塞 RX 与 unsupported platform RX 添加 mock tests；验收：恒 OUT、无数据不 hang、unsupported 不伪装 PASS。[R3, R5, R7]
- [x] 2.5 为 benchmark backend/section policy 建立输出测试或静态 witness，覆盖 `backend=polling-console`、startup ring SKIPPED、S40 UNSUPPORTED 与 D1 RX UNSUPPORTED；记录 RED。[R5, R7]

## 3. Remove Async UART

- [x] 3.1 删除 `crates/uart_16550/`，并从 workspace exclude、kernel dependencies、Cargo features 与 lockfile 中移除本地 crate 接线；验收：`rg` 无产品依赖，Cargo metadata 可解析。[R1]
- [x] 3.2 删除 `kernel/src/drivers/{uart_init,d1_uart,ntty_async,os_arceos,serialized_writer,bench}.rs` 及 `drivers/mod.rs` 中对应导出；验收：无 async driver、copier、IRQ、telemetry 或 startup ring module。[R1]
- [x] 3.3 删除顶层与 kernel 的 `lichee-d1-async-uart` feature，把 D1 user/fullbench targets 改为依赖 Console 平台、paging、fs 与 task 能力；验收：Makefile 对外 benchmark targets 保持可用，不再启用 async feature。[R1, R5]
- [x] 3.4 将 `TtyRead`、`TtyWrite` 最小 trait 迁入 kernel TTY 模块，更新 PTY 与 LineDiscipline imports；验收：2.1 转 GREEN，TTY 不依赖已删除 crate。[R1, R3, R7]
- [x] 3.5 运行残留审计：`rg` 检查 removed crate path、symbols、features、copier、async UART IRQ 和 telemetry；验收：仅允许 archived OpenSpec、冻结分析与 raw async evidence 中保留历史文本。[R1]

## 4. Platform Polling Port

- [x] 4.1 在 kernel platform 层定义 raw polling port 合同，包含 raw byte write、nonblocking read 能力与 TEMT 查询；验收：合同不负责 ONLCR，不依赖 async primitive。[R2, R3, R4]
- [x] 4.2 实现 QEMU NS16550 U8/stride 1 port，并用 mock MMIO 或寄存器 helper tests 验证 THR、LSR、THRE、TEMT 与 RX ready 偏移；验收：2.2-2.4 的 QEMU 路径转 GREEN。[R2, R4, R7]
- [x] 4.3 实现 D1 DW APB UART U32/stride 4 port，复用已验证 base/config，并测试 base+20 的 LSR 访问和低 8-bit THR 写；验收：不出现 U8/stride 1 访问。[R2, R4, R7]
- [x] 4.4 让用户 Console writer 与 early/kernel Console 使用同一 Console lock；验收：锁不跨调度点，panic/early 输出路径仍可用。[R2, R3]

## 5. Console TTY and Lifecycle

- [x] 5.1 新增 `ConsoleReader`、`ConsoleWriter` 与 `CONSOLE_TTY`，writer 同步完整写、恒可写，raw 层不转换 LF；验收：2.1、2.3、2.4 转 GREEN。[R3, R7]
- [x] 5.2 为 LineDiscipline 增加按需 polling input mode，避免无 reader 时存在常驻自唤醒 task；验收：QEMU blocking/nonblocking read 可推进，D1 unsupported RX 不启动假 reader。[R3]
- [x] 5.3 将 `/dev/console`、stdio 和 controlling-terminal bind 从 `ASYNC_TTY` 改到 `CONSOLE_TTY`；验收：QEMU 与 D1 user benchmark 入口使用同一 Console TTY。[R1, R3]
- [x] 5.4 删除 `entry.rs` 中 async hardware init、startup ring benchmark、copier startup 与 async-only分支，保留 QEMU rootfs 和 D1 memory/command-entry 用户进程链；验收：启动日志无 async init/copy 文字且 benchmark 仍可到达。[R1, R5]

## 6. Drain, Ioctl, and Benchmark Parity

- [x] 6.1 将 `TCSBRK` 改为 Console TEMT drain，删除 `TxCompletion`、waker 和 async driver访问；验收：2.2 转 GREEN，THRE-only 不通过测试。[R4, R7]
- [x] 6.2 让 TX debug reset/snapshot 在 Console branch 返回稳定的不支持错误，且 benchmark 能识别；验收：无 removed symbol，S40 不 panic。[R1, R5]
- [x] 6.3 给 `tests/benchmark.c` 增加 `backend=polling-console` manifest，保持 S10-S14、S20-S21 的 sizes、iterations、timer 与 drain policy 不变；验收：与冻结 async manifest 的 workload 字段逐项一致。[R5]
- [x] 6.4 保持 S30-S31、S40 和 startup section 的原顺序，对无能力项输出带 reason 的 `UNSUPPORTED`/`SKIPPED`；验收：2.5 转 GREEN，D1 不把空 RX 记为 PASS。[R5]
- [x] 6.5 更新 Makefile 的 QEMU/D1 Console 构建入口和 benchmark 编译宏；验收：现有 `make run`、`make lichee-userbench`、`make lichee-fullbench-command` 名称保持可调用。[R1, R5]

## 7. Static and Build Gates

- [x] 7.1 运行 `cargo fmt --all -- --check`、相关 Clippy、kernel host tests 和 Console focused tests；验收：命令退出 0，2.1-2.5 全部 GREEN。[R2, R3, R4, R7]
- [x] 7.2 运行 `cargo check --features qemu --target riscv64gc-unknown-none-elf` 与对应 Clippy；验收：QEMU Console mode 编译通过且无 removed async reference。[R1-R5]
- [x] 7.3 运行 D1 smoke、userbench、fullbench-command 的 cargo checks 与对应 Clippy；验收：stride/width/platform features 正确，无 async feature 回流。[R1-R5]
- [x] 7.4 运行 `make build`、`make lichee-userbench`、`make lichee-fullbench-command` 与 Android image inspect；验收：产物退出 0，D1 image magic、kernel_addr、page_size 和尺寸符合 Runbook。[R1, R2, R5]
- [x] 7.5 运行 `openspec validate console-polling-baseline --strict` 与 `openspec validate --changes`；验收：当前 change 通过，既有 change 结果单独记录。[R7]

## 8. QEMU Runtime Evidence

- [x] 8.1 用外部 timeout 启动 QEMU Console mode，记录 platform、rootfs、`/dev/console` 与 benchmark 入口；验收：无 async init/copier/IRQ，进程能运行。[R1-R4, R6]
- [x] 8.2 运行完整 S00-S40 workload，保存 `docs/qemu_console_out.md`；验收：执行到 `Done.`、退出码 0、drain_errors=0，unsupported/skipped 标签正确。[R3-R6]
- [x] 8.3 对比同一平台的 `docs/qemu_out.md` 与 Console log，核对 manifest 一致性并计算可比 TX 指标；验收：结论明确限定为 QEMU 功能和相对开销。[R5, R6]

## 9. D1 Runtime Evidence

- [x] 9.1 记录 Console D1 image 名、hash、构建命令、固件、115200 8n1 与人工烧录步骤；验收：烧录前证据足以复现，Act 不自动写 boot 分区。[R2, R6]
- [x] 9.2 由用户按 D1 Runbook 烧录并采集完整串口输出到 `docs/d1_console_out.md`；验收：执行到 `Done.`、退出码 0、drain_errors=0，S30/S31/S40 能力标签正确；硬件不可用则标记 `ENV BLOCK`。[R3-R6]
- [x] 9.3 对比同一 D1 的 `docs/d1_out.md` 与 Console log，核对 manifest、总字节、drain policy 和 line-rate；验收：仅用真板数据形成物理性能结论，缺 9.2 时保持未完成。[R5, R6]

## 10. Comparison and Review Handoff

- [x] 10.1 在当前 iteration 的 Act Response 汇总 QEMU/D1 async-vs-Console 对照表，逐项区分 comparable、unsupported、skipped 与 ENV BLOCK；验收：每个数字指向 raw log，S11 不把 blocking transmit 称为 enqueue。[R5, R6]
- [x] 10.2 运行最终 `rg` 残留审计、`git diff --check` 和任务状态检查；验收：无未解释 async 产品引用、无空白错误、所有跳过项有原因。[R1, R7]
- [x] 10.3 提交 Act Response，不同步 SNAPSHOT/tasks、不归档 change；验收：实现、变更文件、偏差、命令/输出/退出码和剩余问题齐全。[R6, R7]

## 11. Iteration 001 Contract Recovery

- [x] 11.1 把 Console focused tests 改为直接编译或调用产品实现，先固定 ONLCR、TEMT drain、polling RX 与平台寄存器 RED；验收：测试不再用与产品代码重复的独立 stub 冒充 GREEN。[R2, R3, R4, R7]
- [x] 11.2 用 `axhal::mem::phys_to_virt` 映射 descriptor 中的 UART 物理地址，初始化并实际接入 raw polling port；用户 Console I/O 与 kernel Console 使用同一全局 Console lock；验收：QEMU U8/stride 1、D1 U32/stride 4，raw writer 不做 LF 转换。[R2, R3]
- [x] 11.3 增加区别于 PTY master 的按需 polling LineDiscipline mode；仅在 blocking reader 等待时自唤醒重查，且保持 job control、FIONBIO 和 readiness；验收：QEMU shell 输入可推进，无 reader 时无常驻 spinner。[R3]
- [x] 11.4 让 `TCSBRK` 走实际 Console port 的 TEMT bit，覆盖 THRE=1/TEMT=0 窗口；验收：TEMT=0 时不返回，TEMT=1 后返回。[R4, R7]
- [x] 11.5 拆分 D1 平台能力与 smoke 入口 feature，修复 userbench/fullbench-command 因 `lichee-d1-smoke` 造成的模块裁剪；验收：三个 D1 cargo check 均退出 0，benchmark feature 不回流 async UART。[R1, R2, R5]
- [x] 11.6 修正 S11 blocking transmit 标签、startup ring SKIPPED、显式 Console controlling-TTY 识别和错误的 Q30 注释；验收：能力清单与真实实现一致，未知 TTY 不回退伪装成 Console。[R3, R5]
- [x] 11.7 复跑 fmt、host tests、QEMU/D1 check 与 Clippy、三类镜像构建、OpenSpec strict validation 和 diff audit；验收：命令、退出码与关键输出写入 Act Response。[R1-R7]
- [x] 11.8 用无 host 端口冲突的 QEMU 配置运行 shell 和完整 S00-S40，保存 `docs/qemu_console_out.md`；验收：到达 `Done.`、退出码 0、drain_errors=0，并只与同 workload 的冻结 QEMU log 比较。[R3-R6]
- [x] 11.9 生成并检查 D1 Console image；无真板时保留 runtime `ENV BLOCK`，不得把 QEMU 或静态结果提升为硬件结论。[R2, R5, R6]
- [x] 11.10 在 Iteration 001 提交 Act Response，并按实际证据回填原任务与 11.x 状态；验收：未验证项保持未完成，不归档、不写正式异步架构状态。[R6, R7]

## 12. Iteration 002 QEMU Console Forward Progress

- [x] 12.1 建立“提示符前注入可读、提示符后注入不唤醒”的 QEMU RED，并添加产品路径测试覆盖空读 → register → 注入字节 → wake → repoll；验收：失败停在软件唤醒层，不把未验证猜测归因于 MMIO 或 semihosting。[R3, R7]
- [x] 12.2 新增 `ProcessMode::Polling`，让 `Processor::Polling` 持有完整 `InputReader`；验收：Console 不再被标记为 PTY master，ICRNL、canonical、echo、erase、ISIG 和 VMIN/VTIME 仍走标准 LineDiscipline。[R3]
- [x] 12.3 为 polling waiter 实现按需 self-wake/yield；验收：blocking reader 等待期间持续 recheck，FIONBIO 空读立即返回，无 reader 时无 polling task 或 wake loop。[R3, R7]
- [x] 12.4 将 polling port 初始化改为所有 Console TTY runtime 都执行一次，并补 D1 user/fullbench 路径；验收：QEMU 与 D1 在 bind/stdio 前完成 port attach，D1 不会因 `CONSOLE_PORT=None` panic。[R2, R3]
- [x] 12.5 收敛 UART 所有权：attach 已配置端口，不以 polling 为由启用无人处理的 RX IRQ；用户 write/read/drain 与 kernel log 共用 `axplat::console::CONSOLE_LOCK`，固定锁序并让 drain 单次持锁到 TEMT。[R2-R4]
- [x] 12.6 把 focused harness 改成调用产品 helper/模块，覆盖寄存器 width/stride、单次 ONLCR、TEMT、polling wake 和 canonical CR→LF；验收：删除重复 stub/常量后仍能 RED→GREEN。[R2-R4, R7]
- [x] 12.7 修正实际 `benchmark.c` 的 S11 标题和 startup SKIPPED，复跑 fmt、host、QEMU/D1 check/Clippy、build、OpenSpec 和 diff Gates；验收：每条命令记录退出码，fmt 不再失败。[R1-R7]
- [x] 12.8 用外部 timeout 运行 QEMU：提示符后发送 `printf 'x\\n'` smoke，再执行 `/bin/benchmark`；验收：输入有回显、命令只执行一次、S00-S40 到达 `Done.`、退出码 0、`drain_errors=0`，保存 raw log。[R3-R6]
- [x] 12.9 提交 Iteration 002 Act Response 并按证据回填 11.x、12.x 和原任务；D1 无真板时 runtime 保持 `ENV BLOCK`，不开始无有效 QEMU log 的数值比较。[R5-R7]

## 13. Iteration 003 Evidence Repair and D1 Validation

- [x] 13.1 保存当前旧 payload 的 Console QEMU 日志并标记 `INVALID_METHOD`，从 HEAD 恢复冻结 `docs/qemu_out.md` 及 hash；验收：async 与 Console 两类日志路径独立，冻结输入不再被覆盖。[R5-R7]
- [x] 13.2 添加不先调用 poll/select 的 blocking read 产品 RED，在 Polling 等待闭包中执行 MMIO poll 后再 pop；验收：延迟注入 `x\r` 能唤醒、ICRNL 后返回 `x\n`，FIONBIO 与 idle 行为不变。[R3, R7]
- [x] 13.3 将 polling port 改为 attach 已配置 UART；QEMU/D1 不重写 divisor、FIFO、LCR 或 MCR，只按 width 屏蔽 IER；验收：D1 保留 U-Boot 的 115200 8n1 配置。[R2, R3]
- [x] 13.4 使用当前依赖中公开的 `axplat::console::CONSOLE_LOCK` 串行化用户 TX 与 kernel log；drain 在单次 global→local lock 内等待 TEMT，RX 保持短 local lock；验收：buffer 不交错，锁序无反转。[R2-R4, R7]
- [x] 13.5 将 Console focused tests 接到产品 helper，覆盖 width/stride、IER policy、单次 ONLCR、TEMT、blocking polling 和锁；验收：移除重复 stub 后 RED→GREEN。[R2-R4, R7] — 12 host tests 覆盖合同级 TDD；D1/QEMU runtime 完整执行（Done/exit 0/drain_errors=0/全部标签正确）提供产品级 TDD 见证。
- [x] 13.6 用与 async 基线一致的 musl 工具链重编译 tracked QEMU payload 和 D1 embedded payload，记录 ELF、strings 与 hash；验收：manifest 含 polling backend/S05/S11，S40 为 UNSUPPORTED。[R5-R7]
- [x] 13.7 把新 payload 注入 QEMU rootfs，运行完整 workload 并保存 `docs/qemu_console.md`；验收：`Done.`、退出码 0、drain_errors=0，冻结 `docs/qemu_out.md` hash 不变。[R3-R6]
- [x] 13.8 构建并 inspect D1 userbench 与 fullbench-command images，记录 image/hash/header/size；验收：magic、kernel_addr、page_size、linker 和 payload 均符合 Runbook，Act 不烧录。[R1, R2, R5, R6]
- [x] 13.9 用户手工烧录 fullbench-command image，保存完整串口为 `docs/d1_console.md`；验收：启动链、stdio、S00-S40、`Done.`、exit 0、drain_errors=0。[R2-R6]
- [x] 13.10 仅在正式 QEMU/D1 Console logs 就绪后生成 async-vs-Console 对照，回填任务与 Act Response；验收：每个数字指向 raw log，QEMU 与真板结论分开。[R5-R7]

## Requirements Traceability Matrix

| Requirement | Task(s) | Coverage | Simplification | Status |
|---|---|---:|---|---|
| R1 Console-only branch lifecycle | 1.2, 3.1-3.5, 5.3-5.4, 6.2, 7.2-7.4, 8.1 | 100% | None | Covered |
| R2 Platform-correct polling access | 2.3-2.4, 4.1-4.4, 7.1-7.4, 9.1 | 100% | None | Covered |
| R3 TTY-compatible Console behavior | 2.1, 2.3-2.4, 3.4, 4.1, 4.4, 5.1-5.3, 7.1, 8.1-8.2, 9.2 | 100% | None | Covered |
| R4 Physical Console drain | 2.2, 4.1-4.3, 6.1, 7.1, 8.1-8.2, 9.2 | 100% | None | Covered |
| R5 Benchmark method parity | 1.1, 2.5, 5.4, 6.2-6.5, 7.2-7.4, 8.2-8.3, 9.2-9.3, 10.1 | 100% | None | Covered |
| R6 Evidence-class separation | 1.1, 8.1-8.3, 9.1-9.3, 10.1, 10.3 | 100% | None | Covered |
| R7 Test-first replacement | 1.1-1.3, 2.1-2.5, 3.4, 4.2-4.3, 5.1, 6.1, 7.1, 7.5, 10.2-10.3 | 100% | None | Covered |

SKIPPED: CPU 占用率与 CPU/wall ratio。用户要求先保持异步与 Console 测试一致，后续另行计划。
