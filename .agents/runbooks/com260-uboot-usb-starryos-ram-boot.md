# CoM260 Kit 经 U-Boot + USB FAT32 装载 FIT 临时 RAM 启动 StarryOS（K3）

- Status: active
- Last validated: 2026-10-01
- Environment: CoM260 Kit（SpacemiT K3，U-Boot 2022.10，boot_mode=nor）；host WSL2；串口终端 115200 8N1 无流控；FAT32 U 盘（整盘无分区表）；`make k3` 产物 `StarryOS_riscv64-k3.fit`（kernel load/entry `0x180000000`、FDT load `0x181000000`，FIT 约 276 KB）
- Source: MS09 `ms09-com260-kit-observable-link-baseline` Iteration 002 Cycle 000 Act Response + `openspec/changes/ms09-com260-kit-observable-link-baseline/evidence/002-k3-boot-first-byte/000-initial/board-firstbyte.txt`（2026-10-01 板上两轮实跑：第一轮诊断至分配器 panic 并复位恢复；第二轮（64g 修复后）完整标记序列跑通 + 复位回原系统）

## 适用范围

把 host 构建的 StarryOS K3 FIT（kernel+FDT）经 U 盘装载到 CoM260 Kit RAM 并由 U-Boot `bootm` 临时启动，用于真板测试（首字节、阶段标记、后续 MAC/PHY 探针迭代）。全程易失（RAM only），不动板载固件与持久介质。

**不适用**：持久烧录（fastboot flash / Titan / UFS 分区写入）；K3 其他板卡；非 FIT 格式镜像；未过 MS09 Iteration 002 地址裁决的任意 RAM 地址装载。

## 前置条件

- 串口终端已接（USB-TTL 3.3V，TX/RX 交叉、共地、不接 VCC；115200 8N1 无流控）。
- host 已执行 `make k3`（产物在仓库根：`StarryOS_riscv64-k3.fit`；内部含 `axfeat/page-alloc-64g`，缺它会因位图容量 panic，见失败处理）。
- FAT32 U 盘（整盘格式化，无分区表；先例 R67：板上枚举为 `/dev/sdb`）已拷入 FIT：host 侧 `cp StarryOS_riscv64-k3.fit /mnt/<盘符>/` 后安全弹出插板 USB-A。
- 地址事实（MS09 Iteration 001/002 裁决，勿改）：staging `0x2e0000000`（裁决窗口 `[0x160000000, 0x2f0000000)` 内、避开 CMA/SWIOTLB/framebuffer/U-Boot 工作区）；**禁用 `kernel_addr_r`（=0x140000000，落 CMA）**。

## 操作步骤

1. **进 U-Boot Shell**：给板上电（或按 RESET）的同时，在串口终端**持续长按 `s` 键**（按住不放，非单击），直到出现 `U-Boot>`。这是 SpacemiT 定制入口（官方 docs-buildroot boot.md 交叉验证 + 本板确认），不走标准 autoboot 打断路径，与 `Autoboot in 0 seconds` 不矛盾。
2. **装载 FIT**（文件名**大小写敏感**，须与 `fatls` 列出的原大小写一致）：
   ```
   usb start
   fatls usb 0:0 /
   fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit
   ```
   实测 276000 bytes read in ~55–63 ms（约 4–5 MiB/s）。
3. **阻止 FDT 重定位**（必做）：
   ```
   setenv fdt_high 0xffffffffffffffff
   ```
   不设时 U-Boot 会把 FDT 搬到 `0x2fbd4f000`（其自占区一带），该地址不在内核引导页表映射内，内核按 a1 访问会缺页；设置后 FDT 停留在 `0x181000000`（实测日志：`Using Device Tree in place at 0000000181000000`）。**绝不 `saveenv`**——只影响本次上电。
4. **启动**：
   ```
   bootm 0x2e0000000
   ```
   U-Boot 完成 FIT 解析、crc32 校验、kernel 装载 `0x180000000`、FDT 置于 `0x181000000` 后打印 `Starting kernel ...`（伴随 `fail to get alias node efuse_power` 无害告警与 `Removing MTD device #0` 提示）。
5. **读串口阶段标记**（成功形态，随后静默=受控停留 wfi）：
   ```
   [k3:pt-mmu]
   [starry-k3] stage=1 entry-reached
   [starry-k3] stage=2 console uart=0xd4017000 stride=4 width=u32 baud=115200
   [starry-k3] stage=3 parked, halting (reset to recover)
   ```
   `[k3:pt-mmu]` 之前还有 axruntime 横幅（`arch = riscv64` / `platform = riscv64-k3` / `smp = 1`）。
6. **结束测试**：`reset` → 板子走 ROM→NOR→SPL→U-Boot→UFS Linux 原启动链，回到 `Bianbu … k3 ttyS0` / `k3 login:`。

## 验证

- 成功判据：第 5 步四行标记完整出现且可重复；第 6 步复位后回到 `k3 login:`。
- 2026-10-01 实跑：第二轮完整四行标记（Evidence §1.6）；复位回原 Linux 由操作者确认（原话记录于 Act Response 3.3 收口段）。第一轮（旧 FIT）在 panic 后复位恢复亦验证了失败场景下的回原系统路径。
- 已知注记：`Boot at 1970-01-01…` 为板上 RTC 无电池（epoch 0 起算），不影响判定；完整标记序列的独立重复轮次经用户豁免仅一次完整循环（另一轮为不同 FIT 的诊断循环），后续迭代每次上板自然累积重复证据。

## 失败处理

| 现象 | 诊断方向 | 停止/处置 |
|---|---|---|
| 长按 `s` 无 `U-Boot>` | 按住时机（须上电/复位同时）、串口终端是否真的在发键 | 原样记录现象后停止，不盲试其他按键序列 |
| `fatload` 报 `Failed to load` | 文件名大小写（FAT 区分大小写，须原样 `StarryOS_riscv64-k3.fit`）、`fatls usb 0:0 /` 是否见文件 | 核对后重试；再失败换 `loady 0x2e0000000`（YMODEM） |
| `bootm` 校验失败/报错 | FIT 完整性（host 重新 `make k3`）、装载地址是否被手误改过 | 原样记录 `iminfo 0x2e0000000` 输出后停止 |
| 全无输出（无 `[k3:pt-mmu]`） | 未跳转 / 引导页表 fault / UART clock-pinmux 死 | 停止，保留 `bootm` 全部输出，回 Act/Plan 分层诊断 |
| 仅 `[k3:pt-mmu]` 无横幅 | 页表+MMU+UART 活，axruntime 或 entry 失败 | 保留输出停止；典型：地址/feature 配置面 |
| `bitmap capacity exceeded`（axallocator panic） | FIT 缺 `axfeat/page-alloc-64g`（旧镜像） | host 重新 `make k3` 换新 FIT（2026-10-01 已修复入 feature） |
| 乱码 | UART 时钟/波特率不符 | 停止，核对 115200 与平台 axconfig |
| 复位后未回原 Linux | 持久固件异常 | **立即停止一切装载尝试**，记录串口全文，交用户处置（RAM 路线阻塞信号） |

## 回滚

全程无持久写入（不 `saveenv`、不刷写、不向 UFS 拷文件）。任何时刻 `reset`（或断电重上电）即回到原系统；该恢复路径已两轮板上验证。`setenv fdt_high` 仅存于本次上电的易失 env。

## 证据

- `openspec/changes/ms09-com260-kit-observable-link-baseline/evidence/002-k3-boot-first-byte/000-initial/board-firstbyte.txt`（2026-10-01 两轮全程：U-Boot 采集、装载、iminfo、bootm、panic、完整标记序列、复位；MAC 脱敏）
- `openspec/changes/ms09-com260-kit-observable-link-baseline/iterations/002-k3-boot-first-byte/000-initial.md`（Act Response：修复记录、豁免原话、验收对照）
- 入口机制来源：k3 参考仓库 `docs/boot/com260-boot-chain.md`（引 docs-buildroot boot.md，官方交叉验证）；地址裁决来源：同 change Iteration 001 Act Response
