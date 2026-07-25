# 真板 bring-up 阶梯

## 适用范围

- 新板（如 VisionFive2）的首次适配：从零到 async I/O benchmark
- 已有真板验证此阶梯有效（历史记录见 archive carrier）
- 不适用于：已有平台的增量修改、QEMU 开发

## 核心原则

> 每次只开放一个变量。第一个成功的输出就是 Gate——在它之前不要引入 IRQ、async task、rootfs、benchmark。（历史教训：同时打开太多子系统 → 失败原因不可定位。参见 archive carrier）

## 阶梯

### L0: 平台基线冻结

**目标**：确认不再需要从官方 Linux 继续采集数据。

- 整理硬件事实：SoC/CPU/RAM/serial base/IRQ/MMIO 宽度/boot 分区格式/启动链
- 对照 public datasheet、DTS、官方 Linux 采集
- 输出：`docs/<board>-bringup.md` 或 platform descriptor
- **Gate**：事实覆盖 L1-L4 所需全部参数，无"还需要连上官方 Linux 看看"的未决项

### L1: Boot Image 工具链

**目标**：能解析、备份、重新打包 boot image。

- 解析官方 boot image header（magic、kernel_addr、page_size）
- 编写或复用打包工具（平台相关工具按板级选择）
- 备份官方 boot 分区并保存到开发机
- **Gate**：官方 header 字段可复现 parse → pack → inspect 完整闭环；备份可恢复官方系统

### L2: 平台骨架与链接

**目标**：构建产物链接到正确地址，但不执行。

- 新增板级 axplat crate（如 `axplat-riscv64-visionfive2`）
- 配置 linker base、RAM 范围、boot 加载地址
- 构建 Gate：`objdump` 显示板级 boot symbols（非 QEMU）；`DWARF=n`；尺寸 << boot 分区容量
- 三模式验证：board feature + QEMU feature 并行 `cargo check + clippy`（改动不能破坏 QEMU）
- **Gate**：三模式编译通过；linker map 确认 entry 地址正确

### L3: Polling Early Console

**目标**：真板串口输出第一行 StarryOS 字符串。

- 实现早期串口 polling 写字符（不依赖 IRQ、async task、文件系统）
- 正确配置 MMIO 访问宽度和 stride（不套 QEMU 参数）
- **Gate**：真板串口看到 `[starry-<board>] early boot` 或等价 smoke 字符串
- 这是第一个真板 Gate。在此之前不要接 IRQ。

### L4: SBI Timer 诊断

**目标**：确认 timer 可用，不破坏启动。

- 打印 hart ID、SBI version、timer 频率
- **Gate**：timer 不引发 fault，启动流程正常继续

### L5: PLIC + 串口 IRQ

**目标**：串口中断最小闭环验证。

- PLIC init_primary/init_percpu
- 串口 IRQ claim/complete，计数打印
- **Gate**：IRQ 计数 > 0 且无丢失中断 panic

### L6: Async Console

**目标**：接回 async 驱动 `/dev/console`。

- 在 L3-L5 稳定基础上接入 async driver
- 注册 PLIC handler、启动 RX/TX copier
- **Gate**：`/dev/console` 可输出、shell 或最小 I/O 可工作

### L7: Benchmark

**目标**：运行 async I/O benchmark 并采集数据。

- 根据启动链选择 benchmark 模式（embedded / rootfs / command-entry）
- **Gate**：benchmark exit 0；nonblocking 双 PASS；无异常耗尽
- 原始 log 保存到 change evidence 目录

## 阶梯约束

| 约束 | 原因 |
|---|---|
| 不可跳步 | 每个 L 层为下一层提供已验证基线 |
| 每层一个变量 | 失败时回退到上一层排查，不猜测 |
| L3 之前不接 IRQ | polling early console 不需要 IRQ，引入 IRQ 会增加未初始化的硬件依赖 |
| L5 之前不接 async driver | async driver 依赖 PLIC IRQ，先确认 PLIC 链路再用 |
| L6 之前不接 benchmark | benchmark 依赖 `/dev/console`，先确认 console 可用 |
| 每层保持 QEMU 可构建 | board feature 和 QEMU feature 独立，改动不能破坏 QEMU 回归能力 |

## 注意事项

- **不要在 L0 阶段无限采集**：已有真板验证——现有事实覆盖 bring-up 所需后，放弃从官方 Linux 做无目标数据采集。
- **串口访问宽度不能套 QEMU**：每块板的串口 stride 和 MMIO 宽度不同，必须按 datasheet/DTS 设置。
- **PTE 属性检查**：如 SoC 有自定义 PTE 扩展（如 T-Head C9xx），必须在 early 和 final page table 都启用。
- **virtio-mmio ranges 不能占位**：无 virtio 的板必须写空数组 `[]`，不能用 `[[0,0]]`。
- **首次烧录前必须备份 boot 分区**。
