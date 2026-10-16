# K3 多 hart 异步 I/O 执行路线

000–003 的 T1–T11 已完成并 accepted；其执行记录与审查结论保持。2026-10-16 用户先批准以 16-hart BusyBox RAM rootfs shell 的 UART/GMAC 实际使用替代未执行的恢复/长稳资格，工具选择 wget+nc（TCP/UDP），ping 留待后续；随后明确要求最后一轮补充串口/网卡性能测量。现有 T12/T13 保留准备与最终使用，新增 T14/T15 承载 UART 线速占比及同板原厂 Linux TCP 吞吐对比，共 15 项。集中决策为只测量、不设最低性能门槛，对端沿用 Windows 原生 Python/PowerShell；旧恢复矩阵/30 分钟要求仍取消。批准原话、场景与范围见 proposal，测量设计见 design。

003/000 已于 2026-10-16 审查 accepted。004/000 已执行完毕：Act Response `reported`（T12/T13/T15 完成板验、T14 按用户裁决取消、含两处先行 RED 板证的产品修复），Plan Review 于 2026-10-16 独立核对代码与声明后 **accepted**（非阻塞 finding 留档于该 Cycle Plan Review，含提交集卫生事项）。T1–T15 全部闭合或豁免，change 达到收尾条件，等待用户调用 `openspec-docs-maintainer` 收尾。000–003 历史记录不改写。

## 1. AP SMP、timer/IPI 与固定设备路由

- [x] 1.1 **T1**：在root/kernel Cargo、Makefile和K3 boot/init/power接通`k3-smp`与16 AP次级启动。先以原生`make k3-smp SMP=16`观察缺入口RED，并用项目rustc测试夹具覆盖HSM拒绝/越界、次级栈与页表发布；补平台smp feature、secondary hooks、原生失败停止和独立FIT入口后GREEN。现有axtask/axruntime不重写，先行测试与SMP构建结果记录在当前Cycle。
- [x] 1.2 **T2**（依赖T1）：在K3 irq/timer_irq/time与最小固定soft-IRQ core中分离全局handler和per-hart STIE/SSIE，完成S_SOFT CAS/ack/dispatch及本地timer mask/unmask；扩展现有timer harness和最小IPI生产core harness先RED，覆盖两hart/重复注册/ack/移除顺序/本地隔离后GREEN。
- [x] 1.3 **T3**（依赖T1/T2）：在AIA/IrqIf实现固定boot外设路由与远端全局source门，先扩展现有AIA harness为独立IMSIC文件，RED覆盖远端mask/unmask、register/unregister拒绝无副作用、secondary不重置global，GREEN保持旧mask/claim语义。集成K3 native平台入口，复用已有task-state/waker/IPI计数观察真正Blocked→Ready与timer恢复。
- [x] 1.4 **T4**（依赖T1–T3）：完成首轮受影响host/构建验证与全量diff自检，生成待测FIT并交操作者：16 hart各执行、每hart timer、跨0/8的timer-disabled真实IPI、source1固定boot claim与远端mask/unmask、RESET回官方Linux。审核PASS后填写Response；无板测不得勾选完成。（2026-10-15 真板会话通过：A1–A4 全部真实满足，S04 观测偏差经用户裁决接受现证留档，见 Cycle Act Response 真板结果节）

## 2. NIC 固定角色与稳态专项

- [x] 2.1 **T5**（依赖000 accepted）：在k3_async_net与root/kernel/Make模式接线中建立`k3-smp-net`，复用共享placement为owner/runner提交singleton，保存handle与实际执行观察，应用固定hart9；source60按既有生产初始化武装，adapter/Service仍唯一。先在现有SMP/adapter/Service测试入口验证spawn拒绝/重复启动、IRQ最后事件和安装顺序，必要接线RED→GREEN，旧无paging网络包装器保留。（2026-10-15 完成：uart-plane harness 7/7（含4个新接缝用例，RED先行）、k3-async-net-host 26/26、`make k3-smp-net SMP=16`/`make k3`/`make k3-async`/`make k3-async-probe` 全部 exit 0、FIT 已产出；新入口 install-done 观察行的实际 hart 判定归 T6 板测）
- [x] 2.2 **T6**（依赖T5）：复用既有容量/flush/socket载荷并在项目测试框架建立真实入口专项，覆盖四协议、64/16两级Full→恢复、wrap、C4、多waiter与poll/select/epoll、quiet2s后新流量及跨簇实际执行；已有host GREEN未变化时采信。构建后操作者执行NIC专项，真实结果与账本通过后填写Response，不用QEMU替代K3。（2026-10-15 两轮板测通过，操作者裁决"认可，通过"：A5 owner/runner/app actual hart=2/3/9 与 placement 一致且 duplicate-start 双拒绝（AlreadyStarted）；A6 ping 5发5收0%丢、UDP/TCP 8B 逐字节回显+主动关闭 FIN；A7 两级容量 accepted=80/64+16 边界、单次回收 restored 清零、flush Pending→release→C4、auto-release=0、waiter udp+listener 双 ok（Run 2）；A8 quiet 沉降相 2s 计数逐字节冻结→resume（Run 1 干净窗）；外部 RESET 回官方 Linux。观察项（Run 1 waiter 超时、Run 2 quiet 窗污染、启动行交错乱码）留档 Act Response，不阻塞验收）

## 3. UART 固定角色与用户态交互专项

- [x] 3.1 **T7**（依赖001 accepted）：在新`k3-smp-uart`模式复用PXA port、共享IRQ-save硬件锁、唯一copiers、tty与内嵌shell，固定RX/TX和shell/alarm affinity；先在现有UART core/plane、copier、tty与user-host入口定义跨hart寄存器批次/等待的测试，再做必要组装。检查真实TimerFuture创建/poll/drop不迁移及ISR unlock后wake；不改旧单hart用户入口语义。（2026-10-15 完成：plane harness 12/12（新增5个k3_smp_uart接缝用例，RED先行——rustc 模块缺失 exit 1、make No rule exit 2）；`make k3-smp-uart SMP=16`/`make k3-smp-uart-raw SMP=16` exit 0 且独立 FIT 产出，SMP=1/17 两目标拒绝；`make k3`/`make k3-uart`/`make k3-stdio`/`make k3-shell`/`make k3-smp-net SMP=16`/`make build`/`make lichee` 全部 exit 0；.gitignore 修复经 check-ignore 证实覆盖两新 OUT_CONFIG/OUT_DIR；两处等价接缝（k3.rs 载荷 cfg 扩展、uart_smp_snapshot 只读 `copier_last_poll`）与 alarm affinity 变体记录于当轮 Response；新入口 placement/copier/shell/alarm 观察行的实际 hart 判定归 T8 板测）
- [x] 3.2 **T8**（依赖T7）：完成UART native/用户态专项载荷与一次板测，覆盖FIFO/ring wrap、短RX末事件、Full→恢复、readiness/编辑/EOF、drop分类、有界公平、竞争/取消/timeout与四条件物理drain，固定hart观察跨簇；所有真实入口满足后才填写Response。专项raw reader与shell tty测试按独立boot子形态选择，不同时消费RX。（2026-10-16 两轮主序板测 + 用户裁决"我看了，正常工作，不阻塞在这里，填写回复吧直接"：A9 raw/shell 双形态 placement/copier pinned=actual（rx=0/tx=1）+shell first-write hart=8+alarm hart=0；A10 echo/long 512B wrap/quiet 10s 后 post-quiet len=23；A11 txfull 73728 短写恢复+四条件 drain+账本全零；A12 shell help/echo/sleep/行编辑/非法恢复/exit→join=0→drain done——Ctrl-C 对照、EOF 0-read 与 RESET 逐字确认按用户显式豁免收口（生产面零改动，MS17 板测与 native 结论承载），详见 Cycle Act Response 用户裁决与豁免节；观察项：载荷 `smp=1` 旧标签、banner 与观察行一次交错，留档不阻塞）

## 4. 多 hart BusyBox rootfs 与两设备组装

- [x] 4.1 **T9**（依赖002 accepted）：从 `k3_async_net.rs::board::run_k3_smp_net_boot` 提取可返回的单次 NIC 安装/固定 owner、runner 启动步骤，消费统一 placement 与调用者输出；保留硬件 gate、IRQ/arena/adapter/Service 顺序、handle、失败停止与 backing retention。旧 SMP net 包装器继续执行原诊断载荷，新组合不主动执行专项资格或创建第二 Service。重构先采信 001 GREEN，改动后做必要定向测试/构建；不修改既有恢复算法、不按 ISS02/ISS03 推断盲修。（2026-10-16 完成：001 T5 的 `install_smp_net` 即为可返回安装函数，本任务把 `install_smp_net`/`smp_fault`/`smp_singleton`/`SMP_NET_HANDLES` 的 cfg 从 `k3-smp-net` 扩为 `any(k3-smp-net, k3-smp-combined)` 并新增组合侧 export；`run_k3_smp_net_boot` 原样消费，`make k3-smp-net SMP=16` exit 0 + FIT 产出，安装顺序/所有权/handle 语义静态核对一致）
- [x] 4.2 **T10**（依赖T9）：新增 `k3-smp-combined` root/kernel feature、Make/FIT 与单一入口，直接组合 UART copiers、返回式 NIC 和普通 ext4 RAM rootfs `/bin/sh`；主任务/alarm/初始 shell 入队前固定 affinity；保持单 tty reader、paging 的唯一 axklib provider、rootfs/DTB 高位预留窗与无持久写入。将 K3Env、MAC identity probe、source133 ISR 的物理地址裸指针改为平台 phys_to_virt，覆盖 source60 的 GPIO ack，不改寄存器序/访问宽度或新增低位映射。保留旧入口互斥，只有新入口可组合，不启动内嵌最小 shell 或旧资格应用。先建立组合 placement/非法选择/缺目标 RED，并建立 MMIO 调用边的定向先行测试，再完成组装与目标构建 GREEN；不要求单独板测。（2026-10-16 完成：先行测试三个全 RED 后全 GREEN——combined placement 3/3、identity_probe_ env 化 1/1、k3_mmio phys_to_virt 1/1；`make k3-smp-combined SMP=16` exit 0 + 独立 FIT（kernel 0x180000000/FDT 0x2ef000000），SMP=1/17 原生拒绝；`make k3-rootfs` 与 `make k3-smp-net SMP=16` 兼容 exit 0；paging 下本地 k3_klib stub 排除、axruntime provider 唯一）
- [x] 4.3 **T11**（依赖T10）：在 `syscall/task/clone.rs` 的共用子任务发布点使 clone/clone3 子任务在首次入队前继承父 cpumask，exec 保持 mask；保证 init.sh→login shell→wget/nc/sleep 的 singleton 不被 full mask 替换。先行测试必须行使实际子任务入队调用边，定位 RED 后做最小修复与定向检查；不扩展通用任务迁移、timer runtime 或新调度框架。完成本轮直接受影响构建/自检后仅报告研发结果，不声称最终真板已通过。（2026-10-16 完成：clone_affinity host 测试以真实 `#[path]` include 生产 do_clone，fake 捕获首入队 mask，先观察到 full16≠singleton8 行为 RED（fork/THREAD/非singleton 三用例全失败），在 `spawn_task` 前补 `new_task.set_cpumask(curr.cpumask())` 后 3/3 GREEN；clone3.rs:93 共用发布点与 execve 无 cpumask 静态核对成立；修复后最终组合构建 exit 0 + FIT；包内全部 34 测试无回归）

## 5. 最终使用与短时性能测量

- [x] 5.1 **T12**（依赖003 accepted）：准备已有组合 FIT/RAM rootfs、BusyBox wget/nc/dd/stty/cmp 的实际入口与 Windows 原生对端命令，以及原厂 Linux 在 RAM 中使用的现有 BusyBox/loader 副本。复用 003 有效构建，不改源镜像、不下载/重编工具。当前无产品修复项；真实操作暴露新 syscall/设备阻塞时返回 Plan 给有限契约，不让 Act 决定新接口/恢复政策。无新增脚本、CLI、peer 协议或 validator。（2026-10-16 完成：FIT 1.6M（003 构建）与 rootfs-riscv64-min.img 128M 核验存在，git 只读核对产品无未暂存改动故不重建；debugfs 只读导出 busybox（806K，mode 0755）+ ld-musl-riscv64.so.1（599K）到 `/tmp/k3-t12/`，`file` 核验为 RISC-V 动态 ELF 对且解释器路径匹配；页面命令 CLI 核对：wget `-O/-Y/-T` 由交付副本 `ld-musl-riscv64.so.1 busybox wget --help` 实际运行确认（BusyBox 1.37.0），dd/nc/stty/cmp/wc 采信 004 Investigation Facts 未失效结论；源盘未写、产品/Make/rootfs 零改动。准备不计作真板通过，T13–T15 等待操作者真板执行）
- [x] 5.2 **T13**（依赖T12）：用户手动执行同一次最终形态正常使用：BusyBox 提示符输入/编辑/输出与外部命令、wget 下载已知 HTTP 文本并读回、nc TCP/UDP 输入及同内容回复、结束后 shell 继续响应。未执行不得标完成；失败保留首个直接现象返回 Plan。记录命令、真实输出与用户一句话结论；不运行已取消的恢复/容量/quiet/长稳矩阵。（2026-10-16 完成：两轮修复后 LOG=off 交付镜像板验全过——①取指 fault 崩溃根因=map_elf 惰性 exec 段+K3 跨簇取指 fault stval 污染，`kernel/src/mm/loader.rs` 改 eager populate，板验 shell 交互 GREEN；②wget socket ENOSYS 根因=net syscall 家族 cfg 门对 K3 全关闭，谓词收窄仅 k3-smp-combined 开启（syscall/mod.rs 20 处+file/mod.rs 2 处），板验 wget 10 字节 `k3-http-ok` 读回正确、TCP/UDP 回显、shell 存活；用户结论"很好正常工作"；载荷 dd 短读现象为流程修正（iflag=fullblock）非产品缺陷。详见 004/000 Act Response）
- [ ] 5.3 **T14**（依赖T13）：固定 115200 8N1，无流控，真实 UART TX/RX 各连续传输一次 256 KiB。Windows 独占 COM；TX 计首字节到末字节实收，RX 计首发送到板上 dd 完成记录返回，恢复 tty。原生字节内容检查通过后报告 B/s、8N1 线速利用率及有效数据 bit/s 占波特率比例，不设最低值；不把入队/fsync/stty 当物理完成。（SKIPPED: 用户 2026-10-16 原话"uart的测试不做"——A17 无 UART 吞吐见证，R10 实测缺口随豁免留档）
- [x] 5.4 **T15**（依赖T13；StarryOS TCP 可先于T14采集）：沿用同板口、网线及 Windows PC，以同一 BusyBox nc/dd 与接收端计时，对 StarryOS/原厂 Linux 各测单连接 TCP 板→PC、PC→板，每方向固定 3 个 32 MiB 样本。记录实际字节/时间、各次值、中位数/min-max及同方向 StarryOS/Linux 比例；Linux 数据未取得不得标完成。不设性能门槛、不调优、不引入完整矩阵；原生结果写 Response，Evidence none。（2026-10-16 完成（单样本裁决）：用户原话"我们不做什么多次测试，有当前数据已经足够"，每方向 1 有效样本——板→PC StarryOS 714.3 Mbps / Linux 934.0 Mbps = 76.5%；PC→板 StarryOS 218.8 Mbps / Linux 854.3 Mbps = 25.6%；Linux 基线经 HTTP 部署同一 BusyBox 取得，链路 end1 1Gbps/Full；比例两方向齐全，整体路径对比边界留档）

## Iteration Plan

### Iteration 000: AP 调度与中断底座

- Status: completed — 2026-10-15；Cycle `000-initial` Review accepted（T1–T4 全部完成、真板 A1–A4 真实满足；Minor findings 与提交集卫生事项留档于 Cycle Plan Review，不构成返工）。
- Tasks: T1–T4。依赖同轮T1→T2→T3→T4。
- Depends on: 已批准范围及现有单hart板级初始化、QEMU固定SMP软件机制。
- Stable baseline: 16 AP真的schedulable且执行，per-hart timer、S_SOFT、固定boot外设路由及跨簇remote-ready无需目标timer；原RAM回滚保持。
- Verification boundary: 各生产core的RED/GREEN；轮末受影响K3 AIA/timer/IPI/start host suites各完整一次、`make k3`与最终`make k3-smp SMP=16`、一次平台板测。kernel shared CS/axtask未改变时采信；改变时按疑点定向测试。旧async串口/网卡root features无实现面变化，不重复全量runtime。
- Diagnostic boundary: secondary entry/栈/页表/特性、local timer/soft、APLIC global门与IMSIC local文件；此轮不借GMAC流量或shell定位平台。
- Non-goals: NIC/UART数据面、新用户态组合、恢复故障修复、长稳。
- Balance audit: HSM/entry单独不能形成可调度异步平台；timer/IPI/route与实际任务执行共同闭合一个底座，保留在一轮。两种设备数据面有独立故障域，后置，首轮不承载全变更。
- 最近能力边界: T4构建/自检后真板由操作者执行；只列此前可执行步骤，不把等待列成任务。

### Iteration 001: NIC 多 hart 稳态

- Status: completed — 2026-10-15；Cycle `000-initial` Review accepted（T5–T6 全部完成、两轮真板 A5–A8 真实满足、操作者裁决"认可，通过"；3 项 Minor finding 留档于 Cycle Plan Review，不构成返工）。
- Tasks: T5–T6。
- Depends on: 000 accepted且板测审核PASS。
- Stable baseline: GMAC两方向在固定owner/runner下运行，实际跨簇应用通信、Full→恢复、C4/readiness/quiet成立。
- Verification boundary: 新接线RED/GREEN，轮末K3 production host全包一次、NIC板测一次；共享axnet变动时默认/queue/qemu三feature各全量一次，否则采信最近结论。新网络FIT构建一次，旧受影响入口按需。
- Diagnostic boundary: NIC ISR→queue event→remote IPI→owner→slot→stack→socket，DMA/cache/tail沿既有实现；不混入UART/恢复/soak。
- Non-goals: UART专项、reset/link矩阵、组合长稳。
- Balance audit: placement/安装与真实数据面不可独立作为NIC成果，合并；恢复是独立生命周期故障域，保留003。

### Iteration 002: UART 多 hart 与用户交互

- Status: completed — 2026-10-16；Cycle `000-initial` Review accepted（T7–T8 完成、真板 A9–A12 真实满足，操作者裁决"我看了，正常工作，不阻塞在这里，填写回复吧直接"，Ctrl-C/EOF/RESET 逐字确认三项显式豁免留档；4 项 Minor finding 留档于 Cycle Plan Review，不构成返工）。
- Tasks: T7–T8。
- Depends on: 001 accepted（诊断顺序；技术基础为000与既有UART单hart成果）。
- Stable baseline: PXA IRQ→跨hartcopier→tty/shell真实双向输入输出，Full/readiness/cancel/drain与timer成立。
- Verification boundary: 轮末UART crate、K3 core/plane、tty/user-host各受影响suite一次；UART专项板测一次。共享common面改变时QEMU/D1受影响构建一次，不运行历史UART未验收项代替。
- Diagnostic boundary: PXA硬件锁/IER/LSR、RX/TX SPSC、timer wheel固定affinity、tty/user进程；无NIC流量依赖。
- Non-goals: 网络恢复、同时运行、rootfs/存储。
- Balance audit: port/copier/tty/shell沿同一字节所有权链共同形成可用专项；网络仍独立，不因NIC通过推定UART通过。

### Iteration 003: 可构建的多 hart rootfs 与双设备入口

- Status: completed — 2026-10-16；Cycle `000-initial` Review accepted（T9–T11 全部完成；A13–A15 研发基线判据满足，5 项非阻塞 finding 留档于 Cycle Plan Review，含 T9 计划基线滞后说明与提交集卫生事项，不构成返工）。
- Tasks: T9–T11。
- Depends on: 002 accepted，001 NIC已稳定。
- Stable baseline: 16-hart 组合入口可构建，唯一 NIC owner/runner 与 UART copiers、普通 RAM rootfs loader、固定 shell/alarm 及子任务 affinity 已组装；这只是研发基线，不宣称硬件最终使用已通过。
- Verification boundary: T9 重构采信 001 未失效 GREEN；T10 新接线 RED→GREEN；T11 真实子任务入队路径 RED→GREEN。运行改动所需最小定向测试与 `make k3-smp-combined SMP=16`；只检查直接受影响的旧构建（网络包装器、rootfs、共享 clone 的编译面），不自动扩展全包/QEMU SMP/所有旧模式。昂贵验证预算：本轮无真板或全套历史回归。
- Diagnostic boundary: feature/entry、唯一 Service 与 tty、rootfs 内存布局、MMIO 地址转换、paging/axklib、普通 loader、clone/exec affinity；不把 PHY 历史残余当预设修复任务。
- Non-goals: 独立恢复/容量/C4/quiet/readiness 压力、故障注入资格、长稳、工具完整 Linux 兼容、性能、真实存储和通用迁移框架。
- Balance audit: NIC 可返回、rootfs/串口组装和子任务固定 affinity 共同形成可运行最终用户态的一个入口，缺一不能交付；工具可达性与真板最终使用留在 004 独立诊断，不另做中间最小 shell 交付。
- 展开检查: completed；复用 002 accepted 并读取目标代码，已在当前 Cycle 闭合 T9–T11 完整契约。T10 用既有 plane/MAC harness 与 embedded-user-host 的 native MMIO 定向测试；T11 在该现有包直接 include/调用真实 CloneArgs::do_clone，fake enqueue 捕获首次入队mask，覆盖普通/THREAD与非singleton继承，不复制生产实现。ISS02/ISS03 只在出现与最终目标有关的实际阻塞时补查。

### Iteration 004: 最终正常使用与 UART/TCP 性能测量

- Status: completed — 2026-10-16；Cycle `000-initial` Review accepted（T12/T13/T15 完成、T14 用户豁免；两处产品修复——loader eager populate 与用户态网络 syscall 门收窄——均有先行真板 RED 与修复后板验 GREEN；非阻塞 finding 留档于 Cycle Plan Review）。
- Tasks: T12–T15。
- Depends on: 003 accepted。
- Stable baseline: 同一 16-hart BusyBox RAM rootfs shell 内串口交互、HTTP 下载与 TCP/UDP 交换真实可用；UART RX/TX 实线有效吞吐、同板 StarryOS/原厂 Linux TCP 两方向吞吐已测量并报告，没有最低性能要求。
- Verification boundary: 默认无产品变更，采信 003 构建。昂贵验证预算为一次 StarryOS 最终形态会话（正常使用、UART 两方向各一个样本、TCP 两方向各3个样本），另一次原厂 Linux 阶段（TCP 两方向各3个样本）；由用户手动执行。步骤、计时/字节边界及错误处置自包含于当前 Cycle，不恢复旧矩阵。新实质缺口返回 Plan，修订契约后才实施/定向测试。
- Diagnostic boundary: BusyBox/musl、clone/exec、FS/net syscall/FD/poll、异步数据路径，以及原生计时/串口接管/工具结束；正常使用失败先于测速诊断。
- Non-goals: ping/ICMP socket、DNS/DHCP/TLS/IPv6、Internet、完整 Linux 兼容、恢复/长稳资格、性能门槛/优化/CPU/p99/多流/UDP性能、真实存储、第二 UART、迁移与新框架。
- Balance audit: 用户明确要求最后一轮完成短时测量。现有 applets 和 Windows 原生 I/O 能复用同一次最终形态，不需另建测速产品或硬件 bring-up；T12准备、T13正常使用、T14串口、T15网络各保留独立诊断与完成判据，原厂 Linux 是同一对比任务的能力边界，不增后继轮次。发现需要新工具/产品接口时返回 Plan，不扩大本轮。
- 展开检查: completed；复用003最终 Response/accepted Review，补读真实普通 socket/tty/clock/devfs 调用边，实际检查 BusyBox CLI/单调时钟；完整事实、契约、操作、RTM和停止条件在004/000。Gate 1/2 PASS，用户2026-10-16原话“批准”。

## 取消的后续验证

以下状态均为 `SKIPPED: 用户 2026-10-16 批准以最终 BusyBox 正常使用替代`，不是 PASS 或已修复：

- 原 T9–T11 的多 hart 恢复/取消完成交错资格、Up reset/Down reset→插回双轮矩阵、容量/C4 双轮、delayed/stuck/fatal 注入和门关闭持续入流专项。
- 原 T13 的 30 分钟流量与交互、定时恢复、30 组命令、末尾资源账本/物理 drain 专项、quiet 后恢复。
- 未因本次修改而失效的历史全套 host/QEMU/旧 mode 回归及逐轮外部 RESET 真板确认。

既有恢复代码、错误/取消/超时、安全 backing retention、四条件 drain 与平台隔离继续保持。正常使用遇到的真实阻塞仍必须解决；具体方案不从历史推断直接导出。

## Requirements Traceability Matrix

| Requirement | Task | First Test | Status |
|---|---|---|---|
| R1 | T1, T4 | k3 SMP构建RED；`hart_start_refusal_stops_before_runtime_wait`；native16 AP执行 | Covered |
| R2 | T2, T3, T4 | `secondary_local_enable_does_not_register_handler_twice`、`soft_ack_allows_second_delivery`、`remote_source_gate_preserves_local_eie` | Covered |
| R3 | T3–T8, T10–T11 | 既有 placement/affinity 与真板结论；combined_ placement、clone_affinity integration test直接调用真实do_clone | Covered（当前003契约已闭合） |
| R4 | T7, T8 | K3 UART core/plane、copier/tty/user-host定向先行测试；真实FIFO/ring/Full/drain | Covered |
| R5 | T5, T6 | service_owner/adapter_contract/stack_socket、native容量/C4/readiness/quiet与PC字节 | Covered |
| R6 | T9, T12（仅实际阻塞） | 提取前有效 GREEN，保持既有恢复/安全所有权；实际修复先复现再定向 GREEN | Simplified：用户批准取消恢复专项及预设缺陷修复，原话见 proposal |
| R7 | T9–T13 | 组合接线先行测试；BusyBox 最终形态串口与网络同一启动真实使用 | Simplified：用户批准取消 30 分钟及恢复交错，以最终使用替代 |
| R8 | T9–T15，既有 T1–T8 保留 | 改动所需定向先行测试/构建；RAM/FIT 与既有回滚手段保持；原生短时测量 | Simplified：用户批准取消完整历史/逐轮回滚专项 |
| R9 | T10–T13 | RAM 布局/普通 loader 既有 GREEN；wget/nc 工具已存在，最终真实下载/交换 | Covered（I20 的 wget+nc 范围纳入本变更） |
| R10 | T12,T14 | 真实 UART dd/COM 接收量、物理完成计时与原生内容比较 | Waived：用户 2026-10-16 原话"uart的测试不做"，T14 取消，无板上 UART 吞吐见证，R10 实测缺口随豁免留档 |
| R11 | T12,T15 | 两系统同方法 TCP 接收字节/时间，同方向比值可计算 | Covered（单样本裁决：用户原话"我们不做什么多次测试，有当前数据已经足够"，每方向 1 有效样本，比例 76.5%/25.6%） |

## 当前执行批次与停止条件

000–004 全部 accepted，T1–T15 全部闭合或豁免。004/000 Act Response `reported`、Plan Review `accepted`（2026-10-16）：A16 板验达成；A17 按用户豁免不产生数据；A18 在单样本裁决下达成。change 无剩余执行任务，后续为收尾流程（用户调用 `openspec-docs-maintainer`：合并行为规格、同步全局 tasks/SNAPSHOT、归档）；短读 Issue 候选与 ISS02/ISS03 处置由用户另行决定（Recorder）。完整范围/顺序/任务边界以各 Cycle 为准。

| 阶段/Gate | 状态 |
|---|---|
| 需求与场景 | completed；Gate 1 PASS，用户批准展开并新增性能测量、无最低门槛和Windows对端 |
| 复用与必要补查 | completed；直接前置003 accepted及实际工具/代码边界，事实与完整契约在004 Cycle |
| 设计与分轮平衡 | completed；000–003保持，最后004新增两项短时测量，无后继轮次 |
| RTM | completed；R11已覆盖（单样本裁决），R10按用户豁免Waived，既有R6/R7/R8 Simplified原批准保持 |
| 新 Plan Context | completed：004/000-initial已执行并收口 |
| Gate 2 | PASS：用户2026-10-16原话“批准” |
| Gate 3/4 | PASS（T13两处产品修复各有先行真板RED与修复后板验GREEN；T12/T13/T15完成声明基于真实运行与用户真板结论，T14按用户裁决SKIPPED） |
| Gate 5 | PASS：无未记录阻塞；用户三项裁决原话留档于004 Cycle |
| Evidence创建 | SKIPPED：Mode none，结果写Act Response |
| 全局同步/经验落账/归档 | SKIPPED：未获授权，等待用户调用 Maintainer/Recorder |
