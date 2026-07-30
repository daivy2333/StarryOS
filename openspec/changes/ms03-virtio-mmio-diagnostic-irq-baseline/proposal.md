## Why

MS02 只证明 VirtIO-MMIO 轮询收发。
MMIO probe 未传递网卡 IRQ。
全局 IRQ hook 又被 QEMU UART 占用。

MS03 需要先建立可诊断的设备中断基线。
后续 queue task 不得同时承担 IRQ 排障。

## What Changes

- 固定 QEMU VirtIO-net MMIO 地址与 PLIC IRQ 来源。
- 将 QEMU UART 迁到 IRQ 10 设备 handler。
- 保留 UART 现有 waker、copier 和轮询 console。
- 为 VirtIO-net 注册 IRQ 7 设备 handler。
- IRQ 7 只进入诊断控制面，不改变数据面的轮询 capability。
- 保持单一 VirtIO-net 实例和单一 queue owner。
- 记录 claim、cause、ack、EOI、rearm 和异常见证。
- 在当前 `RING_EVENT_IDX` 协商结果下证明重复投递。
- 保留 MS02 的轮询数据面和网络回归能力。
- 使用独立 UART 与网络日志证明共存。

## BDD Scenario Sketch

### Happy Path

- QEMU 启动后，UART IRQ 10 注册成功。
- VirtIO-net 地址解析出 IRQ 7。
- UART 和网卡设备 handler 同时存在。
- RX 事件使 IRQ 7 与 used-ring 计数增长。
- TX completion 使同一组计数再次增长。
- 每次设备 ack 后仍可接收下一次中断。

### Sad Path

- IRQ 映射缺失时，MS03 Gate 必须失败。
- handler 注册失败时，失败必须可见。
- 错误 IRQ 不得修改网卡计数和状态。
- cause 未清时，风暴检测必须失败。
- rearm 失效时，重复投递 Gate 必须失败。

### Edge Case

- used-ring 与 config-change 必须分开计数。
- RX 与 TX 共享 used-ring cause 时，
  归因必须来自受控事件，不伪造硬件位。
- 无 pending cause 时，记录 spurious 事件。
- UART 与网络并发时，只唤醒所属 UART waker。
- IRQ control 不得创建第二个网卡或 queue owner。

### Error, Timeout, Cancel, and Compatibility

- QEMU 运行见证必须有有界观察窗口。
- 中途终止的见证必须标为未完成。
- 静态设备本轮不支持注销或热插拔。
- 网卡 IRQ 不可用时保留轮询回退。
- UART async handler 失败时不得启动 copier。
- early 和 panic console 必须继续可用。
- MS01 与 MS02 网络行为必须继续通过。

## Capabilities

### New Capabilities

- `qemu-mmio-diagnostic-irq-baseline`:
  定义 MS03 的平台 IRQ 事实、设备 handler、
  cause/ack/EOI/rearm 和重复投递验收。

### Modified Capabilities

- None.

## Impact

- QEMU 平台 IRQ 事实和 MMIO probe。
- QEMU UART handler 注册路径。
- VirtIO-net IRQ 控制接口。
- PLIC 与设备中断诊断计数。
- MS02 QEMU 网络回归和证据采集。

## Non-goals

- 网卡 `AtomicWaker` 和 register-recheck。
- RX/TX queue task 或 descriptor 搬运。
- smoltcp runner 和 socket readiness。
- 将 IRQ 7 暴露为现有 `AxNetDevice::irq_num()`。
- PCI、VF2、SMP 或硬件性能结论。
- 热插拔、设备注销和 reset generation。
- 删除 MS02 轮询数据面。
- 修改全局 milestone 或 SNAPSHOT。

## Gate 1

- Status: approved.
- Decision: 用户于 2026-07-29 回复
  “基本同意……请你继续计划吧”。
- Added constraint: 同一时间只允许一个网卡实例
  和一个数据面 owner。
- UART 迁移纳入同一 change，
  但使用独立任务和验证 Gate。
- 网卡 handler 本轮不唤醒 queue task。
- cause 只区分 used-ring 与 config-change。
- RX/TX 由受控刺激和时间线归因。
- 保留 `RING_EVENT_IDX`，不静默降级 feature。
- handler 注册失败必须可见并停止对应异步路径。
- QEMU 运行日志和计数使用持久化 Evidence。
- MS03 保留轮询 owner，只证明 IRQ 控制面。
- MS04 验证异步路径时，
  对应轮询进度必须关闭或隔离。
- MS03 不创建第二个 `VirtIoNetDev`；
  IRQ control 只访问 transport 中断寄存器。

## Gate 2

- Status: approved.
- Decision: 用户于 2026-07-29 回复
  “批准 Gate 2”。
- Requirements、investigation、design、task contracts、
  RTM、verification 和 Evidence 模式均为 PASS。
- 没有 `Missing`、`Simplified`、waiver 或实现 TBD。
