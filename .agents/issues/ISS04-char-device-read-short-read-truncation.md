# 字符设备读栈每次 read() ≤2048B 短读截断（Zero 设备静默产出不足量数据）

- Status: open
- Filed: 2026-10-16
- Source: MS14 change `2026-10-16-ms14-com260-kit-multi-hart-async-io` Iteration 004 `000-initial.md` Act Response（T15 采集现场记录 + Experience Candidates）与 Plan Review Finding 5（范围外，只报告不落账；用户 2026-10-16 指示登记）
- Environment: CoM260 Kit 16 hart；`k3-smp-combined`（commit `42ea95e2`）；BusyBox 1.37.0（rootfs-riscv64-min.img）+ musl；临时 RAM rootfs

## 缺陷描述

预期：用户态单次 `read()` 消费 `/dev/zero` 等字符设备应返回请求量（Zero 设备 `read_at` 本身填满整个 buf；Linux 下单次 read 返回全量）。实际：读栈每次 `read()` 最多交付 2048B——`dd if=/dev/zero bs=65536 count=512`（无 `iflag=fullblock`）产出 1048576B（512×2048）而非 33554432B，`exit 0` 无报错。已核实：截取不发生在 Zero 设备（`read_at` 填满 buf），发生在 FS 读栈，确切层次未定位（Unconfirmed）。指控点：kernel FS/VFS 字符设备读路径的缓冲上限；具体 symbol 待定位。

## 影响

依赖全量字符设备读的用户态工具静默拿到不足量数据且无错误信号：本轮 32 MiB 载荷变 1 MiB，若未核对字节数将以错误数据继续后续流程（本轮由 Python 接收端按实收字节判定暴露）。POSIX 允许 read 短读，但 BusyBox dd 无 `iflag=fullblock` 时把每次短读计为一整块，构成工具-内核组合陷阱；潜在影响所有默认全量假设的字符设备消费者。

## 事件记录

- 2026-10-16 MS14 Iteration 004 T15 载荷准备：板上 `dd if=/dev/zero of=/dev/shm/net-payload.bin bs=65536 count=512` 完成，文件仅 1048576B。检测：`wc -c` 板上直证 1048576；Windows Python 接收端收到 1 MiB 后 EOF 双证。根因：Unconfirmed（设备层已排除，FS 读栈截取层次未定位）。处置：命令补 `iflag=fullblock`（操作层修正，零产品变更），补后 32 MiB 样本采集成功。

## 处置

MS14 004 Plan Review Finding 5 定为范围外实质缺陷，本轮不修；操作层修正已随 004 落地。产品修复 scheduled → 用户指令。候选调查入口（未评估）：FS/VFS 读路径缓冲上限常量与字符设备 read 分片逻辑。

## 证据

- `openspec/changes/archive/2026-10-16-ms14-com260-kit-multi-hart-async-io/iterations/004-final-use-and-performance/000-initial.md`（Act Response「T15 采集现场记录」载荷短读现象节；「Experience Candidates」Issue 候选原文）
- 同文件 Plan Review「Findings 5」（范围外定性与只报告不落账决定）
