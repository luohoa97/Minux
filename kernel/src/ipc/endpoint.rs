//! IPC endpoints and message queues

use super::{Message, IpcError};
use crate::microkernel::{TaskId, MAX_TASKS};

const IPC_QUEUE_CAPACITY: usize = 16;
const TRACE_IPC_ERRORS: bool = false;

/// Message queue for each task
#[derive(Copy, Clone)]
pub struct MessageQueue {
    messages: [Option<Message>; IPC_QUEUE_CAPACITY],
    head: usize,
    tail: usize,
    count: usize,
}

impl MessageQueue {
    pub const fn new() -> Self {
        Self {
            messages: [None; IPC_QUEUE_CAPACITY],
            head: 0,
            tail: 0,
            count: 0,
        }
    }
    
    fn push(&mut self, msg: Message) -> Result<(), IpcError> {
        if self.count >= IPC_QUEUE_CAPACITY {
            return Err(IpcError::QueueFull);
        }
        
        self.messages[self.tail] = Some(msg);
        self.tail = (self.tail + 1) % IPC_QUEUE_CAPACITY;
        self.count += 1;
        Ok(())
    }
    
    fn pop(&mut self) -> Option<Message> {
        if self.count == 0 {
            return None;
        }
        
        let msg = self.messages[self.head].take();
        self.head = (self.head + 1) % IPC_QUEUE_CAPACITY;
        self.count -= 1;
        msg
    }
    
}

/// IPC endpoints - one lock per task queue (SMP-friendly).
pub static ENDPOINTS: [spin::Mutex<MessageQueue>; MAX_TASKS] =
    [const { spin::Mutex::new(MessageQueue::new()) }; MAX_TASKS];

/// Initialize IPC endpoints
pub fn init() {
    // Endpoints are already initialized with const fn
}

/// Send message to task
pub fn send_message(_sender: TaskId, receiver: TaskId, msg: &Message) -> Result<(), IpcError> {
    if receiver >= MAX_TASKS as TaskId {
        if TRACE_IPC_ERRORS {
            crate::serial_debugln!("[DBG] ipc send invalid receiver: {}", receiver);
        }
        return Err(IpcError::InvalidTask);
    }
    
    {
        let mut endpoint = ENDPOINTS[receiver as usize].lock();
        if let Err(e) = endpoint.push(*msg) {
            if TRACE_IPC_ERRORS {
                crate::serial_debugln!("[DBG] ipc send queue error to {}: {:?}", receiver, e);
            }
            return Err(e);
        }
    }
    
    // Unblock receiver if it was waiting
    crate::microkernel::unblock_task(receiver);
    
    Ok(())
}

/// Receive message (blocking)
pub fn receive_message(task_id: TaskId) -> Result<Message, IpcError> {
    if task_id >= MAX_TASKS as TaskId {
        if TRACE_IPC_ERRORS {
            crate::serial_debugln!("[DBG] ipc receive invalid task: {}", task_id);
        }
        return Err(IpcError::InvalidTask);
    }
    
    // Non-blocking receive for bring-up stability.
    // Full blocking receive requires trapframe-safe context switching.
    if let Some(msg) = try_receive_message(task_id) {
        Ok(msg)
    } else {
        Err(IpcError::NoMessage)
    }
}

/// Try to receive message (non-blocking)
pub fn try_receive_message(task_id: TaskId) -> Option<Message> {
    if task_id >= MAX_TASKS as TaskId {
        return None;
    }
    
    let mut endpoint = ENDPOINTS[task_id as usize].lock();
    endpoint.pop()
}

pub fn clear_task_queue(task_id: TaskId) {
    if task_id >= MAX_TASKS as TaskId {
        return;
    }
    let mut endpoint = ENDPOINTS[task_id as usize].lock();
    *endpoint = MessageQueue::new();
}

/// Process pending messages (called from main loop)
pub fn process_pending_messages() {
    // Fast path: send_message() already unblocks receivers immediately.
    // Keep this hook for future blocked receive queues.
}
