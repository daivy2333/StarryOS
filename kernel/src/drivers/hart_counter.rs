// kernel/src/drivers/hart_counter.rs

//! 纯、宿主可编译的自洽 (event, last-hart, cumulative-mask) 观测寄存器（design D6）。
//!
//! 为 UART ISR 入口与 RX/TX copier 各自维护一个单调、Relaxed 的观测寄存器，供
//! QEMU 快照读取。`record` 先把 bit 折叠进累计 mask，再发布 last hart 并递增事件
//! 计数，因此凡是观察到 `last ∈ mask` 的读取者都看到内部一致的视图。`read` 用有界
//! 重试避免返回撕裂的（last 不在 mask 中）tuple；重试耗尽时强制返回自洽回退值。
//!
//! 该值绝不参与同步决策，是纯观测 telemetry。模块不依赖任何内核/平台符号，因此
//! 宿主测试工具（`tests/ms04-async-rx-host-harness.rs`）可 `#[path]` 包含同一份
//! 源码，运行与产品完全相同的 record/read 并发模型。

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// 有界重试次数：组装 (last, mask) 自洽视图时最多尝试多少次。
const RETRY_BOUND: usize = 1024;

/// “从未记录”的 last hart 哨兵。
pub const UNKNOWN_HART: usize = usize::MAX;

/// `mask` 的宽度：`1u64 << hart` 中的 hart 必须 `< HART_CAPACITY`。
/// 与 run-queue readiness bitset / `MAX_CPU_NUM` 一致（见 design D4）。
const HART_CAPACITY: usize = 64;

/// 单调观测寄存器：(event-count, last-hart, cumulative-hart-mask)。
#[derive(Debug)]
pub struct HartCounter {
    last: AtomicUsize,
    mask: AtomicU64,
    events: AtomicU64,
}

impl HartCounter {
    /// 构造一个空寄存器（从未记录任何观测）。
    pub const fn new() -> Self {
        Self {
            last: AtomicUsize::new(UNKNOWN_HART),
            mask: AtomicU64::new(0),
            events: AtomicU64::new(0),
        }
    }

    /// 记录一次在 `hart` 上的观测。
    ///
    /// 更新顺序固定为：累计 mask `fetch_or` → last store → events `fetch_add`。
    /// 这保证 `read` 通过“last ∈ mask”检查即可得到自洽视图；累计 mask 单调增长，
    /// 因此迟到的读取只会看到自洽、稍旧的 tuple。
    pub fn record(&self, hart: usize) {
        debug_assert!(
            hart < HART_CAPACITY,
            "hart {hart} exceeds HART_CAPACITY {HART_CAPACITY}"
        );
        self.mask.fetch_or(1u64 << hart, Ordering::Relaxed);
        self.last.store(hart, Ordering::Relaxed);
        self.events.fetch_add(1, Ordering::Relaxed);
    }

    /// 读取一个自洽的 `(last, mask, events)` 视图。
    ///
    /// 保证返回的 tuple 只属于两类之一：
    /// - 空：`last == UNKNOWN_HART && mask == 0 && events == 0`（从未记录）；
    /// - 非空：`last < 64` 且 `mask` 包含 bit `last`（tuple 中命名的 hart 一定
    ///   已在累计 mask 中）。
    ///
    /// 首次 `record` 先把累计 mask 发布、最后才发布 `last`，因此“last 已未知但
    /// mask 非零”的中间态在重试中等待稳定，不对外返回撕裂的 tuple。
    pub fn read(&self) -> (usize, u64, u64) {
        for _ in 0..RETRY_BOUND {
            let last = self.last.load(Ordering::Relaxed);
            let mask = self.mask.load(Ordering::Relaxed);
            let events = self.events.load(Ordering::Relaxed);
            if last == UNKNOWN_HART {
                if mask == 0 && events == 0 {
                    // 真正从未记录：严格空 tuple。
                    return (UNKNOWN_HART, 0, 0);
                }
                // mask 已有 bit 或 events 已累加但 last 尚未发布：首次 record 的
                // 中间态或 Relaxed 乱序。此时既非空也不是 'last ∈ mask' 的非空，
                // 重试等待其稳定，绝不返回该撕裂态。
                continue;
            }
            if (mask & (1u64 << last)) != 0 {
                return (last, mask, events);
            }
        }
        // 有界重试耗尽：强制返回自洽回退。空 mask 恒返回严格空 tuple；否则 last
        // 取 mask 最低置位，保证 'last ∈ mask'。events 只读单调；瞬态乱序下宁可
        // 丢弃一次计数也不返回不符合不变量要求的撕裂 tuple。
        let mask = self.mask.load(Ordering::Relaxed);
        if mask == 0 {
            return (UNKNOWN_HART, 0, 0);
        }
        let last = mask.trailing_zeros() as usize;
        (last, mask, self.events.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod test_hooks {
    use super::*;

    impl HartCounter {
        /// 测试专用：只发布累计 mask 位、不发布 `last`，用于确定性复现首次
        /// `record` 的 mask-before-last 中间态（Cycle 002 A3 RED）。产品路径不
        /// 编译（`#[cfg(test)]`），纯测试逻辑不进入内核。
        pub fn record_partial_mask(&self, hart: usize) {
            debug_assert!(hart < HART_CAPACITY);
            self.mask.fetch_or(1u64 << hart, Ordering::Relaxed);
        }
    }
}
