## 1. Characterization Witness

- [x] 1.1 在 `tests/ms01_socket_baseline.c` 增加静态 guest payload，通过 HTTP 下载（`10.0.2.2:18765`，`scripts/ms01-prepare.sh` 编译+打印命令）上传至 guest `/tmp`；对现有 `StarryOS_riscv64-qemu-virt.bin` 运行 TCP/UDP 基本往返、nonblocking、poll、相邻连接、512 固定容量和 close/relisten，WHY 是固定迁移前行为且不依赖 5555 hostfwd，EXPECTED 是所有 marker 通过且 rootfs 未修改；RED 是缺少 payload/harness 或任一 marker，GREEN 是脚本退出 0；禁止把 BusyBox `nc` 或 host compile 当作完整行为见证。✅ 2026-07-28 完成：9/9 PASS；QEMU OS shell 无法自动化，改为 HTTP 下载 + 手动执行（见 R44 runbook）；K33 记录 fork 版 ENOTCONN 和端口释放延迟偏差。

- [ ] 1.2 ~~将迁移验收收紧为 `scripts/ms01-qemu-test.py` 自动 harness~~ CANCELLED 2026-07-29：用户决策取消自动 harness，QEMU 测试永久改为手动执行（OS shell 阻塞脚本、sandbox EPERM、串口分帧不可靠，三重阻塞面）。`scripts/ms01-qemu-test.py` 保留不动，harness self-test 通过但真实 QEMU normal path 未跑。：动态分配 serial TCP 与 payload 传输端口，自动启动/停止 QEMU、上传并执行 guest payload、逐项校验唯一 PASS marker、实施 timeout 并清理子进程和端口；WHY 是 1.1 的人工流程只能作为旧 fork characterization，EXPECTED 是单条命令可复现且不依赖固定 5555/18765 端口，RED 是当前没有该脚本且 payload 容忍旧 fork 偏差，GREEN 是 harness 自测能识别缺 marker、重复 marker、timeout 和 QEMU 非零；不得覆盖 `evidence/000-initial/`，不得把人工 shell 操作计为迁移 Gate。

## 2. Local Dependency Boundary

- [x] 2.1 完成本地 dependency source 边界：保留根 workspace 对 `crates/axnet` 的直接 path dependency 和两个本地 crate 的 exclude，新增 `[patch.crates-io] axnet-ng = { path = "crates/axnet" }`，并在 kernel QEMU feature 中以 `axdriver/virtio-net`、`axruntime/net-ng` 替换聚合的 `axfeat/net-ng`；WHY 是同时统一 `axruntime` 的 transitive `axnet-ng` 来源并切断 `axfeat/net -> axruntime/net -> legacy axnet`，EXPECTED 是依赖图只有一个本地 `axnet-ng` 和一个本地 `smoltcp`，RED 是当前 tree 仍含 registry `axnet-ng`、legacy `axnet` 与 `starry-smoltcp`，GREEN 是 metadata 通过且对这三个包的反向 tree 均不存在；不得本地化或修改 `axfeat`/`axruntime`，不得恢复 smoltcp 私有接口；若精确 feature edge 缺失 QEMU 当前所需的 IRQ、multitask、paging 或 NIC 能力，停止并回到 Plan。

- [x] 2.2 从变更前 lockfile 内容重建最小 `Cargo.lock` 增量，只保留本地 `axnet-ng`、本地 `smoltcp` 及其必要引用变化；WHY 是来源迁移不能顺带刷新全局 registry 版本，EXPECTED 是 `addr2line`、`either`、`regex`、`rand`、`zerocopy` 等无关 package 的 version/checksum 与变更前一致，RED 是当前 lock diff 含多项无关升级，GREEN 是 lockfile 中没有 registry `axnet-ng`、legacy `axnet`、`starry-smoltcp` 且无无关 version/checksum 漂移；不得用全量 `cargo update`，不得覆盖用户的其他 lockfile 增量。

## 3. TCP Bind Ownership

- [x] 3.1 在 `crates/axnet/src/wrapper.rs` 和 `tcp.rs` 将 fork 的 TCP bound endpoint 迁移为 axnet sidecar：外部 TCP handle 拥有 bind 记录，bind/connect/local address/device mask/bind conflict 使用该记录，accepted handle 从 smoltcp connection tuple 取 local endpoint，统一 remove 清理；WHY 是 POSIX bind state 不属于 smoltcp，EXPECTED 是显式 bind、隐式 ephemeral bind、冲突失败和 close 后重绑保持兼容；RED 是编译替换试验中的 9 个 `get_bound_endpoint`/`set_bound_endpoint` 错误，GREEN 是这些错误归零且 focused bind tests/断言通过，允许此步暂时只剩 preprocess 错误；禁止把字段补入 smoltcp 或把内部 listener handles 注册为 bind owners。✅ 2026-07-29 完成：iter 002 新增 4 个 bind 专项测试（getsockname、ephemeral、conflict、close-cleanup），14/14 QEMU PASS；bind_check 通配地址检测修复（wrapper.rs:54，port-only 匹配）。

## 4. Standard smoltcp Listener

- [x] 4.1 完成并审查 `crates/axnet/src/listen_table.rs` 的一个空闲 listening handle 加有界 pending/ready/reset slots：listen 创建首个标准 listener，SYN 占用后入队并按容量补位，accept 至多交付一次，Closed handle 先从 `SocketSet` 移除再报告一次 `ConnectionReset`，unlisten/Drop 清理全部未交付 handle；新增精确容量 RED/GREEN，必须证明 512 个连接全部建立和接受、第 513 个不破坏状态，以及 accept 一个后立即建立的新连接成功；WHY 是保留固定容量和唯一 handle 生命周期，EXPECTED 是 full/release/relisten 不泄漏、不复用、不丢第一个恢复 SYN；禁止把通过阈值降到 256、下传 syscall backlog 或预分配约 64 MiB；若状态分类无法证明唯一所有权或 reset 可观察语义，停止并回到 Plan。

- [x] 4.2 在 `crates/axnet/src/service.rs` 以 `poll_maintenance`、ingress 前 listener reconcile、逐包 `poll_ingress_single` 后 reconcile、循环 `poll_egress` 直到 `PollResult::None` 和最终 cleanup 驱动同步栈，并在 `router.rs` 删除 TCP packet snoop、`SocketSet` 参数和 `RxToken::preprocess`；所有同时访问路径保持 `SocketSet -> ListenTable entry` 锁序，accept/readiness 不反向锁 `SocketSet`；WHY 是 accept 释放容量后必须在下一个 SYN 前补位且一次 poll 要推进全部当前 egress，EXPECTED 是满队列恢复、相邻连接和持续 egress 都有 focused RED/GREEN，本地 axnet check 退出 0 且无私有 hook；若循环无法保证终止、锁序或 `router.dispatch` 推进条件，停止并回到 Plan。

## 5. Layered Regression Gates

- [x] 5.1 运行本地 smoltcp 精确 feature-set lib tests、本地 axnet check、格式检查、Cargo dependency assertions 和当前 QEMU feature 完整 RISC-V build，WHY 是先证明 dependency/protocol layer 再进入 VFS/socket runtime，EXPECTED 是所有命令退出 0、lockfile 无 `starry-smoltcp`、未运行验收的 IPv6/raw/ICMP/DNS features 继续编译；RED 是任何 compile/test/source assertion 失败，GREEN 是各项通过。✅ 2026-07-29 完成：axnet fmt check exit 0；kernel build exit 0；lockfile 无 starry-smoltcp、无无关 drift。smoltcp lib test 豁免（insta 不可用，用户授权）；axnet cargo check --offline 环境阻断（同 iter 001 ENV BLOCK，kernel build 是更强 Gate）。

- [x] 5.2 使用新构建镜像执行 `python3 scripts/ms01-qemu-test.py`，严格验证 TCP bind/listen/accept、两个相邻连接、精确 512 满容量、accept 一个后立即恢复、close 后不等待即 relisten、UDP payload/source/datagram boundary、TCP/UDP 仅 `EAGAIN/EWOULDBLOCK` 和 poll/readiness；WHY 是 kernel syscall、VFS adapter、axpoll、axnet、smoltcp 和 loopback 必须形成同一运行见证，EXPECTED 是脚本退出 0 且每个场景只有一个明确 PASS marker；RED 是 timeout、panic、重复交付、错误 errno、固定端口依赖、缺/重 marker 或 QEMU 非零，GREEN 是全部 marker 和正常 harness cleanup；禁止保留旧 fork 的 `ENOTCONN` 宽容或 2 秒等待，禁止用 boot prompt、serial output 或 basic `nc` 替代该 Gate。WAIVED 2026-07-29：用户授权“这里进行测试你按runbook给我命令行我来手动做”；新镜像严格 payload 10/10、exit 0，手工运行替代自动 QEMU normal path，风险是自动 launcher 的真实串口与 cleanup 路径尚未运行。

## 6. Persisted Evidence and Self-Review

- [x] 6.1 保留 `evidence/000-initial/` 作为旧 fork characterization，建立新栈 evidence 三轮（001/002），检查全量 diff，更新 Act Response。✅ 2026-07-29 完成：evidence/000-initial/（旧 fork 9/9）、evidence/001-dependency-recovery/（新栈 10/10 + dependency source + lock audit）、evidence/002-bind-fmt-closeout/（新增 4 bind 见证 + fmt + smoltcp 豁免 + lock audit）。iter 001 Act Response status `reported`，iter 002 Act Response status `reported`。全量 diff 审查通过（无 IRQ/async/transport/syscall backlog/无关升级）。
