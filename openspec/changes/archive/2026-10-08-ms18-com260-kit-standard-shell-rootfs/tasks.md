## Iteration Map

| Iteration | 目标 | 任务 | 依赖 | 独立验收 | 状态 |
|---|---|---|---|---|---|
| 000-ram-rootfs | RAM 范围保护、唯一块设备与 ext4 文件读取 | 1.1–1.2 | MS17 accepted 底座 | host 边界测试、K3 构建、板上文件读取及缺参数拒绝 | accepted（2026-10-08 Plan Review）：host 18/18、构建门全过、板测步骤 1–4 全绿，坏 FS 按用户豁免跳过 |
| 001-standard-shell | 普通动态 ELF 用户进程与外部命令 | 2.1–2.3 | 000 accepted；D1-r1 | K3 普通 `/bin/sh`、musl、外部命令、原模式兼容、RAM 布局修复 | accepted（2026-10-08 Plan Review）：S1 板测全绿（新 DTB 窗 0x2ef000000 落位、越过 OOM 死亡点）、host 30/30、构建门全过、诊断归零；S2 三变异与 exit 收尾按用户豁免留档 |
| 002-shell-behavior-and-reset | 命令组合、失败/取消、退出与官方恢复 | 3.1–3.2 | 001 accepted | 规格 S1–S9 全覆盖，RESET 官方登录 | cancelled — 2026-10-08 用户裁决取消（原话见「见证状态」节）；串口/shell 交互、错误报告、Ctrl-C 前台取消与 RESET 官方恢复已经手动板测确认 |

首轮执行契约：`iterations/000-ram-rootfs/000-initial.md`。用户已批准完整计划，原话“批准计划”；首轮实施完成，Act Response `reported`，Plan Review `accepted`（2026-10-08）。后继轮只有前轮 Review accepted 后才创建自包含执行契约。

分轮审计：首轮聚合 RAM 布局与 ext4，是同一 root device 数据链；第二轮独立验证普通动态 ELF/用户进程；第三轮聚合已启动 shell 上的组合与恢复行为。三轮各有 host/build 或直接板测结果，避免在存储与用户态同时失败时跨层猜测。未展开的后继轮以下仅规定验收，不构成当前实施契约。

## 1. RAM rootfs 挂载与读取

- [x] 1.1 在 K3 平台 RAM ranges、kernel boot selection、Cargo/Makefile 增加隔离的 `k3-rootfs` 模式与启动范围校验；保护 rootfs/DTB，防 allocator 覆盖；以生产边界函数的 host RED→GREEN、合法构建与冲突模式拒绝验证。（host 18/18 GREEN；`make k3-rootfs` exit 0；`SMP=2` 拒绝；`k3-rootfs,k3-stdio` compile_error；`make k3-shell` 保留）
- [x] 1.2 在 K3 入口使用已有静态 RAM block device 接入 ext4，避免 memory-root/自动 probe 重复挂载；板上解析并读取 `/bin/sh`、BusyBox 与 musl 的文件数据后 drain/park，以合法 rootfs 文件结果、缺参数拒绝及 native 构建验证。（板测：resolve=/bin/busybox、825088/613016 B 均 ELF magic=true、drain done；缺 initrdmem 明确报 MissingInitrdmem 无 fallback；坏 FS 板测按用户原话豁免）

## 2. 普通 shell 与文件系统用户程序

- [x] 2.1 在 `kernel/src/entry.rs` 接入 ordinary `load_user_app`、用户进程/tty/stdio 与 join；复用现有启动 args/init.sh；以缺 shell/解释器/坏 ELF 的启动错误见证，再以 K3 普通 shell 和 `/bin/ls`、`/bin/cat` 运行验证，移除首轮 mount 后 park 的临时阶段行为。（板测 S1 PASS：dtb_paddr=0x2ef000000、mount-ok、banner→提示符、`cd /`+`ls ./` 输出真实 ext4 内容、^C 恢复——等效替代契约点名命令；S2 三变异与 exit 收尾按用户原话豁免，风险留档于 001-replan Board Witness）
- [x] 2.2 检查受影响 `k3-shell`/`k3-stdio` 与 feature 隔离；以 native 构建/已有 host 测试验证兼容；共享 loader/进程/tty 实際改变时运行 QEMU 同源 rootfs 外部命令，保持其原始 image 不写入。（001-replan 复跑全过：k3-rootfs/k3-fit/k3-shell/k3-stdio/QEMU build exit 0、host 30/30、SMP=2 拒绝 exit 2；justrun 以净共享 diff 归零记录 SKIPPED）
- [x] 2.3 修订 RAM 布局并清理诊断（001-replan 新增，D1-r1）：DTB 承载窗上移 `[0x2ef000000,0x2ef100000)`、rootfs 承载窗收档 `[0x240000000,0x250000000)`（ISS01）；k3-rootfs 专用 ITS/独立 FIT 名；移除四处临时诊断；以 host 布局不变量 RED→GREEN、构建门与 dumpimage FDT 地址验证。（host 21/21 RED→GREEN；`StarryOS_riscv64-k3-rootfs.fit` FDT load=0x2ef000000；四诊断文件 diff 归零；旧 k3-fit 产物不变）

## 3. Shell 行为与恢复

- [ ] 3.1 用板上普通 shell 建立脚本、fork/exec/wait、管道/重定向、文件读写、命令错误和 Ctrl-C 恢复见证；仅修复可达缺口，按缺口 RED→GREEN 验证；实质设计差异返回规划。SKIPPED: 2026-10-08 用户裁决取消 Iteration 002——命令错误报告与 Ctrl-C 前台取消已由手动板测覆盖（见见证状态节）；脚本/管道/重定向/文件读写不再正式验证。
- [ ] 3.2 验证空闲/编辑输入、正常 exit→unmount/flush→物理 drain/park、重新加载 image 丢弃当次写入及外部 RESET 官方登录；按原生输出/用户直接观察记录结果，并完成本轮全部规格覆盖检查。SKIPPED: 同上——RESET 官方恢复经用户实测确认（原话见见证状态节）；空闲编辑、exit 收尾与易失写入不再正式验证。

## Scenario Coverage

| 场景 | 任务/设计 | 最简目标行为判定（实施时执行） |
|---|---|---|
| S1 普通 shell | 2.1 / D2–D3 | 提示符后 `/bin/ls /bin`、`/bin/cat /etc/os-release` 输出实际内容，均回到提示符 |
| S2 启动错误 | 1.1–1.2、2.1 / D1–D3 | host 覆盖缺/坏范围；坏FS板上错误；第二轮用可丢弃镜像副本移除 shell/ldso 或破坏 ELF，逐项观察对应失败层，不动原始 image |
| S3 脚本/管道/重定向 | 3.1 / D3 | `printf 'hello\n' \| /bin/cat` 输出 hello；写脚本含 `/bin/echo script-ok` 与 `exit 7`，`/bin/sh` 执行后 `echo $?` 得7；重定向到 `/root/ms18-ram-only` 后 `/bin/cat` 读回 |
| S4 错误/取消 | 3.1 / D3 | 不存在的命令、cat不存在的文件返回错误；`/bin/sh -c 'exit 7'` 的状态为7；`/bin/sleep 30` 时Ctrl-C，随后 `/bin/echo recovered` 可执行 |
| S5 易失写入 | 3.1–3.2 / D1–D2 | 在新加载 image 中确认目标文件不存在，写 `/root/ms18-ram-only` 并读回；重新加载原始 image 后 `test -e /root/ms18-ram-only; echo $?` 为1 |
| S6 官方恢复 | 3.2 / D1 | 外部 RESET，原启动路径显示官方 Linux 登录；不保存启动环境 |
| S7 空闲/编辑 | 3.2 / D3 | 空闲后键入带空格的echo命令，退格修改后执行，得到完整预期字符串 |
| S8 exit/drain | 1.2、3.2 / D3 | 首轮阶段 drain/park；最终 shell `exit 0` 后观察进程exit、fs收尾和真实drain done/timeout，随后park |
| S9 模式/兼容 | 1.1、2.2 / D3–D4 | 新/旧合法模式 native build成功；互斥/SMP拒绝；共享代码改变时QEMU普通命令运行通过 |

### 见证状态（2026-10-08 用户收口裁决）

用户原话：「我想这些手动测试足够说明串口和shell工作正常了，所以最后一个iter的各种功能性验证我们不做，reset我已经测试，确实可以回到官方linux这也确认了，最后一个iter没必要，取消掉吧」。RESET 一句话验证记录：「reset我已经测试，确实可以回到官方linux」。

| 场景 | 见证状态 |
|---|---|
| S1 | 板测达成（001-replan：banner→提示符、`cd /`+`ls ./` 真实 ext4 内容；补充会话：`/bin` 全 applet 清单、`which echo`、`echo 2`） |
| S2 | range 层板测（MissingInitrdmem）+ host 契约测试；resolve/load/InvalidExecutable 板上层按用户豁免，host/QEMU 同源路径有效 |
| S3 | 未正式见证（用户裁决取消） |
| S4 | 核心已见证（补充会话：`man`→`man: not found`、`netstat` 缺文件报错回提示符、`sleep 60`+`^C` 前台取消恢复） |
| S5 | 未正式见证（用户裁决取消） |
| S6 | 用户实测确认：外部 RESET 回官方 Linux（原话留档于上） |
| S7 | 未正式见证；tty canonical/退格编辑为 MS17 已验收同栈契约 |
| S8 | drain/park 为 Iteration 000 板测验收同栈；shell `exit` 后 join/unmount/flush/drain 观察按用户豁免 |
| S9 | 全部自动门 + SMP/互斥拒绝，001-replan 复跑（host 30/30、四构建门、SMP=2 exit 2） |

S3、S5、S7 与 S8 后半（exit 收尾）不再取得正式见证——收尾时行为规格合并按本表如实处理，验收结论限于实际见证的场景。

QEMU 必要回归采用原生命令 `make ARCH=riscv64 SMP=1 build`，然后 `make ARCH=riscv64 SMP=1 DISK_IMG=/home/daivy/projects/serial/work/StarryOS/rootfs-riscv64.img QEMU_ARGS=-snapshot justrun`，直接在普通 shell 执行外部命令。`-snapshot` 保留源 image；运行链依本仓 make/qemu.mk，不扩网络资格测试。第二轮展开时只补查实际改动涉及的共享表面。

## Approval and Gates

- Gate 1：PASS，用户原话“批准，先做 RAM rootfs 正式 shell”。真实存储要求保留后续。
- Gate 2：PASS——首轮已批准并执行完毕；001 轮 Cycle 000 已执行至板测 blocked；Cycle 001-replan 用户 2026-10-08 批准（原话“批准”），Status ready。
- Gate 3 / Gate 4：首轮已执行并经 Plan Review accepted；001 轮 Cycle 000 自动门过、板测 blocked 后经 replan，Cycle 001-replan 自动门 + 板测 S1 + A4 已过并经 Plan Review `accepted`（2026-10-08）；豁免项（S2 三变异、exit 收尾、A1 等效命令序列）原话与风险均已留档。
- 当前停止边界：change 交付范围收口——Iteration 000/001 accepted，Iteration 002 按用户 2026-10-08 裁决取消（原话与见证状态见上节）。收尾（行为规格按见证状态合并、SNAPSHOT/tasks 同步、归档）归 `openspec-docs-maintainer`，待用户指令；ISS01 关闭（按修正数字：原 1 GiB+16 MiB 浪费 → 现 256 MiB 档位 + 1 MiB DTB 窗）与 axruntime 通用选区策略候选落账待用户指示 Recorder。
- 原 MS18 的真实存储后续不在本次任务中，RAM 用户环境验收不标记原里程碑完成；全局 roadmap 和其他已交接 Cycle 未修改。
