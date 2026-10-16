# Iteration 004 / Cycle 000: BusyBox 最终使用与串口、网卡短时性能测量

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: T12–T15；准备 T12 → 正常使用 T13 → UART 测量 T14 → TCP 测量与原厂 Linux 对比 T15。StarryOS 的 TCP 样本可在 UART 改 raw 前采集，两个任务的最终结果分别记录。
- Acceptance gaps: None。
- Repair items: None。
- Inherited scope: R3/R6/R7/R8/R9 的 16-hart 固定角色、唯一异步 UART/GMAC、普通 RAM rootfs 与正常用户使用；000–003 accepted。用户本轮增加 R10/R11 的短时性能测量。
- Excluded scope: 优化、最低性能门槛、ICMP socket/ping、DNS/DHCP/TLS/IPv6、UDP 性能、多流/同时双向饱和、CPU/p99、独立恢复/容量/C4/quiet/长稳矩阵、真实存储与持久固件修改。

**Objective**

用户在同一个 `k3-smp-combined` 的 BusyBox shell 中真实使用串口、wget、nc TCP/UDP；测得 UART RX/TX 有效吞吐与 115200 8N1 上限占比，以及本板 StarryOS/原厂 Linux 的 TCP 发送、接收吞吐对比。完成以实际命令、字节内容、接收量及时间为依据；没有最低速度或必须优于 Linux 的要求。

**Background**

2026-10-16 用户：“现在展开最后一轮……需要一些串口和网卡性能的测试……开始计划”。集中决策：选择“仅测量与对比，不设最低性能门槛（推荐）”；对端选择“Windows 原生 Python / PowerShell，沿用当前串口和网卡（推荐）”。这替代此前 004 仅正常使用、不含性能测量的未执行范围，不改变已冻结轮次。当前只获规划授权，Gate 2 尚待本份具体计划批准。

**Investigation Facts**

- Current Baseline：003/000 的 2026-10-16 Act Response `reported`、Review `accepted`。已实际完成 `make k3-smp-combined SMP=16` exit 0 + 独立 FIT、SMP=1/17 拒绝、旧 rootfs/SMP-net 构建、真实 clone 发布路径 3/3 GREEN、MMIO 1/1、placement 3/3、MAC probe 1/1；embedded-user-host 全包 34 测试通过。该结论只证明研发基线，未包含最终 BusyBox 运行。
- 当前工作树：`k3`，`git diff --name-only` 无未暂存产品改动；003 及其产品改动已在暂存区。只读状态与最新 Review 一致，本轮不重跑上述有效 GREEN，不重新审查已 accepted 的实现。
- 下表事实来自本次实际代码读取；BusyBox CLI/clock 事实来自现有镜像导出副本的实际用户态运行。没有启动真板/QEMU 内核，没有获得新硬件结论。

| 实际表面 | 当前行为、调用关系与本轮边界 |
|---|---|
| `platform/k3_smp_combined.rs::run_smp_combined_boot` | 16 个 schedulable hart、主任务 pin boot、UART init/copiers 一次、`install_smp_net` 一次、`k3_rootfs::run_ram_rootfs_shell(Some(...))`。默认 boot0 下 RX0/TX1/owner2/runner3，shell8/alarm0；本轮不调整角色或创建第二 Service。 |
| `platform/k3_rootfs.rs`、`syscall/task/clone.rs::do_clone` | 普通动态 `/bin/sh`、ext4 RAM rootfs、stdio/tty/alarm；clone/clone3 共用发布点已在 `spawn_task` 前继承父 mask，exec 保持。用户工具继续沿普通 loader，不嵌入新载荷。 |
| `syscall/net/socket.rs`、`net/io.rs`、`net/opt.rs`、`file/net.rs::Socket` | AF_INET TCP/UDP，socket/connect/send/recv/shutdown、SO_ERROR/TCP_NODELAY/超时等已有；普通 read/write → SocketOps，Pollable → socket readiness。`io_mpx/{poll,select}.rs` 已接 ppoll/pselect6 和 timeout。CLI 存在不证明 StarryOS 兼容，本轮须真实执行。 |
| `crates/axnet/src/{tcp,udp}.rs`、`consts.rs` | TcpSocket/UdpSocket → Service/SocketSet/readiness → 固定 runner/owner → K3NetAdapter。TCP RX/TX 各 64 KiB；保持默认缓冲、smoltcp、DMA/cache/IRQ/背压及恢复语义，不为数值好看调优。 |
| `pseudofs/dev/mod.rs`、`pseudofs/mod.rs::mount_all` | `/dev/zero` 实际填零，`/dev/null` 消费全部输入；`/dev/console` 是 ASYNC_TTY；`/dev/shm` 挂 MemoryFs，可放 32 MiB RAM 发送文件。不用 `/dev/tty` 的占位节点作为原始测量入口。 |
| `pseudofs/dev/tty/mod.rs`、`terminal/ldisc.rs` | TCGETS/TCSETS 支持 termios 保存/恢复，raw/-echo/min1/time0 接通非 canonical 读；`dd iflag=fullblock` 能累积短读。TCSETSW 当前没有物理 drain，`dd conv=fsync` 也不提供 tty drain，不能拿它们作为 TX 计时终点。 |
| `syscall/fs/ctl.rs::sys_ioctl` | TCSBRK 0x5409 已等待 ring/copier/staged/hardware 四条件；本轮 UART TX 直接以 Windows 收到末字节为终点，不新增 drain helper 或修改 ioctl。 |
| `drivers/k3_uart{,_core}.rs`、tty termios | 硬件固定 115200 8N1、64-byte FIFO；tty 默认 c_cflag 仍是 B38400 元数据。`stty` 显示值不能替代硬件配置，不因元数据改硬件或扩展 baud 支持。 |
| `syscall/time.rs::sys_clock_gettime` | CLOCK_MONOTONIC 走平台 monotonic_time；本次 `qemu-riscv64 -strace ... busybox dd if=/dev/zero of=/dev/null bs=4096 count=64` 实际显示 dd 前后调用 CLOCK_MONOTONIC，输出 262144 bytes/seconds/rate，exit0。这是工具行为，不是本板吞吐。 |
| 现有 `rootfs-riscv64-min.img` | 128 MiB，BusyBox 1.37.0 动态 RISC-V ELF，解释器 `/lib/ld-musl-riscv64.so.1`。已有 wget/nc/dd/stty/cmp/wc；本次 `--list`、dd/nc/stty/cmp `--help` 实际确认：dd 支持 fullblock/status=none；nc 支持 -n/-u/-w；cmp 支持 -n。复用镜像，不下载/重编工具。 |
| 现有 `tests/benchmark.c`、`network_benchmark.c` | 前者默认执行整个历史 UART 矩阵，后者含专用控制协议/运行身份字段；均不适合此次原生短时测量。不调用、扩展或移植其资格/身份设施，不把无关历史工具改造纳入本轮。 |

**Code and Critical Path**

`make k3-smp-combined SMP=16` 的已有 FIT → U-Boot RAM 启动 → 固定 UART/NIC 后台角色 → 普通 ext4 `/bin/sh` → wget/nc/dd/stty → 普通 FS/net/ioctl/poll syscall → ASYNC_TTY 或 axnet → 真实 UART/GMAC → Windows 对端。原厂 Linux 阶段只替换板上 OS，使用同一现有 BusyBox 的 nc/dd，数据位于 RAM；网线、板口、PC 物理网卡继续沿用。

**Implementation Guidance**

本轮初始执行契约没有产品代码修复项。T12 准备既有产物、工具和本页原生命令；T13–T15 由用户手动执行。若真实工具/设备行为暴露新的 syscall、所有权、恢复或接口缺口，记录直接错误并返回 Plan 建立有限修复契约，不能由 Act 一边调查一边选择新设计。局部命令路径、COM 号与板上 Linux 网口名可以按实际入口填写，不新增运行身份匹配。

**Task Contracts**

### T12: 最终使用与测量的原生入口准备

- Requirement/Scenario: R8/R9/R10/R11；S26/S28–S36。
- Depends on: 003 accepted。
- Targets: 已有 `StarryOS_riscv64-k3-smp-combined.fit`、`rootfs-riscv64-min.img`、其 BusyBox/loader 只读导出副本；本 Cycle Act Response。产品、Make、rootfs 原盘与独立测试工具均不修改。
- Current behavior: FIT 已构建；镜像内工具存在，最终真板与吞吐结果均未取得。
- Required behavior: 准备给用户的 FIT/rootfs 和原厂 Linux 用 BusyBox/loader 临时副本；提供本页启动、Windows 对端、串口接管/恢复、正常使用和测速步骤。Windows 继续原生 Python/PowerShell，不假设 WSL 能直接访问 COM。
- Preserve: RAM-only、16 harts、原固定角色、唯一 tty reader/NIC、原恢复和 backing retention。Linux 只复制工具到 RAM、临时配置现有网口，不安装软件包或改固件。
- Forbidden: 新测速框架、CLI、专用握手/协议/validator、身份字段、判定脚本、改 rootfs 镜像、预先修 ISS02/ISS03、为性能重构驱动/协议栈。
- First test: 采信 003 accepted 的组合 FIT/构建及本页实际工具检查；无产品变更时不虚构 RED，不重跑已通过 suite/构建。
- GREEN condition: 用户可按本页步骤开始操作，所需文件存在、命令与实际工具 CLI 一致；准备不计作真板通过。
- Verification: 原生命令检查需要交付的具体文件；已有有效结果直接采信。产品未变则不重新构建。只读 `debugfs` 导出所缺 BusyBox/loader 到 `/tmp`，不打开源盘写模式。
- Stop when: 新镜像/新外部工具/新 syscall 或设备设计成为必要条件；返回 Plan。准备完成后停在操作者真板边界，Response 如实记部分完成，不提前勾选 T13–T15。

### T13: 同一 BusyBox shell 的最终正常使用

- Requirement/Scenario: R3/R6/R7/R8/R9；S24/S28/S29/S30/S31/S32。
- Depends on: T12，用户提交真实操作结果。
- Targets: 本页“正常使用”中的普通命令入口与 Act Response；产品零预设改动。
- Current behavior: 组装、affinity 和 MMIO 的研发判据通过，wget/nc 在 StarryOS 的结果未知。
- Required behavior: 提示符下编辑/echo、外部命令、wget 下载已知 HTTP 文本且读回正确、nc TCP/UDP 收到同内容回复；工具结束后 shell 可继续输入。实际输入输出均使用异步 UART，网络走异步 GMAC。
- Preserve: 现有错误/取消/超时及安全资源处理；TCP 的 stream/EOF 与 UDP 的消息边界；旧模式不动。
- Forbidden: 内核 echo 载荷、内嵌 shell、另一个网络镜像代替；因为工具不可用就宣称验收通过。
- First test: 本页原生命令首次板测；发生真实失败时保留该失败，不先修改产品。
- GREEN condition: 用户操作的上述内容正确且 shell 继续响应；UDP 可用 -w3 的最后读等待结束，记录实际退出码，不能把仅因等待窗口结束而退出等同“没有收发”。wget 必须成功退出并读回正确内容。
- Verification: 用户一句话结论与决定性原生输出写 Response，不填表、不强制截图/完整日志。功能未通过不开始性能采集。
- Stop when: 任一必要行为失败或未执行；报告首个错误/超时/现象，返回 Plan。不能按 host GREEN 推断板测。

### T14: UART RX/TX 的有效吞吐与线速占比

- Requirement/Scenario: R4/R8/R10；S33/S34/S36。
- Depends on: T13。
- Targets: 组合 shell 的 BusyBox dd/stty/cmp/wc、Windows SerialPort/Stopwatch 原生操作与 Act Response；不新增项目脚本、测试载荷或串口模式。
- Current behavior: 异步 tty 与物理 drain 已有资格，缺最终组合下的实线吞吐数据。
- Required behavior: 在硬件 115200 8N1、无软/硬流控下，TX 和 RX 各执行一次 262144-byte 连续传输。TX 计时为 PC 收到首个零字节到收到其余 262143 bytes；RX 从 PC 开始发送到板上 dd 完成记录的首行返回，包含短回执成本。两方向报告 B/s、`10*Bps/115200*100` 线速利用率及 `8*Bps/115200*100` 有效数据位占比。TX 文件与零数据比较，RX 以板上 wc/cmp 直接核对；恢复原 termios 和终端后 shell 正常。
- Preserve: 唯一 COM 客户端、唯一 tty reader；dd 前台消费时 shell 不并发读 RX；所有数据只在 RAM/PC 临时文件，无日志流污染测量。
- Forbidden: 用 ring push、write/SerialPort.Write 返回时间、dd TX 统计、stty TCSETSW/fsync 当物理发送结束；GUI 渲染计时；把 11520 B/s 写成 11.52 KiB/s；新增波特率或优化。
- First test: T13 真板使用通过后，按下方 UART 两步各一次，直接观察字节/时间；不为这两个原生命令造测试框架。
- GREEN condition: 两个有效样本、原生字节内容检查及恢复完成；数值低不阻塞完成。短收、夹杂数据、异常时间或 timeout 不是有效速率，保留原生现象；不补跑直到数值满意。
- Verification: 本页 UART 命令；原始 N/t 与算式结果写 Response。RX 数值注明包含回执；线速上限 11520 B/s = 11.25 KiB/s，有效数据 bit/s 最大为波特率的 80%。
- Stop when: 数据短收/失真、COM 无法独占、无法恢复 tty 或测量需要新产品接口；停止该项并返回 Plan，不创建替代测量协议。

### T15: 与原厂 Linux 的 TCP 双向吞吐对比

- Requirement/Scenario: R5/R8/R11；S35/S36。
- Depends on: T13；T14 可在 StarryOS TCP 采集之后执行。完成本任务还需用户返回原厂 Linux 的真实结果。
- Targets: 同一 BusyBox nc/dd、`/dev/shm` RAM 文件、Windows Python socket 原生发送/接收与 Act Response；不改内核/驱动/调度/offload/窗口。
- Current behavior: 多 hart NIC 稳态专项通过，尚无最终用户态 TCP 吞吐或原厂 Linux 基线。
- Required behavior: 两个 OS 都经当前同一板口、线和 Windows 网卡，按同一负载各采集板→PC、PC→板两个方向；单 TCP 连接、32 MiB 数据，每方向预定 3 次，报告全部样本、中位数和 min/max，再算 `StarryOS中位数/Linux中位数*100%`。方向以板子为准；每次接收量必须实际达到 33554432 bytes，接收端计时，不使用 sender sendall 返回时间作为 RX 完成。PC→板计时统一采用相同 BusyBox dd 的 CLOCK_MONOTONIC 统计，包含 nc/pipe 启动；板→PC 用同一 PC perf_counter 与 recv 计数。
- Preserve: Linux 按原厂默认调度/offload 运行，StarryOS 保持既有固定角色；记录 PC 网卡实际协商速率、MTU 和 Linux 设备事实作为解释上下文，不作为运行身份匹配字段。系统默认配置差异意味着这是整体路径对比，不能归因为纯驱动收益。
- Forbidden: 只测 Linux iperf、另一侧测 wget 后直接相除；Internet speedtest；不同方向混比；自动舍弃低值或重试直到变快；新增专用 peer 协议/报告器/判定层；缺 Linux 数据时标完成。
- First test: T13 正常使用通过后运行本页流式命令；原厂 Linux 使用镜像中导出的同一 BusyBox/loader。没有产品变更就不追加单位测试或历史 suite。
- GREEN condition: 两个 OS 的 12 个预定样本完整、方向/字节/时间明确、两项比例可计算；性能低不要求优化。速度/MTU 等条件实际不同则保留原值并明确其对比较的限制，不用环境匹配替代真实收发判据。
- Verification: 下方 TCP 命令，每样本等待/无进展最多 120 s；超时或提前 EOF 记录实际字节和错误，不算完整样本。固定 3 次是预先规定的时序测量采样，并非为已通过行为增加可信度的复跑。
- Stop when: 无真实原厂基线、样本不完整、链路/工具失败或需要新的性能工具/产品设计；保留当前任务和已有有效结果交 Plan/操作者，不偷偷降为只测 StarryOS。

**Invariants**

1. 16-hart、固定角色、唯一 UART/GMAC/tty、普通 rootfs 与用户后代 affinity 保持；不恢复旧资格载荷。
2. 不改变 DMA/cache/epoch/恢复/取消/四条件 drain，不释放不确定 backing，不以测速结果授权优化。
3. 先正常使用，再短时性能；同一次 StarryOS 启动采集其全部结果，随后外部 RESET 回原系统取得 Linux 数据。不 saveenv、不刷写、不改板载分区。
4. 不添加脚本文件、专用 CLI、握手、validator 或自动 PASS/FAIL 判定。PC 原生命令只作为真实数据对端/计时器；字节、时间和工具错误直接给操作者观察，普通算术用于报告。

**Non-goals**

除 Cycle Scope 所列排除项外，不改工具自身的兼容范围，不把历史 Issue 的候选根因当作本轮修复；不把短时 TCP/nc/dd 结果称为网卡极限、性能提升或多 hart 恢复/长稳资格。UART 只测当前波特率的分方向持续传输。

**Acceptance**

| ID | 覆盖 | 直接判据 |
|---|---|---|
| A16 | T12/T13，R7/R8/R9，S24/S28–S32 | 同一真实 BusyBox shell 的串口交互、外部命令、wget 正确文件、nc TCP/UDP 正确回复与结束后继续交互；用户实际执行并确认 |
| A17 | T14，R10，S33/S34/S36 | UART TX/RX 各一个完整实线样本，内容正确，N/t 与两种百分比明确，终端设置和交互恢复；没有最低占比 |
| A18 | T15，R11，S35/S36 | StarryOS/Linux 各两个方向×3个完整 TCP 样本，原值/中位数/范围及同方向比值齐全，测量边界与条件差异说明；没有最低比例 |

**Verification**

以下为计划命令，尚未执行真板。COM 号与 Linux 网口名由操作者选取当前真实入口，不能猜测；这是设备选择，不建立 host/peer pin。

1. **RAM 启动与正常使用**。U 盘提供已有 FIT/rootfs；U-Boot 逐条执行：

   ```text
   usb start
   fatload usb 0:0 0x240000000 rootfs-riscv64-min.img
   setenv starry_rootfs_size ${filesize}
   fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3-smp-combined.fit
   setenv bootargs initrdmem=0x240000000,0x${starry_rootfs_size}
   setenv fdt_high 0xffffffffffffffff
   bootm 0x2e0000000
   ```

   PC `192.168.137.1` 临时目录放内容为 `k3-http-ok` 的 `hello.txt`，运行 `python -m http.server 8000 --bind 192.168.137.1 --directory <该临时目录>`。板上输入/编辑一条 echo，再执行 `/bin/busybox ls /`；`/bin/busybox wget -Y off -T 5 -O /tmp/bb-download http://192.168.137.1:8000/hello.txt`，`echo $?` 与 `/bin/busybox cat /tmp/bb-download` 直接观察。

   TCP/UDP 对端在 Windows 的 Python 交互提示符执行标准 socket 操作；一项结束后再启动下一项，不保存项目脚本：

   ```python
   # TCP：先 listen，再运行板上 nc，输入一行并回车
   import socket
   s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
   s.bind(('192.168.137.1', 26002)); s.listen(1); s.settimeout(120)
   c, addr = s.accept(); c.settimeout(120)
   data = c.makefile('rb').readline(); c.sendall(data); print(data)
   c.close(); s.close()
   # UDP：单个数据报原样回复；在另一次 Python 交互中执行
   import socket
   s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.settimeout(120)
   s.bind(('192.168.137.1', 26001))
   data, addr = s.recvfrom(2048); s.sendto(data, addr); print(data); s.close()
   ```

   TCP 板上 `/bin/busybox nc -n -w 3 192.168.137.1 26002`，输入 `k3-tcp-ok` 回车，看回复与提示符。UDP 用 `/bin/busybox nc -u -n -w 3 192.168.137.1 26001`，输入 `k3-udp-ok` 回车，看同内容回复；-w3 自然结束后记录实际退出码，不把 idle timeout 当丢包，也不把没有回复的退出当通过。结束输入 `echo after-net`，实际看到输出。

2. **UART TX**。关闭 GUI 串口终端，Windows PowerShell 用实际 COM 独占端口；不并行运行其他串口客户端。下例 N=262144，读取到首个零字节后才计时，避免命令回显和启动文字进入分母；输出文件随后由原生 `fc.exe /b` 与全零文件比较，不在传输过程中渲染字节。

   ```powershell
   $port = [System.IO.Ports.SerialPort]::new('COM实际号',115200,[System.IO.Ports.Parity]::None,8,[System.IO.Ports.StopBits]::One)
   $port.Handshake = [System.IO.Ports.Handshake]::None
   $port.ReadTimeout = 60000; $port.WriteTimeout = 60000; $port.ReadBufferSize = 65536
   $data = [byte[]]::new(262144)
   try {
     $port.Open(); $port.DiscardInBuffer()
     $port.Write("/bin/busybox dd if=/dev/zero bs=4096 count=64 status=none`n")
     do { $first = $port.ReadByte() } while ($first -ne 0)
     $clock = [System.Diagnostics.Stopwatch]::StartNew(); $got = 1
     while ($got -lt $data.Length) { $got += $port.Read($data,$got,$data.Length-$got) }
     $clock.Stop(); $seconds = $clock.Elapsed.TotalSeconds
     $bps = ($got-1)/$seconds
     "TX timed_bytes=$($got-1) seconds=$seconds Bps=$bps line_pct=$(10*$bps/115200*100) data_bit_pct=$(8*$bps/115200*100)"
     [System.IO.File]::WriteAllBytes("$env:TEMP\k3-uart-tx.bin",$data)
     [System.IO.File]::WriteAllBytes("$env:TEMP\k3-uart-zero.bin",[byte[]]::new(262144))
   } finally { $port.Close() }
   fc.exe /b "$env:TEMP\k3-uart-tx.bin" "$env:TEMP\k3-uart-zero.bin"
   ```

   异常/短收/非零污染导致内容比较失败时，报告原生错误和实收数，本样本不计为有效。单次无数据读取由端口 timeout 结束；持续文字却无载荷时操作者在60秒停止，保留原文，不增加协议重试。重新打开终端并确认提示符。

3. **UART RX**。终端先执行一条完整命令，让 dd 前台等待；随后断开终端连接，把 COM 交给 PowerShell：

   ```sh
   uart_saved=$(/bin/busybox stty -g); /bin/busybox stty raw -echo min 1 time 0; /bin/busybox dd of=/dev/shm/uart-rx.bin bs=4096 count=64 iflag=fullblock; /bin/busybox stty "$uart_saved"
   ```

   PowerShell 重用上例端口构造/timeout 设置，在 try/finally 内执行下面原生发送；计时包含物理发送和板上 dd 完成记录返回，不使用 Write 返回作为结束：

   ```powershell
   $port.Open(); $port.DiscardInBuffer(); $port.NewLine = "`n"
   $data = [byte[]]::new(262144)
   $clock = [System.Diagnostics.Stopwatch]::StartNew()
   $port.Write($data,0,$data.Length)
   $ddLine = $port.ReadLine()
   $clock.Stop(); $seconds = $clock.Elapsed.TotalSeconds; $bps = $data.Length/$seconds
   $ddLine
   "RX bytes=$($data.Length) seconds=$seconds Bps=$bps line_pct=$(10*$bps/115200*100) data_bit_pct=$(8*$bps/115200*100) includes_dd_reply=yes"
   ```

   首行必须实际看到 dd 完成记录，不能拿其他诊断文字替代。读取余下 dd 原生输出后关闭端口；重连终端，观察 `64+0 records in/out`，执行 `/bin/busybox wc -c /dev/shm/uart-rx.bin`（262144）、`/bin/busybox cmp -n 262144 /dev/shm/uart-rx.bin /dev/zero` 与 `echo $?`（0），再 `echo after-uart`。RX timeout 后保留现象，尝试原已保存 termios 恢复命令；无法恢复时外部 RESET，不以重启掩盖测量失败。

4. **TCP 性能**。在 UART raw 操作之前完成 StarryOS TCP 样本可减少终端切换。板→PC 准备 RAM 文件（不计入计时）：

   ```sh
   /bin/busybox dd if=/dev/zero of=/dev/shm/net-payload.bin bs=65536 count=512 status=none
   /bin/busybox wc -c /dev/shm/net-payload.bin
   ```

   Windows Python 交互中先运行以下普通 TCP 接收对端，之后板上运行 `/bin/busybox nc -n -w 10 192.168.137.1 26003 < /dev/shm/net-payload.bin`。对端接收实际字节，输出 n/t 和有效 Mbps；不解析板上日志或产生 PASS/FAIL：

   ```python
   import socket, time
   s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
   s.bind(('192.168.137.1', 26003)); s.listen(1); s.settimeout(120)
   c, addr = s.accept(); c.settimeout(120)
   n = 0; start = time.perf_counter()
   while n < 33554432:
       block = c.recv(min(65536, 33554432-n))
       if not block: break
       n += len(block)

   seconds = time.perf_counter()-start
   print('board TX received_bytes=', n, 'seconds=', seconds, 'Mbps=', 8*n/seconds/1000000)
   c.close(); s.close()
   ```

   PC→板在 Python 交互中准备 `payload = bytes(33554432)`，再 bind/listen 同样方式的 26004，accept 后 `c.settimeout(120); c.sendall(payload); c.shutdown(socket.SHUT_WR); c.close(); s.close()`；PC 只提供数据，不把 sendall 的耗时当接收吞吐。板上执行单层原生命令管道：

   ```sh
   /bin/busybox nc -n -w 10 192.168.137.1 26004 | /bin/busybox dd of=/dev/null bs=65536 count=512 iflag=fullblock
   ```

   直接看到 dd 的 33554432 bytes/seconds；`Mbps=8*33554432/seconds/1000000`。管道退出码不是完整收发证明，必须读取 dd 实际字节数。每方向固定 3 次，各次另建一个普通连接；按各次实际 N/t 计算中位数和 min/max，不删除低样本、不自动补跑。

   StarryOS 全部操作结束后外部 RESET 回官方 Linux。用户按既有 USB 流程只读挂载工具所在 U 盘，把从现有 RAM 镜像导出的 `busybox` 与 `ld-musl-riscv64.so.1` 放入 `/dev/shm/k3-bb/` 并 chmod +x；不改 `/bin`、`/lib` 或安装软件包。Linux 用 `/dev/shm/k3-bb/ld-musl-riscv64.so.1 /dev/shm/k3-bb/busybox <applet> ...` 替换上述 `/bin/busybox <applet> ...`，两方向 nc/dd 的参数、字节数及 PC 对端保持同一方法。普通原厂 shell 的重定向/管道即可，不需要替换 login shell。Linux 沿已有静态 IPv4；实际地址不同只临时配置现有有线口到与 PC 连通的网段，不保存配置。`ip -br addr`、`cat /sys/class/net/<实际有线口>/{speed,duplex,mtu}` 和 Windows `Get-NetAdapter`/`netsh interface ipv4 show subinterfaces` 记录实际条件；差异作为解释，不建立匹配或拒绝协议。

   最终直接按两方向 3 个样本写原值、中位数、范围与 StarryOS/Linux 百分比。Linux 未执行或少字节时任务仍未完成；已有正常使用和有效样本保留，不要求重跑。

**Requirements Traceability Matrix**

| Requirement | Task | First Test | Status |
|---|---|---|---|
| R3/R6 | T12/T13 | 003 accepted 的固定角色/clone/MMIO 基线；真实普通 shell 调用 | Covered，既有恢复保留，取消专项的批准保持 |
| R7/R8/R9 | T12/T13 | 本页真实 wget 文件、nc TCP/UDP 及继续交互 | Covered；历史恢复/长稳缩减仍为已批准 Simplified |
| R10 | T12/T14 | 真实 dd/COM 连续数据与原生文件比较，物理实收计时 | Covered |
| R11 | T12/T15 | 同方法的两系统 TCP 原生接收量/时间 | Covered |

**Gate 2 Readiness**

Gate 2 PASS：技术设计与 RTM 已闭合，用户于2026-10-16以原话“批准”接受本份004具体计划，Status改为ready。Gate 1 PASS保持；尚未调用Act，执行契约不变。Background中的等待批准描述保留为创建时现场。

**Persisted Evidence**

- Mode: none。决定性命令/内容、N/t、原生错误及用户一句话结论写 Act Response 足够；不保存完整串口日志或创建 evidence 占位。PC/板 RAM payload 是测量输入/临时输出，不是持久证据目录。

**Risks and Notes**

- 预算：本轮一次 StarryOS 最终形态会话（正常使用 + UART 两个分方向样本 + TCP 两方向各3次），另一次原厂 Linux 阶段（TCP 两方向各3次）。不重建历史专项/全套回归；发生失败只按公共规则定向处理。
- UART 接收端 PC 包含 USB-UART/OS 缓冲开销，RX 还含 dd 短回执；短时 TCP 包含 nc、pipe、默认 stack/调度配置，结果是实际整条路径，不是独立硬件峰值或优化收益。
- PC 原生命令只做实际数据收发/计时，不保存成新脚本或另写测试、fixture、CLI。SerialPort 只用 byte[] API，不混用 BaseStream/文本解码测数据，遵循 [Microsoft SerialPort.Read 文档](https://learn.microsoft.com/en-us/dotnet/api/system.io.ports.serialport.read?view=netframework-4.8.1)；socket 使用 [Python 标准 socket API](https://docs.python.org/3/library/socket.html)，处理短 recv，不假设一次 recv 等于一次发送。
- 真板、Windows COM 及 Linux 操作由用户执行；工具/地址/端口不可用时保留当前任务交接，不能伪造结果或默默跳过性能。
- 本轮不维护 SNAPSHOT、全局 tasks、M/R/I、Runbook/Issue，不提交、归档或调用 Maintainer。

## Act Response

- Status: reported（T12/T13/T15 完成并板验；T14 按用户裁决取消；含两轮产品修复与一次流程修正，全部有先行板证与板验 GREEN，等待 Plan Review）

### 实际改动

产品代码、Makefile、rootfs 镜像与测试工具零改动（`git diff --name-only` 仅剩本 change 自身文档）。T12 为准备任务，交付物核验与导出：

- 交付物核对：仓库根目录 `StarryOS_riscv64-k3-smp-combined.fit`（1.6M，003 构建产物，2026-10-16 12:36）、`rootfs-riscv64-min.img`（128M，ext4）存在。
- 原厂 Linux 用副本：debugfs（未加 `-w`，源盘只读）从镜像导出 `/bin/busybox`（806K，inode mode 0755）与 `/lib/ld-musl-riscv64.so.1`（599K）到 `/tmp/k3-t12/`，供用户拷入 U 盘。导出后宿主侧 chmod +x 仅作用于 `/tmp` 副本。
- 操作步骤：启动 U-Boot 序列、Windows Python/PowerShell 对端、串口接管/恢复、正常使用与两向测量步骤均以本页 Verification 1–4 为准，未新增或改写。

### 与计划的偏差

None。

### 自检发现

无 Critical/Important 问题，无遗留 Minor 问题。禁止项核查：无新增脚本/CLI/协议/validator/判定层，未触碰 ISS02/ISS03，未打开源盘写模式，未重跑 003 已 accepted 的构建与 suite。

### 验证命令与实际结果

| 命令 | 实际结果 |
|---|---|
| `ls -lh` 两交付物 + `git diff --name-only` | FIT 1.6M / img 128M 存在；产品无未暂存改动 → 采信 003 构建结论，不重建 |
| `debugfs -R 'stat /bin/busybox'` / `dump` ×2 | 导出成功，busybox mode 0755、825088 字节；源盘只读 |
| `file /tmp/k3-t12/{busybox,ld-musl-riscv64.so.1}` | 两者均为 UCB RISC-V 64 位 ELF；busybox 为动态 PIE，解释器 `/lib/ld-musl-riscv64.so.1`，与 Linux 阶段调用形态一致 |
| `ld-musl-riscv64.so.1 busybox wget --help`（交付副本实跑） | BusyBox v1.37.0，确认支持页面所用 `-O FILE`、`-Y on/off`、`-T SEC` |

dd/nc/stty/cmp/wc 的 CLI 一致性采信本 Cycle Investigation Facts（同镜像内容导出副本的实际用户态运行，覆盖范围未变化）。

### GREEN 条件判定

用户可按本页步骤开始操作：所需文件已核验存在，页面命令与实际工具 CLI 一致。准备不计作真板通过；A16–A18 尚无任何真板见证。

### Persisted Evidence

None required（Mode: none）。

### Experience Candidates

- Issue 候选（范围外实质缺陷，只报告不落账）：字符设备/STREAM 节点读路径存在每次 `read()` ≤2048 字节的短读上限——Linux /dev/zero 单次 read 返回全量，本内核返回短读且 `dd bs=` 无 `iflag=fullblock` 时按短读计块（本轮 512×2048=1048576 板证，`wc -c` 与 Python 接收字节数双证）。Zero 设备 `read_at` 填满整个 buf，截取发生在 FS 读栈，确切层次未定位。证据引用本节"载荷短读现象"与 T15 采集现场记录。

### 未解决问题与边界

T13（最终正常使用）、T14（UART 双向吞吐）、T15（TCP 双向对比与原厂 Linux 基线）等待用户按本页 Verification 1–4 手动执行；操作结果返回后按当前执行批次先审核 PASS 再确认。操作中若暴露新 syscall/设备/工具缺口，保留首个直接现象返回 Plan。`/tmp/k3-t12/` 为临时导出，用户拷入 U 盘后可删。

### T13 定位（2026-10-16，用户指令：Act 定位问题并填写恢复）

首个现象（保留记录）：组合镜像首次真板启动，16 hart 全部起核、placement `rx=0 tx=1 owner=2 runner=3 alarm=0 shell=8` 与计划一致，AIA/UART/GMAC/PHY（RTL8211F，`link=down an-complete=0`）/ext4 挂载各阶段正常；`shell spawned hart=8` 后 `task.join()` 立即返回，`exit join=0 → fs-close done → drain done` 在用户无任何输入时打印，shell 从未出提示符，用户键入仅得 tty 回显。组合入口的 BusyBox shell 任务启动即退出，交互会话未建立；T13 停止，T14/T15 未开始。

静态排查已建立的约束：

- join 语义正常（`crates/axtask/src/task.rs:173` 等 `TaskState::Exited`）→ shell 任务确实被 hart 8 调度运行并到达退出，不是 spawn 失败或 join 提前返回。
- 用户态 TX 路径正常：copier 启动后全部 diag 行（stage4–7、ms13/ms14/ms18）经异步串行写出可见 → shell 若打印过提示符/错误必然可见；实际零输出 → 死于首次用户态 write 之前（交互 ash 启动即打提示符）。
- 静默与"崩溃"和"干净退出"都相容：页面故障 → SIGSEGV（`kernel/src/task/user.rs:65` info!）、致命信号 → `do_exit(128+signo)`（`kernel/src/task/signal.rs:23-31`）、干净退出 → `do_exit(0<<8)`（`kernel/src/syscall/task/exit.rs`），三条路径全部走 `info!`（`kernel/src/task/ops.rs:200` exit code 行），而组合构建 `LOG=off` 全部压制。
- 排除项：MS18 单 hart 用同一 k3_rootfs.rs 代码、同一 LOG=off 构建板测动态 BusyBox shell 正常；T8 k3-smp-uart 用同一 `spawn_task_with_affinity` 原语把静态 ELF shell 放 hart 8 板测正常；ISS02/ISS03 均为单 hart MS13 遗留（饥饿形态是停摆不是秒退），不构成当前解释。故障面收敛为从未同板验证的交集：{动态 BusyBox 全 syscall 面} × {SMP=16 + 固定 affinity + NIC owner/runner 常驻}。
- 幸存假设：H1 用户态崩溃（ld-musl/sh 早期在 SMP 下受害，含内核共享态数据竞争）；H2 干净早退（非交互判定 + 首个 read 返回 0，或 musl 早期 syscall 失败静默退出）；H3 内核侧任务 panic 被 axtask 捕获。

判别构建（恢复步骤第 1 步，已完成）：

- 已用同一命令仅改 `LOG=info` 重建组合内核并重新出 FIT（产品代码零改动，Makefile 未改，手动传参）：`StarryOS_riscv64-k3-smp-combined.fit`（仓库根，2026-10-16 14:44，布局不变 kernel 0x180000000/FDT 0x2ef000000）。`LOG=info` 下三条退出路径都会打印：崩溃行 `segmentation fault pc=… va=… sp=… ra=…`（含进程名）、信号致死 `exit with code: 128+signo`、干净退出 `exit with code: 0`；内核任务 panic 行亦可见。
- 原交付 FIT（LOG=off，003 构建）备份于 `/tmp/k3-t12/StarryOS_riscv64-k3-smp-combined.fit.LOGoff-bak`。

### T13 根因定位与修复（2026-10-16，LOG=info 诊断轮用户真板证据）

诊断轮观测（决定性）：`Enter user space: ip=0x4056900`（ld-musl 入口，基址 0x4000000+0x56900 ✓ 入口链路正确）→ busybox 已执行到 main 分发区（`ra=0x4f95c`、`pc=0x4a660`，映射后对应链接地址 0x4e95c/0x49660，经反汇编证实 `jal 0x49660` 直跳目标吻合）→ **fetch 页 fault：`sepc=0x4a660`（正确目标）而 `stval=0xffffff000004a660`（SV39 非规范地址，= sepc | 0xffffff0000000000）** → `handle_page_fault` 按污染 stval 解析失败 → SIGSEGV，`exit with code: 139`。

根因（代码 + 板证闭环）：

1. `kernel/src/mm/loader.rs::map_elf` 将 ELF 各 LOAD 段映射为 **COW + 文件后端的惰性填充**（`populate: false`）：正文页加载时不驻留，首次取指在**执行 hart** 上页 fault 后从文件缓存填充。同函数旁原有 `// TDOO: flush the I-cache` 注释标记该路径。
2. K3 核心对**跨簇执行 hart 上的 instruction page fault 会间歇性交付污染的 stval**（本例高位 0xffffff0000000000、低位精确正确；此前 ldso 入口至 busybox main 的几十次取指 fault 全部正常解析，仅 0x4a000 页这次失败——间歇性，形态与 C910 系 stval 不可靠勘误同类）。`sepc` 始终正确。
3. 单 hart MS18 不暴露（fault 与填充同 hart 时 stval 可靠）；T8 嵌入式 shell 不暴露（嵌入式加载器 `populate_area` eager，无取指 fault）；堆路径也不暴露（`load_user_app` 堆映射本就 `populate: true`）——三者中只有 FS 加载器的 exec 段是惰性的，组合入口首次真板运行正好踩中。

修复（1 处，最小变更）：`map_elf` 段映射改为 `populate: true`，加载 hart 上把文件填充/bss 零填在 exec 前完成，消除 exec 路径上的全部首次取指 fault（与嵌入式加载器、堆路径既有 eager 形态一致）。COW 写语义保持：可写段 populate 时即升私有副本（`handle_cow_fault`），只读段后续写仍走 COW。变更仅 `kernel/src/mm/loader.rs` 一处（含约束注释）。

验证与残留风险：

- 受影响构建：`make k3-smp-combined SMP=16`（LOG=info，15:18 FIT）与 `make k3-rootfs` 均 exit 0。内核 src/mm 无既有 host 测试，先行测试为两次真板 RED（已记录），GREEN 由板测承担。
- 残留风险（记录不扩大）：fork/COW 数据写 fault 的 stval 可靠性未受本轮证据挑战——失败轮中 GOT/数据段的存储 fault 全部正常解析，仅取指 fault 出现污染。若后续板测在数据 fault 上再现同型非规范 stval，返回 Plan 决定容错层（按 sepc 兜底解析或平台勘误登记）。
- 交付 FIT（LOG=off）备份仍在 `/tmp/k3-t12/…LOGoff-bak`；验证轮 FIT 为修复+LOG=info，用户确认 shell 可交互后重建 LOG=off 作为 T13–T15 测量镜像。

用户重跑（同 U-Boot 序列，FIT 已是修复版）：GREEN 判据 = BusyBox 提示符出现、echo/help/外部命令可交互；仍失败则 LOG=info 日志继续判读。

### T13 修复板验（2026-10-16，用户真板，GREEN）

修复版（LOG=info）板上：BusyBox shell 可交互——`ls`/`cd /bin` 正常、applet 子进程干净退出（`Task(43/44,"busybox") exit with code: 0`，无任何 fault 行）、SIGINT 交付 shell 进程后提示符存活、工作目录提示符随 `cd` 更新。取指 fault 类暴露面消除，根因修复成立。LOG=off 交付镜像已重建（15:30 FIT），T13 正式采集（wget/nc/交互全场景）在其上进行；仍保留 LOG=info 修复版 FIT 可按需重建。

### T13 第二现象与修复（2026-10-16，wget socket ENOSYS）

现象：修复镜像上 shell 交互正常，`wget` 在 `socket(AF_INET,1,0)` 处报 `Function not implemented`（ENOSYS）——非 rootfs/BusyBox 缺功能，BusyBox 已正确发起调用。

根因：`kernel/src/syscall/mod.rs` 与 `kernel/src/file/mod.rs` 以 `#[cfg(not(any(feature = "lichee-d1", feature = "k3")))]` 把整个用户态网络 syscall 家族（mod net、socket/bind/connect/listen/accept/getsockname/getpeername/send/recv/sendto/recvfrom/setsockopt/getsockopt/shutdown/socketpair 共 18 分发臂 + Socket 文件包装）对一切 K3/D1 形状编译排除（门来自仓库初始化提交 046a519b，当时 K3 尚无网络栈）；`Sysno::socket` 落入兜底臂返回 ENOSYS。`sys_socket` 实现本身齐全，组合入口的 axnet Service 在 rootfs 之前已由 `install_smp_net` 装好。MS12/MS13 的 socket 载荷均为内核态任务直用 axnet，K3 用户态 socket 路径从未被行使；004 Investigation Facts 记录"AF_INET socket 系统调用已有"来自实现文件阅读、未核对分发编译条件——分类为 Plan 遗漏。

修复：谓词收窄为 `#[cfg(not(any(feature = "lichee-d1", all(feature = "k3", not(feature = "k3-smp-combined")))))]`（syscall/mod.rs 20 处、file/mod.rs 2 处）——用户态网络仅在 k3-smp-combined 形状编译；`k3`/`k3-rootfs`/`k3-smp*`/`lichee-d1` 保持关闭（k3-rootfs 无网卡，socket 不得达 axnet），QEMU/VF2 保持开启不变。

验证与残留风险：

- `make k3-smp-combined SMP=16`（LOG=off）exit 0，FIT 1.8M（15:48，较 1.6M 增量即网络 syscall 家族编入）；`make k3-rootfs` exit 0（关闭分支不受影响）。
- 残留风险：这是 K3 上用户态 socket/connect/readiness 经常驻 runner/owner 的首次真实行使；若真实行为暴露新缺口，保留首个直接现象返回 Plan（同本 Cycle 既有边界）。

### T13 正常使用板验（2026-10-16，LOG=off 交付镜像，GREEN）

wget 下载 10 字节 `k3-http-ok` 内容逐字节正确（socket/connect/HTTP 读全链路经用户态 syscall→axnet→GMAC 常驻 owner/runner）；nc TCP 26002 与 UDP 26001 同内容回显成立；shell 全程可交互。用户态网络门修复板验成立，T13 主体判据达成。

### T15 采集现场记录（2026-10-16，进行中）

- 载荷短读现象：页面原命令 `dd if=/dev/zero bs=65536 count=512`（无 `iflag=fullblock`）产出文件仅 1048576 字节——字符设备读栈每次 read 返回 ≤2048 字节短读（Zero 设备 `read_at` 本身填满 buf，截取层次未定位），dd 无 fullblock 时把每次短读计为一整块：512×2048=1048576，exit 0 静默。Python 接收端如实报告收到 1 MiB 后 EOF，判定正确。处置：载荷命令补 `iflag=fullblock`（与页面内另两条 dd 一致），属计划命令疏漏的操作性修正，零产品变更。同轮现象观察（不阻塞）：TCP 回显时 tty 回显与应用输出交输出现 `heelohello` 型交错乱码；nc 首次无监听时 connect 干净返回无报错。
- StarryOS 有效样本（板→PC 26003 / PC→板 26004，各 3 预定）：板→PC #1 = 33554432 B / 0.375802 s（714.3 Mbps）；PC→板 #1 = 33554432 B / 1.227043 s（dd 26.1 MiB/s ≈ 218.8 Mbps）。两方向速率不对称（~3.3×）作为解释上下文记录，不调优、不解读为结论。链路条件记录（`Get-NetAdapter`/`speed` 等）随样本一并收集。

### 用户裁决豁免（2026-10-16，原话保留）

- T14 UART 线速测量：**取消**。用户原话："uart的测试不做"。A17 不产生 UART 吞吐见证；R10 的板上实测数据缺口随豁免留档。
- T15 多次采样：**缩减为每方向 1 个有效样本**。用户原话："我们不做什么多次测试，有当前数据已经足够"。3 样本/中位数/min-max 判据按用户裁决放宽；"Linux 数据未取得不得标完成"仍然有效，Linux 侧两方向各 1 样本为 T15 完成下限。
- T13 正常使用：用户原话"很好正常工作"（wget 10 字节读回正确、TCP/UDP 回显、shell 存活），T13 判据达成。

### T15 测量结果（2026-10-16，用户真板 + Windows 原生对端，单样本裁决下完成）

方法：同一 U 盘口/网线/PC 网卡；单 TCP 连接、32 MiB、接收端计时（板→PC 用 Python perf_counter+recv 计数，PC→板用板上同一 BusyBox dd 的 CLOCK_MONOTONIC 统计，含 nc/pipe 启动）；原厂 Linux 使用镜像导出的同一 BusyBox/loader（经 HTTP 部署至 `/dev/shm/k3-bb`），两方向 nc/dd 参数与 PC 对端与 StarryOS 侧完全同法。

| 方向 | 系统 | 接收字节 | 秒 | Mbps |
|---|---|---|---|---|
| 板→PC | StarryOS | 33554432 | 0.375802 | 714.3 |
| 板→PC | 原厂 Linux | 33554432 | 0.287398 | 934.0 |
| PC→板 | StarryOS | 33554432 | 1.227043 | 218.8 |
| PC→板 | 原厂 Linux | 33554432 | 0.314224 | 854.3 |

同方向比例：板→PC StarryOS/Linux = **76.5%**；PC→板 StarryOS/Linux = **25.6%**。单样本裁决下中位数=min=max=该样本；两方向比例均可计算，Linux 基线已取得。

链路条件（解释上下文）：官方 Linux dmesg 记录 `end1 Link is Up - 1Gbps/Full - flow control off`；StarryOS 侧启动日志 `runtime link snapshot up=true`（同一板口/网线/PC 网卡）。PC 网卡协商速率未单独采集（用户豁免多次测试时一并从简）。

解读边界（Non-goals 约束）：两系统默认配置不同（调度/offload/协议栈实现/缓冲），这是整体路径对比，不能归因为纯驱动收益；板→PC 与 PC→板的不对称（两系统同向存在）记录为上下文，本轮不调优、不下性能结论、不称为网卡极限。

### 本轮收口

- T12 ✓（交付物核验与部署）、T13 ✓（正常使用全场景）、T15 ✓（单样本裁决下两方向测量+比例）；T14 按"uart的测试不做"用户裁决取消，无 UART 吞吐见证。
- A16 达成；A17 按用户豁免不产生数据；A18 在单样本裁决下达成（两 OS 两方向原值+比例齐全）。
- 本轮产品变更两处（均先行 RED 板证→修复→板验 GREEN）：①`kernel/src/mm/loader.rs` map_elf eager populate（K3 跨簇取指 fault stval 污染容避）；②`kernel/src/syscall/mod.rs`+`kernel/src/file/mod.rs` 用户态网络 syscall 门收窄为仅 k3-smp-combined 开启。第三处为操作流程修正（载荷 dd 补 `iflag=fullblock`），零产品变更。
- 交付镜像：仓库根 `StarryOS_riscv64-k3-smp-combined.fit`（LOG=off，含全部修复）；修复前交付 FIT 备份于 `/tmp/k3-t12/…LOGoff-bak`；原厂 Linux 工具副本位于 `/tmp/k3-t12/` 与 Windows `…\tmp\k3-bb\`。

## Plan Review

- Review Result: accepted（2026-10-16；用户预授权"没问题就接收"，独立检查未发现阻塞项）

### 独立检查范围

读取本 Cycle 全文与 Act Response；独立阅读本 Cycle 全部产品 diff——工作树未暂存改动恰为三文件（`kernel/src/mm/loader.rs`、`kernel/src/syscall/mod.rs`、`kernel/src/file/mod.rs`），暂存区其余改动属 000–003 已 accepted 轮次不在本轮重审；核对 Act 声明与代码一致。Evidence Mode `none`，无 evidence 目录要求。Act 已报告且覆盖未变化的构建结论（`make k3-smp-combined SMP=16` LOG=off exit 0、`make k3-rootfs` exit 0）与用户真板裁决直接采信，不重复运行。

### 代码核对结论

1. `loader.rs`：`map_elf` 段映射 `populate` false→true，`Backend::new_cow` 与映射参数不变；新增注释如实记录约束（K3 跨簇取指 fault 间歇性交付污染 stval、sepc 正确、handler 无法解析致任务死亡）。顺带移除的 `// TDOO: flush the I-cache` 标记的是被本次消除的惰性路径，属孤儿清理。与声明"1 处最小变更"一致。
2. syscall/file 门：谓词统一为 `not(any(lichee-d1, all(k3, not(k3-smp-combined))))`。grep 实测 syscall/mod.rs 18 处（2 个模块/导出门 + 16 个分发臂）、file/mod.rs 2 处；旧谓词零残留，combined（启用分支）与 k3-rootfs（关闭分支）双侧编译通过，门完备。feature 闭包核对：`k3-smp-combined` 经 `k3-smp`→`k3` 蕴含 `k3`；`k3-rootfs`/`k3-smp-net`/`k3-smp-uart` 均不含 combined，用户态网络保持关闭（k3-rootfs 无网卡，socket 不得达 axnet）；QEMU/VF2/D1 行为不变。与声明的各形状开关方向一致。
3. 板验与裁决链完整：T13 两轮修复各有先行真板 RED 与修复后 GREEN；T13"很好正常工作"、T14"uart的测试不做"、T15"我们不做什么多次测试，有当前数据已经足够"三处用户原话均留档于 Act Response。

### Findings（全部非阻塞）

1. Act Response 计数笔误：实际 18 处/16 分发臂，正文写"syscall/mod.rs 20 处""18 分发臂"。以 grep 为准；双侧编译加零旧谓词残留证明门完备，笔误不影响行为。
2. Act Response 顶部"实际改动：产品代码零改动"为 T12 时点快照，与后文 T13 两处修复及"本轮收口"汇总矛盾；按时间序成文、终稿汇总正确，不修改 Act 区域。
3. change `tasks.md` 头部与批次/Gate 表状态滞后（"尚未调用 Act""Gate 3/4 SKIPPED：无产品实施"），与本轮两处产品修复事实不符——已按 Plan 职责随本 Review 更新。
4. 提交集卫生（继承 003 留档）：`tests/network_benchmark-host-asan`（staged，520K x86-64 ASan 宿主二进制）不应进入提交，收尾提交前移出。
5. Issue 候选（范围外，只报告不落账）：字符设备/STREAM 读栈每次 `read()` ≤2048B 短读上限（Act Response 双证、层次未定位）；ISS02/ISS03 维持登记未处置。是否落账/处置由用户决定（Recorder）。

### Acceptance 判定

- A16 达成：真板 BusyBox shell 串口交互、wget 逐字节正确读回、nc TCP/UDP 同内容回显、结束后 shell 存活，用户结论留档。
- A17 按用户显式豁免不产生见证（原话留档）；R10 板上实测缺口随豁免留档，delta spec 收尾合并按见证状态处理（Maintainer）。
- A18 在用户单样本裁决下达成：两 OS 两方向原值与比例（板→PC 76.5%、PC→板 25.6%）齐全，原厂 Linux 基线已取得，解读边界已留档。

Acceptance Gaps: None（阻塞项为零；两项判据变更均由用户原话裁决并保留原话与风险）。

- Iteration Plan Update: None
- Next Cycle: None
- Next Iteration: None（004 为最终 Iteration，T1–T15 全部闭合或豁免）
- Follow-up Decision: None（无当前 Cycle 修复项）
