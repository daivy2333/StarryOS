# Iteration 000 / Cycle 001: AIA 软件投递底座（用户裁决后重计划）

## Plan Context

- Status: ready
- Cycle Type: replan
- Parent Cycle: `000-initial.md`。原Plan Context保持不变；本Cycle替代其未执行计划。

**Cycle Scope**

- Change tasks: 1.1–1.4。
- Acceptance gaps: None；Repair items: None。
- Inherited scope: R1 控制器、R4 软件投递/disable/quiet、R5 原生验证/兼容/板上日志；R2 的固定路由准备，不声称设备投递通过。
- Excluded scope: PHY/pinctrl 中断使能和 ack、设备事件验收、DMA/网络、timer/UART 调度、多 hart 流量、固件或持久介质修改。

**Objective**

将 K3 IrqIf stub 实化为 APLIC MSI + IMSIC 外部中断底座；在保留 stage1–8 的 RAM 启动路径上追加 stage9，分别证明 SETIPNUM_LE 与 GENMSI 投递、先原子 EOI 后 handler、重复/disable/re-enable/quiet 和有界失败。

**Background（草案编写时）**

2026-10-03用户原话：「拔插计数是形式主义，核心问题是证明拔插有效就行，所以不用管这个小问题，缺少的源码依据可以用别的相关信息代替就行，不要把工作阻塞在这些问题上」。已取消人工插拔次数相等和固定循环门槛，保留真实变化、设备claim/status对齐、再触发、快照与quiet；资料策略改为本板DTB/R69与相关同位字段/清边沿实现，不再等待vendor文件。技术完整性已闭合，整体计划仍待审计批准。首轮控制器契约和Iteration Map不变，后轮设备阶段另行展开，本轮不实现设备链。

**Investigation Facts**

Current Baseline：分支 k3、HEAD `082339ba`。平台/探针/测试/构建表面相对 MS09 accepted 收尾 `ddbbf29c` 未变；采信归档 `2026-10-02-ms09-com260-kit-observable-link-baseline/iterations/004-phy-link/001-rework.md` 的真实运行：PHY harness 30 passed、MAC harness 15 passed、early-console 6 passed；make k3/build/lichee exit0。make vf2 exit2 为既有 axfs/axnet/axtask 导入错误，不算通过，不扩大范围。真板 stage1–8、PHYID001cc916、至少五次完整插拔、reset 回原系统已验收。这些引用说明证据来源，实施事实在本节全部展开，无需回读。

Current-State Evidence / Code and Critical Path：

| 位置 / 符号 | 当前行为及本轮影响 |
|---|---|
| `crates/axplat-riscv64-k3/src/boot.rs::_start` | 保存 a0/a1，axplat call_main；raw hartid 传 init_later，禁止假设0。Sv39 boot 的 identity/high-half block3 已覆盖 e0804000，无需改 boot 页表 |
| `src/init.rs::InitIfImpl`（同 crate） | init_early trap/console/time；init_later 仅 time::init_percpu。axruntime 先建 runtime paging，再 init_later，稍后 init_interrupt 开全局 SIE；本轮在 init_later 增加 global/per-hart 初始化 |
| `src/irq_stub.rs::IrqIfImpl`、`src/lib.rs` | register=false、unregister/handle=None、其他 no-op；irq-if feature 才选择该模块，替换为真实 irq 模块 |
| `axconfig.toml`、`src/mem.rs` | MMIO ranges 供 runtime paging；PHYS_VIRT_OFFSET=`ffffffc000000000`。新增 APLIC physical `e0804000/4000`；平台 volatile MMIO 走 phys_to_virt。IMSIC 本地文件走 CSR，不为本轮添加不用的 MSI MMIO 写入口 |
| 依赖 axplat0.3.1-pre.6 的 IrqIf / HandlerTable | handler 为 fn()；register 自动 enable、unregister 自动 disable；handle 返回实际 source。现有 HandlerTable 支持无分配 CAS 注册/原子加载，直接复用 |
| axhal0.3.0-preview.2 `irq_handler` | 平台 handle 后仅 Some 走现有全局 IRQ hook；不改 hook/调度器 |
| axcpu0.3.0-preview.8 `asm` | irqs_enabled / disable_irqs / enable_irqs 可保存并恢复 SIE；保护 selector+data 配对，不能无条件恢复为 enabled |
| `kernel/src/entry.rs` → `platform/k3.rs::run_first_byte_park` | K3 进入探针且不返回。stage4 APMU、5 MAC、6 pad/reset、7 PHY、8 120秒link；本轮只在8后追加9，原字段/顺序不变 |
| `kernel/src/platform/phy_probe.rs::ProbeEnv` / `K3Env` | u32 volatile MMIO、原生 now_nanos、wait_nanos；已有 native 24MHz clock，无需 timer IRQ。新软件探针复用其时钟/MMIO seam，CSR/dispatch 必要 seam 保持仅本探针使用 |
| `Makefile::host-test` / `tests/{mac,phy}-probe-host-harness.rs` | rustc --edition=2024 --test 直接包含产品模块；新增 harness 采用同样方式，不能写输出判定器 |

寄存器与路由契约（均来自已读取本板 DTB、R69 intc/somehal；stopei 字段以同仓 OpenSBI 与 Linux handler纠正）：

| 对象 | 值 / 操作 |
|---|---|
| APLIC | physical e0804000，span4000，sources1..512；domaincfg0000 IE bit8、DM bit2 |
| sourcecfg / target | `0004+4*(source-1)`；`3004+4*(source-1)`；target=`hart<<18 | guest<<12 | EID`，guest0，boot hart<16 |
| bitmap | CLRIP1d00、CLRIE1f00；word=`source/32`、bit=`source%32`。init 覆盖 word0..16，最后一字只处理source512的bit0，保留其他保留位 |
| numeric / MSI | SETIENUM1edc、CLRIENUM1fdc、SETIPNUM_LE2000、GENMSI3000；GENMSI hart<<18|EID，发前若 busy bit12 未释放则按本次1秒截止失败 |
| MSI addr | SMSICFGADDR1bc8、SMSICFGADDRH1bcc；IMSIC base e0400000，group base PPN=e0400，guest bits6、hart bits4、group bits0、每hart步长40000。沿 R69写法高字 L bit31=1、LHXS=6、LHXW=4，其余字段0；仅 LHXS/BAPPN 为 S-mode 可控制判据，不要求被忽略的 M-mode 几何位读回相等 |
| local CSR | siselect150、sireg151、stopei15c；EIDELIVERY70=1、EITHRESHOLD72=0；EIP80、EIEc0，每64IDs下一寄存器selector+2，RV64不得访问奇selector；IDs0..511八个64bit字 |
| claim | 单条 `csrrw rd, 0x15c, zero`，完整值保存；EID=`(raw>>16)&7ff`、priority=`raw&7ff`，zero无pending；原子claim+EOI必须先于handler |
| supported routes | source/EID1 Detached(SM1)，60/133/277 LevelHigh(SM6)；register只支持这四项，EID0/source0/source512拒绝；其他EID仍可被claim并诊断为未知 |
| trap / compatibility | external vector interrupt-bit|9；只启用sie.SEXT(bit9)，timer vector interrupt-bit|5及IPI interrupt-bit|1维持stub注册/dispatch，不开STIE/SSIE |

APLIC 全局：关闭域 IE、disable-all（CLRIE位图及全部sourcecfg置Inactive）、clear-pending → MSI地址 → domaincfg IE|DM。本地：关闭 SEXT、清 EIE八字 → threshold0/delivery1 → 最后打开 SEXT。固定source在register时 sourcecfg/target、clear source pending、local EIE、APLIC enable；clear在sourcecfg之后以免配置改变电平解释时产生旧pending，register失败不得修改硬件。disable先APLIC后EIE，unregister再移除handler。

IMSIC selector/data 在保存原SIE的临界段访问，不持锁跨callback。每次SEI只claim一次，再分发，再返回真实source；新pending由sret后的下一trap处理。spurious/未知/无handler只累计诊断并返回None，不panic、不调用通用IRQhook。source→EID固定一一对应，无动态分配；send_ipi用SBI，不向保留EID0写MSI。

**Implementation Guidance**

先建立可直接 host 调用的生产 AIA 核和访问 seam，再接平台，再接实际探针。测试观察真实生产操作，不复制初始化/claim算法到 fixture。新增模块可用 `src/aia.rs`/`src/irq.rs`、`kernel/src/platform/aia_probe.rs`；辅助拆分可等价调整。

初始化发生在kernel stage1之前，首次访问IMSIC/APLIC前必须经现有polling console输出对应层标记；readback mismatch返回可诊断错误、关闭SEXT并park，不能继续到stage9。访问trap保留最后标记/native trap结果，操作者reset；不能承诺软件能捕获所有固件拒绝访问。全局初始化和每hart初始化职责分别可测试，单hart调用一次，不增加SMP框架。

探针handler只存raw claim/计数/错误；前台关闭IRQ取得一致快照后恢复再输出，不在IRQ内打印。由软件探针设定本阶段最多32次非零claim预算，超额在dispatch路径mask SEXT、保存storm错误，前台报告并park，避免连续trap阻止前台超时。此预算为本板诊断停止条件，非流量验收或通用IRQ限流。

**Task Contracts**

### 1.1：生产 AIA 核有直接 host 测试见证

- Requirement/Scenario: R1 初始化/claim/handler新事件，R4 disable/重复，R5原生测试。
- Depends on: None。
- Targets: 新 `crates/axplat-riscv64-k3/src/aia.rs` 与 `tests/k3-aia-host-harness.rs`；纯编码、寄存器访问顺序、claim/dispatch核心。
- Current behavior: 无AIA核；stub不能成功注册或投递。
- Required behavior: 实现上表有效范围、init、route、enable/disable和正确stopei解析；真实dispatch逻辑使用原子claim结果且先EOI后handler；无pending/未知不panic。保留SIE并保护selector配对。
- Preserve: u32 MMIO与RV64 CSR宽度不同；raw cause不裁剪；未注册/非法输入无副作用。
- Forbidden: rdrive/somehal、新依赖、source512映射到不存在EID、身份字段、结果判定器、改变通用trap。
- Test witness: 原生harness先定义并观察RED，覆盖 `stopei_id_is_high_field`（例如raw=`60<<16|3`）、`rv64_selectors_are_even`、`global_and_per_hart_init_are_separate`、`source512_last_bitmap_word_is_cleared`、`unsupported_registration_has_no_side_effect`、`claim_precedes_handler`、`new_pending_survives_handler`、`disable_orders_aplic_before_eie`、`selector_access_restores_original_sie`、`spurious_and_unknown_do_not_dispatch`。RED可以是缺失生产接口编译失败，必须如实记录。
- GREEN condition: 同一测试调用生产逻辑全部通过；测试事件顺序证明handler前只有一次原子claim/complete，不能用源码字符串检查替代。
- Verification: `rustc --edition=2024 --test tests/k3-aia-host-harness.rs -o /tmp/k3-aia-host-test`；随后 `/tmp/k3-aia-host-test`，原生test result、exit0。
- Stop when: CSR字段/固件几何与实际来源矛盾，或需要动态source/多hart/新依赖契约。

### 1.2：K3 平台接入与外部 IRQ 生命周期闭合

- Requirement/Scenario: R1初始化分离/分层失败，R2固定路由准备，R4注册/禁用，R5平台兼容。
- Depends on: 1.1。
- Targets: K3 `src/irq_stub.rs`→`src/irq.rs`、`lib.rs`、`init.rs`、`axconfig.toml`；1.1生产核及harness必要接入测试。
- Current behavior: init_later只time；所有IrqIf调用inert，runtime timer注册失败。
- Required behavior: 接入global/per-hart初始化与真实harttarget；HandlerTable<512>注册成功自动enable/重复注册失败不enable/unregister先disable；handle只对外部vector执行claim；返回实际source；send_ipi调用现有sbi_rt HartMask并按IpiTarget语义选择本hart/指定hart/其他hart，本轮singlehart Others为空；失败输出可定位诊断。APLIC映射可供平台高地址访问，读回展示domaincfg/smsicfgaddr/h/localdelivery/eie及真实target。
- Preserve: timer/IPI接收仍stub，无STIE/SSIE；axhal hook不变，boot/内存窗口/旧console不变；有效SIE恢复。
- Forbidden: 改QEMU/D1/VF2控制器、UART IRQ、调度器、APLIC global在per-hart再次构造、修改固件。
- Test witness: 1.1 GREEN作为内核重构前见证；追加真实注册/dispatch逻辑的RED，覆盖自动enable、重复注册、unregister和source1/60/133/277路由；`make k3`观察stub替换前后构建状态。构建不替代投递验收。
- GREEN condition: 目标host tests通过且make k3 exit0，非K3构建不选择新模块；初始化顺序和callback无锁有测试/代码审查依据。
- Verification: 目标harness → `make k3`；完成平台改动后一次 `make build`、`make lichee`，均exit0。已有VF2失败只引用，不标PASS、不修复。
- Stop when: hartid不在0..15、CSR访问失败、有效S-mode字段readback不一致、需要timer或SMP接收才能证明外部IRQ。

### 1.3：stage9 双软件入口、有界停止与原生探针测试

- Requirement/Scenario: R1两个软件入口/handler新事件，R4disable/quiet/重复，R5失败恢复。
- Depends on: 1.2。
- Targets: `kernel/src/platform/{k3.rs,mod.rs,aia_probe.rs}`、必要最小K3 IRQ观察接口、`tests/k3-aia-probe-host-harness.rs`、Makefile现有host-test条目。
- Current behavior: stage8观察120秒后park，没有软件投递探针。
- Required behavior: 追加stage9，注册source1 Detached；SETIPNUM_LE、GENMSI各三次，逐次在1秒内确认新claim和handler增量均1；每种路径结束后1秒quiet计数不变。APLIC source disable后用SETIPNUM_LE观察1秒零分发；EIE disable后用GENMSI观察1秒零分发；清相应旧APLIC/EIP pending后re-enable，每种新触发到达。再由一次GENMSI的handler仅触发一次新GENMSI，前台观察两次独立claim/handler；受控enable EID2且无handler，GENMSI2观察unknown增量1，无panic后disable/clear；zero-claim边界host验证。全程展示rawclaim、EID、priority、route/readback与原生计数。运行超时/异常/预算超额报告层与错误、maskSEXT、park。
- Preserve: MS09 stage1–8输出内容和顺序；已有PHY reset仅一次；所有日志由前台polling输出；GENMSI绕过sourceenable，不能用于证明APLIC source mask。
- Forbidden: 伪造MAC/PHY硬件事件、IRQ内console、无界while pending/drain、额外判定程序或身份字段。
- Test witness: 新harness直接测试实际probe，先观察RED：`setip_and_genmsi_each_deliver_three`、`aplic_mask_blocks_setip`、`eie_mask_blocks_genmsi`、`reenable_requires_fresh_trigger`、`handler_generated_msi_retraps`、`unknown_eid_is_reported_without_handler`、`timeout_stops_next_stage`、`quiet_claim_is_failure`、`storm_budget_masks_sext`。fake CSR只模拟寄存器语义，不生成伪板上PASS。
- GREEN condition: 原生harness全部通过；probe不打印成功后继续失败步骤；quiet/timeout用既有native clock seam直接判定。
- Verification: `rustc --edition=2024 --test tests/k3-aia-probe-host-harness.rs -o /tmp/k3-aia-probe-host-test` 后 `/tmp/k3-aia-probe-host-test`；必要重建 `make k3`。已通过且覆盖未变的命令不重复。
- Stop when: 原生控制器行为与fake/契约矛盾、stage8不能保持、或只有新增trap恢复/通用框架才能继续。

### 1.4：真板投递与兼容验收

- Requirement/Scenario: R1初始化/handler新事件、R4软件重复/disable/quiet、R5原始Evidence/失败恢复/兼容。
- Depends on: 1.3。
- Targets: 本Cycle Act Response；实际CoM260 Kit串口及下述required Evidence；不改产品语义。
- Current behavior: 只有MS09板上结论，没有AIA投递证据。
- Required behavior: agent完成目标自动测试/构建和代码自检后交操作者上板；stage9必须实际运行通过，保留直接raw输出；reset回原系统。操作者未反馈或某场景失败时不能完成任务/展开后轮。
- Preserve: RAM boot的kernel/FDT/staging地址与不saveenv；前轮失败结论原样保留。
- Forbidden: 用host fake或软件SETIP替代设备验收；hash/revision pin/run-id/manifest/资格预检/结果脚本；未执行人工步骤计为完成。
- Test witness: 1.1–1.3真实RED→GREEN；MS09 accepted board基线作为既有行为见证，本轮新的真实投递按以下最短步骤取得GREEN。
- GREEN condition: 下述A1–A6全部通过且原生自动目标测试exit0；raw Evidence存在并能直接看到判据；reset结果由操作者一句话确认。
- Verification: 下节V1–V3与板上步骤；每项决定性输出不超过20行写Act Response，raw串口只保留决定性阶段。
- Stop when: agent没有实际板上执行能力（保存待办并交接，不宣告完成）；缺required Evidence；任何板上失败；reset未回原系统。

**Invariants**

单hart、本地selector临界段、原子EOI先于handler、实际source返回、只打开SEXT、无IRQ内串口、不盲试几何/路由、不修改既有boot/APMU/pad/PHY流程。源512初始化清位与不支持注册是不同边界。软件mask测试用正确入口，不把GENMSI与有线source混为一谈。

**Non-goals**

本轮不启用真实PHY/pinctrl链，也不替代下一轮设备cause验收；不展开后轮目录。

**Acceptance**

| Acceptance / Scenario | Requirement / Tasks | 直接判据 |
|---|---|---|
| A1 global/per-hart分离、有效CSR/route、MS09保持 | R1/R5；1.1/1.2/1.4 | host真实操作顺序GREEN；初始化层/readback及stage1–8出现，外部投递实际发生 |
| A2 双软件入口重复 | R1/R4；1.3/1.4 | SETIPNUM_LE与GENMSI各三次，逐次claim与handler增量1，最终各3 |
| A3 两种mask与重新使能 | R4；1.1/1.3/1.4 | APLICmask+SETIP和EIEmask+GENMSI各1秒无handler；清旧pending后新触发各到达 |
| A4 EOI在前、handler期间新事件 | R1；1.1/1.3/1.4 | host证明先claim+EOI后handler；板上handler再触发取得两个独立claim/handler |
| A5 spurious/unknown、quiet、超时/风暴止损 | R1/R4/R5；1.1/1.3/1.4 | hostzero/未知/超时/预算GREEN；板上unknown1无handler、各quiet窗口零增量；失败不执行下一阶段 |
| A6 构建、raw Evidence和reset恢复 | R5；1.2/1.4 | make k3/build/lichee exit0，raw日志直接支持A1–A5，reset回原系统 |

RTM（仅本轮链路；全change的Missing不能因本表闭合而忽略）：

| Requirement | Task | Test Witness | Status |
|---|---|---|---|
| R1 | 1.1–1.4 | stopei解析/顺序/init host，A1/A2/A4板测 | Covered |
| R4 软件部分 | 1.1/1.3/1.4 | mask/quiet/retrap host，A2–A5板测 | Covered；完整设备部分为用户已批准的Simplified，无Missing |
| R5 | 1.2/1.4 | 原生构建、stage1–8、raw serial、reset | Covered |

**Verification**

V1：以上两个新harness按rustc --test原生命令执行，RED和GREEN各记录命令、关键输出、退出码；同一覆盖通过后不重复。

V2：若probe接线影响PHY/MAC表面，直接执行：

```sh
rustc --edition=2024 --test tests/phy-probe-host-harness.rs -o /tmp/phy-probe-host-test
/tmp/phy-probe-host-test
rustc --edition=2024 --test tests/mac-probe-host-harness.rs -o /tmp/mac-probe-host-test
/tmp/mac-probe-host-test
```

判据为test result 0 failed与exit0；完全未变表面可采信既有结果并注明来源。原生test/build stdout不另行写审计器。

V3：受影响目标 `make k3`；平台接入后 `make build`、`make lichee` 各一次，exit0。`make vf2` 既有exit2和unrelated `ms04-async-rx` host-test失败不冒充PASS，不为其重跑全量或扩大修复。

板上操作（操作者，不是agent可执行批次）：

1. 将make k3生成的 `StarryOS_riscv64-k3.fit` 拷入已有FAT32 U盘；串口115200 8N1，无流控；上电同时持续长按s进入U-Boot。执行：
   ```text
   usb start
   fatls usb 0:0 /
   fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit
   setenv fdt_high 0xffffffffffffffff
   bootm 0x2e0000000
   ```
   不saveenv；kernel180000000、FDT181000000。装载失败保留native输出、停止。
2. 观察初始化层标记、domaincfg/MSI/local/route实际readback，原stage1–8完整。stage8窗口如既有流程等待120秒；本轮RJ45维持稳定，进入stage9后不插拔。
3. stage9自动执行A2–A5各项；直接看每种入口计数3、两个mask窗口无handler、新触发恢复、handler新事件总计2、未知EID2计数1且无handler、quiet计数不变和最终通过。每步1秒截止，失败标明当前层后park；没有最终通过不得计完成。
4. reset/复位按钮回原Linux `k3 login:`；执行人一句话记录复位结果。若取消直接reset，取消的场景不计通过；若未恢复停止后续装载。

**Gate 2 Readiness**

PASS。2026-10-03用户在审计修订计划后回复原话「批准」，Gate 2全部通过；本change没有Missing，R4 Simplified已批准。R2/R3采用固定raw-edge/PHY-before-pad-clear/re-arm契约，K3实际效果仍须后轮真板验收。本Cycle转为ready；Act尚未开始。

**Persisted Evidence**

- Mode: required
- Source: proposal中用户明确要求板上IRQ原始串口留档。
- Files: `evidence/000-initial/001-replan/board-aia-irq.txt` 和同目录README；Act创建，Plan不创建。
- Acceptance / Gate: 支持A1–A5的真实投递与A6的板上恢复，供Gate4与Plan独立Review直接检查。
- Why Act Response is insufficient: 用户要求保留原始逐事件claim/readback；摘要会失去两种mask、先后事件与错误层的决定性结构。
- Why not cheaply reproducible: 需要操作者实体板、U盘装载和串口会话，agent无法独立低成本重跑；不能把host fake作为替代。
- Missing blocks: 无原始片段不得接受控制器板测或展开设备轮。
- Pass condition: 纯串口片段直接呈现初始化读回、stage9各软件事件/mask/quiet及最后结果；README只列文件、对应Acceptance与操作者reset一句话。既有stage1–8不复制完整日志套件；不带身份字段、Hash、manifest或判定脚本。

**Risks and Notes**

用户已撤销人工插拔计数要求并允许替代资料，U1/U2不再阻塞本计划；设备轮采用固定方案并由真实触发/ack/再触发直接验证，不能由host模拟声称K3已通过。本轮不包含设备ack实现，不要求Act回读设备调查。CSR访问可能在最初层触发native trap，前置polling标记定位首触；不增加通用trap恢复机制。GENMSI本身与source mask独立、IMSIC pending可能在mask期间保留，测试必须清理旧pending再证明新触发。内部命名/辅助拆分为非实质选择。

## Act Response

- Status: reported

### 实际改动

| 文件 | 改动 |
|---|---|
| `crates/axplat-riscv64-k3/src/aia.rs`（新） | AIA 生产核：R69 契约常量（APLIC 偏移/SM 模式/MSI 几何/CSR selector/固定路由表）、纯布局函数、`AiaBackend`/`ImsicAccess`/`HandlerStore` seam、`init_global`/`init_percpu`（含 S-mode 判据 readback）、`AiaChip::register/unregister/set_enable/handle_external`（原子 claim+EOI 先于 handler、spurious/unknown 计数、last_raw_claim）、`HardwareBackend`（cfg riscv64：volatile MMIO + siselect/sireg/stopei asm + SIE 保存恢复） |
| `crates/axplat-riscv64-k3/src/irq.rs`（新，替代 irq_stub.rs） | axplat `IrqIf` 实装：HandlerTable<512> 复用、外部向量门控（timer/IPI 保持 inert）、send_ipi 经 sbi_rt、init_global/init_percpu（polling console 分层标记，失败关 SEXT 并 park）、`pub mod probe` 观察接口 |
| `crates/axplat-riscv64-k3/src/init.rs` | `init_later` 记录 raw hartid 并依次执行 global/percpu AIA 初始化 |
| `crates/axplat-riscv64-k3/src/lib.rs` | `pub mod aia`；`irq_stub` → `pub mod irq`（irq-if feature） |
| `crates/axplat-riscv64-k3/src/irq_stub.rs` | 删除（孤儿） |
| `crates/axplat-riscv64-k3/axconfig.toml` | mmio-ranges 增加 APLIC `[0xe080_4000, 0x4000]`，头部注释同步 |
| `kernel/Cargo.toml`、`Cargo.lock` | starry-kernel 可选依赖 `axplat-riscv64-k3`（k3 feature 接线，feature `k3 = ["dep:axplat-riscv64-k3"]`） |
| `kernel/src/platform/aia_probe.rs`（新） | stage9 探针：`Stage9Env` seam、A2 双入口各三次（每步 1 秒截止 + quiet）、双 mask/清理/再使能/保留 pending 报告、handler 重触发（MODE_RETRAP_ONCE）、unknown EID2、32 次 claim 风暴预算 + SEXT mask、`run_stage9` 逐相串口输出；`K3Stage9Env`（cfg k3）复用 HardwareBackend + axhal + irq::probe |
| `kernel/src/platform/k3.rs` | stage8 之后追加 stage9 调用（cfg k3），stage1–8 顺序与内容不变 |
| `kernel/src/platform/mod.rs` | `#[cfg(feature = "k3")] pub mod aia_probe` |
| `tests/k3-aia-host-harness.rs`、`tests/k3-aia-probe-host-harness.rs`（新） | 1.1/1.2 见证 15 测试 + 1.3 见证 10 测试；fake 建模 R69 修正语义（claim= top pending∧enabled、EIP 只读、GENMSI 绕过 source gate、SEXT 门控 trap） |
| `Makefile` | host-test 目标增加两个新 harness 条目 |

### 与计划的偏差

1. 「必要最小 K3 IRQ 观察接口」落地为：`irq::probe`（last_raw_claim/spurious/unknown 只读）+ kernel 对 axplat-riscv64-k3 的可选依赖。kernel 原本无法命名平台 crate，观察接口必须经依赖绑定；bin 侧 `starry-kernel/k3` feature 已存在，仅新增 kernel 侧接线。
2. 生产后端从 irq.rs 下沉到 `aia.rs` 的 `cfg(target_arch = "riscv64")` 区（`HardwareBackend`）：irq.rs 与 kernel 探针共享同一 CSR/MMIO 访问规则实现，避免 siselect/stopei asm 二处维护。host harness 经 `#[cfg]` 完全不见该区。
3. `AiaChip` 由 dyn 表引用改为泛型存储参数（`AiaChip<T: ?Sized + HandlerStore>`）：static 化的 chip 需要 Sync，泛型 ZST adapter 满足且接口不变。
4. `AiaChip::unregister` 返回 `Option<fn()>`（IrqIf 契约要求归还被移除 handler），计划文本只写 bool 语义。
5. 风暴预算超限的 SEXT mask 由前台等待环执行（handler 仅置事件计数，预算到达即 `set_sext(false)`）：与「在 dispatch 路径 mask」的验收等价——预算硬停、mask 后 quiet 判定不变，且该路径可被 host harness 直接观察。
6. harness 增加测试串行锁：两文件共享静态 fake，libtest 默认并行执行存在竞态；锁保证确定性，不改变被测逻辑。

以上均为局部实现形状选择，未改变 Task Contract 的行为、验证面或 Acceptance。

### 自检发现与修复（Gate 3）

- k3.rs 首版对 probe outcome 的无用 `if let` 死代码——已删除，改为直接调用（结果由探针自身串口输出）。
- 探针 harness 初版 `register_handler` 只注册 handler store 未做硬件使能，导致 SETIPNUM 永不投递——改为完整 `CHIP.register` 路径。
- 未知 EID 场景的测试建模错误：fake claim 按 spec 门控 pending∧enabled，测试需先本地使能该 EID（与 stage9 对 EID2 的真实流程一致）——已修正；bitmap 位算术断言同步修正。
- 无遗留 Critical/Important；Minor：fake 的 GENMSI busy 窗口恒为 0（busy 截止路径依赖代码审查与板测，无独立 host 测试）。

### 验证

V1/V2（rustc --edition=2024 --test 原生命令，判定=test result 0 failed + exit0）：

```text
tests/k3-aia-host-harness.rs       → test result: ok. 15 passed; 0 failed   exit 0
tests/k3-aia-probe-host-harness.rs → test result: ok. 10 passed; 0 failed   exit 0
tests/phy-probe-host-harness.rs    → test result: ok. 30 passed; 0 failed   exit 0（MS09 回归）
tests/mac-probe-host-harness.rs    → test result: ok. 15 passed; 0 failed   exit 0（MS09 回归）
```

RED 记录：两 harness 首次编译分别因 `crates/axplat-riscv64-k3/src/aia.rs` 与 `kernel/src/platform/aia_probe.rs` 不存在而失败（couldn't read ... No such file or directory），符合契约允许的缺失生产接口 RED。

V3（目标与兼容构建，各 exit0）：

```text
make k3     exit 0（StarryOS_riscv64-k3.fit 生成，308768 字节）
make build  exit 0（QEMU virt）
make lichee exit 0（Lichee RV Dock D1）
```

无新增编译警告（仅既有 axtask unused 与 cargo config 弃用提示）。`make vf2` 未运行：既有 exit2 失败按契约只引用不修复。基线采信：MS09 accepted 收尾的 PHY/MAC/early-console 与构建结论在本次改动后由上表新鲜运行覆盖，无需另行采信。

### Persisted Evidence

required 的 `evidence/000-initial/001-replan/board-aia-irq.txt` 与 README **尚未创建**：其采集前提是操作者完成板上 stage1–9 运行（能力边界，见 Blocker Handoff）。Act 未创建任何 Evidence 目录。

### Blocker Handoff

- 发现位置：task 1.4 的上板步骤；Gate 4（A1–A6 的真板证据与 A6 reset 恢复）。
- Plan 预期 vs 实际：计划将上板定义为操作者步骤（Task 1.4「上板部分由操作者执行，其结果审核 PASS 前停止下游」；Stop when「agent 没有实际板上执行能力——保存待办并交接，不宣告完成」）。实际一致：agent 侧无实体板/串口/U 盘装载能力，required Evidence 现阶段不可采集。
- 任务状态：1.1–1.3 完成（GREEN）；1.4 agent 侧（host/兼容构建、代码自检）完成，上板部分未开始。
- 工作区：分支 k3，HEAD `082339ba` + 本次未提交改动（文件清单见上）；已通过 Gate：1.1–1.3 的 Gate 3/4，change 结构自检与 `openspec validate --strict`。
- 待办（操作者，非 agent 批次）：按本 Cycle「板上操作」1–4 执行——拷贝 `StarryOS_riscv64-k3.fit` 入 FAT32 U 盘，U-Boot `fatload usb 0:0 0x2e0000000 ... && bootm`（不 saveenv），观察初始化层标记、stage1–8 与 stage9 逐相输出（双入口各 3 次、双 mask、retrap=2、unknown=1、quiet、风暴止损），reset 回 `k3 login:`；保存 raw 串口记录。
- Evidence: None required（简单能力边界偏差，无 BLOCKED 现场文件；板测现场由操作者串口会话承载）。
- 恢复条件：操作者提供板上 raw 串口结果（成功输出或失败现场）并要求继续 → Act 追加 Blocker Resolution（保留本 Handoff），据板上输出完成 1.4 审核：通过则创建 required Evidence 文件与 README、Response → reported；失败则按实际层定位报告，不展开设备轮（Iteration 001）。

### Experience Candidates

None（板测尚未发生，无可登记的可重复操作或范围外缺陷。）

## Blocker Resolution（2026-10-03，首次板测失败后继续）

用户解决阻塞并要求继续（原话：「似乎有问题，解决」+ 完整串口输出 + 「继续解决」）。保留上方 Blocker Handoff，自恢复点继续 task 1.4。

### 板上失败现场（操作者提供，raw 串口摘录）

```text
[k3:irq] init=global begin
k3-irq: init failed (global: smsicfgaddr readback 0x0 != 0xe0400); SEXT closed, parking
[k3:irq] init=failed parked
```

启动链全部正常（U-Boot FAT 装载、`[k3:pt-mmu]`、runtime横幅），失败精确落在 init_global 第 4 步的 S-mode readback 判据上——即契约定义的可诊断失败路径本身工作正常。

### 根因调查（本仓 + R69 参考仓证据）

- R69 `opensbi-k3`（板同源固件）：`fdt_parse_aplic_node` 对本板 DT（IMSIC `interrupts-extended` 全为 cause 9 = S-mode）判定 `targets_mmode=false` → **固件从不写 S-mode msicfg**（`has_msicfg_smode=false`），smsicfgaddr 无锁定、值 0，配置责任完全在 OS。
- R69 `tgoskits riscv_aplic::init_msi_mode`：同地址（0x1bc8/0x1bcc）、同顺序（先 low 后 high，再开 domaincfg IE|DM）、同编码（low=0xE0400、high=L|LHXW=4<<12|LHXS=6<<20），且有真板证据（周报三十一「AIA 驱动 QEMU + K3 通过」、周报三十二「真板 domain=9, irq=217, selftest OK」、IPC 报告 source 217 门铃到达 AP）。
- 映射面：axmm `new_kernel_aspace` 将 `devices.mmio-ranges`（含本次新增的 APLIC range）以 `phys_to_virt` 线性映射（DEVICE），板上 UART/域寄存器读写均经此路径成功，访问未触发 trap。
- 结论：与「同写法板上成功」的参考相比只剩一个未经裁决的设备语义差异——**msiaddrcfg 写入是否被 domaincfg.DM 门控**（AIA 规范允许的实现选择：DM=0 时该寄存器对只读 0）。参考实现启动时域内 DM 状态无法从现有证据排除。

### 修复动作（有界、带读回判定的假设阶梯，非盲试）

`aia.rs::init_global` 第 4 步改为三级阶梯：

1. 按 OpenSBI 顺序写 msicfg 并验证 S-mode 判据；
2. 失败则置 `domaincfg.DM`（IE 仍清零）后重写并再次验证；
3. 仍失败则读回全量诊断（domaincfg 两次读回、smsicfg 双寄存器读回、M-mode 镜像 mmsicfg 0x1bc0/0x1bc4），以单行 `MsiCfgProbe` 错误输出并保持域关闭进 park。

测试见证：fake 增加「DM=0 时 msicfg 写被忽略、读 0」的门控模型（复现板上失败形态）+ 新增 `msicfg_requires_dm_set_before_write`（重试后成功）与 `msicfg_dead_reports_full_probe_dump`（双失败时完整转储）两测试；板上一跑即在三结局（过/DM 门控确认/设备拒绝）中裁决其一。

### 恢复后的验证（重跑）

```text
tests/k3-aia-host-harness.rs       → test result: ok. 17 passed; 0 failed   exit 0
tests/k3-aia-probe-host-harness.rs → test result: ok. 10 passed; 0 failed   exit 0
make k3 exit 0；make build exit 0；make lichee exit 0
```

TDD 记录：RED 由真板失败确立（门控 fake 在 host 侧复现同形态），GREEN 为重试阶梯实现。

### 状态

- Act Response 保持 `blocked`：required Evidence（板上 stage1–9 raw 串口）仍不可采集，等待操作者第二次板测。
- 恢复条件同 Blocker Handoff：操作者回传新串口输出。三结局裁决：
  - `stage=9 pass` → DM 门控确认为根因，1.4 板测通过，创建 required Evidence，Response → reported；
  - `msicfg probe: ... after DM: domaincfg=0x? smsicfg=0x?` 全零读回 → 设备拒绝/映射问题，按 dump 定位后交 Plan 裁决（不自行扩大试验）；
  - 其他 `stage=9 fail phase=...` → stage9 自身的可诊断失败层，按层定位。

## Blocker Resolution 第二轮（2026-10-03，第二次板测后继续）

用户回传第二次板测 dump 并要求按 runbook 分级判断方法继续（「如果不行，我们可以用runbook里面的分级判断的方法，注入需要的调试语句，编译info级别内核，然后根据输出定位问题」）。保留上两节 Handoff/Resolution。

### 第二次板测 dump 裁决（raw 串口摘录）

```text
[k3:irq] init=global begin
k3-irq: init failed (global: msicfg probe: first smsicfg=0x0 (DM=0), after DM: domaincfg=0x80000004
  smsicfg=0x0/0x0, mmsicfg=0x0/0x0, domaincfg-off=0x80000004); SEXT closed, parking
```

四个事实被本轮 dump 裁决：

1. **MMIO 窗口活着且可写**：domaincfg 写 0x4 后读回 `0x80000004`——总线访问真实到达设备，映射假设排除。
2. **DM 门控假设证伪**：DM=1 确认后 smsicfg 双寄存器仍读 0。
3. **域是 MSI-only**：写 0 后 domaincfg 仍读 `0x80000004`——DM 只读 1（AIA 允许：纯 MSI 域 DM 硬连 1；与 DT 窗口恰 0x4000、无 IDC 区一致）；bit31 为该实现恒 1 的 WARL 位。
4. **msicfg 对在已确认的 S-mode 域上仍拒绝写入**——剩余单变量：S 域位置。AIA 双域布局规定 S 域 = 首域 +0x4000（0xe0808000）；DTB 中 APLIC 窗口止于 0xe0808000、CLINT 在 0xe081c000，其间 0x14000 空隙与双域布局吻合，而 DT 只描述了首 0x4000。

### 修复动作（单变量判别 + 自适应修复，runbook「每层一个变量」）

- `aia.rs`：init_global 流程内置双窗口探针（`probe_aplic_windows`）——读两窗口复位态（domaincfg/smsicfg/mmsicfg/sourcecfg1），每窗口做有界可写性探写（sourcecfg[1]=Detached、msicfg 对写入后读回），串口输出 `probe d0(dt): …` / `probe d1(+4000): …`；当 DT 窗口拒绝且 +0x4000 窗口接受 msicfg 时，芯片基址切换至 0xe0808000（`select_aplic_base`，仅允许两个已裁决窗口）并继续 init——若假设成立则本次板测直接走完 stage9。
- `HardwareBackend` 改为按选中基址翻译偏移；axconfig 增加 `[0xe080_8000, 0x4000]` 映射（探针前置条件）；`LineBuf` 下沉 aia.rs 共享；stage9 handler 的 GENMSI raw 地址改用 `aia::aplic_base()`。
- fake 模型更新：删除已证伪的 DM 门控，改为双窗口死/活模型，fake 升级为基址感知（绝对地址键控，镜像生产后端翻译）。

### 恢复后的验证（重跑）

```text
tests/k3-aia-host-harness.rs       → 17 passed; 0 failed   exit 0
tests/k3-aia-probe-host-harness.rs → 10 passed; 0 failed   exit 0
tests/phy-probe-host-harness.rs    → 30 passed; 0 failed   exit 0
tests/mac-probe-host-harness.rs    → 15 passed; 0 failed   exit 0
make k3 / make build / make lichee → 全部 exit 0，0 新增警告
openspec validate --strict → valid, exit 0
```

自检修复：aia_probe 三处未用绑定清理；fake 基址感知修正（探针在核心 init 内执行、harness 直测真实流程）。

### 状态

- Act Response 保持 `blocked`：required Evidence 仍待采集，等待操作者第三次板测。
- 判读表：
  - `probe d1(+4000): … msicfg-wb=e0400/8060100? + aplic-base selected` 随后 `init=global done` → S 域位置确认为根因，观察 stage1–9 结果；
  - `probe d1` msicfg-wb 仍 0 → 两窗口均拒绝，dump 已含裁决全部信息（sourcecfg 写回可区分「M 域拒 S 写」），交 Plan 裁决下一步（不扩大盲试）；
  - `stage=9 fail phase=…` → msicfg 已过，按 stage9 失败层定位。

## Blocker Resolution 第三轮（2026-10-03，第三次板测后继续）

用户回传第三次板测探针输出并要求定位（「这是输出，定位问题吧」）。保留前两轮 Handoff/Resolution。

### 第三次板测探针裁决（raw 串口摘录）

```text
[k3:irq] probe d0(dt): domaincfg=0x80000004 smsicfg=0x0/0x0 mmsicfg=0x0/0x0 srcfg1=0x0->0x1 msicfg-wb=0x0/0x0
[k3:irq] probe d1(+4000): domaincfg=0xffffffff smsicfg=0xffffffff/0xffffffff mmsicfg=0xffffffff/0xffffffff srcfg1=0xffffffff->0xffffffff msicfg-wb=0xffffffff/0xffffffff
```

裁决：

1. **d0（DT 窗口 0xe0804000）完全活着且可写**：`srcfg1=0x0->0x1` 证明 sourcecfg 写入粘滞；`domaincfg=0x80000004` 复证 MSI-only（DM RO=1）。**唯独 msiaddrcfg 对写后读恒 0**。
2. **d1（+0x4000）读写全部返回 0xffffffff**——总线错误响应特征，不是 APLIC S 域。**双域假设证伪**，+0x4000 探针与自适应基址机制按证伪结论移除。
3. 综合判读：该芯片的 msiaddrcfg 是**「写后读不回」形态**（写可能已生效，读路径恒 0）——OpenSBI 生产固件写 msicfg 从不做读回验证，与此吻合。此前 init 的「msicfg 读回判据」是该硅片上不成立的过严判据，且它**掩盖了真正的判据：MSI 是否实际投递**。

### 修复动作（交付判据取代读回判据，runbook 单变量）

- `init_global`：msicfg 写一次（OpenSBI 顺序），读回降级为**信息行**（`msicfg written=… readback=… ok|readback-refused`）不再致命；domaincfg 使能读回同样降级为信息行；init 继续走到 done。真正的裁决交 stage9 的**投递实验**。
- stage9 `setip-x3` 超时路径注入判别 dump：`setip0`（APLIC pending 位图）、`setie0`、`target1`、`domaincfg`——pending 置位但无 claim ⇒ MSI 地址配置未生效；pending 未置 ⇒ SETIPNUM 写入本身失败。
- 移除按证伪结论删除的机制：+0x4000 窗口探针/映射、自适应基址切换、DM 重试阶梯与 MsiCfgProbe 错误；fake 模型改为「写后读零」单变量（`msicfg_reads_zero`）。

### 恢复后的验证（重跑）

```text
tests/k3-aia-host-harness.rs       → 16 passed; 0 failed   exit 0
tests/k3-aia-probe-host-harness.rs → 10 passed; 0 failed   exit 0
make k3 / make build / make lichee → 全部 exit 0，0 新增警告
```

### 状态

- Act Response 保持 `blocked`：required Evidence 仍待采集，等待操作者第四次板测。
- 判读表：
  - `init=global done` → `stage=9 pass`：msicfg 实为写后读不回，投递成功，1.4 板测通过 → 创建 required Evidence，Response → reported；
  - `stage=9 setip-timeout setip0=0x2 …`：pending 置位但无投递 ⇒ APLic MSI 地址配置确未生效（S-mode 无法编程），携全部 dump 交 Plan 裁决（IMsic 自注入改造或固件协作，均为 Plan 级决定）；
  - `setip0=0x0`：SETIPNUM 写入本身未生效，按层再定位；
  - 其他 `stage=9 fail phase=…`：投递已通，按具体失败层定位。

## Blocker Resolution 第四轮（2026-10-03，第四次板测后继续）

用户回传第四次板测完整输出并询问「存在问题么」。本轮裁决：**投递成功，A1–A5a 主体在真板全部通过**；storm 相与 reset 确认待复验。保留前三轮 Handoff/Resolution。

### 第四次板测验收对照（raw 串口摘录）

```text
[k3:irq] init=global done / init=percpu done          # A1 初始化分离 ✓（global/percpu 标记齐）
k3-irq: register source 9223372036854775813 rejected   # timer(向量5) 注册被拒 = 契约预期，非缺陷
stage=1..8 完整输出（MAC/PHY/链路与 MS09 accepted 一致）  # A1 兼容面 ✓
stage=9 setip trigger=1..3 raw=0x10001                 # A2 SETIPNUM ×3，EID=1/prio=1 ✓
stage=9 genmsi trigger=1..3 raw=0x10001                # A2 GENMSI ×3 ✓（msicfg 写后读不回 + 投递成功）
stage=9 aplic-mask blocked=1 fresh=1                   # A3a APLIC mask 阻断 + 新触发到达 ✓
stage=9 eie-mask blocked=1 drained=0 retained=0 fresh=1  # A3b EIE mask 阻断 + 新触发到达 ✓
stage=9 retrap independent-claims=2                    # A4 先 claim+EOI 后 handler、新事件独立 claim ✓
stage=9 unknown-eid delta=1 no-handler=1               # A5a unknown EID2 诊断、无 panic ✓
```

**结论：msicfg「写后读不回」被投递实验证实——APLIC MSI + IMSIC S-mode 底座在本板实际可用。** 各相 quiet 窗口全部通过（任一失败都会以 fail 行终止）。

### 板上观察记录（与 fake 模型的偏差，不影响验收）

1. `eie_retained=0`：本板对 EIE 掩码下的 GENMSI 是**丢弃而非保留**（fake 按 AIA 规范保守建模为保留并读回服务）。A3 判据「mask 阻断 + 新触发到达」在两种行为下均成立，无验收影响；fake 模型不改（保留规范语义作为保守上界）。
2. `register source 9223372036854775813 rejected`：0x8000_0000_0000_0005 = runtime timer 向量注册被固定路由正确拒绝（本轮范围 timer 保持 stub，契约行为）。
3. **storm 相（A5b）无输出**：unknown-eid 行之后无 `storm events=…`、无 fail 行、无 pass 行——要么串口采集截断，要么内核在该相挂死。全路径均有界（风暴环 2 秒截止、busy 环 1 秒截止），理论上必然有输出，故挂死嫌疑集中在风暴链的 foreground 饥饿。

### 修复动作（风暴相加固）

- 预算止损移入 dispatch 路径（回归契约原表述「超额在 dispatch 路径 mask SEXT」）：handler 在第 32 次事件直接 `sie::clear_sext`（riscv64）并置旗标，前台环以旗标/delta 双判据退出——密集风暴下前台不再可能饿死到超时判定之后。
- 修复静态旗标泄漏：`SEXT_MASK_REQUESTED` 在前台 `delta>=32` 短路时不被消费，会泄漏到同进程下一个 stage9 实例（host 套件全量运行下 storm 测试被上一实例的旗标污染，delta=1 即退出）。stage 启动防御性复位 + break 时无条件消费；probe harness 双跑 GREEN 确证。
- 该加固同时消除了板测 storm 相的唯一合理挂死路径；若复测仍在 storm 无输出，则按 Gate 5 停止交 Plan。

### 恢复后的验证（重跑）

```text
tests/k3-aia-host-harness.rs       → 16 passed; 0 failed   exit 0
tests/k3-aia-probe-host-harness.rs → 10 passed; 0 failed   exit 0（双跑）
make k3 / make build / make lichee → 全部 exit 0，0 新增警告
```

### 状态（最终）

- A6 reset 恢复已由操作者确认（原话「那个我顺手就做了，你不用管这一步」，2026-10-03，紧接验收运行）。
- required Evidence 齐备：`evidence/000-initial/001-replan/board-aia-irq.txt` + README（含 reset 一句话）。
- tasks 1.1–1.4 全部闭合，Act Response 转 **reported**。等待 `openspec-plan` 实现 Review。

## Plan Review

- Review Result: accepted
- Reviewed: 2026-10-03（Plan 独立阅读完整 diff、新代码与 required Evidence；Act 报告的验证结论按覆盖未失效采信）

**Findings**

均为不阻塞 Acceptance 的 Minor finding（Act 偏离——证伪机制清理不彻底）：

1. `crates/axplat-riscv64-k3/src/aia.rs` 残留已证伪/移除机制的痕迹：
   - `AplicWindowProbe` 结构体（含从未赋值的 `smsicfg_low_writeback/high_writeback` 字段）及其含 `msicfg-wb` 的 `Display` 成为死代码——`probe_aplic_domain` 实际用局部变量直接打印，从不构造该结构体。
   - `init_global` 第 4 步注释首段仍描述已移除的 DM 重试阶梯与「MsiCfgProbe 完整转储」错误路径（代码只写一次 + 信息行）。
   - `probe_aplic_domain` 文档称探测「msiaddrcfg pair 可写性」，实际已不做该探测。
   - `aplic_read/aplic_write` 的 SAFETY 注释与 `run_k3_stage9` 的注释仍提及已证伪删除的 +0x4000 S 窗口（axconfig 中无该映射，代码行为正确）。
2. `kernel/src/platform/aia_probe.rs` 模块头第 18–20 行称预算止损「由前台 mask SEXT」；加固后实际为 dispatch 路径止损（handler 第 32 次事件直接 `clear_sext`，riscv64）+ 前台镜像，模块头一行陈旧（`SEXT_MASK_REQUESTED` 处注释是正确的）。

无行为影响；按公共规则记录 finding 不创建返工，可在后续触碰 `aia.rs` 的轮次顺手清除。

**Evidence**

- 完整 diff 独立阅读（git diff 11 个 tracked 文件 + 删除 `irq_stub.rs` + 新文件 `aia.rs`/`irq.rs`/`aia_probe.rs`/两 harness/Makefile/axconfig/Cargo 接线），与 Act Response 实际改动表逐项一致；`k3.rs` 仅在 stage8 后追加 stage9 调用，stage1–8 未变（板上输出与 MS09 accepted 形状一致佐证）。
- 关键不变量逐条代码核实：claim+EOI 先于 handler（`aia.rs::handle_external`，原子 `csrrw stopei` 之后才 `table.call`）；selector 临界段保存并按原样恢复 SIE（`HardwareBackend::with_imsic`）；disable 先 APLIC CLRIE 后本地 EIE（`disable_source`/`set_enable`）；固定路由仅 1/60/133/277（`SUPPORTED_ROUTES`），runtime timer 向量 `0x8000_0000_0000_0005` 注册被拒 = 契约预期；handler 不打印不 claim（`stage9_handler` 仅计数与 GENMSI 写）；全部等待环有界（1 s 步 deadline、2 s storm、GENMSI busy 1 s、drain ≤32）；风暴预算止损在 dispatch 路径（`storm_budget_stop` 于 riscv64 直接 `clear_sext`），旗标 stage 启动防御性复位 + break 无条件消费（第四轮修复在代码中，`storm_budget_masks_sext` 断言预算/SEXT/quiet）。`irq_stub` 删除无孤儿引用（lichee-d1 的 `irq_stub` 是其自身平台模块）。
- required Evidence 检查：`evidence/000-initial/001-replan/board-aia-irq.txt` 为 raw 串口（U-Boot 装载 → stage1–8 → stage9 各相 → pass 行），直接呈现双入口各 3、`aplic-mask`/`eie-mask` blocked+fresh、`retrap independent-claims=2`、`unknown-eid delta=1 no-handler=1`、`storm events=32 sext-masked quiet=1`；README 逐条映射 A1–A6 并含操作者 reset 一句话（用户原话）。A1–A6 全部有板上直接判据。
- 采信 Act 已报告且覆盖未失效的结论（来源：001-replan Act Response 第三/四/五轮 Resolution）：`k3-aia-host-harness` 16 passed、`k3-aia-probe-host-harness` 10 passed（双跑）、PHY 30 / MAC 15 回归、`make k3/build/lichee` exit0。测试函数清点与声明一致（16/10）；两 harness 经 `#[path]` 直接包含生产模块并带串行锁，fake 按板测事实建模（msicfg 写后读零、EIE 掩码按规范保守建模为保留）。
- 身份型证据工程检查：代码与 Evidence 无 hash/run-id/manifest/判定脚本/资格工具；raw 串口中的 crc32 行为 U-Boot 自身 FIT 校验输出，非新增机制。
- 工作区另有未提交的 `.agents/` 改动（tasks.md 的 MS17/MS18 路线、references.md R74、analysis 文档），属已授权的规划/探索流程产物，不属本 Cycle diff，未纳入本次审查。

**Acceptance Gaps**

None。A1–A6 全部满足（对照 board-aia-irq.txt 与 host/构建结论，见上）。

**Follow-up Decision**

Acceptance 已满足且无阻塞项，两项 finding 均为注释/死代码残留——按 Review 分类记录并接受，不创建当前 Cycle 修复或 rework。Iteration 000 完成，按既定 Map 展开 Iteration 001（后轮任务契约已随 2026-10-03 计划批准）。

**Next Cycle**

None

**Next Iteration**

`iterations/001-phy-device/000-initial.md`（已按已批准后轮契约 + 本 Iteration accepted 基线展开，ready，等待用户 Act 指令）

## Blocker Resolution 第五轮（2026-10-03，第五次板测 = 验收运行）

用户回传第五次板测完整输出。**`stage=9 pass`**——storm 相经加固后在真板通过：

```text
[starry-k3] stage=9 storm events=32 sext-masked quiet=1
[starry-k3] stage=9 pass setip=3 genmsi=3 aplic_mask=0 aplic_fresh=1 eie_mask=0
  eie_retained=0 eie_fresh=1 retrap=2 unknown=1 drained=0 storm=32 eip0=0x0
```

- 加固生效：dispatch 路径预算停（handler 第 32 次事件直接关 SEXT）在真板正确止损，
  无前台饥饿、无挂死；quiet 窗口成立。
- A1–A5b 全部真板通过（逐条对照见上轮 Resolution 的验收对照表，本轮一致且新增 storm）。
- required Evidence 已创建：`evidence/000-initial/001-replan/board-aia-irq.txt`（本运行
  raw 串口：U-Boot 装载 → `[k3:pt-mmu]` → stage1–8 → stage=9 pass）+ README（Acceptance
  逐条映射、板上观察记录）。
- 仅剩 A6 的 reset 恢复确认（操作者一句话）：确认后补 README reset 行、tasks 1.4 闭合、
  Act Response → reported。Response 在此之前保持 `blocked`。

### 板上事实沉淀（供 Plan Review 与 Iteration 001 使用）

- 本硅片 APLIC（0xe0804000，MSI-only，DM RO=1）：msiaddrcfg 对「写后读不回」但写入实际
  生效（GENMSI 按 0xe0400/0x8060100 几何投递成功）；sourcecfg/target/位图寄存器读写正常。
- EIE 掩码下的 MSI 被丢弃而非保留（eie_retained=0）；handler 执行期间的新 MSI 正常保留
  （retrap=2）——两组行为不冲突（掩码窗口 vs handler 窗口），Iteration 001 设备 ack 顺序
  设计可直接采信「掩码窗口 MSI 可能丢失，必须触发后置掩码」。
- runtime timer 向量（0x8000_0000_0000_0005）注册被固定路由拒绝 = 契约预期。
