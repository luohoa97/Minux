//! CPU exception handlers

use x86_64::structures::idt::InterruptStackFrame;
use x86_64::structures::idt::PageFaultErrorCode;
use x86_64::registers::control::Cr2;

fn terminate_faulting_task(reason: &str) -> bool {
    if let Some(task_id) = crate::microkernel::current_task() {
        if task_id != 0 {
            crate::serial_println!("[EXCEPTION] {} in task {}, halting kernel", reason, task_id);
            let _ = crate::microkernel::set_task_state(task_id, crate::microkernel::TaskState::Terminated);
            crate::microkernel::cleanup_task(task_id);
            crate::kernel_fatal("Fatal exception in user task");
        }
    }
    false
}

/// Divide by zero exception handler
pub extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    crate::serial_println!("[EXCEPTION] DIVIDE ERROR\n{:#?}", stack_frame);
    if terminate_faulting_task("divide error") {
        return;
    }
    crate::kernel_fatal("Kernel exception: divide error");
}

/// Invalid opcode exception handler
pub extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    crate::serial_println!("[EXCEPTION] INVALID OPCODE\n{:#?}", stack_frame);
    if terminate_faulting_task("invalid opcode") {
        return;
    }
    crate::kernel_fatal("Kernel exception: invalid opcode");
}

/// Breakpoint exception handler
pub extern "x86-interrupt" fn breakpoint_handler(_stack_frame: InterruptStackFrame) {
    // Minimal breakpoint handling
}

/// General protection fault handler
pub extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    crate::serial_println!(
        "[EXCEPTION] GENERAL PROTECTION FAULT: error_code=0x{:x}\n{:#?}",
        error_code,
        stack_frame
    );
    if terminate_faulting_task("general protection fault") {
        return;
    }
    crate::kernel_fatal("Kernel exception: general protection fault");
}

/// Page fault handler
pub extern "x86-interrupt" fn page_fault_handler(
    _stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    let fault_addr = Cr2::read().as_u64();
    crate::serial_println!(
        "[EXCEPTION] PAGE FAULT: addr=0x{:x}, error_bits=0x{:x}",
        fault_addr,
        error_code.bits()
    );
    if terminate_faulting_task("page fault") {
        return;
    }
    crate::arch::disable_interrupts();
    loop {
        crate::arch::halt();
    }
}

/// Double fault exception handler
pub extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    crate::serial_println!("[EXCEPTION] DOUBLE FAULT\n{:#?}", stack_frame);
    crate::kernel_fatal("Kernel exception: double fault");
}
