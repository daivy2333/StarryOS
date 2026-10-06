# CoM260 Kit FIT 临时 RAM 启动 + 板测全流程（K3，U-Boot + USB FAT32）

- Status: active
- Last validated: 2026-10-06（MS11 002 Cycle 板测：boot→stage1–13→四协议交换→reset 全程成功）
- Environment: CoM260 Kit（SpacemiT K3，U-Boot 2022.10，boot_mode=nor）；host WSL2；串口
  115200 8N1 无流控；FAT32 U 盘（整盘无分区表）；PC 直连网口 192.168.137.1/24（Windows +
  Python 标准库）；产物 `make k3` → `StarryOS_riscv64-k3.fit`（load/entry 0x180000000、FDT
  0x181000000，2026-10-06 实测 398880 B）
- Source: MS09 `board-firstbyte.txt`（2026-10-01 首轮实跑）；MS11
  `ms11-com260-kit-polling-bidirectional-network-baseline` Iteration 002 Cycle Act Response
  （2026-10-06 板测粘贴与用户裁决）

## 适用范围

把 host 构建的 StarryOS K3 FIT 经 U 盘装载 RAM 由 U-Boot `bootm` 临时启动，完成 stage1–13
自动流程，并在 stage13 会话窗内由 PC 执行四协议（ICMP/UDP/TCP/ARP）测试。全程 RAM only，
不动板载固件与持久介质。**不适用**：持久烧录；其他板卡；非 FIT 镜像。

## 前置条件

- 串口终端已接（115200 8N1）；PC 网卡静态 192.168.137.1/24，网线直连板子且链路 up。
- host 已 `make k3`；FIT 已拷入 FAT32 U 盘并插板 USB-A。
- 装载地址固定 `0x2e0000000`（MS09 裁决窗口内）；**禁用 `kernel_addr_r`（落 CMA）**。

## 操作步骤

1. **进 U-Boot**：上电/RESET 同时**长按 `s`**（按住不放）直到 `U-Boot>`。
2. **装载 FIT**（文件名大小写敏感，须与 `fatls` 一致）：

   ```tcl
   usb start
   fatls usb 0:0 /
   fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit
   setenv fdt_high 0xffffffffffffffff
   bootm 0x2e0000000
   ```

   `setenv fdt_high` 必做（否则 FDT 被搬出引导映射导致缺页）；**绝不 `saveenv`**。
3. **等待自动阶段**（链路保持插好，无需交互）：stage1–8（约 30 s link 窗）→ stage9（软件
   探针，秒级）→ stage10（30 s 设备窗）→ stage11（30 s RX）→ stage12（约 12 s：20 ARP+回应窗）
   → 自动 `stage=13 begin`。
4. **stage13 会话窗（60 s）内 PC 依次执行**（出现 `stage=13 begin` 行后立即开打）：

   ```powershell
   ping -n 20 192.168.137.2

   python -c "import socket; s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); s.settimeout(5); s.sendto(b'ms11-udp',('192.168.137.2',26001)); data,_=s.recvfrom(1024); print('UDP echo:',data.decode())"

   python -c "import socket; s=socket.create_connection(('192.168.137.2',26002),timeout=5); s.sendall(b'ms11-tcp'); print('TCP echo:',s.recv(64).decode()); print('FIN:',s.recv(64)); s.close(); print('TCP closed')"
   ```

   首个 ping 请求可能超时 1 次（邻居解析未热，实测出现过）；TCP 的 `FIN: b''` 表示板侧主动
   关闭到达。
5. **读结果**：串口 `stage=13 result icmp-replies=... udp-echo=... tcp-echo=... tcp-close=Closed
   rx-frames=... tx-terminals=... window-elapsed-ms=60000`；与 PC 原生输出逐项对应；`err=` 行
   自带阶段与 owner。
6. **结束**：串口/按 RESET → 回原系统 `Bianbu … k3 login:`。

## 验证

- 2026-10-06 全程实跑：`stage=13 result icmp-replies=10 udp-echo=8 udp-over-limit=0
  tcp-echo=8 tcp-close=Closed rx-frames=23 rx-drops=0 tx-terminals=16 tx-errors=0
  tx-timeouts=0 window-elapsed-ms=60000`；PC 侧 ping 10 发 9 收（首请求超时 1 次，次轮 5/5）、
  UDP/TCP 回显逐字节一致、FIN 观察到；stage12 同轮 `sent=20 terminals=20` 零错误零超时；
  reset 回原系统（操作者确认）。
- 2026-10-01 首轮：boot 四行标记 + 复位回原系统（MS09 evidence）。

## 失败处理（停止点）

- 长按 `s` 无 `U-Boot>` → 按住时机（须与上电/复位同时），原样记录后停。
- `fatload` 失败 → 核对文件名大小写与 `fatls` 列表；再失败换 `loady 0x2e0000000`。
- `bootm` 后无任何输出/仅 `[k3:pt-mmu]` → 保留全文停止，分层诊断（引导链/axruntime）。
- 串口 `stage=N err=...` → 行内含阶段与 owner；按 owner 定位，reset 后重跑或交 Plan。
- PC 侧全程不通 → 核对 PC IP 192.168.137.1/24 与链路；看串口是否已到 `stage=13 begin`
  （stage13 之前 ping 不通是预期）。
- 复位后未回原 Linux → **立即停止一切装载**，保留串口全文交用户处置。

## 回滚

全程无持久写入（不 `saveenv`、不刷写）。任意时刻 `reset` 或断电即回原系统；该路径
2026-10-01/2026-10-06 多轮验证。

## 证据

- MS11 002 Cycle Act Response「V3' 真板结果」节（2026-10-06 串口与 PC 粘贴、用户裁决原话）。
- `openspec/changes/archive/2026-10-03-ms10-com260-kit-diagnosable-device-irq-baseline/`、
  `openspec/changes/ms09-com260-kit-observable-link-baseline/evidence/002-k3-boot-first-byte/000-initial/board-firstbyte.txt`（首轮 boot 与历史阶段）。
- 限制：ping 形态（5+5 手动）与 reset 描述经用户原话豁免（见 Act Response）。
