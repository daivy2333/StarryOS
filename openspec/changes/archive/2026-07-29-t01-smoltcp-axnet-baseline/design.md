## Context

当前 QEMU 网络路径为：

```text
socket syscall
  -> kernel::file::net::Socket
  -> registry axnet-ng 0.3.0-preview.2
  -> starry-smoltcp 0.12.1-preview.1
  -> Router
  -> axdriver_net
```

registry axnet 在 `Router::RxToken::preprocess` 中解析 TCP SYN，再动态创建
smoltcp listening socket。该接口只存在于 `starry-smoltcp` fork。仓库内
smoltcp 0.13.1 使用公开的 `TcpSocket::listen` 和
`Interface::poll_ingress_single`。

实际编译替换试验只暴露两类 axnet API 不兼容：

- `RxToken::preprocess` 已删除。
- TCP `get_bound_endpoint` 和 `set_bound_endpoint` 已删除。

当前 `sys_listen` 只校验 backlog，axnet 实际使用固定
`LISTEN_QUEUE_SIZE = 512`。UDP 已在 axnet 自身保存 local/peer state。
kernel 的 VFS adapter、poll 和 epoll 继续委托 axnet。

现有 QEMU 镜像已完成 BusyBox `nc` loopback TCP、UDP payload 往返。
`init.sh` 不启动 5555 服务，raw socket 和 netlink 不受支持。当前完整
构建在 `lwext4_rust` C 子构建处被执行环境的 `Bad system call` 阻断；
网络依赖在该点之前可以编译。

## Goals / Non-Goals

**Goals:**

- 本地化 axnet，并以 path dependency 使用仓库内 smoltcp 0.13.1。
- 在 axnet 内保存 TCP bind 状态。
- 使用标准 smoltcp listening sockets 保持固定 512 容量的同步 listener。
- 保持 TCP、UDP、nonblocking 和 poll 的 kernel 调用边界。
- 建立可复现的依赖、构建和 QEMU socket 证据。

**Non-Goals:**

- IRQ、queue task、stack runner、async socket bridge 或新的 executor。
- 修改 `sys_listen` 的 backlog 下传语义。
- 修复无关 syscall 问题或扩展 raw/netlink socket。
- PCI、VF2、DWMAC、SMP、吞吐和延迟优化。
- 修改 smoltcp trait 或恢复 `RxToken::preprocess`。

## Decisions

### D1. 用 path、patch 和精确 feature edge 统一 axnet 来源

**Decision:** 将 axnet 放在 `crates/axnet`。根 workspace dependency
直接指向该路径，根 `[patch.crates-io]` 同时把 transitive
`axnet-ng` 解析到同一路径；axnet manifest 直接指向 `../smoltcp`。
kernel 的 QEMU feature 不再启用聚合的 `axfeat/net-ng`，而是直接启用
`axdriver/virtio-net` 与 `axruntime/net-ng`。根 workspace exclude
继续列出 `crates/axnet` 和 `crates/smoltcp`。

**Reason:** `axfeat/net-ng` 会经 `axfeat/net` 同时激活
`axruntime/net`，从而保留 legacy `axnet` 和 `starry-smoltcp`。
只修改根直接 dependency 不能覆盖 `axruntime` 的 transitive
`axnet-ng` 边。patch 统一同名 package 来源，精确 feature edge 则切断
legacy `net`，两者缺一不可。

**Impact:** 依赖图只有一个本地 `axnet-ng` 和一个本地 `smoltcp`；
registry `axfeat`/`axruntime` 继续使用，不需要复制其源码。
`Cargo.lock` 只允许出现完成这次来源迁移所需的包块变化，不附带无关
registry 升级。

**Alternatives:** 本地化并修改 `axfeat`、`axruntime` 会扩大维护面；
只用 patch 仍会编译 legacy `axnet`；只拆 feature edge 仍不能替换
`axruntime` 的 registry `axnet-ng`。把本地 crate 加入 workspace 会扩大
workspace lint 和 test 范围。本轮均不采用。

### D2. TCP bind 状态由 axnet sidecar 持有

**Decision:** `SocketSetWrapper` 维护以外部 TCP `SocketHandle` 为键的
bind sidecar。`TcpSocket::bind` 和隐式 connect 更新它；bind conflict
检查读取它。accepted socket 不登记为新的 bind owner，其 local address
从 smoltcp `local_endpoint()` 读取。删除 socket 时同步删除 sidecar。

**Reason:** POSIX bind state 不属于 smoltcp TCP state machine。内部
listener pool 的多个 handle 也不能被误判为重复 bind。

**Impact:** TCP bind、connect、local address 和 device mask 不依赖 fork
API。UDP 的当前独立状态保持不变。

**Alternatives:** 把字段补回 smoltcp 会继续维护私有 fork；只在
`TcpSocket` 对象中保存会使全局 bind conflict 检查无法枚举。本轮不采用。

### D3. Listener 使用一个空闲 slot 和有界 pending queue

**Decision:** 每个 axnet listener entry 持有一个标准 smoltcp listening
handle，以及至多 512 个 pending/ready/reset slot。开始 listen 时创建
一个空闲 handle。该 handle 接收 SYN 后进入队列；只要队列未满，立即
创建新的空闲 handle。

`Service::poll` 改用以下同步顺序：

```text
router.poll
  -> poll_maintenance
  -> listener reconcile and refill
  -> loop poll_ingress_single
       -> listener reconcile and refill
  -> loop poll_egress until PollResult::None
  -> listener reconcile and cleanup
  -> router.dispatch
```

进入 ingress 前先恢复 accept 已释放的容量，每处理一个 ingress packet
再补位，因此满队列释放后的第一个新 SYN 和相邻 SYN 都不依赖包嗅探。
Closed pending handle 被移出 `SocketSet`，并保留一次
`ConnectionReset` 可观察结果。accept 只消费 ready/reset slot；每个
connected handle 最多交付一次。

**Reason:** 这是 smoltcp 0.13.1 公开 API 支持的 listener 模型。它按
连接增长内存，不会在 listen 时一次分配 512 组 128 KiB socket buffers。

**Impact:** `router.rs` 删除 TCP 解析和 preprocess；`service.rs` 采用
细粒度 poll；`listen_table.rs` 拥有 listener slot 生命周期。

**Alternatives:** 预分配 512 个 sockets 会为每个 listener 立即占用约
64 MiB。把 SYN snoop 移到 Router 仍会复制协议解析，并可能为重传 SYN
重复建 socket。本轮不采用。

### D4. 固定 backlog 兼容，不扩展 syscall

**Decision:** pending/ready/reset 总数上限保持 512。合法
`listen(fd, backlog)` 仍只由 kernel 校验，数值不传入 axnet。队列满时
不再补空闲 handle；accept 或 reset cleanup 释放容量后再补位。

**Reason:** 这是 Gate 1 批准的当前兼容范围。backlog 下传是独立的
syscall 行为变更。

**Impact:** MS01 测试显式覆盖 512 边界，但不声称实现 POSIX 动态
backlog。

**Alternatives:** 本轮下传 backlog 会改变既有行为和 API，需要新的
需求审批。

### D5. 固定锁顺序和状态所有权

**Decision:** 同时需要两类状态时，统一先锁 `SocketSet`，再锁
`ListenTable` entry。service reconciliation 接收已借用的
`&mut SocketSet`。accept 和 readiness 只读取 listener entry 的分类
结果，不反向获取 `SocketSet`。

**Reason:** 当前 service 路径和 accept 路径存在相反锁序。即使 MS01
不做 SMP，也不应把死锁边界固化到本地 crate。

**Impact:** listener entry 必须保存 pending、ready 和 reset 分类；
socket handle 的创建、移除和交付责任唯一。

**Alternatives:** 依赖单 hart 避免死锁会使后续 milestone 无法安全复用。

### D6. 运行见证使用 guest loopback payload

**Decision:** 新增静态 C socket payload 和 QEMU serial harness。harness
使用动态 serial TCP port 和动态 payload 传输端口，把 payload 传入
guest `/tmp` 后执行。它负责 QEMU 启停、timeout、marker 唯一性检查和
资源清理。测试不依赖 5555 host forwarding，也不修改 rootfs 镜像。

payload 覆盖：

- TCP bind/listen/accept、相邻连接、512 容量和 close/relisten。
- 512 满载后 accept 一个，并立即建立一个新连接。
- UDP 双向 payload、source address 和 datagram boundary。
- TCP/UDP nonblocking would-block。
- listener 和 data readiness 与紧随其后的 I/O。

迁移后的 payload 只接受 `EAGAIN/EWOULDBLOCK` 作为无数据的
would-block，不保留旧 fork 的 UDP `ENOTCONN` 宽容项；close/relisten
不加入人为等待。旧 fork 的宽容输出只保存在 characterization evidence。

**Reason:** BusyBox `nc` 只能提供基本 payload 见证，不能稳定断言 errno、
poll 和资源边界。serial 与网络结果需要分离。

**Impact:** QEMU 是行为 Gate；host compile 和 Cargo checks 不能替代它。

**Alternatives:** hostfwd 受端口占用影响；写入 rootfs 会污染测试输入。

## Risks / Trade-offs

- [单次 poll 中 listener 状态未补位] → 每个
  `poll_ingress_single` 后 reconcile，并用相邻连接测试验证。
- [512 sockets 消耗较多内存] → 保持按 SYN 分配，只在边界用例达到上限。
- [handle 重复移除或交付] → entry 分类具有唯一所有权，close/relisten
  和 reset 路径检查 handle 计数。
- [sidecar 泄漏导致假 AddrInUse] → `SocketSetWrapper::remove` 是统一
  cleanup 点，测试 bind、close 和重新 bind。
- [构建环境阻断误判] → 分开记录 axnet/smoltcp compile、kernel build
  和 `lwext4_rust` 环境失败；最终 Gate 仍要求可执行环境中完整构建通过。
- [现有未测试协议退化] → 使用当前 axnet feature 集完成 compile Gate；
  不声明 IPv6、raw、ICMP 或 DNS 的运行行为。

## Migration Plan

1. 先加入 characterization payload 和 harness，在现有镜像记录 TCP/UDP
   基线。
2. 本地化 axnet 和依赖边界，确认迁移前的 API compile failure。
3. 迁移 TCP bind sidecar，再迁移 listener 和 service poll。
4. 完成 crate checks、完整 QEMU build 和 payload Gate。
5. 若运行 Gate 失败，回退根 Cargo 的 axnet path 入口即可恢复 registry
   组合；不得以修改 smoltcp trait 作为回退。

## Open Questions

None.
