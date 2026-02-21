//! Microkernel scheduler
//!
//! Priority-based scheduler with nice values for microkernel tasks.
//! In a microkernel, scheduling is minimal - most coordination
//! happens through IPC between userspace servers.

use super::task::{TaskId, TaskState, current_task, set_current_task, set_task_state, get_task};
use spin::Mutex;

static LAST_SCHEDULED: Mutex<[TaskId; crate::microkernel::MAX_CPUS]> =
    Mutex::new([0; crate::microkernel::MAX_CPUS]);

/// Initialize scheduler
pub fn init() {
    // Bootstrap policy: run init (task 2) before elf_loader (task 1) on first handoff.
    // This makes service launch deterministic during bring-up.
    let mut last = LAST_SCHEDULED.lock();
    for slot in last.iter_mut() {
        *slot = 1;
    }
}

/// Schedule next task (called from timer interrupt)
/// Uses priority/nice values to select the highest priority ready task
pub fn schedule() {
    let current = current_task();
    
    // Find highest priority schedulable task
    if let Some(next_id) = next_schedulable_task_priority() {
        // If we have a current task, mark it as ready (unless it's blocked)
        if let Some(current_id) = current {
            if current_id != next_id {
                // Only set to ready if not blocked on IPC
                if let Some(task) = get_task(current_id) {
                    if task.state == TaskState::Running {
                        // Keep task 0 as an internal kernel task, not a runnable user task.
                        if current_id == 0 {
                            let _ = set_task_state(current_id, TaskState::Blocked);
                        } else {
                            let _ = set_task_state(current_id, TaskState::Ready);
                        }
                    }
                }
            }
        }
        
        // Switch to next task
        let _ = set_task_state(next_id, TaskState::Running);
        
        // Mark current task before switching so syscalls in the new context
        // resolve sender/receiver against the correct task ID.
        set_current_task(next_id);

        // Perform context switch
        if current != Some(next_id) {
            context_switch(current, next_id);
        }
    }
}

/// Find next schedulable task based on priority (nice value)
/// Lower nice = higher priority (like Unix nice)
/// Nice range: -20 (highest) to 19 (lowest), default 0
fn next_schedulable_task_priority() -> Option<TaskId> {
    let cpu = crate::microkernel::current_cpu_index();
    let mut last_guard = LAST_SCHEDULED.lock();
    let last = last_guard[cpu];
    let tasks = crate::microkernel::task::TASKS.lock();

    let mut best_nice: i8 = i8::MAX;
    let mut best_after: Option<TaskId> = None;
    let mut best_before: Option<TaskId> = None;

    for task in tasks.iter().filter_map(|slot| slot.as_ref()) {
        if task.id == 0 || !task.is_schedulable() {
            continue;
        }
        if task.nice < best_nice {
            best_nice = task.nice;
            best_after = None;
            best_before = None;
        }
        if task.nice != best_nice {
            continue;
        }
        if task.id > last {
            if best_after.map(|x| task.id < x).unwrap_or(true) {
                best_after = Some(task.id);
            }
        } else if best_before.map(|x| task.id < x).unwrap_or(true) {
            best_before = Some(task.id);
        }
    }

    let chosen = best_after.or(best_before)?;
    last_guard[cpu] = chosen;
    Some(chosen)
}

/// Yield current task (voluntary scheduling)
pub fn yield_task() {
    if let Some(current_id) = current_task() {
        let _ = set_task_state(current_id, TaskState::Ready);
        schedule();
    }
}

/// Block current task (waiting for IPC)
pub fn block_current_task() {
    if let Some(current_id) = current_task() {
        let _ = set_task_state(current_id, TaskState::Blocked);
        schedule();
    }
}

/// Unblock task (IPC message received)
pub fn unblock_task(task_id: TaskId) {
    let _ = set_task_state(task_id, TaskState::Ready);
}

/// Perform context switch between tasks
fn context_switch(from_task: Option<TaskId>, to_task: TaskId) {
    if let Some(from_id) = from_task {
        let _ = super::task::save_task_fpu_state(from_id);
    }
    let _ = super::task::restore_task_fpu_state(to_task);

    let to = match super::task::get_task(to_task) {
        Some(t) => t,
        None => return,
    };
    if to.instruction_ptr == 0 || to.stack_ptr == 0 {
        return;
    }

    let to_root = match crate::mm::get_address_space(to.address_space) {
        Some(space) => space.page_table_root,
        None => return,
    };
    if let Some(from_id) = from_task {
        let from_rsp_ptr = super::task::task_stack_ptr_ptr(from_id);
        let from_rip_ptr = super::task::task_instruction_ptr_ptr(from_id);
        if !from_rsp_ptr.is_null() && !from_rip_ptr.is_null() {
            unsafe {
                core::arch::asm!(
                    "mov [{from_rsp}], rsp",
                    "lea rax, [rip + 2f]",
                    "mov [{from_rip}], rax",
                    "mov cr3, {to_cr3}",
                    "mov rsp, {to_rsp}",
                    "jmp {to_rip}",
                    "2:",
                    from_rsp = in(reg) from_rsp_ptr,
                    from_rip = in(reg) from_rip_ptr,
                    to_cr3 = in(reg) to_root,
                    to_rsp = in(reg) to.stack_ptr,
                    to_rip = in(reg) to.instruction_ptr,
                    out("rax") _,
                );
            }
            return;
        }
    }

    // First handoff from kernel task to first runnable task.
    unsafe {
        core::arch::asm!(
            "mov cr3, {to_cr3}",
            "mov rsp, {to_rsp}",
            "jmp {to_rip}",
            to_cr3 = in(reg) to_root,
            to_rsp = in(reg) to.stack_ptr,
            to_rip = in(reg) to.instruction_ptr,
            options(noreturn)
        );
    }
}

/// Save task context (registers, stack pointer, etc.)
fn save_task_context(task_id: TaskId) {
    let _ = task_id;
}

/// Load task context (registers, stack pointer, etc.)
fn load_task_context(task_id: TaskId) {
    let _ = task_id;
}

/// Switch to task's address space
fn switch_address_space(task_id: TaskId) {
    if let Some(task) = super::task::get_task(task_id) {
        let _ = crate::mm::activate_address_space(task.address_space);
    }
}
