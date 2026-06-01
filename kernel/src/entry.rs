use alloc::{
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};

use axfs::FS_CONTEXT;
use axhal::uspace::UserContext;
use axsync::Mutex;
use axtask::{AxTaskExt, spawn_task};
use starry_process::{Pid, Process};

use crate::{
    drivers::{uart_init, isr, async_driver::DRIVER, ASYNC_TTY, benchmark},
    file::FD_TABLE,
    mm::{copy_from_kernel, load_user_app, new_user_aspace_empty},
    pseudofs,
    task::{ProcessData, Thread, add_task_to_table, new_user_task, spawn_alarm_task},
};

/// 运行启动时的性能测试
fn run_startup_benchmark() {
    use crate::drivers::ring_buffer::BUF_SIZE;
    use axhal::time::monotonic_time_nanos;

    ax_println!("[BENCH] Running startup benchmark...");

    // 测试 1: Ring Buffer 吞吐量测试（模拟异步路径）
    // 使用 0 字节避免数据泄漏到输出
    let test_data = vec![0u8; 1024];
    let iterations = 100;

    // 开始 CPU 占用测量
    benchmark::start_cpu_measurement();
    let start_time = monotonic_time_nanos();

    // 通过 ring buffer 写入，模拟 TX 路径
    for _ in 0..iterations {
        let mut tx_buf = crate::drivers::async_driver::DRIVER.tx.lock();
        tx_buf.push(&test_data);
        drop(tx_buf);
    }

    let end_time = monotonic_time_nanos();
    let cpu_cycles = benchmark::stop_cpu_measurement();

    let elapsed_ns = end_time - start_time;
    let elapsed_s = elapsed_ns as f64 / 1_000_000_000.0;
    let total_bytes = iterations * 1024;
    let throughput_kbps = total_bytes as f64 / elapsed_s / 1024.0;
    let cpu_usage = benchmark::calculate_cpu_usage(cpu_cycles, elapsed_ns);

    ax_println!("[BENCH] Ring Buffer Write: {:.2} KB/s", throughput_kbps);
    ax_println!("[BENCH] Total: {} bytes in {:.2} ms", total_bytes, elapsed_ns as f64 / 1_000_000.0);
    ax_println!("[BENCH] CPU Cycles: {}", cpu_cycles);
    ax_println!("[BENCH] CPU Usage: {:.1}%", cpu_usage);

    // 测试 2: 硬件理论极限
    ax_println!("[BENCH] Hardware Line Rate: 11.52 KB/s (115200 bps)");
    ax_println!("[BENCH] FIFO Depth: 16 bytes");

    // 测试 3: 内存统计
    ax_println!("[BENCH] Ring Buffer Memory: {} KB ({} bytes)", BUF_SIZE / 1024, BUF_SIZE);
    ax_println!("[BENCH] Total Buffer Memory: {} KB ({} bytes)", BUF_SIZE * 2 / 1024, BUF_SIZE * 2);

    // 测试 4: ISR 统计（通过 uart_init 模块）
    let irq_count = crate::drivers::uart_init::get_irq_count();
    ax_println!("[BENCH] ISR Count: {}", irq_count);

    // 测试 5: 中断频率统计
    match benchmark::get_irq_frequency(elapsed_ns) {
        Some(freq) => ax_println!("[BENCH] IRQ Frequency: {:.2} IRQ/s", freq),
        None => ax_println!("[BENCH] IRQ Frequency: N/A (only {} IRQs)", crate::drivers::uart_init::get_irq_count()),
    }

    // 测试 6: NAPI 效果报告
    benchmark::report_napi_effect();

    // 测试 7: RX Ring Buffer 读取速度
    benchmark::run_rx_throughput_test();

    ax_println!("[BENCH] Startup benchmark complete");
    ax_println!("[BENCH] Note: Actual throughput limited by UART line rate (11.52 KB/s)");
}

/// Initialize and run initproc.
pub fn init(args: &[String], envs: &[String]) {
    // Q4: Full async RX+TX
    uart_init::init_uart_hardware();
    axhal::irq::register_irq_hook(isr::uart_isr_handler);
    DRIVER.start_rx_copier();
    DRIVER.start_tx_copier();
    ax_println!("[kernel] Q4: AsyncUart RX+TX copiers started");

    // 初始化 benchmark 模块并显示内存统计
    benchmark::memory_usage();

    // 运行简单的性能测试
    run_startup_benchmark();


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
