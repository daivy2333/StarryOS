# Iteration 000 / Cycle 000: AIA 软件投递底座

## Plan Context

- Status: draft
- Cycle Type: initial

**Cycle Scope**

- Change tasks: 1.1–1.4。
- Acceptance gaps: None；Repair items: None。
- Inherited scope: R1 控制器、R4 软件投递/disable/quiet、R5 原生验证/兼容/板上日志；R2 的固定路由准备，不声称设备投递通过。
- Excluded scope: PHY/pinctrl 中断使能和 ack、设备事件验收、DMA/网络、timer/UART 调度、多 hart 流量、固件或持久介质修改。

**Objective**

将 K3 IrqIf stub 实化为 APLIC MSI + IMSIC 外部中断底座；在保留 stage1–8 的 RAM 启动路径上追加 stage9，分别证明 SETIPNUM_LE 与 GENMSI 投递、先原子 EOI 后 handler、重复/disable/re-enable/quiet 和有界失败。

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
| R4 软件部分 | 1.1/1.3/1.4 | mask/quiet/retrap host，A2–A5板测 | Covered；完整设备部分在Map中仍Missing |
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

BLOCKED：全change R2/R3缺pinctrl status/ack实际契约（U1），R4快速插拔精确次数与pending合并的用户决策未闭合（U2），本计划尚未获批；本Cycle保持draft，无Act批次。

**Persisted Evidence**

- Mode: required
- Source: proposal中用户明确要求板上IRQ原始串口留档。
- Files: `evidence/000-initial/000-initial/board-aia-irq.txt` 和同目录README；Act创建，Plan不创建。
- Acceptance / Gate: 支持A1–A5的真实投递与A6的板上恢复，供Gate4与Plan独立Review直接检查。
- Why Act Response is insufficient: 用户要求保留原始逐事件claim/readback；摘要会失去两种mask、先后事件与错误层的决定性结构。
- Why not cheaply reproducible: 需要操作者实体板、U盘装载和串口会话，agent无法独立低成本重跑；不能把host fake作为替代。
- Missing blocks: 无原始片段不得接受控制器板测或展开设备轮。
- Pass condition: 纯串口片段直接呈现初始化读回、stage9各软件事件/mask/quiet及最后结果；README只列文件、对应Acceptance与操作者reset一句话。既有stage1–8不复制完整日志套件；不带身份字段、Hash、manifest或判定脚本。

**Risks and Notes**

U1/U2是阻塞项，不交由Act设计。CSR访问可能在最初层触发native trap，前置polling标记定位首触；不增加通用trap恢复机制。GENMSI本身与source mask独立、IMSIC pending可能在mask期间保留，测试必须清理旧pending再证明新触发。内部命名/辅助拆分为非实质选择。

## Act Response

- Status: pending

尚未授权实施或验证新代码。Experience Candidates: None。

## Plan Review

- Review Result: replan-required

**Findings**

本次仅为规划修订，不是实现Review，没有Act结果。2026-10-03用户裁决撤销人工插拔计数门槛、允许替代资料；属于需求与验证契约变化。后轮设备链已按DTB/R69和相关字段/清边沿实现闭合，不再要求取得特定vendor handler。

**Evidence**

用户原话和修订边界见proposal「计划修订裁决」；design D5记录依据、固定操作、推导边界和实际验证；tasks的RTM无Missing，R4 Simplified有用户批准。

**Follow-up Decision**

原Plan Context保留，未交给Act。通过同Iteration的replan Cycle承载新的完整性结论，控制器任务1.1–1.4和Iteration Map不变；不按普通返工补测试或伪造板上结果。

**Iteration Plan Update**

后轮人工验证由固定次数/动作数相等改为真实link变化、source60/INSR对齐、ack后再触发和quiet；设备契约采用替代资料，具体操作已定。首轮范围和两轮诊断边界不变。

**Next Cycle**

`001-replan.md`

**Next Iteration**

None。
