//! Message types for microkernel IPC

use crate::microkernel::TaskId;

/// Message types in microkernel IPC
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// Request for service
    Request,
    /// Response to request
    Reply,
    /// Asynchronous notification
    Notification,
    /// Interrupt notification
    Interrupt,
}

/// Capability for accessing services
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capability {
    pub service_id: u32,
    pub permissions: u32,
}

/// IPC message structure
#[derive(Debug, Clone, Copy)]
pub struct Message {
    pub sender: TaskId,
    pub receiver: TaskId,
    pub msg_type: MessageType,
    pub capability: Option<Capability>,
    pub data: [u8; 64], // Fixed-size for simplicity
    pub length: usize,
    pub zero_copy: bool,
    pub grant_ptr: u64,
    pub grant_len: usize,
    pub fast_words: [u64; 4],
    pub fast_len: usize,
}

impl Message {
    /// Create new message
    pub fn new(sender: TaskId, receiver: TaskId, msg_type: MessageType) -> Self {
        Self {
            sender,
            receiver,
            msg_type,
            capability: None,
            data: [0; 64],
            length: 0,
            zero_copy: false,
            grant_ptr: 0,
            grant_len: 0,
            fast_words: [0; 4],
            fast_len: 0,
        }
    }
    
    /// Set message data
    pub fn set_data(&mut self, data: &[u8]) {
        let len = core::cmp::min(data.len(), 64);
        self.data[..len].copy_from_slice(&data[..len]);
        self.length = len;
    }
    
    /// Get message data
    pub fn data(&self) -> &[u8] {
        &self.data[..self.length]
    }

    /// Set zero-copy payload descriptor
    pub fn set_zero_copy(&mut self, ptr: u64, len: usize) {
        self.zero_copy = true;
        self.grant_ptr = ptr;
        self.grant_len = len;
        self.length = 0;
    }

    /// Set fast IPC register words (L4-style short message).
    pub fn set_fast_words(&mut self, words: &[u64]) {
        let n = core::cmp::min(words.len(), self.fast_words.len());
        self.fast_words[..n].copy_from_slice(&words[..n]);
        self.fast_len = n;
    }
    
    /// Set capability
    pub fn set_capability(&mut self, cap: Capability) {
        self.capability = Some(cap);
    }
}
