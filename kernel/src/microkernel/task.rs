//! Task management for minux microkernel
//!
//! In a microkernel, tasks are lightweight execution contexts.
//! Each userspace server runs as one or more tasks.

/// Task identifier
pub type TaskId = u32;
pub const MAX_TASKS: usize = 32;

/// Task states in microkernel
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,      // Waiting for IPC
    Receiving,    // Waiting for specific message
    Sending,      // Blocked on send
    Terminated,   // Task has exited
}

/// Complete task control block for microkernel
#[derive(Debug, Clone, Copy)]
pub struct Task {
    pub id: TaskId,
    pub state: TaskState,
    pub priority: u8,
    pub nice: i8,            // Nice value: -20 (high priority) to 19 (low priority)
    pub address_space: u32,  // Memory protection domain
    pub stack_ptr: u64,
    pub instruction_ptr: u64,
    // Full CPU context
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rflags: u64,
    pub fpu_state: [u128; 32], // 512B FXSAVE area, 16-byte aligned
    pub fpu_valid: bool,
}

impl Task {
    /// Create new task with default nice value (0)
    pub const fn new(id: TaskId, address_space: u32) -> Self {
        Self {
            id,
            state: TaskState::Ready,
            priority: 0,
            nice: 0,  // Default nice value (normal priority)
            address_space,
            stack_ptr: 0,
            instruction_ptr: 0,
            rax: 0,
            rbx: 0,
            rcx: 0,
            rdx: 0,
            rsi: 0,
            rdi: 0,
            rbp: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rflags: 0x202, // Default RFLAGS with interrupts enabled
            fpu_state: [0; 32],
            fpu_valid: false,
        }
    }
    
    /// Check if task can be scheduled
    pub fn is_schedulable(&self) -> bool {
        matches!(self.state, TaskState::Ready)
    }
}

use spin::Mutex;
use core::arch::x86_64::__cpuid;

/// Task table - minimal for microkernel
pub(crate) static TASKS: Mutex<[Option<Task>; MAX_TASKS]> = Mutex::new([None; MAX_TASKS]);
pub const MAX_CPUS: usize = 8;
static CURRENT_TASKS: Mutex<[Option<TaskId>; MAX_CPUS]> = Mutex::new([None; MAX_CPUS]);

/// Initialize task management
pub fn init() {
    // Create initial kernel task
    let mut kernel_task = Task::new(0, 0);
    kernel_task.state = TaskState::Running;
    TASKS.lock()[0] = Some(kernel_task); // Kernel address space
    let mut current = CURRENT_TASKS.lock();
    current[0] = Some(0);
}

/// Create new task
pub fn create_task(address_space: u32) -> Result<TaskId, ()> {
    let mut tasks = TASKS.lock();
    for (idx, slot) in tasks.iter_mut().enumerate().skip(1) {
        if slot.is_none() {
            let id = idx as TaskId;
            *slot = Some(Task::new(id, address_space));
            return Ok(id);
        }
    }
    Err(()) // No free slots
}

/// Get current task ID
pub fn current_task() -> Option<TaskId> {
    let idx = current_cpu_index();
    CURRENT_TASKS.lock()[idx]
}

/// Set current task for this CPU.
pub fn set_current_task(task_id: TaskId) {
    let idx = current_cpu_index();
    CURRENT_TASKS.lock()[idx] = Some(task_id);
}

pub fn current_cpu_index() -> usize {
    // Use APIC ID as CPU key. In bring-up this is BSP (CPU0).
    let apic_id = unsafe { (__cpuid(1).ebx >> 24) as usize };
    apic_id % MAX_CPUS
}

/// Get task by ID (returns a copy)
pub fn get_task(id: TaskId) -> Option<Task> {
    let tasks = TASKS.lock();
    tasks.iter()
        .find_map(|t| t.as_ref().filter(|task| task.id == id))
        .copied()
}

pub fn task_stack_ptr_ptr(id: TaskId) -> *mut u64 {
    let mut tasks = TASKS.lock();
    for task_opt in &mut *tasks {
        if let Some(task) = task_opt {
            if task.id == id {
                return &mut task.stack_ptr as *mut u64;
            }
        }
    }
    core::ptr::null_mut()
}

pub fn task_instruction_ptr_ptr(id: TaskId) -> *mut u64 {
    let mut tasks = TASKS.lock();
    for task_opt in &mut *tasks {
        if let Some(task) = task_opt {
            if task.id == id {
                return &mut task.instruction_ptr as *mut u64;
            }
        }
    }
    core::ptr::null_mut()
}

/// Get task's address space
pub fn get_task_address_space(id: TaskId) -> Result<u32, ()> {
    let tasks = TASKS.lock();
    tasks.iter()
        .find_map(|t| t.as_ref().filter(|task| task.id == id))
        .map(|task| task.address_space)
        .ok_or(())
}

/// Set task state
pub fn set_task_state(id: TaskId, state: TaskState) -> Result<(), ()> {
    let mut tasks = TASKS.lock();
    for task_opt in &mut *tasks {
        if let Some(task) = task_opt {
            if task.id == id {
                task.state = state;
                return Ok(());
            }
        }
    }
    Err(())
}

pub fn save_task_fpu_state(id: TaskId) -> Result<(), ()> {
    let mut tasks = TASKS.lock();
    for task_opt in &mut *tasks {
        if let Some(task) = task_opt {
            if task.id == id {
                let ptr = task.fpu_state.as_mut_ptr() as *mut u8;
                unsafe { crate::arch::x86_64::fpu_save(ptr); }
                task.fpu_valid = true;
                return Ok(());
            }
        }
    }
    Err(())
}

pub fn restore_task_fpu_state(id: TaskId) -> Result<(), ()> {
    let mut tasks = TASKS.lock();
    for task_opt in &mut *tasks {
        if let Some(task) = task_opt {
            if task.id == id {
                if task.fpu_valid {
                    let ptr = task.fpu_state.as_ptr() as *const u8;
                    unsafe { crate::arch::x86_64::fpu_restore(ptr); }
                } else {
                    crate::arch::x86_64::fpu_reset_thread();
                    task.fpu_valid = true;
                }
                return Ok(());
            }
        }
    }
    Err(())
}

/// Set task nice value (priority)
/// Nice range: -20 (highest priority) to 19 (lowest priority)
pub fn set_task_nice(id: TaskId, nice: i8) -> Result<(), ()> {
    let nice = nice.clamp(-20, 19); // Clamp to valid range
    let mut tasks = TASKS.lock();
    for task_opt in &mut *tasks {
        if let Some(task) = task_opt {
            if task.id == id {
                task.nice = nice;
                return Ok(());
            }
        }
    }
    Err(())
}

/// Get task nice value
pub fn get_task_nice(id: TaskId) -> Option<i8> {
    let tasks = TASKS.lock();
    tasks.iter()
        .find_map(|t| t.as_ref().filter(|task| task.id == id))
        .map(|task| task.nice)
}

/// Get next schedulable task
pub fn next_schedulable_task() -> Option<TaskId> {
    let tasks = TASKS.lock();
    tasks.iter()
        .find_map(|t| t.as_ref().filter(|task| task.is_schedulable()))
        .map(|task| task.id)
}

/// Get first task in Ready state (excluding task 0).
pub fn first_ready_task() -> Option<Task> {
    let tasks = TASKS.lock();
    tasks
        .iter()
        .filter_map(|t| t.as_ref())
        .find(|task| task.id != 0 && task.state == TaskState::Ready)
        .copied()
}

/// Clean up terminated task
pub fn cleanup_task(task_id: TaskId) {
    let mut tasks = TASKS.lock();
    for slot in &mut *tasks {
        if let Some(task) = slot {
            if task.id == task_id {
                // Clean up task resources
                let address_space = task.address_space;
                
                // Free the task slot
                *slot = None;
                
                // Clean up address space if no other tasks are using it
                drop(tasks); // Release lock before calling other functions
                cleanup_address_space_if_unused(address_space);
                
                // Clear any pending messages for this task
                clear_task_messages(task_id);
                return;
            }
        }
    }
}

/// Clear all messages for a terminated task
fn clear_task_messages(task_id: TaskId) {
    if task_id < MAX_TASKS as TaskId {
        crate::ipc::clear_task_queue(task_id);
    }
}

/// Clean up address space if no tasks are using it
fn cleanup_address_space_if_unused(address_space_id: u32) {
    let tasks = TASKS.lock();
    
    // Check if any other task is using this address space
    let still_in_use = tasks.iter()
        .any(|slot| {
            if let Some(task) = slot {
                task.address_space == address_space_id
            } else {
                false
            }
        });
    
    // If no tasks are using it, we could clean up the address space
    if !still_in_use {
        // The address space cleanup would happen here
        // For simplicity, we're not implementing full cleanup yet
    }
}

/// Set up task with entry point and stack
pub fn setup_task(task_id: TaskId, entry_point: u64, stack_ptr: u64) -> Result<(), ()> {
    let mut tasks = TASKS.lock();
    for task_opt in &mut *tasks {
        if let Some(task) = task_opt {
            if task.id == task_id {
                task.instruction_ptr = entry_point;
                task.stack_ptr = stack_ptr;
                return Ok(());
            }
        }
    }
    Err(())
}

/// Update task context with current CPU state
pub fn update_task_context(task_id: TaskId, rsp: u64, rip: u64) -> Result<(), ()> {
    let mut tasks = TASKS.lock();
    for task_opt in &mut *tasks {
        if let Some(task) = task_opt {
            if task.id == task_id {
                task.stack_ptr = rsp;
                task.instruction_ptr = rip;
                return Ok(());
            }
        }
    }
    Err(())
}

/// Save full CPU context to task
pub fn save_full_context(task_id: TaskId, context: &CpuContext) -> Result<(), ()> {
    let mut tasks = TASKS.lock();
    for task_opt in &mut *tasks {
        if let Some(task) = task_opt {
            if task.id == task_id {
                task.rax = context.rax;
                task.rbx = context.rbx;
                task.rcx = context.rcx;
                task.rdx = context.rdx;
                task.rsi = context.rsi;
                task.rdi = context.rdi;
                task.rbp = context.rbp;
                task.r8 = context.r8;
                task.r9 = context.r9;
                task.r10 = context.r10;
                task.r11 = context.r11;
                task.r12 = context.r12;
                task.r13 = context.r13;
                task.r14 = context.r14;
                task.r15 = context.r15;
                task.stack_ptr = context.rsp;
                task.instruction_ptr = context.rip;
                task.rflags = context.rflags;
                return Ok(());
            }
        }
    }
    Err(())
}

/// CPU context structure for full context switching
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CpuContext {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rsp: u64,
    pub rip: u64,
    pub rflags: u64,
}
