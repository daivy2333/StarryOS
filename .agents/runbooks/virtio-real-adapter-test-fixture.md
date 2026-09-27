# 为 virtio-drivers 依赖 crate 编写真实 adapter 测试

- Status: active
- Last validated: 2026-08-13
- Environment: Rust nightly-2026-02-25；`axdriver_virtio`（no_std，host `cargo test`）；`virtio-drivers` 0.7.5（workspace path 依赖）；`axdriver_net` 0.1.4-preview.3
- Source: change `ms05-qemu-bounded-bidirectional-device-data-plane` Iteration 001 Act Response（`reported`）；`crates/axdriver_virtio/src/net.rs` tests；revision `1a2bc99` + 本轮工作树

## 适用范围

- 需要让测试驱动**真实 adapter**（如 `VirtIoNetDev`）的 submit/reclaim/ownership 转移，而不是复制 ledger 算法到 helper 断言时。
- 测试位于**依赖 virtio-drivers 的 crate**（如 axdriver_virtio）中，需要模拟设备侧 completion（used-ring 写入）。
- 不适用：只验证公共 contract 纯逻辑（用 axdriver_net 内 fake model 即可）；需要真实硬件/QEMU 的证据（走 QEMU 手工轮次）。

## 前置条件

- `virtio-drivers` 的 `transport::fake` / `hal::fake` 均为 `#[cfg(test)]` 门控 —— **依赖 crate 的测试看不到它们**（依赖以 `cfg(test)=false` 编译）。不要尝试复用。
- 本地测试 HAL：identity DMA 映射（phys == virt），`dma_alloc`/`dma_dealloc` 用 `alloc::alloc`，`share`/`unshare` 恒等/空操作。
- 测试目标设备用真实 `try_new` 初始化（驱动真正执行 feature 协商、queue_set、RX/TX buffer 填充）。

## 操作步骤

1. **实现本地 fake Transport**（`impl virtio_drivers::Transport`）：
   - `read_device_features` 返回 `0`（flags 通知模式，避免 EVENT_IDX 干扰 ownership 测试）。
   - `queue_set` 记录每个 queue 的 `device_area`（= used ring 基地址），保存到 `used_rings[queue]`。
   - `config_space::<T>()` 返回本地 `#[repr(C)]` net Config（`mac: [u8;6]`、`status: u16`、`max_virtqueue_pairs: u16`、`mtu: u16`）的指针，cast 到任意 `T` —— 与驱动私有 `Config` 布局一致。
   - 其余方法（status/notify/queue_used/ack）按最小实现。

2. **模拟设备完成 TX**：`complete_tx(token, len)` 向 send queue 的 used ring 写入 used elem 并推进 `used_idx`。used ring 布局：`flags(u16)@0`、`idx(u16)@2`、`used_elem[id:u32, len:u32]@4+8*slot`、`used_event(u16)` 在末尾；`slot = used_idx % QS`。驱动 `peek_used`/`pop_used` 读同一位置，因此完成顺序必须与驱动消费顺序一致。

3. **访问 transport**：`#[cfg(test)]` 方法对依赖 crate 不可见，所以在 `VirtIONetRaw` 加非门控公开 `transport_mut()`（注明测试用途）或在 adapter 加本地 `#[cfg(test)]` seam，二选一。

4. **驱动 post-accept invariant**：真实 `VirtQueue::add` 只会返回 free descriptor，"occupied/out-of-range token" 无法自然触发。用 adapter 本地 `#[cfg(test)] forced_tx_token: Option<u16>` seam：`submit_tx`/`transmit` 的 token 获取路径先真实 `transmit_begin`（消耗 descriptor），再返回伪造 token 给 install 逻辑。

5. **断言真实 ledger**：测试模块与 adapter 同 crate，可直接断言私有字段（`free_tx_bufs`、`tx_slots`）。`DevError` 只 derive `Debug`，用 `matches!` 而非 `assert_eq!`。

6. **观察 RED→GREEN**：先对当前行为写断言（如 runtime exhaustion 应为 `Again`、post-accept 不应 panic、fault 后 `can_transmit` 为 false），确认 RED 后实现，再 GREEN。

## 验证

- `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net` → 9 passed（Iteration 001 实测）。
- 回归：`cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline`（7 passed）、`cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib -- --nocapture`（36 passed）。
- 关键输出：无 panic；exhaustion 返回 `Again`；post-accept invariant 返回稳定 `BadState` 且 buffer 守恒。

## 失败处理

- **`cfg(test)` seam 在依赖中不可见** → 改用非门控公开访问器（注明测试用途）或 adapter 本地 seam；不要尝试 `#[cfg(any(test, feature))]` 特性化 fake。
- **无法让真实队列返回 QueueFull**（1:1 buffer/descriptor，`try_new` 强制 `max_queue_size >= QS`）→ 用 net-local mapper 单元测试 + 运行期 exhaustion（free 耗尽 → `Again`）共同见证，不要伪造 `add` 失败。
- **completion error 无法注入**（`peek_used` 返回的 id 与 `pop_used` 读到的一致，WrongToken 结构性不可达）→ 用 duplicate/unknown completion（fake 写越界或重复 id）证明"ledger 在错误 completion 下原状保留"。
- **驱动真实 fixture 编译失败** → 先检查 `dev.inner`（`VirtIONetRaw.transport` 是私有字段，测试模块需经由公开访问器）。

## 回滚

- 测试与 fixture 是纯测试代码：删除测试模块与 seam 即可回到 helper-based 测试，不影响产品行为。
- 产品侧 seam 仅两处：`VirtIONetRaw::transport_mut()`（公开访问器）与 adapter `forced_tx_token` 字段（`#[cfg(test)]`）；移除后产品代码不依赖它们。
- 不可回滚项：无 —— 本路径不修改 DMA 布局、queue size、feature 协商或 Cargo registry。

## 证据

- Act Response：`openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/iterations/001-tx-contract-stabilization.md`（Verification Evidence 表）。
- 代码：`crates/axdriver_virtio/src/net.rs`（tests 模块，TestHal/FakeTransport/complete_tx）；`crates/virtio-drivers/src/device/net/dev_raw.rs::transport_mut`。
- 限制：只证明 host 侧真实 adapter 的软件 ownership 转移；不构成 QEMU 或真板证据。
