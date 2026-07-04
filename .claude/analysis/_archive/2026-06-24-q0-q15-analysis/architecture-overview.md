# StarryOS 架构总览

> Part of StarryOS codebase analysis (branch: asyncuart-dev) | Generated 2026-06-11

---

## §1 项目上下文

StarryOS 是一个兼容 Linux ABI 的宏内核操作系统，构建于 **ArceOS** unikernel 框架之上。内核采用 Rust 编写（nightly-2026-02-25），以组件化方式组织核心 OS 服务，前后端分离为一系列 `ax*` crate。当前支持 RISC-V 64、LoongArch64、AArch64 三种架构，x86_64 尚在开发中。

### 1.1 仓库结构

仓库以 Cargo workspace 方式管理，`kernel/` 是唯一的 workspace member，编译入口在项目根。

```
StarryOS/
├── src/main.rs                    # 二进制入口 (no_std, no_main)
│   └── 调用 starry_kernel::entry::init()
├── kernel/src/                    # 内核核心逻辑
│   ├── entry.rs                   # 内核初始化：mount、spawn init 进程
│   ├── lib.rs                     # 模块声明
│   ├── config/                    # 架构相关常量
│   │   ├── riscv64.rs, aarch64.rs, loongarch64.rs, x86_64.rs
│   │   └── mod.rs
│   ├── file/                      # 类文件对象（FileLike trait）
│   │   ├── mod.rs                 # FileLike trait、FD_TABLE、add_stdio
│   │   ├── fs.rs                  # 文件封装（VFS 桥接）
│   │   ├── pipe.rs                # 异步管道（PollSet）
│   │   ├── event.rs               # 异步 eventfd
│   │   ├── epoll.rs               # epoll fd
│   │   ├── net.rs                 # 网络 socket 封装
│   │   ├── pidfd.rs               # pidfd
│   │   └── signalfd.rs            # signalfd
│   ├── mm/                        # 内存管理
│   │   ├── mod.rs, access.rs, io.rs, loader.rs
│   │   └── aspace/                # 地址空间（cow, file, linear, shared）
│   ├── pseudofs/                  # 伪文件系统
│   │   ├── mod.rs                 # mount_all(), mount_at()
│   │   ├── device.rs              # DeviceOps trait、Device struct
│   │   ├── dir.rs, file.rs, fs.rs, tmp.rs, proc.rs
│   │   └── dev/                   # /dev 设备实现
│   │       ├── mod.rs             # new_devfs(), builder() — 所有设备注册
│   │       └── tty/               # TTY 子系统（ntty, ptm, pts, pty, terminal/）
│   ├── syscall/                   # 系统调用处理器
│   │   ├── mod.rs                 # 主分发入口
│   │   ├── fs/                    # 文件 I/O（io.rs, fd_ops.rs, poll/select/epoll）
│   │   ├── io_mpx/                # poll/select/epoll
│   │   ├── task/                  # clone/execve/exit/schedule
│   │   ├── mm/                    # brk/mmap/mprotect
│   │   ├── net/                   # socket 操作
│   │   ├── ipc/                   # msg/shm
│   │   ├── sync/                  # futex/membarrier
│   │   └── ...                    # signal, time, resources, sys
│   ├── task/                      # 任务/线程/进程管理
│   │   ├── mod.rs                 # Thread, ProcessData, AsThread
│   │   ├── ops.rs                 # new_user_task, spawn_alarm_task
│   │   ├── futex.rs, signal.rs, timer.rs, user.rs, stat.rs, resources.rs
│   └── time.rs                    # 时间相关工具
├── Cargo.toml                     # Workspace 根（members: [kernel]）
├── Makefile                       # 构建：make build/run/debug
├── docs/                          # 设计文档
└── .claude/docs/                  # 开发文档体系
```

### 1.2 构建系统

构建流程以 Makefile 驱动，核心路径为：

```
make run → make defconfig → make build (cargo build --features qemu) → QEMU 启动
make ARCH=loongarch64 run → 同上，切换目标架构
make debug → build + QEMU with debug symbols
```

关键构建变量：

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `ARCH` | riscv64 | 目标架构 |
| `LOG` | warn | 日志级别 |
| `BLK` | y | 块设备支持 |
| `NET` | y | 网络支持 |
| `MEM` | 1G | QEMU 内存 |
| `APP_FEATURES` | qemu | 平台特性 |

Cargo feature 体系由平台特性和可选功能组成：

| Feature | 依赖 | 用途 |
|---------|------|------|
| `qemu` | `axfeat/defplat`, `bus-pci`, `display`, `input`, `vsock`, `dev-log` | 默认 QEMU 构建 |
| `vf2` | `axplat-riscv64-visionfive2`, `driver-sdmmc` | VisionFive 2 真板 |
| `smp` | `axfeat/smp` | 多核支持 |
| `dev-log` | (kernel crate) | /dev/log socket |
| `memtrack` | `axfeat/dwarf`, `axalloc/tracking`, `gimli` | 内存追踪调试 |
| `input` | `axinput` | 输入设备支持 |

### 1.3 技术栈

内核直接依赖的 ArceOS 组件 crate（均使用 0.3.0-preview.2 或对应版本）：

```
starryos (binary — src/main.rs)
  └── starry-kernel (kernel/)
       ├── axtask        — 任务调度器，async future 支持
       ├── axpoll        — IoEvents, Pollable, PollSet
       ├── axhal         — 硬件抽象（console, interrupt, PLIC）
       ├── axfs          — 文件系统（ext4, VFS）
       ├── axsync        — Mutex, SpinLock
       ├── axmm          — 内存管理
       ├── axalloc       — 内存分配器
       ├── axconfig      — 配置常量
       ├── axruntime     — 运行时初始化（调用 main()）
       ├── axdriver      — 设备驱动框架
       ├── axdisplay     — 显示/帧缓冲
       ├── axlog         — 日志
       ├── axnet         — 网络栈
       ├── axio          — I/O traits
       ├── axerrno       — 错误类型
       └── axbacktrace   — 回溯支持
```

外部关键依赖：

| Crate | 版本 | 用途 |
|-------|------|------|
| `ringbuf` | 0.4.8 | 无锁环形缓冲区（HeapRb） |
| `axpoll` | 0.1 | PollSet, IoEvents, Pollable |
| `axfs-ng-vfs` | 0.1 | VFS traits（FileNodeOps, DirNodeOps） |
| `starry-process` | 0.2 | 进程抽象 |
| `starry-signal` | 0.3 | 信号处理 |
| `starry-vm` | 0.3 | 用户空间内存访问 |
| `linux-raw-sys` | 0.12 | Linux ABI 常量 |
| `flatten_objects` | 0.2.4 | Slab 分配 FD 表 |
| `scope-local` | — | 每进程 scope-local 存储 |
| `bitflags` | 2.10 | 位标志类型 |
| `spin` | 0.10 | 自旋锁 |
| `ouroboros` | 0.18 | 自引用结构体 |

### 1.4 架构相关配置

`kernel/src/config/` 按架构存放内存布局常量：

| 文件 | 常量 |
|------|------|
| `riscv64.rs` | `USER_HEAP_BASE`, `PAGE_SIZE`, `SIGNAL_TRAMPOLINE` |
| `aarch64.rs` | 架构特定内存布局 |
| `loongarch64.rs` | LoongArch 内存布局 |
| `x86_64.rs` | x86_64 内存布局（开发中） |

---

## §2 启动与初始化

### 2.1 启动链总览

StarryOS 的启动过程分为两个阶段：axruntime 运行时初始化（外部 crate，完成硬件平台初始化），以及内核自身的 `entry::init()`（完成文件系统挂载和 init 进程创建）。完整链路如下：

```
Power On
  │
  ▼
axruntime::init()                       [外部 crate]
  │
  ├── Arch 设置（trap vectors, PLIC, timer）
  ├── 堆初始化（axalloc）
  ├── 调度器初始化（axtask）
  ├── 设备初始化（axdriver）
  ├── 文件系统初始化（axfs）
  │
  ▼
main()                                  [src/main.rs:12]
  │
  ▼
starry_kernel::entry::init(&args, &envs) [kernel/src/entry.rs:20]
  │
  ├── 1. pseudofs::mount_all()
  │    ├── mount_at("/dev",       dev::new_devfs())     — 字符/块设备
  │    ├── mount_at("/dev/shm",   MemoryFs::new())      — tmpfs
  │    ├── mount_at("/tmp",       MemoryFs::new())      — tmpfs
  │    ├── mount_at("/proc",      proc::new_procfs())   — 进程信息
  │    └── mount_at("/sys",       MemoryFs::new())      — sysfs 桩
  │
  ├── 2. spawn_alarm_task()                — 周期性闹钟定时器
  │
  ├── 3. 解析 init 可执行文件路径
  │    └── FS_CONTEXT.lock().resolve(&args[0])
  │
  ├── 4. 创建用户地址空间
  │    ├── new_user_aspace_empty()
  │    ├── copy_from_kernel(&mut uspace)    — 复制内核映射
  │    └── load_user_app(&mut uspace, None, args, envs)
  │
  ├── 5. 创建 init 进程与任务
  │    ├── UserContext::new(entry_vaddr, ustack_top, 0)
  │    ├── new_user_task(name, uctx, 0)
  │    ├── 设置页表根
  │    ├── Process::new_init(pid) + add_thread(pid)
  │    ├── N_TTY.bind_to(&proc)             — 绑定控制终端
  │    ├── ProcessData::new(...)
  │    └── add_stdio → /dev/console 被 open 3 次 (stdin/out/err)
  │
  ├── 6. 派发 init 任务
  │    ├── Thread::new(pid, proc_data)
  │    ├── TaskExt attach
  │    └── spawn_task(task)
  │
  └── 7. 等待 init 完成
       └── task.join() → unmount + flush
```

### 2.2 入口点

`src/main.rs` 是二进制入口，标记为 `#![no_std]` 和 `#![no_main]`，由 axruntime 调用。init 进程的启动命令通过编译期 `include_str!()` 嵌入：

```rust
#![no_std]
#![no_main]

pub const CMDLINE: &[&str] = &["/bin/sh", "-c", include_str!("init.sh")];

#[unsafe(no_mangle)]
fn main() {
    let args = CMDLINE.iter().copied().map(str::to_owned).collect::<Vec<_>>();
    let envs = [];
    starry_kernel::entry::init(&args, &envs);
}
```

init 进程启动 `/bin/sh -c <init.sh>`，其中 `init.sh` 内容在编译时静态嵌入，无需磁盘文件。

### 2.3 内核初始化详情

#### 文件系统挂载（mount_all）

`mount_all()` 利用全局 `FS_CONTEXT` 逐级构建 VFS 命名空间：

```rust
pub fn mount_all() -> LinuxResult<()> {
    let fs = FS_CONTEXT.lock();
    mount_at(&fs, "/dev",     dev::new_devfs())?;
    mount_at(&fs, "/dev/shm", tmp::MemoryFs::new())?;
    mount_at(&fs, "/tmp",     tmp::MemoryFs::new())?;
    mount_at(&fs, "/proc",    proc::new_procfs())?;
    mount_at(&fs, "/sys",     tmp::MemoryFs::new())?;
    // /sys/class/graphics/fb0/device 符号链接 ...
}
```

`mount_at` 解析或创建目标路径后调用 `resolve(path)?.mount(&mount_fs)`。所有挂载点均为内存文件系统或伪文件系统，不依赖磁盘 I/O。

#### init 进程配置

init 进程获得以下资源：
- **标准 I/O**：stdin/stdout/stderr → `/dev/console`（N_TTY 线路规程）
- **控制终端**：通过 `N_TTY.bind_to(&proc)` 绑定
- **PID**：`task.id().as_u64()`
- **地址空间**：用户空间 + 内核映射 + 加载的应用程序二进制

`add_stdio` 打开 `/dev/console` 两次（一次只读、一次只写），分配 fd 0/1/2：

```rust
pub fn add_stdio(fd_table: &mut FlattenObjects<...>) -> AxResult<()> {
    let cx = FS_CONTEXT.lock();
    let tty_in  = open("/dev/console", read(true).write(false));
    let tty_out = open("/dev/console", read(false).write(true));
    fd_table.add(tty_in);   // fd 0 — stdin
    fd_table.add(tty_out);  // fd 1 — stdout
    fd_table.add(tty_out);  // fd 2 — stderr
}
```

fd 1 和 fd 2 共享同一个 `Arc<File>` 实例，因此对 stdout 和 stderr 的写入操作经过同一对象。

---

## §3 任务与进程模型

StarryOS 的任务模型在 ArceOS 的 `axtask::TaskInner`（可调度实体）之上构建了 Linux 兼容的线程/进程抽象层。核心设计目标是通过 scope-local 机制实现 FD 表的自动按进程隔离，避免在系统调用代码中手动切换 FD 表。

### 3.1 核心类型

| 类型 | 位置 | 职责 |
|------|------|------|
| `TaskInner` | `axtask` crate（外部） | 可调度实体：拥有独立栈、上下文、调度器状态 |
| `Thread` | `kernel/src/task/mod.rs` | 每线程内核数据：信号、时间追踪、退出通知 |
| `ProcessData` | `kernel/src/task/mod.rs` | 每进程共享数据：地址空间、FD 表（scope-local）、信号动作、futex 表 |
| `Process` | `starry-process` crate | 进程标识（PID, PGID, SID） |
| `AxTaskExt` | `axtask` crate | 扩展 trait，用于将内核数据附加到 axtask 上 |

### 3.2 Thread 结构

`Thread` 承载线程级别的内核状态，通过 `Arc<ProcessData>` 指回所属进程：

```rust
pub struct Thread {
    pub proc_data: Arc<ProcessData>,           // 共享进程状态
    clear_child_tid: AtomicUsize,               // set_tid_address
    robust_list_head: AtomicUsize,              // robust futex 链表
    pub signal: Arc<ThreadSignalManager>,       // 每线程信号状态
    pub time: AssumeSync<RefCell<TimeManager>>, // 每线程时间
    oom_score_adj: AtomicI32,
    pub exit: Arc<AtomicBool>,                  // 退出标志
    pub exit_event: Arc<PollSet>,               // 退出时唤醒等待者
}
```

### 3.3 线程与任务的桥接

`axtask::TaskInner` 与内核 `Thread` 之间的连接通过 `TaskExt` trait 和 `AsThread` trait 实现。每一次上下文切换都触发 `on_enter`/`on_leave`，完成 scope-local 的切换：

```rust
impl TaskExt for Box<Thread> {
    fn on_enter(&self) {
        // 设置每进程 scope-local 变量
        let scope = self.proc_data.scope.read();
        unsafe { ActiveScope::set(&scope) };
        core::mem::forget(scope);  // 阻止 scope drop
    }

    fn on_leave(&self) {
        // 恢复全局 scope
        ActiveScope::set_global();
        unsafe { self.proc_data.scope.force_read_decrement() };
    }
}

impl AsThread for TaskInner {
    fn try_as_thread(&self) -> Option<&Thread> {
        self.task_ext()
            .map(|ext| ext.downcast_ref::<Box<Thread>>().as_ref())
    }
}
```

这是 FD 表按进程隔离的关键机制：任务被调度运行时，其进程的 scope 被激活，全局 `FD_TABLE` 自动路由到当前进程的表；任务被切换出时，scope 恢复到全局默认。

### 3.4 ProcessData 与 Scope-Local FD 表

`ProcessData` 聚合了进程级别的所有共享资源：

```rust
pub struct ProcessData {
    pub proc: Arc<Process>,                     // PID, PGID, SID
    pub exe_path: RwLock<String>,               // 可执行文件路径
    pub cmdline: RwLock<Arc<Vec<String>>>,      // 命令行参数
    pub aspace: Arc<Mutex<AddrSpace>>,          // 虚拟地址空间
    pub scope: RwLock<Scope>,                   // 资源 scope（FD_TABLE 等）
    heap_top: AtomicUsize,                      // brk 堆位置
    pub rlim: RwLock<Rlimits>,                  // 资源限制
    pub child_exit_event: Arc<PollSet>,         // SIGCHLD 通知
    pub exit_event: Arc<PollSet>,               // 进程退出通知
    pub exit_signal: Option<Signo>,             // 终止时发送的信号
    pub signal: Arc<ProcessSignalManager>,      // 信号动作
    futex_table: Arc<FutexTable>,               // Futex 状态
    umask: AtomicU32,                           // 文件创建掩码
}
```

FD 表使用 `scope_local` crate 声明为全局静态，通过 scope 切换自动隔离：

```rust
scope_local::scope_local! {
    pub static FD_TABLE: Arc<RwLock<FlattenObjects<FileDescriptor, AX_FILE_LIMIT>>> = Arc::default();
}
```

运行时语义：线程 A（进程 1）被调度 → `FD_TABLE` 路由到进程 1 的表；线程 B（进程 2）被调度 → `FD_TABLE` 路由到进程 2 的表。系统调用代码中直接访问 `FD_TABLE`，无需感知进程边界。

### 3.5 异步任务派发

内核在 N_TTY 初始化期间派发 `tty-reader` 异步任务，作为控制台输入处理的核心协程。该任务在 `poll_fn` 循环中周期性执行，注册 IRQ waker 后挂起到 `Poll::Pending`，由 UART 中断唤醒：

```rust
// ldisc.rs（外部进程模式）:
axtask::spawn_with_name("tty-reader", move || {
    block_on(poll_fn(|cx| {
        // 处理输入数据
        while reader.poll() { poll_rx.wake(); }
        // 注册唤醒事件
        poll_tx.register(cx.waker());
        register(cx.waker().clone());  // register_irq_waker
        // 再次 drain
        while reader.poll() { poll_rx.wake(); }
        Poll::Pending  // 挂起，等待下一次 IRQ
    }))
});
```

### 3.6 进程创建与退出事件

init 进程的创建发生在 `entry.rs` 中，经历从用户上下文构造到最终派发的完整流程：

```rust
let uctx = UserContext::new(entry_vaddr.into(), ustack_top, 0);
let mut task = new_user_task(name, uctx, 0);
task.ctx_mut().set_page_table_root(uspace.page_table_root());

let pid = task.id().as_u64() as Pid;
let proc = Process::new_init(pid);
proc.add_thread(pid);
N_TTY.bind_to(&proc).expect("Failed to bind ntty");

let proc = ProcessData::new(proc, path, ...);
let thr = Thread::new(pid, proc);
*task.task_ext_mut() = Some(AxTaskExt::from_impl(thr));
let task = spawn_task(task);
```

退出通知机制基于 `Arc<PollSet>`，支持三种粒度：
- `Thread::exit_event` — 特定线程退出时唤醒
- `ProcessData::exit_event` — 进程退出时唤醒
- `ProcessData::child_exit_event` — 子进程退出时唤醒（用于 SIGCHLD）

这三类事件被 `wait4`/`waitpid` 系统调用和进程回收逻辑所消费。

---

## §4 中断框架

### 4.1 RISC-V 中断层次

StarryOS 的中断处理在 S-mode（Supervisor）下完成，从硬件信号到软件分发的完整路径为：

```
硬件信号 → PLIC → Hart 0 M-mode trap → trap_handler → 软件分发
```

QEMU virt 平台的 PLIC 配置参数如下：

| 参数 | 值 |
|------|-----|
| 基地址 | 0x0C00_0000 |
| 最大中断源数 | 127 |
| UART 中断号 | 10（0x0a） |
| 优先级阈值 | 0（允许所有中断） |

关键控制寄存器：
- `stvec` 指向 trap 入口（`axhal/src/platform/qemu_virt_riscv/trap.S`）
- `scause` 标识中断类型和来源
- `sie`（Supervisor Interrupt Enable）控制 S-mode 中断使能

### 4.2 当前框架实现

#### 初始化流程

中断子系统在 axhal 层初始化，依次配置 PLIC 和使能外部中断：

```
axhal::init()
  └─ irq::init()
       ├─ plic::init(plic_base, hart_id)    // 初始化 PLIC
       ├─ plic::set_threshold(0)            // 阈值设为 0（接收所有优先级）
       ├─ plic::enable(UART_IRQ)            // 使能 UART 中断
       ├─ plic::set_priority(UART_IRQ, 1)   // 设置 UART 优先级为 1
       └─ 开启 S-mode 外部中断 (sie::set_sext())
```

#### Trap 处理流程

trap 入口在汇编中保存上下文后，转入 Rust 层按 `scause` 分发：

```
trap_entry (trap.S, 汇编)
  → 保存上下文
  → 调用 rust_trap_handler(trap_frame)
    → axhal::trap::rust_trap_handler()
      → match scause:
          Interrupt::SupervisorExternal → handle_supervisor_external()
          Interrupt::SupervisorTimer   → timer handler
          Exception::*                  → exception handler
```

`handle_supervisor_external()` 从 PLIC claim 中断号后分发到已注册的回调：

```rust
fn handle_supervisor_external() {
    let irq = plic::claim();       // 获取最高优先级待处理中断号
    if irq != 0 {
        dispatch_irq(irq);         // 分发到注册的回调
        plic::complete(irq);       // 通知 PLIC 处理完成
    }
}
```

#### IRQ 注册机制

当前框架提供两种互补的中断注册方式：

**机制 1：`register_irq(irq, handler)` — 原始回调**

回调类型为 `fn()`，在 trap 上下文中直接调用。存储于长度为 128 的 `SpinLock<Option<fn()>>` 数组中。回调执行受中断上下文的严格约束：不能获取锁（可能死锁）、不能调用阻塞函数、不能访问 per-CPU 数据。适用于简单的"设标志 + 唤醒"操作。

**机制 2：`register_irq_waker(irq, waker)` — Waker 注册**

将 `Waker` 存入 per-CPU IRQ waker 表，中断到来时直接调用 `waker.wake_by_ref()` 将对应内核任务加入就绪队列。N_TTY 的 `tty-reader` 任务即通过此机制与 UART 中断联动：

```
UART 中断信号
  → PLIC claim (获取 irq=10)
    → dispatch_irq(10)
      → 查找 IRQ_WAKERS[10]
        → waker.wake_by_ref()
          → 将任务加入就绪队列
            → axtask 调度器在下次 yield 时恢复任务
```

### 4.3 当前框架的局限

四种问题限制了框架在异步串口场景的适用性：

1. **Waker 与回调的语义冲突**：`register_irq` 和 `register_irq_waker` 使用独立存储，对同一 IRQ 同时注册两种机制时行为未定义。
2. **无中断源识别**：`dispatch_irq(10)` 仅知 UART 中断到达，不区分 RX 就绪、TX 空或其他源。ISR 中需要读 UART 的 IIR 寄存器来判定，但当前 `dispatch_irq` 不做此事。
3. **单一 Waker 限制**：每个 IRQ 仅支持一个 Waker。若 RX 任务和 TX 任务同时等待同一个 UART 中断，当前机制无法分别唤醒。
4. **中断上下文约束**：`dispatch_irq` 在 trap 上下文中执行，仅能做原子写、`waker.wake_by_ref()` 和 MMIO 读（无锁），不可获取 SpinLock 或访问 per-CPU 数据。

### 4.4 异步串口的中断需求

为实现异步 UART 驱动，ISR 需承担更细粒度的职责：

```
UART ISR:
  1. 读 IIR → 判断中断源 (RX/TX/其他)
  2. 根据中断源:
     RX 就绪 → 禁用 RX 中断 + 唤醒 RX copier
     TX 空   → 禁用 TX 中断 + 唤醒 TX copier
  3. 中断处理完毕（PLIC complete 已做，无需额外清除）
```

| 需求 | 当前状态 | 所需扩展 |
|------|---------|---------|
| 区分 RX/TX 中断源 | 不支持 | ISR 中读 IIR |
| 分别唤醒 RX/TX 任务 | 不支持 | 双 Waker 或 AtomicWaker |
| ISR 中操作 UART 寄存器 | 不支持 | ISR 需访问 MMIO |
| 禁用特定 UART 中断 | 不支持 | ISR 写 IER |

推荐方案采用 `AtomicWaker` 替代单一 Waker，ISR 中按中断源分派：

```rust
// 异步 UART 的中断分发模型
static RX_WAKER: AtomicWaker = AtomicWaker::new();
static TX_WAKER: AtomicWaker = AtomicWaker::new();

fn uart_isr() {
    let iir = read_iir();
    let source = (iir >> 1) & 0x7;
    match source {
        0b010 => { // RX 就绪
            disable_rx_intr();   // IER &= ~0x01
            RX_WAKER.wake();     // 唤醒 RX copier
        }
        0b001 => { // TX 空
            disable_tx_intr();   // IER &= ~0x02
            TX_WAKER.wake();     // 唤醒 TX copier
        }
        _ => {}
    }
}
```

`AtomicWaker::wake()` 和 MMIO 寄存器写入在中断上下文中均安全（无锁、无阻塞），满足 ISR 极简原则。

### 4.5 PLIC 操作参考

PLIC 的使能控制和 UART 的 IER 是两层独立的中断门控。UART 中断的完整路径需要两层同时使能：

```
UART IER 使能 → PLIC enable → S-mode sie.sext → CPU 响应
```

禁用 UART 中断可在任意一层实现——写 IER（推荐，精确控制 RX/TX）、PLIC disable（粗粒度）、或清 sie.sext（影响全部外部中断）。PLIC 操作接口如下：

| 操作 | 函数 | 说明 |
|------|------|------|
| 获取待处理中断 | `plic::claim()` | 返回最高优先级中断号，同时开始处理 |
| 完成中断处理 | `plic::complete(irq)` | 通知 PLIC 可再次触发该中断 |
| 使能中断源 | `plic::enable(irq)` | 在 PLIC 层面使能 |
| 禁用中断源 | `plic::disable(irq)` | 在 PLIC 层面禁用 |
| 设置优先级 | `plic::set_priority(irq, prio)` | 优先级越高值越大 |
| 设置阈值 | `plic::set_threshold(thresh)` | 低于阈值的中断不触发 |

---

## §5 关键文件索引

以下表格合并了上述四个领域涉及的全部关键文件，按内核模块分组：

| 文件 | 所属模块 | 职责 |
|------|---------|------|
| `src/main.rs` | 入口 | 二进制入口点，定义 CMDLINE，调用 kernel init |
| `kernel/src/entry.rs` | 启动 | 内核初始化：mount 文件系统、创建 init 进程、绑定终端、派发任务 |
| `kernel/src/lib.rs` | 全局 | 模块声明 |
| `kernel/src/config/mod.rs` | 配置 | 架构相关常量定义 |
| `kernel/src/config/riscv64.rs` | 配置 | RISC-V 64 内存布局常量 |
| `kernel/src/config/aarch64.rs` | 配置 | AArch64 内存布局常量 |
| `kernel/src/config/loongarch64.rs` | 配置 | LoongArch64 内存布局常量 |
| `kernel/src/config/x86_64.rs` | 配置 | x86_64 内存布局常量（开发中） |
| `kernel/src/pseudofs/mod.rs` | 伪文件系统 | `mount_all()`，文件系统编排器 |
| `kernel/src/pseudofs/device.rs` | 伪文件系统 | `DeviceOps` trait，`Device` 结构体 |
| `kernel/src/pseudofs/dev/mod.rs` | 设备 | `new_devfs()`，设备注册入口 |
| `kernel/src/pseudofs/dev/tty/` | TTY | TTY 子系统（ntty, ptm, pts, pty） |
| `kernel/src/file/mod.rs` | 文件 I/O | `FileLike` trait，`FD_TABLE`，`add_stdio()` |
| `kernel/src/file/fs.rs` | 文件 I/O | VFS 文件封装 |
| `kernel/src/file/pipe.rs` | 文件 I/O | 异步管道 |
| `kernel/src/file/event.rs` | 文件 I/O | 异步 eventfd |
| `kernel/src/file/epoll.rs` | 文件 I/O | epoll fd |
| `kernel/src/task/mod.rs` | 任务 | `Thread`，`ProcessData`，`AsThread` trait |
| `kernel/src/task/ops.rs` | 任务 | `new_user_task`，`spawn_alarm_task`，`add_task_to_table` |
| `kernel/src/task/user.rs` | 任务 | 用户任务创建与管理 |
| `kernel/src/task/timer.rs` | 任务 | 闹钟定时器任务 |
| `kernel/src/task/signal.rs` | 任务 | 信号投递 |
| `kernel/src/task/futex.rs` | 任务 | Futex 实现 |
| `kernel/src/task/resources.rs` | 任务 | 资源限制（Rlimits） |
| `kernel/src/task/stat.rs` | 任务 | 任务统计 |
| `kernel/src/syscall/mod.rs` | 系统调用 | 主分发入口 |
| `kernel/src/syscall/fs/` | 系统调用 | 文件 I/O 系统调用 |
| `kernel/src/syscall/task/` | 系统调用 | clone/execve/exit/schedule |
| `kernel/src/syscall/mm/` | 系统调用 | brk/mmap/mprotect |
| `kernel/src/mm/mod.rs` | 内存管理 | 内存管理入口 |
| `kernel/src/mm/aspace/` | 内存管理 | 地址空间类型（cow, file, linear, shared） |
| `Cargo.toml` | 构建 | Workspace 根配置 |
| `Makefile` | 构建 | 构建流程定义 |

