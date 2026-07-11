use alloc::{
    string::{String, ToString},
    sync::Arc,
    vec,
};

// ── D1 user/fullbench imports (full set for user processes) ──────────
#[cfg(all(not(feature = "lichee-d1-rootfs-probe"), any(feature = "lichee-d1-userbench", feature = "lichee-d1-fullbench", feature = "lichee-d1-fullbench-command")))]
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
// ── QEMU imports (no D1 feature at all) ──────────────────────────────
#[cfg(not(feature = "lichee-d1"))]
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

// ── D1 benchmark imports (kbench and userbench) ──────────────────────
#[cfg(feature = "lichee-d1-async-uart")]
use crate::drivers::{bench, uart_init};
#[cfg(feature = "lichee-d1-userbench")]
use crate::mm::load_embedded_user_app;
#[cfg(all(not(feature = "lichee-d1-rootfs-probe"), any(feature = "lichee-d1-fullbench", feature = "lichee-d1-fullbench-command")))]
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

    // ── QEMU mode ────────────────────────────────────────────────────
    #[cfg(not(feature = "lichee-d1"))]
    {
        // Initialize UART hardware + async driver (MMIO, ring buffers, ISR, copiers)
        uart_init::init_uart_hardware();
        ax_println!("[kernel] Async UART driver initialized");

        // Run kernel-side benchmark (ring buffer throughput/latency, memory, NAPI, IRQ)
        crate::drivers::bench::run_startup_benchmark();

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

    #[cfg(all(not(feature = "lichee-d1-rootfs-probe"), feature = "lichee-d1-fullbench"))]
    ax_println!("[starry-d1] Lichee D1 fullbench memory-root path mode");
    #[cfg(all(not(feature = "lichee-d1-rootfs-probe"), feature = "lichee-d1-fullbench-command"))]
    ax_println!("[starry-d1] Lichee D1 fullbench command-entry mode");
    #[cfg(feature = "lichee-d1-rootfs-probe")]
    ax_println!("[starry-d1] Lichee D1 rootfs-probe mode");
    #[cfg(all(feature = "lichee-d1-userbench", not(any(feature = "lichee-d1-fullbench", feature = "lichee-d1-fullbench-command", feature = "lichee-d1-rootfs-probe"))))]
    ax_println!("[starry-d1] Lichee D1 userbench mode");
    #[cfg(not(any(feature = "lichee-d1-userbench", feature = "lichee-d1-fullbench", feature = "lichee-d1-fullbench-command", feature = "lichee-d1-rootfs-probe")))]
    ax_println!("[starry-d1] Lichee D1 kbench mode");

    // Phase 4: Initialize async UART hardware (D1 32-bit MMIO path)
    uart_init::init_uart_hardware();
    ax_println!("[kernel] Async UART driver initialized (D1)");

    // Phase 4: Run kernel ring buffer benchmark
    bench::run_startup_benchmark();

    // Phase 5-6: Userbench path (mount devfs, load user benchmark payload)
    #[cfg(all(feature = "lichee-d1-userbench", not(any(feature = "lichee-d1-fullbench", feature = "lichee-d1-fullbench-command", feature = "lichee-d1-rootfs-probe"))))]
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
    #[cfg(all(not(feature = "lichee-d1-rootfs-probe"), feature = "lichee-d1-fullbench"))]
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
    #[cfg(all(not(feature = "lichee-d1-rootfs-probe"), feature = "lichee-d1-fullbench-command"))]
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
        ax_println!(
            "[starry-d1] shell_status=SKIPPED: no known-good static /bin/sh"
        );
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
                    INIT_SH_PATH, err
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
        ax_println!("[starry-d1] envp_count={} (kernel-side construction)", envs.len());
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

    // ── Q19C-M3: rootfs-probe ─────────────────────────────────────────
    #[cfg(feature = "lichee-d1-rootfs-probe")]
    {
        ax_println!("[starry-d1] log_label=lichee-rootfs-probe");
        ax_println!("[starry-d1] target_mode=lichee-d1-rootfs-probe");
        ax_println!("[starry-d1] probe_goal=SDMMC/block discovery, D1 known facts, no rootfs init");

        // ── D1 known partition facts ──────────────────────────────────
        ax_println!("[starry-d1] --- D1 SDMMC known facts ---");
        ax_println!("[starry-d1] d1_sdmmc_controller_base=TBD (from D1 User Manual / DTS)");
        ax_println!("[starry-d1] d1_sdmmc_irq=TBD");
        ax_println!("[starry-d1] d1_sdmmc_clock_reset=TBD (may be inherited from U-Boot)");
        ax_println!("[starry-d1] d1_sdmmc_pinmux=TBD (may be inherited from U-Boot)");
        ax_println!("[starry-d1] d1_sdmmc_card_detect=TBD");
        ax_println!(
            "[starry-d1] d1_sdmmc_mmio_access=SKIPPED: controller base/init sequence not confirmed"
        );
        ax_println!("[starry-d1] d1_sdmmc_transfer_mode=probe-only (no PIO/DMA implemented)");
        ax_println!(
            "[starry-d1] d1_sdmmc_first_block_read=SKIPPED: PIO/DMA driver not implemented"
        );
        ax_println!("");
        ax_println!("[starry-d1] --- D1 known partition layout ---");
        ax_println!("[starry-d1] /dev/mmcblk0p1: vfat (boot-resource)");
        ax_println!("[starry-d1] /dev/mmcblk0p4: Android boot image");
        ax_println!("[starry-d1] /dev/mmcblk0p7: ext4 (rootfs)");
        ax_println!("[starry-d1] u-boot_chain=sunxi_flash read 45000000 boot; bootm 45000000");
        ax_println!("");

        // ── StarryOS block provider status ────────────────────────────
        ax_println!("[starry-d1] --- StarryOS block provider status ---");
        ax_println!("[starry-d1] virtio-mmio-ranges=[] (D1 has no virtio block)");
        ax_println!("[starry-d1] axdriver_mmio_bus=virtio-only (does not enumerate D1 SDMMC)");
        ax_println!("[starry-d1] sdmmc_driver_status=not implemented (no D1 AxBlockDevice registered)");
        ax_println!("[starry-d1] simple-sdmmc=reference crate available, not connected to D1 block");
        ax_println!("");
        ax_println!(
            "[starry-d1] block_status=SKIPPED: missing D1 SDMMC/block driver"
        );
        ax_println!("[starry-d1] rootfs_init=NOT called (block_devs.len() == 0, would panic)");
        ax_println!("");

        // ── Q19D precondition evidence ─────────────────────────────────
        ax_println!("[starry-d1] --- Q19D preconditions ---");
        ax_println!("[starry-d1] q19d_requires=controller base/IRQ/clock/reset/pinmux/card-detect facts");
        ax_println!("[starry-d1] q19d_requires=PIO-first block read path (LBA0 or known block)");
        ax_println!("[starry-d1] q19d_requires=D1 AxBlockDevice registration before rootfs mount");
        ax_println!("[starry-d1] q19d_scope=real D1 SDMMC/block/rootfs implementation (separate change)");
        ax_println!("");
        ax_println!("[starry-d1] probe complete, halting. No panic.");
        loop {
            riscv::asm::wfi();
        }
    }

    // ── kbench mode: halt after kernel benchmark ─────────────────────
    #[cfg(not(any(feature = "lichee-d1-userbench", feature = "lichee-d1-fullbench", feature = "lichee-d1-fullbench-command", feature = "lichee-d1-rootfs-probe")))]
    {
        ax_println!("[starry-d1] kernel benchmark complete, halting.");
        loop {
            riscv::asm::wfi();
        }
    }
}
