use axhal::uspace::{ExceptionKind, ReturnReason, UserContext};
use axtask::TaskInner;
use starry_process::Pid;
use starry_signal::{SignalInfo, Signo};
use starry_vm::{VmMutPtr, VmPtr};

use super::{
    AsThread, TimerState, check_signals, raise_signal_fatal, set_timer_state, unblock_next_signal,
};
use crate::syscall::handle_syscall;

/// Read the saved user PC, SP and return-address from a user context.
///
/// `ip()`/`sp()` are portable across platforms; the return address is stored
/// differently per architecture and must not be read through the generic
/// `regs` path.  Returns `(pc, sp, ra)`.
fn user_fault_pc_sp_ra(uctx: &UserContext) -> (usize, usize, usize) {
    #[cfg(any(target_arch = "riscv64", target_arch = "loongarch64"))]
    {
        (uctx.ip(), uctx.sp(), uctx.regs.ra)
    }
    #[cfg(target_arch = "aarch64")]
    {
        // x30 is the link register (return address) on AArch64.
        (uctx.ip(), uctx.sp(), uctx.x[30] as usize)
    }
    #[cfg(target_arch = "x86_64")]
    {
        // The trap frame has no return-address register; the callee's return
        // address lives on the user stack at `sp`.  Report `sp` only so the
        // `ra=` label stays present across architectures; the value is not a
        // true return address and is not used for RISC-V runtime alignment.
        (uctx.ip(), uctx.sp(), uctx.rsp)
    }
}

/// Create a new user task.
pub fn new_user_task(name: &str, mut uctx: UserContext, set_child_tid: usize) -> TaskInner {
    TaskInner::new(
        move || {
            let curr = axtask::current();

            if let Some(tid) = (set_child_tid as *mut Pid).nullable() {
                tid.vm_write(curr.id().as_u64() as Pid).ok();
            }

            info!("Enter user space: ip={:#x}, sp={:#x}", uctx.ip(), uctx.sp());

            let thr = curr.as_thread();
            while !thr.pending_exit() {
                let reason = uctx.run();

                set_timer_state(&curr, TimerState::Kernel);

                match reason {
                    ReturnReason::Syscall => handle_syscall(&mut uctx),
                    ReturnReason::PageFault(addr, flags) => {
                        if !thr.proc_data.aspace.lock().handle_page_fault(addr, flags) {
                            let (user_pc, user_sp, user_ra) = user_fault_pc_sp_ra(&uctx);
                            info!(
                                "{:?}: segmentation fault pc={user_pc:#x} va={addr:#x} {flags:?} \
                                 sp={user_sp:#x} ra={user_ra:#x}",
                                thr.proc_data.proc
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
