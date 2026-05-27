> ⚠️ 此文档为早期分析，部分内容已过时。
> 最新决策参见 architecture.md ADR-013~ADR-015。

# Pseudo Filesystem & Device Registration

> Part of StarryOS codebase analysis (branch: `feat/uart-async`)
> See also: [boot-init.md](boot-init.md) | [tty-console-stack.md](tty-console-stack.md) | [async-io-framework.md](async-io-framework.md)

---

## 1. Architecture Overview

```
/kernel/src/pseudofs/
├── mod.rs       — mount_all(), mount_at(), DirMaker type alias
├── device.rs    — DeviceOps trait, Device struct, DeviceMmap enum
├── dir.rs       — SimpleDir, DirMapping, DirNode
├── file.rs      — SimpleFile (generic file for proc/sys)
├── fs.rs        — SimpleFs (the filesystem driver)
├── tmp.rs       — MemoryFs (in-memory tmpfs)
├── proc.rs      — procfs (/proc)
└── dev/         — Device directory (/dev implementations)
    ├── mod.rs   — new_devfs(), builder() — all static device registration
    ├── tty/     — TTY subsystem (ntty, ptm, pts, pty, terminal/)
    ├── rtc.rs   — RTC0 device
    ├── fb.rs    — Framebuffer device
    ├── loop.rs  — Loop block devices (16)
    ├── event.rs — Input event devices
    ├── log.rs   — dev-log Unix socket
    └── memtrack.rs — Memory tracking debug device
```

## 2. DeviceOps Trait (Core Abstraction)

```rust
pub trait DeviceOps: Send + Sync {
    /// Required:
    fn read_at(&self, buf: &mut [u8], offset: u64) -> VfsResult<usize>;
    fn write_at(&self, buf: &[u8], offset: u64) -> VfsResult<usize>;
    fn as_any(&self) -> &dyn Any;

    /// Optional (with defaults):
    fn ioctl(&self, _cmd: u32, _arg: usize) -> VfsResult<usize> { Err(VfsError::NotATty) }
    fn as_pollable(&self) -> Option<&dyn Pollable> { None }  // ← key for async support
    fn mmap(&self) -> DeviceMmap { DeviceMmap::None }
    fn flags(&self) -> NodeFlags { NodeFlags::empty() }
}
```

Required: `read_at`, `write_at`, `as_any`. Everything else has sensible defaults.

## 3. Device Struct (VFS Wrapper)

```rust
pub struct Device {
    node: SimpleFsNode,    // metadata: inode, mode, size, rdev, times
    ops: Arc<dyn DeviceOps>,
}

impl Device {
    pub fn new(fs: Arc<SimpleFs>, node_type: NodeType, device_id: DeviceId, ops: Arc<dyn DeviceOps>) -> Arc<Self>;
}
```

### Trait Implementations

```
Device
  ├── impl NodeOps (via inherit_methods macro)
  │   ├── inode()      → self.node.inode
  │   ├── metadata()   → self.node.metadata
  │   ├── flags()      → self.ops.flags()
  │   └── ...
  │
  ├── impl FileNodeOps  (delegates to ops)
  │   ├── read_at()    → self.ops.read_at(buf, offset)
  │   ├── write_at()   → self.ops.write_at(buf, offset)
  │   ├── ioctl()      → self.ops.ioctl(cmd, arg)
  │   ├── append()     → Err(NotATty)
  │   └── set_len()    → probe write_at
  │
  └── impl Pollable    (delegates to ops)
      ├── poll()       → self.ops.as_pollable().poll() or IoEvents::IN|OUT
      └── register()   → self.ops.as_pollable().register()
```

## 4. VFS Integration Chain

```
DeviceOps trait
    │ impl DeviceOps for MyDevice
    ▼
Arc<dyn DeviceOps>
    │ wrap in Device::new(fs, type, id, ops)
    ▼
Arc<Device>
    │ Device impl FileNodeOps + NodeOps + Pollable
    ▼
Arc<dyn FileNodeOps>  →  NodeOpsMux::File(device)
    │ lookup by name in SimpleDir
    ▼
FileNode::new(device)  →  Location  →  File (axfs)
    │ userspace: open("/dev/mydev")
    ▼
File (kernel/src/file/fs.rs)  →  FileLike  →  FD_TABLE
```

Key insight: once a device implements `DeviceOps`, the entire VFS→FD chain is automatic.

## 5. Device Registration (Static — via builder)

All static devices are registered in `pseudofs/dev/mod.rs:builder()`:

```rust
fn builder(fs: Arc<SimpleFs>) -> DirMaker {
    let mut root = DirMapping::new();
    root.add("null",    Device::new(fs.clone(), Char, (1,3), Arc::new(Null)));
    root.add("zero",    Device::new(fs.clone(), Char, (1,5), Arc::new(Zero)));
    root.add("full",    Device::new(fs.clone(), Char, (1,7), Arc::new(Full)));
    root.add("random",  Device::new(fs.clone(), Char, (1,8), Arc::new(Random::new())));
    root.add("urandom", Device::new(fs.clone(), Char, (1,9), Arc::new(Random::new())));
    root.add("rtc0",    Device::new(fs.clone(), Char, rtc::RTC0_DEVICE_ID, Arc::new(rtc::Rtc)));
    // ... conditional: fb0, tty, console, ptmx, pts, loop*, input, ...
    root.add("cpu_dma_latency", Device::new(..., Char, (10,1024), Arc::new(CpuDmaLatency)));
    SimpleDir::new_maker(fs, Arc::new(root))
}
```

### Full Device Table

| Path | Type | Major,Minor | Backend | File |
|------|------|-------------|---------|------|
| `/dev/null` | Char | 1,3 | `Null` | dev/mod.rs |
| `/dev/zero` | Char | 1,5 | `Zero` | dev/mod.rs |
| `/dev/full` | Char | 1,7 | `Full` | dev/mod.rs |
| `/dev/random` | Char | 1,8 | `Random` (SmallRng) | dev/mod.rs |
| `/dev/urandom` | Char | 1,9 | `Random` (SmallRng) | dev/mod.rs |
| `/dev/rtc0` | Char | 250,0 | `Rtc` | dev/rtc.rs |
| `/dev/fb0` | Char | 29,0 | `FrameBuffer` (cond: display) | dev/fb.rs |
| `/dev/tty` | Char | 5,0 | `CurrentTty` (CTTY) | dev/tty/mod.rs |
| `/dev/console` | Char | 5,1 | `N_TTY` (lazy_static) | dev/tty/ntty.rs |
| `/dev/ptmx` | Char | 5,2 | `Ptmx(fs)` — PTY factory | dev/tty/ptm.rs |
| `/dev/pts` | Dir | — | `PtsDir` — dynamic PTY slaves | dev/tty/pts.rs |
| `/dev/loop0-15` | Block | 7,0 | `LoopDevice` (16 instances) | dev/loop.rs |
| `/dev/cpu_dma_latency` | Char | 10,1024 | `CpuDmaLatency` | dev/mod.rs |
| `/dev/shm` | Dir | — | DirMapping (remounted as tmpfs) | dev/mod.rs |
| `/dev/log` | Socket | — | SimpleFile (cond: dev-log) | dev/log.rs |
| `/dev/memtrack` | Char | 114,514 | `MemTrack` (cond: memtrack) | dev/memtrack.rs |
| `/dev/input/eventN` | Char | 13,N | `EventDev` (cond: input) | dev/event.rs |

## 6. Dynamic Device Registration (PTY Pattern)

For runtime-created devices (like PTY slaves), the system uses a different pattern:

```
open("/dev/ptmx")
  → Ptmx::create_pty()
      → pty::create_pty_pair() generates (master, slave) Tty pair
      → Device::new(fs, Char, id, Arc::new(master))  — returned as fd
      → pts::add_slave(fs, slave)                    — added to PTS_TABLE
          → /dev/pts/N lookupable via PtsDir
```

Key components:
- `Ptmx` holds `Arc<SimpleFs>` to create new `Device` instances
- `PTS_TABLE` is a `FlattenObjects<Arc<Device>, 16>` slab allocator (max 16 PTYs)
- `PtsDir` implements `DirNodeOps` and resolves names from the table

## 7. DeviceMmap — Memory Mapping Strategy

```rust
pub enum DeviceMmap {
    None,                              // Not mappable (default)
    Physical(PhysAddrRange),           // Maps to hardware (fb0)
    ReadOnly,                          // CoW mapping
    Cache(CachedFile),                 // Maps to file cache (loop)
}
```

Used by:
- `FrameBuffer` → `DeviceMmap::Physical(virt_to_phys(self.base), self.size)`
- `LoopDevice` → `DeviceMmap::Cache(cached_file)` (backed by image file)

## 8. Helper Types

### DirMapping

A `BTreeMap<String, NodeOpsMux>` that implements `DirNodeOps`:

```rust
pub struct DirMapping(BTreeMap<&'static str, NodeOpsMux>);

impl DirMapping {
    pub fn add(&mut self, name: &'static str, ops: impl Into<NodeOpsMux>);
}
```

### DirMaker

```rust
pub type DirMaker = Arc<dyn Fn(WeakDirEntry) -> Arc<dyn DirNodeOps> + Send + Sync>;
```

Used for lazy directory creation. `SimpleDir::new_maker(fs, maker)` wraps a closure.

### NodeOpsMux

```rust
pub enum NodeOpsMux {
    Dir(DirMaker),                       // Subdirectory
    File(Arc<dyn FileNodeOps>),         // File or device
}
```

## 9. How to Add a New Device

To add a new device (e.g., `/dev/ttyS0`):

1. **Implement `DeviceOps`** for your device struct:
   ```rust
   struct MyDevice;
   impl DeviceOps for MyDevice {
       fn read_at(&self, buf: &mut [u8], _offset: u64) -> VfsResult<usize> { ... }
       fn write_at(&self, buf: &[u8], _offset: u64) -> VfsResult<usize> { ... }
       fn as_any(&self) -> &dyn Any { self }
       fn as_pollable(&self) -> Option<&dyn Pollable> { Some(self) }  // if async
       fn flags(&self) -> NodeFlags { NodeFlags::NON_CACHEABLE | NodeFlags::STREAM }
   }
   ```

2. **Register in `builder()`** function in `dev/mod.rs`:
   ```rust
   root.add("ttyS0", Device::new(fs.clone(), NodeType::CharacterDevice,
       DeviceId::new(4, 64), Arc::new(MyDevice::new())));
   ```

3. **Implement `Pollable`** if async I/O is needed:
   ```rust
   impl Pollable for MyDevice {
       fn poll(&self) -> IoEvents { ... }
       fn register(&self, context: &mut Context<'_>, events: IoEvents) { ... }
   }
   ```

## 10. Key Files

| File | Role |
|------|------|
| `kernel/src/pseudofs/device.rs` | `DeviceOps` trait, `Device` struct, `DeviceMmap` |
| `kernel/src/pseudofs/dev/mod.rs` | `new_devfs()`, `builder()` — all device entries |
| `kernel/src/pseudofs/mod.rs` | `mount_all()`, `mount_at()` |
| `kernel/src/pseudofs/fs.rs` | `SimpleFs` filesystem driver |
| `kernel/src/pseudofs/dir.rs` | `SimpleDir`, `DirMapping` |
| `kernel/src/pseudofs/file.rs` | `SimpleFile` |
