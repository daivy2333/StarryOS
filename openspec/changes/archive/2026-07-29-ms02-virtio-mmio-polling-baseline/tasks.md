## 1. 建立测试见证

- [x] 1.1 在 `crates/axnet/src/service.rs` 增加 deadline
  策略单元测试，并新增 `tests/ms02_guest_service.c` 与对应 Makefile
  target；映射“无 IRQ 的同步网络进度”“明确的 guest 网络服务”和
  “失败与 timeout 可诊断”。当前 `Service` 负责 smoltcp timer，
  测试先约束无协议 timer、仅协议 timer、仅 polling fallback、
  两种 timer 取较早值。payload 必须用单一 `poll()` 等待 TCP/UDP
  5555，输出固定 READY、PASS、FAIL 标记。RED：只加入测试后，
  axnet 测试因缺少 polling deadline 策略而失败；当前依赖树查询
  `auto-icmp-echo-reply` 退出非零。GREEN：T2/T3 后策略测试、
  payload 编译和 feature 查询通过。不得驱动 QEMU 或 guest shell。
  若 axnet host test 无法进入测试主体，停止并写 Blocker Handoff。

- [x] 1.2 为 device mask 与 polling eligibility 增加策略测试。
  当前四个 deadline tests 只覆盖 deadline 的 `min` 选择，未覆盖
  mask 外 polling device、mask 内 IRQ device 和 mask 内无 IRQ
  device。先保留 4/4 GREEN，再提取不改变行为的纯策略 helper，
  新测试必须覆盖上述组合。不得改变 10 ms、Device trait 语义、
  单 waiter 边界或 QEMU 手工政策。

## 2. 增加无 IRQ timer fallback

- [x] 2.1 修改 `crates/axnet/src/device/mod.rs` 的 `Device` trait、
  `crates/axnet/src/device/ethernet.rs` 的 `EthernetDevice` 实现，
  以及 `crates/axnet/src/service.rs::Service::register_waker`。
  映射“无 IRQ 的同步网络进度”和“MS02 范围隔离”。当前 Ethernet
  仅在 `irq_num()` 存在时注册 waker；修改后只对 mask 命中的无 IRQ
  Ethernet 选择 `min(smoltcp poll_at, now + 10 ms)`。loopback 与有 IRQ
  设备保持原行为。GREEN：T1 deadline 测试通过，现有
  register-recheck 调用链不变。禁止启动后台 task、stack runner，
  也不得增加 IRQ、AtomicWaker 或多 waiter 状态。若单 timeout 无法
  支持 payload 的单一 `poll()`，停止并返回 Plan。

## 3. 启用 ICMP echo reply

- [x] 3.1 在 `crates/axnet/Cargo.toml` 的本地 smoltcp feature
  列表加入 `auto-icmp-echo-reply`。映射”协议级独立见证”和
  “MS02 范围隔离”。当前 `process_icmpv4` 在该 feature 关闭时忽略
  echo request；修改后由 smoltcp 生成 echo reply。GREEN：
  `cargo tree` 显示 feature 已启用，目标构建通过。不得修改 kernel
  socket syscall、增加 raw socket，或改动 smoltcp echo 实现。
  若 feature 不能在当前 QEMU feature 组合中解析，停止并写
  Blocker Handoff。

## 4. 回归与手工验证交接

- [x] 4.1 依次执行格式、axnet deadline test、payload 编译、
  feature tree、QEMU target build 和 MS01 非 QEMU 回归。
  映射全部 requirement 的构建与兼容边界。每条命令记录关键输出和
  退出码；失败时保留当前任务，不进入下游验证。完成完整 diff 的
  spec review 与 code review 后，准备 Runbook 格式的三组手工命令：
  无 hostfwd 启动、user-net TCP/UDP、TAP ARP/ICMP 与 CPU 采样。
  禁止由脚本、pipe、pexpect 或 agent 驱动 QEMU/guest shell。

## External Verification Boundary

T4 完成后到达用户能力边界。
以下事项不是 agent 可执行任务：

- 用户按 Runbook 手工执行无 hostfwd 启动。
- 用户手工验证 MMIO net/block 与 `eth0`。
- 用户手工运行 user-net TCP/UDP 5555。
- 用户手工创建 `tap-ms02`，运行 `tcpdump` 和 ICMP `ping`。
- 用户手工记录 30 秒空闲 QEMU CPU 样本。

所需 Evidence 位于当前执行 iteration 对应目录。
本轮使用 `evidence/003-policy-coverage-and-runtime-evidence/`。
缺少任一要求文件时，Gate 5 保持 blocked。
若功能失败，Act Response 必须写 `blocked` 与 Blocker Handoff。
若当前行为与 RED 假设不符，停止并返回 Plan Review。
