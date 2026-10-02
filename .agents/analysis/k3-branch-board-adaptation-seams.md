# K3 分支真板适配接缝与待证事实（待迁入 OpenSpec）

> Snapshot: `/home/daivy/projects/serial/work/StarryOS/.agents/docs/SNAPSHOT.md`
> Captured at: 2026-09-29
> StarryOS: branch `k3`, revision `160d7967b585f2319e89188d0644c16f3c79b04b`, clean worktree
> Reference: `/home/daivy/projects/serial/work/k3`, revision `e6d055720de5f13f3cde6026817e03412c8acda6`, dirty worktree; this investigation made no changes there
> See also: `.agents/analysis/k3-reference-repository-migration-assessment.md` (R62), `.agents/analysis/arceos-true-board-validation.md`

## 目标与范围

为 StarryOS `k3` 分支后续真板适配回答三个主题：目标板事实、启动和平台接入、GMAC 到现有异步网络的接入。本文只提供调查输入，不替代 OpenSpec Plan Context。当前 `openspec list` 输出 `No active changes found`；`.agents/docs/tasks.md` 的 MS09/T13 仍待真板现场事实。用户已确认目标板型号为 CoM260 Kit，但未提供板卡修订或运行时 DTB。

## 已确认事实

1. 新分支尚无 K3 构建和平台入口。根 `Cargo.toml:60-113` 只有 QEMU、D1、VF2 和 SMP feature；`Makefile:286-302` 没有 K3 目标；`src/main.rs:20-27` 只显式链接 VF2 和 D1 平台 crate。`make/platform.mk:22-59` 的 RISC-V 默认平台是 QEMU，指定 `MYPLAT` 则需要可解析的 `PLAT_CONFIG`。分支名不等于硬件支持。
2. 当前 `kernel/src/platform/mod.rs:30-40::descriptor()` 仅在 `lichee-d1` 选 D1，其余返回 QEMU descriptor；`kernel/src/platform/descriptor.rs:26-91` 中断配置只表达 PLIC 基址，`virtio_net` 是 VirtIO 专属事实。`kernel/src/entry.rs:86-109` 对非 D1 构建调用 QEMU UART 与 `virtio_net_irq::init_virtio_net_irq_diag()`；`kernel/src/drivers/mod.rs:30-33` 对非 D1 编译 VirtIO handler。新增 K3 feature 若不分流，会落入 QEMU 路径。
3. D1 `crates/axplat-riscv64-lichee-d1/src/boot.rs:21-101` 示范 `_start` 保留 `a0` hartid、`a1` DTB，建立栈/页表并转入 `axplat::call_main`；同 crate 的 `init.rs`、`mem.rs` 给出 InitIf/MemIf 实现形状。D1 的物理地址、Sv39 映射和 T-Head PTE 位不能作为 K3 参数。
4. K3 参考仓库 `docs/platform/com260-board-resources.md:2` 将 SoC、模组、Kit 分层。CoM260 Kit 有 UART0 调试口与 RJ45；SoC 能力为 8 X100 + 8 A100 AP 核，但当前固件可用/可调度 hart 集合未由此证明。`docs/boot/com260-image-and-dts.md:43-52,120-141` 列出多个 CoM260 DTS 候选，未唯一映射当前实物板。
5. 参考仓库 `docs/boot/com260-boot-chain.md:82-111` 记录 local boot 的 BootROM → FSBL → ESOS → OpenSBI → U-Boot → payload。`docs/boot/k3-ram-boot-and-fastboot.md:63-88` 的 RAM stage/FIT `load`/`entry` 是不同地址空间；`0x180000000` stage、`0x140000000` kernel、`0x138000000` FDT 只是固定第三方示例，须以目标板 DRAM、reserved-memory 和 U-Boot 活动区检查。
6. 参考仓库 `docs/serial/com260-uart.md:123-173` 给出 `uart0` 候选：`0xd4017000`、IRQ 42、stride 4、32-bit MMIO；Kit 12 pin 调试排针为 115200-8-N-1。StarryOS `kernel/src/platform/console.rs:7-45` 已分离 stride 与访问宽度，`early_console.rs:34-181` 有 8-bit NS16550 和 32-bit stride-4 polling 输出，但 K3 PXA 行为、当前 bootloader 的 clock/pinmux 状态仍未验证。
7. 参考仓库 `docs/interrupts/k3-interrupt-and-time.md:28-67` 的官方 DTS 交叉证据是 AP CLINT `0xe081c000`、IMSIC `0xe0400000`、APLIC `0xe0804000`，外设线中断经 APLIC→IMSIC→hart。`others/Rt-Async-AMP/tgoskits/platforms/somehal/src/arch/riscv64/imsic_aplic.rs` 展示 FDT probe、EID 分配和 `stopei` claim/complete，但不是 StarryOS 的 `axplat::irq::IrqIf` 实现，也没有本项目真板 delivery 证据。
8. 参考仓库 `docs/network/com260-gmac-phy.md:91-137` 将已观察 CoM260 DTS 网络口关联到 `&eth1`、RGMII、PHY 地址 1；基础款和 Kit V02 的 PHY label/FIFO 等字段不同。`eth1` 的 MMIO `0xcac82000`/`0x2000` 来源是固定第三方 IFX DTS，不是已确认的当前板事实。
9. 本仓库 `crates/axdriver_net/src/lib.rs:227-389` 定义 `NetDriverOps`、`NetTxQueue`、`NetQueueControl`：TX submit 成功后 buffer 归驱动，每次 reclaim 返回带 epoch 的 `TxCookie`；arm 后需要重新检查 completion。`NetRecoveryControl:172-202` 定义有界恢复步进。`crates/axnet/src/lib.rs:93-152` 安装 Service；`kernel/src/drivers/virtio_net_irq.rs:168-290` 在 secondary-ready 后固定 runner 和唯一 queue owner。`crates/axnet/src/async_rx.rs:1229-1252,2899-2925` 的 ISR 事件发布与 register→arm→recheck 是等待契约。
10. 第三方 `others/Rt-Async-AMP/tgoskits/drivers/ax-driver/src/net/k3_gmac/mod.rs:33-112` 用 `rdrive` FDT probe 和 `rd_net` 注册；`core.rs:59-86,161-210` 管理 descriptor ring、bus-address 队列和 DMA status；`queue.rs:40-160` 返回 `rd_net` 队列/IRQ 事件，不返回本仓 `TxCookie`/epoch。它不能直接注册成 StarryOS 的异步后端。`queue.rs:98-105` 的 IRQ `try_lock` 失败直接返回空事件；`docs/network/k3-gmac-dma-irq.md:156-175` 将最后一次完成后的确定性重查列为未闭合风险。第三方 DMA reset 超时后继续初始化的行为也不证明本仓 recovery 契约。
11. 参考仓库 `docs/dma/k3-dma-and-memory-ownership.md:1-6` 区分 GMAC 内建 DMA、通用 DMA 和 AP↔RP 共享内存；`docs/dma/k3-cache-pma-address-translation.md:4-5` 对目标板 coherency、IOMMU domain、device address 仍保留未知。descriptor clean/invalidate 不能替代 data-buffer cache 可见性或 CPU PA→DMA bus address 证明。

## 推断与未确认项

用户已确认当前目标实物板为 CoM260 Kit。若其保留板载 U-Boot/OpenSBI，首轮可以考虑只将 StarryOS 作为 AP RAM payload 接入；这仍依赖板卡修订、运行 DTB、SBI 能力、内存区间与复位恢复的现场证据。不能从参考仓库名或 FIT 示例地址推出这些条件已经满足。

必须由目标板或固件裁决：板修订和启动介质；运行 DTB 与 `eth1` 节点；TIME/IPI/HSM/SRST 能力；boot hart 和可调度 X100/A100 集合；DRAM/reserved/U-Boot 内存图；UART clock/pinmux；GMAC MMIO/IRQ/clock/reset/PHY；DMA aperture、cache line、IOMMU 与 coherency。上述任一实质未知会改变 Plan 的接口、地址、错误路径或 Acceptance。

## 依赖链与失败路径

```text
目标板身份/DTB/固件 → FIT 区间与 RAM 引导 → axplat _start/页表 → polling 首字节
  → SBI/time/可用 hart → APLIC/IMSIC delivery → GMAC 寄存器/MDIO/link
  → DMA descriptor 与 data-buffer ownership → 轮询 RX/TX → IRQ + 异步 queue owner
  → reset/link/epoch 恢复 → 真板 SMP
```

首字节前的失败优先检查 FIT/load/entry、页表和 UART；寄存器全零/全一先检查 MMIO/clock/reset；一次 IRQ 后停顿分开检查 APLIC/IMSIC、设备 cause/ack 与重复投递；ring 停顿分开检查 bus address、OWN、tail、cache；异步永久 Pending 检查最后事件的 arm/recheck；reset 后资源复用须证明 DMA 停止和 owner epoch。QEMU `SMP=16` 和第三方 `rd_net` 运行结论均不能替代这些目标板证据。

## 测试、验证入口与影响面

本次实际只运行只读 `git status --short --branch`、`git rev-parse HEAD`、`openspec list`、`rg` 和文件读取；没有构建、测试、QEMU、FIT、Fastboot 或真板操作。现有 `kernel/src/platform/early_console.rs` 单元测试、D1 `time_math.rs` 测试、`axdriver_net/src/lib.rs` trait/model 测试和 `axnet/src/async_rx.rs` queue/recovery/wait 模型测试可作为软件契约验证入口，但不证明 K3 硬件。

MS09/T13–T15 应先关闭板型事实、RAM boot、首字节、寄存器和 PHY/link；MS10/T16 再证明设备 IRQ；MS11/T17–T19 证明 DMA/cache 与轮询 RX/TX；MS12/T20–T21 才接异步数据面；MS13/14 处理真板恢复与 SMP。后续 OpenSpec Plan 应重新核对目标板与代码状态，写 BDD、RTM、测试见证和当前 Cycle；本文不使任何 Gate 自动通过。

影响面为根 feature/Makefile、K3 axplat 和镜像/FIT、kernel 平台描述与 entry 分流、AP AIA/SBI、GMAC backend 和 `axdriver_net` 适配。现有代码证据不要求提前改动 QEMU VirtIO transport 或 socket readiness。

## 关键文件

- StarryOS: `Cargo.toml`, `Makefile`, `src/main.rs`, `make/platform.mk`, `kernel/src/platform/{mod,descriptor,console,early_console}.rs`, `kernel/src/entry.rs`, `kernel/src/drivers/{mod,virtio_net_irq}.rs`, `crates/axdriver_net/src/lib.rs`, `crates/axnet/src/{lib,async_rx}.rs`。
- K3 参考仓库: `docs/boot/{com260-boot-chain,com260-image-and-dts,k3-ram-boot-and-fastboot}.md`, `docs/serial/com260-uart.md`, `docs/interrupts/k3-interrupt-and-time.md`, `docs/network/{com260-gmac-phy,k3-gmac-dma-irq}.md`, `docs/dma/{k3-dma-and-memory-ownership,k3-cache-pma-address-translation}.md`, `others/Rt-Async-AMP/tgoskits/drivers/ax-driver/src/net/k3_gmac/{mod,core,queue}.rs`。
