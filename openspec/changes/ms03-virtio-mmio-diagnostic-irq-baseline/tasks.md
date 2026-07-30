## 1. 建立纯逻辑见证与平台事实

- [ ] 1.1 在
  `kernel/src/drivers/virtio_net_irq_logic.rs`
  建立 MMIO status 分类、单调 telemetry 和 snapshot
  的纯逻辑边界；在
  `tests/ms03-irq-host-harness.rs`
  先写 RED tests，再实现 GREEN。
  测试覆盖 used-ring、config-change、组合 cause、
  unknown、spurious、ack 后残留和单调快照。
  `Makefile::host-test` 必须执行该 harness。
  当前没有该模块，RED 应为缺少符号或断言失败；
  GREEN 为全部 host cases 通过。
  禁止在纯逻辑层访问 MMIO、axnet 或 waker。
  若逻辑无法脱离 kernel target 编译，停止并返回 Plan。

- [ ] 1.2 修改
  `kernel/src/platform/descriptor.rs`、
  `kernel/src/platform/qemu.rs`、
  `kernel/src/platform/lichee_d1.rs`、
  `kernel/src/platform/visionfive2.rs`
  和 `kernel/src/platform/mod.rs`。
  平台描述必须表达可选 VirtIO-MMIO net 事实。
  QEMU 固定 base `0x10007000`、size `0x1000`、
  device ID 1 和 IRQ 7；其他平台显式为无。
  当前描述只有 console/PLIC 等事实；
  修改后 net 事实仍不得进入通用 VirtIO driver 常量。
  目标构建和启动 header 验证为 GREEN。
  若 QEMU 当前设备顺序不能唯一映射该地址，
  停止，不得猜测新地址。

## 2. 迁移 QEMU UART 设备 handler

- [ ] 2.1 修改
  `kernel/src/drivers/uart_init.rs::init_uart_hardware`
  和 QEMU ISR wrapper。
  当前 QEMU 注册单槽 global hook；
  修改后使用 IRQ 10 零参数设备 handler，
  调用现有 `uart_isr_wrapper`。
  必须检查 `axhal::irq::register` 返回值；
  失败时在 copier 启动前 panic。
  D1 handler、UART waker、ring、copier、
  early console 和 panic console 保持不变。
  这是重构：先记录现有 UART async tests GREEN，
  修改后同一组 tests 保持 GREEN。
  source check 必须确认 QEMU UART 不再调用
  `register_irq_hook(uart_isr_wrapper)`。
  QEMU IRQ 10 注册、RX/TX/drain 属于后续运行见证。
  若迁移要求修改 `uart_16550` ISR 语义，
  停止并返回 Plan。

## 3. 增加 VirtIO-MMIO IRQ 诊断控制面

- [ ] 3.1 新增
  `kernel/src/drivers/virtio_net_irq.rs`，
  并修改 `kernel/src/drivers/mod.rs`
  与 QEMU `kernel/src/entry.rs::init`。
  初始化必须验证 magic、version、device ID，
  再为 IRQ 7 注册设备 handler。
  handler 只读取 status、分类 cause、写 ACK、
  读取 ack 后状态并更新 Relaxed atomics。
  禁止获取 axnet `Service`、调用 `receive`、
  访问 descriptor、创建任务或唤醒 waker。
  注册失败必须记录并保留 MS02 轮询网络。
  `VirtIoNetDev` 仍只构造一次且 `irq_num()` 保持
  `None`。
  T1 host tests 为 GREEN；
  QEMU runtime 负责验证设备寄存器和 handler。
  若实现需要第二个 `NetDriverOps`、第二份 queue
  或把 IRQ 7 传给现有 axnet waker，立即停止。

- [ ] 3.2 对 ACK/EOI/rearm 边界做 source review。
  记录 QEMU axplat 的 claim -> handler -> complete
  顺序，以及 `VirtQueue::pop_used` 在
  `RING_EVENT_IDX` 下更新 `used_event` 的位置。
  不调用 `set_dev_notify` 伪造 rearm，
  不注册 global hook 伪造 EOI counter，
  不关闭 `RING_EVENT_IDX`。
  若当前 registry 版本与调查代码不一致，
  停止并返回 Plan Review。

## 4. 增加按需快照与手工 probe

- [ ] 4.1 修改
  `kernel/src/syscall/fs/ctl.rs::sys_ioctl`，
  为命令 `0x4e49_4431` 增加只读 `repr(C)`
  IRQ snapshot。
  快照字段和顺序必须与 design D5 一致，
  并包含 UART handler count。
  当前 ioctl 已有 UART debug 先例；
  新命令不得 reset counter、修改设备状态或
  绕过用户地址写入检查。
  T1 snapshot tests 必须覆盖单调值和字段转换。
  若该命令会改变非 QEMU 平台 ABI，
  使用 QEMU cfg 隔离；不得扩大为通用稳定 ABI。

- [ ] 4.2 新增 `tests/ms03_irq_probe.c`
  和对应 Makefile target。
  payload 提供 RX2、TX2、UART-only、concurrent
  和 idle 模式。
  每个模式在 READY 后 `tcdrain`，
  在前后 snapshot 之间不打印，
  输出固定 PRE/MID/POST/DELTA/PASS/FAIL 标记。
  RX 不发送应用响应；TX 在测量前完成 warm-up；
  idle 使用有界窗口。
  host `cc -Wall -Wextra -Werror -fsyntax-only`
  必须通过。
  RISC-V static 编译和 guest 运行属于用户边界。
  禁止新增脚本、pipe 或 pexpect 驱动 QEMU。

## 5. Agent Gate 与运行边界交接

- [ ] 5.1 依次运行：
  `cargo fmt --all -- --check`、
  MS03 host harness、
  axnet 8 个 service tests、
  UART async tests、
  probe host syntax check、
  `openspec validate ... --strict`
  和 `git diff --check`。
  每条记录命令、关键输出与退出码。
  完成 spec review、code review 和完整 diff review。
  当前 sandbox 已知无法执行 `lwext4_rust`
  交叉 C 冷构建，因此 target build 不得伪报通过。
  任一 agent Gate 失败时保留当前任务并停止。

## External Verification Boundary

以下是用户能力边界，不是 Act 可执行任务：

- `make LOG=info build` 成功。
- 单网卡 QEMU 命令中 net 位于 `0x10007000`。
- UART IRQ 10 与 net IRQ 7 都注册成功。
- RX2 和 TX2 每次刺激都增加 net handler、
  used-ring 与 ack，且第二次可重复投递。
- UART-only 不增加 net handler。
- concurrent 同时增加各自 handler。
- idle 有界窗口不形成 IRQ storm。
- `RING_EVENT_IDX` 保持协商。
- MS01 14/14 与 MS02 TCP/UDP 回归通过。

运行结果必须保存到后续 execution iteration 对应
`evidence/`，包含 README 索引、构建日志、
QEMU 完整串口日志、probe markers、命令环境和判定。
任一文件缺失或运行中断时 Gate 5 保持 blocked。
