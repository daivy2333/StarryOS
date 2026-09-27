// kernel/src/drivers/uart_snapshot_types.rs

//! QEMU-only UART SMP placement/progress 快照的 wire 布局（design D6）。
//!
//! 独立的 `repr(C)` 纯类型：只含原生字段、不依赖内核/平台符号，因此内核
//! `uart_smp_snapshot.rs` 与宿主测试工具（`#[path]` 包含同一份源码）使用字节一致
//! 的同一布局。旧 `UART_TXDBG_*` 命令和结构不受影响。
//!
//! **Wire 安全性（Cycle 002 / A3）**：`#[repr(C)]` 下混合 `u32/i32/u64/u8` 会在
//! 字段之间插入隐式 padding；`starry_vm::VmMutPtr::vm_write` 会把整对象按字节复制
//! 给 guest，未初始化的 padding 字节会形成不确定 wire frame 并可能泄露内核栈字节。
//! 因此该类型用显式 `_r0.._r4` reserved 字节数组填满所有对齐空洞，使 `size_of` 恰好
//! 等于“全部字段字节和”（无隐式/尾部 padding）；`snapshot()` 通过 [`Self::zeroed`]
//! 显式把所有 reserved 字节置零，并经 [`Self::wire_bytes`] 输出完全定义的 wire frame。

/// `UartSmpSnapshot` 的 magic 前缀；与 ioctl 命令 `UART_SMP_SNAPSHOT` 数值一致，
/// 用于宿主侧（或 kernel 消费者）校验读取到的确实是该 wire 类型。
pub const UART_SMP_SNAPSHOT_MAGIC: u32 = 0x5553_4d31;

/// magic 前缀在字节 0、长度为一个 u32（用于 ABI "prefix" 断言）。
/// 仅被宿主测试工具消费（该 crate 是纯 ABI 载体），故允许 dead_code。
#[allow(dead_code)]
pub const UART_SMP_SNAPSHOT_PREFIX: usize = core::mem::size_of::<u32>();

/// 独立 QEMU-only UART SMP placement/progress 快照。
///
/// 复合 placement tuple（IRQ/copier/affinity/mask）在单帧中读取；
/// `irq_events`/`rx_polls`/`tx_polls` 是每方向单调观测，使受控 UART-only 窗口能
/// 把远程 enqueue/IPI/copier resume 归因到 RX 或 TX。该结构取代不了旧 TXDBG。
///
/// `_r0.._r4` 是显式 reserved 字节（对齐空洞），由 [`Self::zeroed`] 置零，因此
/// wire 的每一个字节都由字段或显式零值定义，`vm_write` 复制整对象时不会携带未初始
/// 化 padding。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UartSmpSnapshot {
    /// Magic 前缀（`UART_SMP_SNAPSHOT_MAGIC`）。
    pub magic: u32,
    /// Configured hart 数（`axhal::cpu_num()`）。
    pub configured_harts: u32,
    /// 已发布 schedulable run-queue 的 bitmask。
    pub schedulable_mask: u64,
    /// RX copier 被固定的 singleton hart（placement 时记录）。
    pub rx_affinity: u64,
    /// TX copier 被固定的 singleton hart（placement 时记录）。
    pub tx_affinity: u64,
    /// UART ISR 最近一次实际执行 hart（`UNKNOWN_HART` 表示未触发）。
    pub irq_last_hart: i32,
    /// 对齐空洞：`irq_last_hart`(i32@32) → `irq_hart_mask`(u64@40)。
    _r0: [u8; 4],
    /// UART ISR 实际执行 hart 的累计 bitmask。
    pub irq_hart_mask: u64,
    /// RX copier 最近一次 poll 的实际 hart（`UNKNOWN_HART` 表示未运行）。
    pub rx_last_hart: i32,
    /// 对齐空洞：`rx_last_hart`(i32@48) → `rx_hart_mask`(u64@56)。
    _r1: [u8; 4],
    /// RX copier 实际 poll hart 的累计 bitmask。
    pub rx_hart_mask: u64,
    /// TX copier 最近一次 poll 的实际 hart（`UNKNOWN_HART` 表示未运行）。
    pub tx_last_hart: i32,
    /// 对齐空洞：`tx_last_hart`(i32@64) → `tx_hart_mask`(u64@72)。
    _r2: [u8; 4],
    /// TX copier 实际 poll hart 的累计 bitmask。
    pub tx_hart_mask: u64,
    /// RX ring 当前占用字节数。
    pub rx_occupancy: u32,
    /// TX ring 当前可用（vacant）字节数。
    pub tx_vacancy: u32,
    /// 四阶段 completion：ring empty。
    pub ring_empty: u8,
    /// 四阶段 completion：copier 处于 poll cycle。
    pub copier_active: u8,
    /// 对齐空洞：`copier_active`(u8@89) → `staged_bytes`(u32@92)。
    _r3: [u8; 2],
    /// 四阶段 completion：从 ring 弹出但尚未确认发送的字节。
    pub staged_bytes: u32,
    /// 四阶段 completion：UART 发送器空（TEMT）。
    pub transmitter_empty: u8,
    /// 对齐空洞：`transmitter_empty`(u8@96) → `irq_events`(u64@104)。
    _r4: [u8; 7],
    /// UART ISR 入口的单调事件计数（可归因的 "event"）。
    pub irq_events: u64,
    /// RX copier poll 的单调计数（RX 可归因的 resume/progress）。
    pub rx_polls: u64,
    /// TX copier poll 的单调计数（TX 可归因的 resume/progress）。
    pub tx_polls: u64,
    /// 已发送的 reschedule IPI 计数（仅 `smp` 有意义；否则为 0）。
    pub ipi_sent: u64,
    /// 已接收的 reschedule IPI 计数（仅 `smp` 有意义；否则为 0）。
    pub ipi_received: u64,
    /// 被拒绝的无效 affinity placement 计数。
    pub affinity_rejects: u64,
    /// RX copier 迁移阶段（`uart_migration_logic::MigrationPhase` 判别值）。
    pub rx_migration_state: u8,
    /// TX copier 迁移阶段（`uart_migration_logic::MigrationPhase` 判别值）。
    pub tx_migration_state: u8,
    /// 对齐空洞：`tx_migration_state`(u8@153) → `rx_migration_from`(u32@156)。
    _r5: [u8; 2],
    /// RX 迁移原 singleton hart。
    pub rx_migration_from: u32,
    /// RX 迁移第二个允许 hart。
    pub rx_migration_to: u32,
    /// TX 迁移原 singleton hart。
    pub tx_migration_from: u32,
    /// TX 迁移第二个允许 hart。
    pub tx_migration_to: u32,
    /// 对齐空洞：`tx_migration_to`(u32@168) → `rx_migration_requested_polls`(u64@176)。
    _r6: [u8; 4],
    /// RX 迁移请求时刻的 poll 计数（加宽前）。
    pub rx_migration_requested_polls: u64,
    /// RX 在 second hart 上被直接观察到的 poll 计数。
    pub rx_migration_observed_polls: u64,
    /// RX 迁移目标选择/提交被拒绝的次数。
    pub rx_migration_rejects: u64,
    /// TX 迁移请求时刻的 poll 计数（加宽前）。
    pub tx_migration_requested_polls: u64,
    /// TX 在 second hart 上被直接观察到的 poll 计数。
    pub tx_migration_observed_polls: u64,
    /// TX 迁移目标选择/提交被拒绝的次数。
    pub tx_migration_rejects: u64,
}

impl UartSmpSnapshot {
    /// wire frame 总字节数。必须等于 `size_of::<UartSmpSnapshot>()`（无隐式 padding）。
    pub const WIRE_SIZE: usize = core::mem::size_of::<Self>();

    /// 构造一个所有字节（含 reserved）均为零的 wire 帧。
    ///
    /// 这是唯一合法的“empty wire”来源：显式逐字段置零，不依赖 struct literal 自动
    /// 清 padding（该类型本就没有隐式 padding，但显式构造仍保持可证安全性）。
    pub const fn zeroed() -> Self {
        Self {
            magic: 0,
            configured_harts: 0,
            schedulable_mask: 0,
            rx_affinity: 0,
            tx_affinity: 0,
            irq_last_hart: 0,
            _r0: [0; 4],
            irq_hart_mask: 0,
            rx_last_hart: 0,
            _r1: [0; 4],
            rx_hart_mask: 0,
            tx_last_hart: 0,
            _r2: [0; 4],
            tx_hart_mask: 0,
            rx_occupancy: 0,
            tx_vacancy: 0,
            ring_empty: 0,
            copier_active: 0,
            _r3: [0; 2],
            staged_bytes: 0,
            transmitter_empty: 0,
            _r4: [0; 7],
            irq_events: 0,
            rx_polls: 0,
            tx_polls: 0,
            ipi_sent: 0,
            ipi_received: 0,
            affinity_rejects: 0,
            rx_migration_state: 0,
            tx_migration_state: 0,
            _r5: [0; 2],
            rx_migration_from: 0,
            rx_migration_to: 0,
            tx_migration_from: 0,
            tx_migration_to: 0,
            _r6: [0; 4],
            rx_migration_requested_polls: 0,
            rx_migration_observed_polls: 0,
            rx_migration_rejects: 0,
            tx_migration_requested_polls: 0,
            tx_migration_observed_polls: 0,
            tx_migration_rejects: 0,
        }
    }

    /// 输出完全定义的 wire frame：按小端编码逐字段写入，reserved 字节保持为零，
    /// 长度恰为 [`Self::WIRE_SIZE`]。供 ioctl（`VmMutPtr::vm_write`）直接复制，
    /// 不从包含隐式 padding 的对象做整对象 guest copy。
    pub fn wire_bytes(&self) -> [u8; Self::WIRE_SIZE] {
        let mut b = [0u8; Self::WIRE_SIZE];
        let mut put = |off: usize, bytes: &[u8]| {
            b[off..off + bytes.len()].copy_from_slice(bytes);
        };
        put(0, &self.magic.to_le_bytes());
        put(4, &self.configured_harts.to_le_bytes());
        put(8, &self.schedulable_mask.to_le_bytes());
        put(16, &self.rx_affinity.to_le_bytes());
        put(24, &self.tx_affinity.to_le_bytes());
        put(32, &self.irq_last_hart.to_le_bytes());
        put(40, &self.irq_hart_mask.to_le_bytes());
        put(48, &self.rx_last_hart.to_le_bytes());
        put(56, &self.rx_hart_mask.to_le_bytes());
        put(64, &self.tx_last_hart.to_le_bytes());
        put(72, &self.tx_hart_mask.to_le_bytes());
        put(80, &self.rx_occupancy.to_le_bytes());
        put(84, &self.tx_vacancy.to_le_bytes());
        put(88, &[self.ring_empty]);
        put(89, &[self.copier_active]);
        put(92, &self.staged_bytes.to_le_bytes());
        put(96, &[self.transmitter_empty]);
        put(104, &self.irq_events.to_le_bytes());
        put(112, &self.rx_polls.to_le_bytes());
        put(120, &self.tx_polls.to_le_bytes());
        put(128, &self.ipi_sent.to_le_bytes());
        put(136, &self.ipi_received.to_le_bytes());
        put(144, &self.affinity_rejects.to_le_bytes());
        put(152, &[self.rx_migration_state]);
        put(153, &[self.tx_migration_state]);
        put(156, &self.rx_migration_from.to_le_bytes());
        put(160, &self.rx_migration_to.to_le_bytes());
        put(164, &self.tx_migration_from.to_le_bytes());
        put(168, &self.tx_migration_to.to_le_bytes());
        put(176, &self.rx_migration_requested_polls.to_le_bytes());
        put(184, &self.rx_migration_observed_polls.to_le_bytes());
        put(192, &self.rx_migration_rejects.to_le_bytes());
        put(200, &self.tx_migration_requested_polls.to_le_bytes());
        put(208, &self.tx_migration_observed_polls.to_le_bytes());
        put(216, &self.tx_migration_rejects.to_le_bytes());
        // _r0(36..40), _r1(52..56), _r2(68..72), _r3(90..92), _r4(97..104),
        // _r5(154..156), _r6(172..176) stay at the zero-initialized positions:
        // reserved bytes are explicitly zero.
        b
    }

    /// 校验 magic 前缀，识别有效快照帧。
    /// 消费方是宿主布局工具与 2.6 QEMU smoke，二者不常驻 kernel 常规编译面，
    /// 故允许 dead_code。
    #[allow(dead_code)]
    pub fn is_valid_frame(&self) -> bool {
        self.magic == UART_SMP_SNAPSHOT_MAGIC
    }
}
