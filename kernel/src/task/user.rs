#[cfg(feature = "lichee-d1-userbench")]
use core::sync::atomic::{AtomicUsize, Ordering};

use axhal::uspace::{ExceptionKind, ReturnReason, UserContext};
use axtask::TaskInner;
use starry_process::Pid;
use starry_signal::{SignalInfo, Signo};
use starry_vm::{VmMutPtr, VmPtr};

use super::{
    AsThread, TimerState, check_signals, raise_signal_fatal, set_timer_state, unblock_next_signal,
};
use crate::syscall::handle_syscall;

#[cfg(feature = "lichee-d1-userbench")]
static D1_USER_MARKS: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "lichee-d1-userbench")]
fn d1_user_mark_allowed() -> bool {
    D1_USER_MARKS.fetch_add(1, Ordering::Relaxed) < 80
}

/// Create a new user task.
pub fn new_user_task(name: &str, mut uctx: UserContext, set_child_tid: usize) -> TaskInner {
    TaskInner::new(
        move || {
            let curr = axtask::current();

            #[cfg(feature = "lichee-d1-userbench")]
            ax_println!("[d1-user] task closure entered");

            if let Some(tid) = (set_child_tid as *mut Pid).nullable() {
                tid.vm_write(curr.id().as_u64() as Pid).ok();
            }

            info!("Enter user space: ip={:#x}, sp={:#x}", uctx.ip(), uctx.sp());
            #[cfg(feature = "lichee-d1-userbench")]
            ax_println!(
                "[d1-user] prepared user context ip={:#x}, sp={:#x}",
                uctx.ip(),
                uctx.sp()
            );

            let thr = curr.as_thread();
            while !thr.pending_exit() {
                #[cfg(feature = "lichee-d1-userbench")]
                if d1_user_mark_allowed() {
                    ax_println!(
                        "[d1-user] before uctx.run ip={:#x}, sp={:#x}",
                        uctx.ip(),
                        uctx.sp()
                    );
                }

                let reason = uctx.run();

                set_timer_state(&curr, TimerState::Kernel);

                #[cfg(feature = "lichee-d1-userbench")]
                if d1_user_mark_allowed() {
                    match &reason {
                        ReturnReason::Syscall => ax_println!("[d1-user] uctx.run -> syscall"),
                        ReturnReason::PageFault(addr, flags) => ax_println!(
                            "[d1-user] uctx.run -> page fault addr={:#x} flags={:?}",
                            addr.as_usize(),
                            flags
                        ),
                        ReturnReason::Interrupt => ax_println!("[d1-user] uctx.run -> interrupt"),
                        ReturnReason::Exception(exc_info) => ax_println!(
                            "[d1-user] uctx.run -> exception kind={:?}",
                            exc_info.kind()
                        ),
                        _ => ax_println!("[d1-user] uctx.run -> other"),
                    }
                }

                match reason {
                    ReturnReason::Syscall => handle_syscall(&mut uctx),
                    ReturnReason::PageFault(addr, flags) => {
                        if !thr.proc_data.aspace.lock().handle_page_fault(addr, flags) {
                            info!(
                                "{:?}: segmentation fault at {:#x} {:?}",
                                thr.proc_data.proc, addr, flags
                            );
                            raise_signal_fatal(SignalInfo::new_kernel(Signo::SIGSEGV))
                                .expect("Failed to send SIGSEGV");
                        }
                    }
                    ReturnReason::Interrupt => {}
                    #[allow(unused_labels)]
                    ReturnReason::Exception(exc_info) => 'exc: {
                        // TODO: detailed handling
                        let signo = match exc_info.kind() {
                            ExceptionKind::Misaligned => {
                                #[cfg(target_arch = "loongarch64")]
                                if unsafe { uctx.emulate_unaligned() }.is_ok() {
                                    break 'exc;
                                }
                                Signo::SIGBUS
                            }
                            ExceptionKind::Breakpoint => Signo::SIGTRAP,
                            ExceptionKind::IllegalInstruction => Signo::SIGILL,
                            _ => Signo::SIGTRAP,
                        };
                        raise_signal_fatal(SignalInfo::new_kernel(signo))
                            .expect("Failed to send SIGTRAP");
                    }
                    r => {
                        warn!("Unexpected return reason: {r:?}");
                        raise_signal_fatal(SignalInfo::new_kernel(Signo::SIGSEGV))
                            .expect("Failed to send SIGSEGV");
                    }
                }

                if !unblock_next_signal() {
                    while check_signals(thr, &mut uctx, None) {}
                }

                set_timer_state(&curr, TimerState::User);
                curr.clear_interrupt();
            }
        },
        name.into(),
        crate::config::KERNEL_STACK_SIZE,
    )
}
