## Context

Q18 来自 `.claude/analysis/platform-parameter-decoupling.md` 和 ADR-044。当前 StarryOS 已有 `MYPLAT` / `PLAT_CONFIG` / `axconfig` / `axplat` 平台选择基础，但 async UART 初始化没有消费这套平台事实，而是在 `uart_init.rs` 内部写死 QEMU UART 参数。

CodeGraph 探索确认：

- `get_uart_mmio_virt()` 只影响 `uart_isr_wrapper()` 和 `init_uart_hardware()`。
- `init_uart_hardware()` 是 `entry::init()` 中的 async UART 初始化入口。
- `ntty_async.rs` 和 TTY line discipline 已经通过 `TtyRead` / `TtyWrite` 抽象工作，不应纳入 Q18 改动。

## Goals / Non-Goals

**Goals:**

- 建立 StarryOS platform descriptor 或等价集中配置。
- 让 QEMU UART facts 从 `uart_init.rs` 移出。
- 新增 early console 抽象和 QEMU U8 baseline。
- 保持 QEMU 构建与启动行为。
- 为 Q19 Lichee D1 `DwApbUart32EarlyConsole` 留出接口边界。

**Non-Goals:**

- 不实现 Q19 Lichee boot image/smoke test。
- 不实现 VisionFive2 真板诊断。
- 不改上层 TTY 行为。
- 不改 `uart_16550` backend access width。

## Design

### D1: Build-time platform descriptor

Q18 使用 build-time descriptor，不做运行时 DTB 解析。descriptor 至少表达：

```rust
pub struct PlatformDescriptor {
    pub name: &'static str,
    pub memory: MemoryLayout,
    pub kernel: KernelImageLayout,
    pub console: ConsoleConfig,
    pub interrupt: InterruptConfig,
    pub timer: TimerConfig,
    pub boot: BootImageConfig,
}

pub struct ConsoleConfig {
    pub kind: ConsoleKind,
    pub base_paddr: usize,
    pub irq: Option<usize>,
    pub reg_stride: u8,
    pub reg_width: MmioAccessWidth,
    pub baud: u32,
}
```

Q18 must include a QEMU descriptor matching current behavior:

| Field | QEMU value |
|-------|------------|
| `console.kind` | `Ns16550` |
| `console.base_paddr` | `0x10000000` |
| `console.irq` | `Some(10)` |
| `console.reg_stride` | `1` |
| `console.reg_width` | `U8` |
| `boot` | direct QEMU |

### D2: Early console is separate from async UART

Early console must not depend on:

- ring buffer
- async task
- IRQ
- PLIC
- `/dev/console`
- rootfs

Minimal trait:

```rust
pub trait EarlyConsole {
    fn putchar(&self, ch: u8);
    fn write_str(&self, s: &str);
}
```

Q18 implements the QEMU `Ns16550U8EarlyConsole` baseline. `DwApbUart32EarlyConsole` may be defined as a type/interface boundary, but Q18 does not need to prove it on hardware.

### D3: async UART consumes descriptor

`uart_init.rs` should use `platform::descriptor().console` or equivalent. After Q18, no new platform-specific base/irq/stride/access-width constants should be introduced in `uart_init.rs`.

Q18 does not need to make async UART fully generic for D1/VisionFive2. It only makes QEMU behavior descriptor-driven and prepares the early-console path.

## Requirements Traceability Matrix

| Requirement | Task(s) | Coverage | Simplification | Status |
|-------------|---------|----------|----------------|--------|
| R1: Platform facts centralized | 1.1, 1.2, 2.1, 2.2 | 100% | None | Covered |
| R2: QEMU behavior preserved | 2.1, 2.2, 5.1, 5.2 | 100% | None | Covered |
| R3: Early console separated from async UART | 3.1, 3.2, 3.3 | 100% | None | Covered |
| R4: No Lichee/VF2 execution in Q18 | 4.1, 4.2 | 100% | U32 implementation is interface-only | Covered |
| R5: Verification before implementation claims | 5.1, 5.2, 5.3 | 100% | None | Covered |

## Test / Witness Plan

Q18 Phase 3 must start each task with current-state evidence. Planned witnesses:

| Witness | Command | Expected use |
|---------|---------|--------------|
| Current build | `make ARCH=riscv64 build` | Establish QEMU baseline before code changes |
| Rust check | `cargo check --package starry-kernel` or repo-equivalent command | Fast compile witness after descriptor changes |
| QEMU run smoke | `make ARCH=riscv64 run` | Verify boot and existing async UART behavior |
| Source audit | CodeGraph impact/callers for `get_uart_mmio_virt` and `init_uart_hardware` | Confirm no unplanned TTY impact |

Exact commands may be adjusted in Phase 3 if the repo's package names require it, but every implementation task must include fresh current/new-state output.

## Risks / Trade-offs

- **Descriptor overgrowth**: keep fields limited to Q18/Q19 needs. Do not add full DTB model.
- **Early console duplicate with axplat ConsoleIf**: acceptable because Q18 needs StarryOS-owned bring-up output independent of async UART; it must remain small.
- **QEMU-only proof**: Q18 only proves structure and QEMU preservation. D1/VisionFive2 hardware behavior remains Q19/Q20.
- **Access width gap**: Q18 must not pretend stride is enough for D1/VF2. It records `reg_width`, but backend implementation remains future work.

## Stop Point

This change is prepared for audit only. Phase 3 execution must not start until the user approves this design and tasks.
