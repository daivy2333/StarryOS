use alloc::{
    string::{String, ToString},
    sync::Arc,
};

#[cfg(not(feature = "lichee-d1"))]
use axfs::FS_CONTEXT;
#[cfg(not(feature = "lichee-d1"))]
use axhal::uspace::UserContext;
#[cfg(not(feature = "lichee-d1"))]
use axsync::Mutex;
#[cfg(not(feature = "lichee-d1"))]
use axtask::{AxTaskExt, spawn_task};
#[cfg(not(feature = "lichee-d1"))]
use starry_process::{Pid, Process};

#[cfg(not(feature = "lichee-d1"))]
use crate::{
    drivers::{ASYNC_TTY, uart_init},
    file::FD_TABLE,
    mm::{copy_from_kernel, load_user_app, new_user_aspace_empty},
    pseudofs,
    task::{ProcessData, Thread, add_task_to_table, new_user_task, spawn_alarm_task},
};

/// Initialize and run initproc.
pub fn init(args: &[String], envs: &[String]) {
    #[cfg(all(target_arch = "riscv64", feature = "lichee-d1"))]
    {
        let _ = (args, envs);
        // SAFETY: D1 platform MMIO is identity-mapped by OpenSBI; UART0 clock
        // and pins are configured by U-Boot.
        unsafe {
            crate::platform::smoke::run_lichee_d1_smoke();
        }
    }

    #[cfg(not(all(target_arch = "riscv64", feature = "lichee-d1")))]
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
