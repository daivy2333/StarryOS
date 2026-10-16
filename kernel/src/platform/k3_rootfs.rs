//! Shared RAM-rootfs ordinary shell body (MS14 Iteration 003, task T10).
//!
//! The MS18-accepted chain — DTB handoff validation, the bootargs
//! `initrdmem` extent check against the reserved carrying window, the unique
//! static-backed RamDisk/ext4 root mount, the pseudofs mounts, the alarm
//! task, the ordinary dynamic loader (`PT_INTERP` dispatch included), the
//! initial process publication, the join and the 30 s physical drain —
//! lives here exactly once. The single-hart `k3-rootfs` entry calls it with
//! no fixed roles (the original `spawn_task`/`spawn_alarm_task` APIs); the
//! `k3-smp-combined` entry calls it after the NIC install with the shell and
//! alarm singletons committed before their first enqueue. Every fallible
//! stage before the process commit point panics at its layer; there is no
//! fallback and no persistent write.

use alloc::string::ToString;
use alloc::sync::Arc;

/// Fixed singletons for the combined entry: the initial shell user task and
/// the alarm deadline task are pinned to these harts before their first
/// enqueue. `None` (the single-hart entries) keeps the original unplaced
/// spawn APIs.
#[derive(Debug, Clone, Copy)]
pub struct FixedRootfsRoles {
    /// The initial shell task's singleton hart.
    pub shell: usize,
    /// The alarm deadline task's singleton hart.
    pub alarm: usize,
}

fn singleton(hart: usize) -> axtask::AxCpuMask {
    let mut mask = axtask::AxCpuMask::new();
    mask.set(hart, true)
        .expect("hart index within mask capacity");
    mask
}

/// Runs the ordinary RAM-rootfs shell chain from the DTB handoff to the
/// final drain, then parks. `args`/`envs` are the init process arguments
/// (`/bin/sh -c <init.sh>` shape on the rootfs images); `fixed` selects the
/// original unplaced APIs (`None`) or the combined entry's pre-enqueue
/// singletons (`Some`).
pub fn run_ram_rootfs_shell(args: &[alloc::string::String], envs: &[alloc::string::String], fixed: Option<FixedRootfsRoles>) -> ! {
    use core::ptr::NonNull;
    use core::time::Duration;

    use alloc::boxed::Box;
    use alloc::format;

    use axdriver::{AxBlockDevice, AxDeviceContainer};
    use axdriver_block::ramdisk_static::RamDisk;
    use axfs::FS_CONTEXT;
    use axhal::uspace::UserContext;
    use axsync::Mutex;
    use axtask::{AxTaskExt, spawn_task, spawn_task_with_affinity};
    use starry_process::{Pid, Process};

    use crate::drivers::{ASYNC_TTY, k3_uart};
    use crate::file::FD_TABLE;
    use crate::mm::{copy_from_kernel, load_user_app, new_user_aspace_empty};
    use crate::pseudofs;
    use crate::task::{
        ProcessData, Thread, add_task_to_table, new_user_task, spawn_alarm_task,
        spawn_alarm_task_with_affinity,
    };

    let diag = |line: &str| k3_uart::diag_line(line);

    // DTB handoff: validate the pointer first, then the header, and
    // only then let fdt-parser consume the validated blob. Bootargs are
    // read as raw bytes (no str() unwrap on unvalidated data).
    let dtb_paddr = axhal::dtb::get_bootarg();
    axplat_riscv64_k3::ram_layout::validate_dtb_pointer(dtb_paddr).unwrap_or_else(|e| {
        panic!("[ms18] fault stage=dtb-pointer paddr={dtb_paddr:#x} err={e:?}")
    });
    let dtb_vaddr = axhal::mem::phys_to_virt(dtb_paddr.into());
    // SAFETY: the pointer passed validate_dtb_pointer, so the full
    // header stays inside the reserved 1 MiB DTB window; the window is
    // board RAM mapped by the full paging stage.
    let mut header = [0u8; axplat_riscv64_k3::ram_layout::FDT_HEADER_SIZE];
    header.copy_from_slice(unsafe {
        core::slice::from_raw_parts(
            dtb_vaddr.as_usize() as *const u8,
            axplat_riscv64_k3::ram_layout::FDT_HEADER_SIZE,
        )
    });
    let totalsize = axplat_riscv64_k3::ram_layout::validate_dtb_header(dtb_paddr, &header)
        .unwrap_or_else(|e| {
            panic!("[ms18] fault stage=dtb-header paddr={dtb_paddr:#x} err={e:?}")
        });

    let fdt_vptr = NonNull::new(dtb_vaddr.as_mut_ptr())
        .expect("[ms18] fault stage=dtb-pointer reason=null-vaddr");
    // Pointer, header and totalsize are validated against the reserved
    // DTB window above; from_ptr itself re-reads through the parser.
    let fdt = fdt_parser::Fdt::from_ptr(fdt_vptr).unwrap_or_else(|e| {
        panic!("[ms18] fault stage=dtb-parse paddr={dtb_paddr:#x} err={e:?}")
    });
    // Path form ("/chosen") — fdt-parser treats a bare name as an alias
    // lookup and panics "aliase not found" when /aliases lacks it.
    let bootargs: &[u8] = fdt
        .find_nodes("/chosen")
        .next()
        .and_then(|node| node.find_property("bootargs"))
        .map(|prop| prop.raw_value())
        .unwrap_or(&[]);

    let (base, len) = axplat_riscv64_k3::ram_layout::parse_initrdmem(bootargs)
        .and_then(|(b, l)| axplat_riscv64_k3::ram_layout::validate_rootfs_extent(b, l))
        .unwrap_or_else(|e| {
            panic!(
                "[ms18] fault stage=bootargs bootargs={:?} err={e:?}",
                alloc::string::String::from_utf8_lossy(bootargs)
            )
        });
    diag(&format!(
        "[ms18] rootfs stage=extent base={base:#x} size={len:#x} dtb_paddr={dtb_paddr:#x} dtb_totalsize={totalsize}"
    ));

    let rootfs_vaddr = axhal::mem::phys_to_virt(base.into()).as_usize();
    // SAFETY: the extent passed validate_rootfs_extent — base pinned to
    // the reserved carrying window, checked end inside the window — and
    // the window is excluded from the allocator before init and mapped
    // by full paging, so the 'static backing outlives the mount.
    let backing: &'static mut [u8] =
        unsafe { core::slice::from_raw_parts_mut(rootfs_vaddr as *mut u8, len) };
    let dev: AxBlockDevice = Box::new(RamDisk::new(backing));
    axfs::init_filesystems(AxDeviceContainer::from_one(dev));
    diag("[ms18] rootfs stage=mount fs=ext4 blocks-loaded");

    // Ordinary shell chain: mirrors the QEMU branch order exactly.
    pseudofs::mount_all().expect("Failed to mount pseudofs");
    match fixed {
        None => spawn_alarm_task(),
        Some(roles) => {
            if !spawn_alarm_task_with_affinity(singleton(roles.alarm)) {
                panic!("[ms14] fault stage=alarm-spawn detail=affinity-committed alarm spawn rejected");
            }
            diag(&format!("[ms14] rootfs alarm spawned hart={}", roles.alarm));
        }
    }

    let loc = FS_CONTEXT
        .lock()
        .resolve(&args[0])
        .unwrap_or_else(|e| panic!("[ms18] fault stage=resolve path={} err={e:?}", args[0]));
    let path = loc
        .absolute_path()
        .unwrap_or_else(|e| panic!("[ms18] fault stage=resolve-path err={e:?}"));
    let name = loc.name();

    let mut uspace = new_user_aspace_empty()
        .and_then(|mut it| {
            copy_from_kernel(&mut it)?;
            Ok(it)
        })
        .expect("Failed to create user address space");

    // None path: args[0] drives the loader (shebang/PT_INTERP dispatch
    // included), so a missing interpreter or a non-ELF binary fails at
    // this layer without any fallback.
    let (entry_vaddr, ustack_top) = load_user_app(&mut uspace, None, args, envs)
        .unwrap_or_else(|e| panic!("[ms18] fault stage=load err={e:?}"));

    let uctx = UserContext::new(entry_vaddr.into(), ustack_top, 0);
    let mut task = new_user_task(name, uctx, 0);
    task.ctx_mut().set_page_table_root(uspace.page_table_root());

    let pid = task.id().as_u64() as Pid;
    let proc = Process::new_init(pid);
    proc.add_thread(pid);

    ASYNC_TTY.bind_to(&proc).expect("Failed to bind async tty");

    let proc = ProcessData::new(
        proc,
        path.to_string(),
        Arc::new(args.to_vec()),
        Arc::new(Mutex::new(uspace)),
        Arc::default(),
        None,
    );

    {
        let mut scope = proc.scope.write();
        crate::file::add_stdio(&mut FD_TABLE.scope_mut(&mut scope).write())
            .unwrap_or_else(|e| panic!("[ms18] fault stage=stdio-open err={e:?}"));
    }

    let thr = Thread::new(pid, proc);
    *task.task_ext_mut() = Some(AxTaskExt::from_impl(thr));

    let task = match fixed {
        None => spawn_task(task),
        Some(roles) => {
            let shell_hart = roles.shell;
            let spawned = spawn_task_with_affinity(task, singleton(shell_hart));
            if spawned.is_none() {
                panic!("[ms14] fault stage=shell-spawn detail=affinity-committed shell spawn rejected");
            }
            diag(&format!("[ms14] rootfs shell spawned hart={shell_hart}"));
            spawned.unwrap()
        }
    };
    add_task_to_table(&task);

    let exit_code = task.join();
    diag(&format!("[ms18] rootfs stage=exit join={exit_code:?}"));

    {
        let cx = FS_CONTEXT.lock();
        cx.root_dir()
            .unmount_all()
            .expect("Failed to unmount all filesystems");
        cx.root_dir()
            .filesystem()
            .flush()
            .expect("Failed to flush rootfs");
    }
    diag("[ms18] rootfs stage=fs-close done");

    // Physical drain: the four TxCompletion conditions; on K3 the final
    // TEMT wait rides the native 1 ms timer hook.
    let drained = axtask::future::block_on(axtask::future::timeout(
        Some(Duration::from_secs(30)),
        crate::drivers::uart_init::wait_tx_drain(&crate::drivers::uart_init::driver()),
    ));
    match drained {
        Ok(()) => diag("[ms18] rootfs stage=drain done (TxCompletion four conditions)"),
        Err(_) => diag("[ms18] fault stage=drain deadline=30s"),
    }
    loop {
        riscv::asm::wfi();
    }
}
