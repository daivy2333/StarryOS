#[cfg(any(
    feature = "lichee-d1-userbench",
    feature = "lichee-d1-fullbench",
    feature = "lichee-d1-fullbench-command"
))]
use alloc::vec;
use alloc::{
    string::{String, ToString},
    sync::Arc,
};

// ── D1 user/fullbench imports (full set for user processes) ──────────
#[cfg(any(
    feature = "lichee-d1-userbench",
    feature = "lichee-d1-fullbench",
    feature = "lichee-d1-fullbench-command"
))]
use {
    crate::{
        drivers::ASYNC_TTY,
        file::FD_TABLE,
        mm::{copy_from_kernel, new_user_aspace_empty},
        pseudofs,
        task::{ProcessData, Thread, add_task_to_table, new_user_task, spawn_alarm_task},
    },
    axfs::FS_CONTEXT,
    axfs_ng_vfs::NodePermission,
    axhal::uspace::UserContext,
    axsync::Mutex,
    axtask::{AxTaskExt, spawn_task},
    starry_process::{Pid, Process},
};
// ── K3 stdio userland imports (MS17 Iteration 002) ───────────────────
#[cfg(all(target_arch = "riscv64", feature = "k3-stdio"))]
use {
    crate::{
        drivers::ASYNC_TTY,
        file::FD_TABLE,
        mm::{copy_from_kernel, load_embedded_elf_into_aspace, new_user_aspace_empty},
        pseudofs,
        task::{ProcessData, Thread, add_task_to_table, new_user_task, spawn_alarm_task},
    },
    axhal::uspace::UserContext,
    axsync::Mutex,
    axtask::{AxTaskExt, spawn_task},
    starry_process::{Pid, Process},
};
// ── K3 RAM-rootfs ordinary shell imports (MS18 Iteration 001) ─────────
#[cfg(all(target_arch = "riscv64", feature = "k3-rootfs"))]
use {
    crate::{
        drivers::ASYNC_TTY,
        file::FD_TABLE,
        mm::{copy_from_kernel, load_user_app, new_user_aspace_empty},
        pseudofs,
        task::{ProcessData, Thread, add_task_to_table, new_user_task, spawn_alarm_task},
    },
    axfs::FS_CONTEXT,
    axhal::uspace::UserContext,
    axsync::Mutex,
    axtask::{AxTaskExt, spawn_task},
    starry_process::{Pid, Process},
};
// ── QEMU imports (no D1 and no K3 feature) ───────────────────────────
#[cfg(not(any(feature = "lichee-d1", feature = "k3")))]
use {
    crate::{
        drivers::{ASYNC_TTY, uart_init},
        file::FD_TABLE,
        mm::{copy_from_kernel, load_user_app, new_user_aspace_empty},
        pseudofs,
        task::{ProcessData, Thread, add_task_to_table, new_user_task, spawn_alarm_task},
    },
    axfs::FS_CONTEXT,
    axhal::uspace::UserContext,
    axsync::Mutex,
    axtask::{AxTaskExt, spawn_task},
    starry_process::{Pid, Process},
};

#[cfg(feature = "lichee-d1-async-uart")]
use crate::drivers::bench;
// ── D1 benchmark imports (kbench and userbench) ──────────────────────
#[cfg(feature = "lichee-d1-async-uart")]
use crate::drivers::uart_init;
// ── K3 data-plane imports (MS17 Iteration 001) ───────────────────────
#[cfg(all(target_arch = "riscv64", feature = "k3-uart"))]
use crate::drivers::uart_init;
#[cfg(feature = "lichee-d1-userbench")]
use crate::mm::load_embedded_user_app;
#[cfg(any(
    feature = "lichee-d1-fullbench",
    feature = "lichee-d1-fullbench-command"
))]
use crate::mm::load_user_app_eager_from_path;

/// Initialize and run initproc.
pub fn init(args: &[String], envs: &[String]) {
    // ── D1 smoke mode (Q19A regression target) ───────────────────────
    #[cfg(all(
        target_arch = "riscv64",
        feature = "lichee-d1",
        not(feature = "lichee-d1-async-uart")
    ))]
    {
        let _ = (args, envs);
        // SAFETY: D1 platform MMIO is identity-mapped by OpenSBI; UART0 clock
        // and pins are configured by U-Boot.
        unsafe {
            crate::platform::smoke::run_lichee_d1_smoke();
        }
    }

    // ── D1 benchmark mode (kbench or userbench) ──────────────────────
    #[cfg(all(target_arch = "riscv64", feature = "lichee-d1-async-uart"))]
    {
        lichee_d1_init(args, envs);
    }

    // ── K3 async UART data-plane mode (MS17 Iteration 001): port + copiers + rings ──
    // The qualification payload owns the sole raw reader/writer of this
    // boot; ASYNC_TTY is never constructed here (single-consumer argument,
    // see ntty_async). k3-stdio selects its own boot path instead, and
    // k3-rootfs (MS18) mounts the ext4 root over its own branch.
    #[cfg(all(
        target_arch = "riscv64",
        feature = "k3-async-uart",
        feature = "k3-uart",
        not(any(feature = "k3-stdio", feature = "k3-rootfs"))
    ))]
    {
        let _ = (args, envs);
        uart_init::init_uart_hardware();
        // SAFETY: this boot path runs once; no other ring producer (no
        // benchmark in this mode) ran before the copiers start.
        unsafe { uart_init::start_copiers() };
        crate::platform::k3::run_uart_data_plane();
    }

    // ── K3 stdio userland (MS17 Iteration 002): tty + embedded probe ──
    // Boot order: copiers start first, then the memory root mounts (the
    // devfs console node construction is the ASYNC_TTY construction point,
    // making the tty-reader the sole RX consumer), then the embedded probe
    // loads and spawns with stdio. Everything fallible happens before the
    // process commit point (`Process::new_init`).
    #[cfg(all(target_arch = "riscv64", feature = "k3-stdio"))]
    {
        use alloc::format;
        use core::time::Duration;

        use axtask::future::{block_on, timeout};

        use crate::{drivers::k3_uart, file::publish_stdio};

        let _ = (args, envs);
        let diag = |line: &str| k3_uart::diag_line(line);

        uart_init::init_uart_hardware();
        // SAFETY: this boot path runs once; no other ring producer (no
        // benchmark in this mode) ran before the copiers start.
        unsafe { uart_init::start_copiers() };

        pseudofs::init_memory_root();
        pseudofs::mount_all().expect("Failed to mount pseudofs");
        spawn_alarm_task();

        // Embedded payload selection: the k3-shell shape embeds the
        // minimal interactive shell; plain k3-stdio keeps the
        // Iteration 002 probe. cfg-stripped arms never touch the other
        // ELF file, so each shape builds without the sibling artifact.
        #[cfg(feature = "k3-shell")]
        let payload_name = "k3-shell";
        #[cfg(not(feature = "k3-shell"))]
        let payload_name = "k3_stdio_probe";

        diag(&format!("[ms17] mode={payload_name} begin tty=live memory-root=live irq=42"));

        // Recoverable phase (before the commit point): an open or load
        // failure drops its objects and nothing is published.
        let stdio_files = crate::file::open_stdio_files()
            .unwrap_or_else(|e| panic!("[ms17] stdio fault stage=stdio-open err={e:?}"));

        #[repr(align(8))]
        struct AlignedBytes<const N: usize>([u8; N]);

        #[cfg(feature = "k3-shell")]
        const PAYLOAD_ELF_LEN: usize = include_bytes!("../resources/k3-shell.elf").len();
        #[cfg(not(feature = "k3-shell"))]
        const PAYLOAD_ELF_LEN: usize = include_bytes!("../resources/k3_stdio_probe.elf").len();
        #[cfg(feature = "k3-shell")]
        static EMBEDDED_PAYLOAD: AlignedBytes<PAYLOAD_ELF_LEN> =
            AlignedBytes(*include_bytes!("../resources/k3-shell.elf"));
        #[cfg(not(feature = "k3-shell"))]
        static EMBEDDED_PAYLOAD: AlignedBytes<PAYLOAD_ELF_LEN> =
            AlignedBytes(*include_bytes!("../resources/k3_stdio_probe.elf"));

        let mut uspace = new_user_aspace_empty()
            .and_then(|mut it| {
                copy_from_kernel(&mut it)?;
                Ok(it)
            })
            .expect("Failed to create user address space");

        let payload_args = alloc::vec![String::from(payload_name)];
        let (entry_vaddr, ustack_top) =
            load_embedded_elf_into_aspace(&mut uspace, &EMBEDDED_PAYLOAD.0, &payload_args, &[])
                .unwrap_or_else(|e| panic!("[ms17] stdio fault stage=load err={e:?}"));

        // Commit point: from here on the process exists; invariant failures
        // terminate to reset (no rollback of new_init is attempted).
        let uctx = UserContext::new(entry_vaddr.into(), ustack_top, 0);
        let mut task = new_user_task(payload_name, uctx, 0);
        task.ctx_mut().set_page_table_root(uspace.page_table_root());

        let pid = task.id().as_u64() as Pid;
        let proc = Process::new_init(pid);
        proc.add_thread(pid);
        ASYNC_TTY.bind_to(&proc).expect("Failed to bind async tty");

        let proc = ProcessData::new(
            proc,
            String::from(payload_name),
            Arc::new(payload_args),
            Arc::new(Mutex::new(uspace)),
            Arc::default(),
            None,
        );

        {
            let mut scope = proc.scope.write();
            let mut table_scope = FD_TABLE.scope_mut(&mut scope);
            let mut table = table_scope.write();
            let files = stdio_files;
            publish_stdio(&mut *table, &mut |read| {
                if read {
                    Ok(files.input.clone())
                } else {
                    Ok(files.output.clone())
                }
            })
            .expect("Failed to publish stdio");
        }

        let thr = Thread::new(pid, proc);
        *task.task_ext_mut() = Some(AxTaskExt::from_impl(thr));

        let task = spawn_task(task);
        add_task_to_table(&task);

        let exit_code = task.join();
        diag(&format!("[ms17] stdio stage=exit join={exit_code:?}"));

        // Physical drain: the four TxCompletion conditions; on K3 the final
        // TEMT wait rides the native 1 ms timer hook.
        match block_on(timeout(
            Some(Duration::from_secs(30)),
            uart_init::wait_tx_drain(&uart_init::driver()),
        )) {
            Ok(()) => diag("[ms17] stdio stage=drain done (TxCompletion four conditions)"),
            Err(_) => diag("[ms17] stdio fault stage=drain deadline=30s"),
        }
        loop {
            riscv::asm::wfi();
        }
    }

    // ── K3 RAM-rootfs (MS18 Iteration 001): ordinary dynamic-ELF shell ──
    // The accepted UART/copier base starts, then the bootargs `initrdmem`
    // extent is validated against the reserved carrying window and the
    // unique static-backed RamDisk is mounted as the ext4 root. From there
    // the process assembly mirrors the QEMU branch line by line: pseudofs
    // mounts on top (the devfs console node construction is the ASYNC_TTY
    // construction point, making the tty-reader the sole RX consumer), the
    // ordinary loader spawns the dynamic /bin/sh from the rootfs, and the
    // run ends with the 30 s physical drain and parks. Range (bootargs),
    // FS (mount), resolve, load and stdio failures panic at their layer;
    // no fallback.
    #[cfg(all(target_arch = "riscv64", feature = "k3-rootfs"))]
    {
        use core::ptr::NonNull;
        use core::time::Duration;

        use alloc::boxed::Box;
        use alloc::format;

        use axtask::future::{block_on, timeout};
        use axdriver::{AxBlockDevice, AxDeviceContainer};
        use axdriver_block::ramdisk_static::RamDisk;

        use crate::drivers::k3_uart;

        use axplat_riscv64_k3::ram_layout;

        let diag = |line: &str| k3_uart::diag_line(line);

        uart_init::init_uart_hardware();
        // SAFETY: this boot path runs once; no other ring producer (no
        // benchmark in this mode) ran before the copiers start.
        unsafe { uart_init::start_copiers() };

        diag("[ms18] mode=k3-rootfs stage=begin uart=live irq=42 smp=1");

        // DTB handoff: validate the pointer first, then the header, and
        // only then let fdt-parser consume the validated blob. Bootargs are
        // read as raw bytes (no str() unwrap on unvalidated data).
        let dtb_paddr = axhal::dtb::get_bootarg();
        ram_layout::validate_dtb_pointer(dtb_paddr).unwrap_or_else(|e| {
            panic!("[ms18] fault stage=dtb-pointer paddr={dtb_paddr:#x} err={e:?}")
        });
        let dtb_vaddr = axhal::mem::phys_to_virt(dtb_paddr.into());
        // SAFETY: the pointer passed validate_dtb_pointer, so the full
        // header stays inside the reserved 1 MiB DTB window; the window is
        // board RAM mapped by the full paging stage.
        let mut header = [0u8; ram_layout::FDT_HEADER_SIZE];
        header.copy_from_slice(unsafe {
            core::slice::from_raw_parts(
                dtb_vaddr.as_usize() as *const u8,
                ram_layout::FDT_HEADER_SIZE,
            )
        });
        let totalsize =
            ram_layout::validate_dtb_header(dtb_paddr, &header).unwrap_or_else(|e| {
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

        let (base, len) = ram_layout::parse_initrdmem(bootargs)
            .and_then(|(b, l)| ram_layout::validate_rootfs_extent(b, l))
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
        spawn_alarm_task();

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

        let task = spawn_task(task);
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
        match block_on(timeout(
            Some(Duration::from_secs(30)),
            uart_init::wait_tx_drain(&uart_init::driver()),
        )) {
            Ok(()) => diag("[ms18] rootfs stage=drain done (TxCompletion four conditions)"),
            Err(_) => diag("[ms18] fault stage=drain deadline=30s"),
        }
        loop {
            riscv::asm::wfi();
        }
    }

    // ── K3 async UART raw mode (MS17 Iteration 000): timer proof + raw PXA ──
    #[cfg(all(
        target_arch = "riscv64",
        feature = "k3-async-uart",
        not(feature = "k3-uart")
    ))]
    {
        let _ = (args, envs);
        // SAFETY: K3 UART0 MMIO is identity-mapped via the platform
        // mmio-ranges and left in the bootloader handoff state (MS09
        // Iteration 001 adjudication); this path runs once at boot and
        // never enters the GMAC/PHY polling stages.
        crate::platform::k3::run_uart_port_bringup();
    }

    // ── K3 minimal first-byte path (MS09 Iteration 002) ──────────────
    #[cfg(all(
        target_arch = "riscv64",
        feature = "k3",
        not(feature = "k3-async-uart")
    ))]
    {
        let _ = (args, envs);
        // SAFETY: K3 UART0 MMIO is identity-mapped via the platform
        // mmio-ranges and left in the bootloader handoff state (MS09
        // Iteration 001 adjudication); this path runs once at boot.
        unsafe {
            crate::platform::k3::run_first_byte_park();
        }
    }

    // ── QEMU mode ────────────────────────────────────────────────────
    #[cfg(not(any(feature = "lichee-d1", feature = "k3")))]
    {
        // Initialize UART hardware + async driver (MMIO, ring buffers, ISR, copiers)
        uart_init::init_uart_hardware();
        ax_println!("[kernel] Async UART driver initialized");

        // Initialize VirtIO-net IRQ diagnostic control plane (MS03).
        // Validates the net device, registers IRQ 7 handler, keeps
        // MS02 polling fallback active.
        crate::drivers::virtio_net_irq::init_virtio_net_irq_diag();

        // QEMU+SMP bounded network placement smoke (Task 3.5): after
        // secondary-ready the pinned owner/runner roles have (or fail to) poll
        // on distinct schedulable harts. QEMU-only; no data-plane claim.
        #[cfg(feature = "qemu")]
        crate::drivers::net_placement::snapshot_boot_smoke();

        // QEMU+SMP bounded network role migration smoke (Task 4.3): widen and
        // restore the pinned owner/runner through the safe mask wrapper with
        // direct poll observations on both allowed harts. QEMU-only; no
        // data-plane claim.
        #[cfg(feature = "qemu")]
        crate::drivers::net_placement::migration_boot_smoke();

        // Run kernel-side benchmark (ring buffer throughput/latency, memory, NAPI, IRQ)
        crate::drivers::bench::run_startup_benchmark();

        // Start async UART copier tasks (must run after benchmark to avoid
        // SPSC producer conflict on RX/TX ring buffers)
        // SAFETY: This boot path runs once, after the startup benchmark, and
        // the mutually exclusive D1 path cannot start the same driver tasks.
        unsafe { uart_init::start_copiers() };

        // QEMU+SMP bounded startup smoke: wait until both copiers polled, then
        // read and validate the UART SMP snapshot (Task 2.6). QEMU-only; no data
        // plane, no guest protocol.
        #[cfg(feature = "qemu")]
        crate::drivers::uart_smp_snapshot::snapshot_boot_smoke();

        // QEMU+SMP bounded controlled-migration smoke (Task 4.2): widen and
        // restore each copier through the safe mask wrapper, observing direct
        // polls on both allowed harts. QEMU-only; no data-plane claim.
        #[cfg(feature = "qemu")]
        crate::drivers::uart_smp_snapshot::migration_boot_smoke();

        pseudofs::mount_all().expect("Failed to mount pseudofs");
        spawn_alarm_task();

        let loc = FS_CONTEXT
            .lock()
            .resolve(&args[0])
            .expect("Failed to resolve executable path");
        let path = loc
            .absolute_path()
            .expect("Failed to get executable absolute path");
        let name = loc.name();

        let mut uspace = new_user_aspace_empty()
            .and_then(|mut it| {
                copy_from_kernel(&mut it)?;
                Ok(it)
            })
            .expect("Failed to create user address space");

        let (entry_vaddr, ustack_top) = load_user_app(&mut uspace, None, args, envs)
            .unwrap_or_else(|e| panic!("Failed to load user app: {}", e));

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
                .expect("Failed to add stdio");
        }

        let thr = Thread::new(pid, proc);
        *task.task_ext_mut() = Some(AxTaskExt::from_impl(thr));

        let task = spawn_task(task);
        add_task_to_table(&task);

        // TODO: wait for all processes to finish
        let exit_code = task.join();
        info!("Init process exited with code: {exit_code:?}");

        let cx = FS_CONTEXT.lock();
        cx.root_dir()
            .unmount_all()
            .expect("Failed to unmount all filesystems");
        cx.root_dir()
            .filesystem()
            .flush()
            .expect("Failed to flush rootfs");
    }
}

// ── D1 benchmark init (kbench + userbench) ───────────────────────────

#[cfg(feature = "lichee-d1-async-uart")]
fn lichee_d1_init(args: &[String], envs: &[String]) {
    let _ = (args, envs);

    #[cfg(feature = "lichee-d1-fullbench")]
    ax_println!("[starry-d1] Lichee D1 fullbench memory-root path mode");
    #[cfg(feature = "lichee-d1-fullbench-command")]
    ax_println!("[starry-d1] Lichee D1 fullbench command-entry mode");
    #[cfg(all(
        feature = "lichee-d1-userbench",
        not(any(
            feature = "lichee-d1-fullbench",
            feature = "lichee-d1-fullbench-command"
        ))
    ))]
    ax_println!("[starry-d1] Lichee D1 userbench mode");
    #[cfg(not(any(
        feature = "lichee-d1-userbench",
        feature = "lichee-d1-fullbench",
        feature = "lichee-d1-fullbench-command"
    )))]
    ax_println!("[starry-d1] Lichee D1 kbench mode");

    // Phase 4: Initialize async UART hardware (D1 32-bit MMIO path)
    uart_init::init_uart_hardware();
    ax_println!("[kernel] Async UART driver initialized (D1)");

    // Phase 4: Run kernel ring buffer benchmark
    bench::run_startup_benchmark();

    // Start async UART copier tasks (must run after benchmark to avoid
    // SPSC producer conflict on RX/TX ring buffers)
    // SAFETY: This boot path runs once, after the startup benchmark, and
    // the mutually exclusive QEMU path cannot start the same driver tasks.
    unsafe { uart_init::start_copiers() };

    // Phase 5-6: Userbench path (mount devfs, load user benchmark payload)
    #[cfg(all(
        feature = "lichee-d1-userbench",
        not(any(
            feature = "lichee-d1-fullbench",
            feature = "lichee-d1-fullbench-command"
        ))
    ))]
    {
        ax_println!("[starry-d1] Initializing memory rootfs...");
        pseudofs::init_memory_root();
        pseudofs::mount_all().expect("Failed to mount pseudofs");
        spawn_alarm_task();

        ax_println!("[starry-d1] Loading embedded benchmark payload...");

        let mut uspace = new_user_aspace_empty()
            .and_then(|mut it| {
                copy_from_kernel(&mut it)?;
                Ok(it)
            })
            .expect("Failed to create user address space");

        #[repr(align(8))]
        struct AlignedBytes<const N: usize>([u8; N]);

        const BENCHMARK_ELF_LEN: usize = include_bytes!("../resources/benchmark.elf").len();
        static EMBEDDED_BENCHMARK: AlignedBytes<BENCHMARK_ELF_LEN> =
            AlignedBytes(*include_bytes!("../resources/benchmark.elf"));

        let bench_args = vec![String::from("benchmark")];

        let (entry_vaddr, ustack_top) =
            load_embedded_user_app(&mut uspace, &EMBEDDED_BENCHMARK.0, &bench_args, &[])
                .expect("Failed to load embedded benchmark");

        let uctx = UserContext::new(entry_vaddr.into(), ustack_top, 0);
        let mut task = new_user_task("benchmark", uctx, 0);
        task.ctx_mut().set_page_table_root(uspace.page_table_root());

        let pid = task.id().as_u64() as Pid;
        let proc = Process::new_init(pid);
        proc.add_thread(pid);
        ASYNC_TTY.bind_to(&proc).expect("Failed to bind async tty");

        let proc = ProcessData::new(
            proc,
            String::from("benchmark"),
            Arc::new(bench_args),
            Arc::new(Mutex::new(uspace)),
            Arc::default(),
            None,
        );

        {
            let mut scope = proc.scope.write();
            crate::file::add_stdio(&mut FD_TABLE.scope_mut(&mut scope).write())
                .expect("Failed to add stdio");
        }

        let thr = Thread::new(pid, proc);
        *task.task_ext_mut() = Some(AxTaskExt::from_impl(thr));

        let task = spawn_task(task);
        add_task_to_table(&task);

        ax_println!("[starry-d1] benchmark process spawned, waiting...");
        let exit_code = task.join();
        ax_println!("[starry-d1] benchmark exited with code: {:?}", exit_code);
        ax_println!("[starry-d1] halting.");
        loop {
            riscv::asm::wfi();
        }
    }

    // ── Q19C-M1: memory-root path loader fullbench ───────────────────
    #[cfg(feature = "lichee-d1-fullbench")]
    {
        const BENCH_PATH: &str = "/bin/benchmark";

        ax_println!("[starry-d1] log_label=lichee-memory-root-path");
        ax_println!("[starry-d1] target_mode=lichee-d1-fullbench");
        ax_println!(
            "[starry-d1] startup_chain=android-boot-image -> memory-root /bin/benchmark -> \
             eager_elf_mapping"
        );
        ax_println!("[starry-d1] root_provider=d1-memory-root-path");
        ax_println!("[starry-d1] Initializing populated memory rootfs...");
        pseudofs::init_memory_root();

        {
            let fs = FS_CONTEXT.lock();
            if fs.resolve("/bin").is_err() {
                fs.create_dir("/bin", NodePermission::from_bits_truncate(0o755))
                    .expect("Failed to create /bin in memory root");
            }
            fs.write(BENCH_PATH, include_bytes!("../resources/benchmark.elf"))
                .expect("Failed to populate /bin/benchmark");
            match fs.resolve(BENCH_PATH) {
                Ok(_) => ax_println!(
                    "[starry-d1] root_provider=d1-memory-root-path requested_path={} resolved=true",
                    BENCH_PATH
                ),
                Err(err) => panic!(
                    "[starry-d1] path-loader resolve failed: requested_path={} error={:?}",
                    BENCH_PATH, err
                ),
            }
        }

        pseudofs::mount_all().expect("Failed to mount pseudofs");
        spawn_alarm_task();

        let bench_args = vec![String::from(BENCH_PATH)];
        let envs = vec![];
        let loc = match FS_CONTEXT.lock().resolve(BENCH_PATH) {
            Ok(loc) => loc,
            Err(err) => panic!(
                "[starry-d1] path-loader resolve failed after mount_all: requested_path={} \
                 error={:?}",
                BENCH_PATH, err
            ),
        };
        let path = loc
            .absolute_path()
            .expect("Failed to get executable absolute path");
        let name = loc.name();

        ax_println!("[starry-d1] Loading /bin/benchmark via path eager loader...");

        let mut uspace = new_user_aspace_empty()
            .and_then(|mut it| {
                copy_from_kernel(&mut it)?;
                Ok(it)
            })
            .expect("Failed to create user address space");

        let (entry_vaddr, ustack_top) =
            load_user_app_eager_from_path(&mut uspace, BENCH_PATH, &bench_args, &envs)
                .expect("Failed to load /bin/benchmark");

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
            Arc::new(bench_args),
            Arc::new(Mutex::new(uspace)),
            Arc::default(),
            None,
        );

        {
            let mut scope = proc.scope.write();
            crate::file::add_stdio(&mut FD_TABLE.scope_mut(&mut scope).write())
                .expect("Failed to add stdio");
        }

        let thr = Thread::new(pid, proc);
        *task.task_ext_mut() = Some(AxTaskExt::from_impl(thr));

        let task = spawn_task(task);
        add_task_to_table(&task);

        ax_println!(
            "[starry-d1] stage=loaded-process-before-first-section requested_path={} spawned=true",
            BENCH_PATH
        );
        ax_println!("[starry-d1] benchmark process spawned from /bin/benchmark, waiting...");
        let exit_code = task.join();
        ax_println!("[starry-d1] benchmark exited with code: {:?}", exit_code);
        ax_println!("[starry-d1] halting.");
        loop {
            riscv::asm::wfi();
        }
    }

    // ── Q19C-M2: command-entry fullbench ──────────────────────────────
    #[cfg(feature = "lichee-d1-fullbench-command")]
    {
        const BENCH_PATH: &str = "/bin/benchmark";
        const INIT_SH_PATH: &str = "/init.sh";

        ax_println!("[starry-d1] log_label=lichee-memory-root-command");
        ax_println!("[starry-d1] target_mode=lichee-d1-fullbench-command");
        ax_println!(
            "[starry-d1] startup_chain=android-boot-image -> memory-root /bin/benchmark -> \
             eager_elf_mapping (equivalent_command_entry)"
        );
        ax_println!("[starry-d1] root_provider=d1-memory-root-path");
        ax_println!("[starry-d1] shell_status=SKIPPED: no known-good static /bin/sh");
        ax_println!("[starry-d1] equivalent_entry={}", BENCH_PATH);
        ax_println!("[starry-d1] Initializing populated memory rootfs...");
        pseudofs::init_memory_root();

        {
            let fs = FS_CONTEXT.lock();
            if fs.resolve("/bin").is_err() {
                fs.create_dir("/bin", NodePermission::from_bits_truncate(0o755))
                    .expect("Failed to create /bin in memory root");
            }
            // Inject /bin/benchmark (same as M1)
            fs.write(BENCH_PATH, include_bytes!("../resources/benchmark.elf"))
                .expect("Failed to populate /bin/benchmark");
            // Inject /init.sh as packaging/evidence text (NOT executed without shell)
            let init_sh_content = b"#!/bin/sh\n/bin/benchmark\n";
            fs.write(INIT_SH_PATH, init_sh_content)
                .expect("Failed to populate /init.sh");
            // Verify both paths resolve
            match fs.resolve(BENCH_PATH) {
                Ok(_) => ax_println!(
                    "[starry-d1] root_provider=d1-memory-root-path requested_path={} resolved=true",
                    BENCH_PATH
                ),
                Err(err) => panic!(
                    "[starry-d1] path-loader resolve failed: requested_path={} error={:?}",
                    BENCH_PATH, err
                ),
            }
            match fs.resolve(INIT_SH_PATH) {
                Ok(_) => ax_println!(
                    "[starry-d1] evidence_path={} resolved=true (not executed, shell unavailable)",
                    INIT_SH_PATH
                ),
                Err(err) => ax_println!(
                    "[starry-d1] evidence_path={} resolved=false error={:?}",
                    INIT_SH_PATH,
                    err
                ),
            }
        }

        pseudofs::mount_all().expect("Failed to mount pseudofs");
        spawn_alarm_task();

        let bench_args = vec![
            String::from(BENCH_PATH),
            String::from("--q19c-m2-command-entry"),
        ];
        let envs = vec![];
        ax_println!(
            "[starry-d1] argv_evidence=kernel-side-construction argv={},--q19c-m2-command-entry",
            BENCH_PATH
        );
        ax_println!(
            "[starry-d1] envp_count={} (kernel-side construction)",
            envs.len()
        );
        ax_println!("[starry-d1] stdio=/dev/console");
        ax_println!(
            "[starry-d1] note=user-observed-argv-not-claimed (payload does not print argc/argv; \
             see q19c-m2-m3-acceptance-alignment §D4)"
        );

        let loc = match FS_CONTEXT.lock().resolve(BENCH_PATH) {
            Ok(loc) => loc,
            Err(err) => panic!(
                "[starry-d1] path-loader resolve failed after mount_all: requested_path={} \
                 error={:?}",
                BENCH_PATH, err
            ),
        };
        let path = loc
            .absolute_path()
            .expect("Failed to get executable absolute path");
        let name = loc.name();

        ax_println!("[starry-d1] Loading /bin/benchmark via path eager loader (command-entry)...");

        let mut uspace = new_user_aspace_empty()
            .and_then(|mut it| {
                copy_from_kernel(&mut it)?;
                Ok(it)
            })
            .expect("Failed to create user address space");

        let (entry_vaddr, ustack_top) =
            load_user_app_eager_from_path(&mut uspace, BENCH_PATH, &bench_args, &envs)
                .expect("Failed to load /bin/benchmark");

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
            Arc::new(bench_args),
            Arc::new(Mutex::new(uspace)),
            Arc::default(),
            None,
        );

        {
            let mut scope = proc.scope.write();
            crate::file::add_stdio(&mut FD_TABLE.scope_mut(&mut scope).write())
                .expect("Failed to add stdio");
        }

        let thr = Thread::new(pid, proc);
        *task.task_ext_mut() = Some(AxTaskExt::from_impl(thr));

        let task = spawn_task(task);
        add_task_to_table(&task);

        ax_println!(
            "[starry-d1] stage=loaded-process-command-entry requested_path={} spawned=true",
            BENCH_PATH
        );
        ax_println!("[starry-d1] benchmark process spawned (command-entry), waiting...");
        let exit_code = task.join();
        ax_println!("[starry-d1] benchmark exited with code: {:?}", exit_code);
        ax_println!("[starry-d1] halting.");
        loop {
            riscv::asm::wfi();
        }
    }

    // ── kbench mode: halt after kernel benchmark ─────────────────────
    #[cfg(not(any(
        feature = "lichee-d1-userbench",
        feature = "lichee-d1-fullbench",
        feature = "lichee-d1-fullbench-command"
    )))]
    {
        ax_println!("[starry-d1] kernel benchmark complete, halting.");
        loop {
            riscv::asm::wfi();
        }
    }
}
