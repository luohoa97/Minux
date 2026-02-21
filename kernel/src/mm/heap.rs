//! Kernel heap allocator for dynamic memory allocation

use spin::Mutex;

/// Simple bump allocator for kernel heap
pub struct BumpAllocator {
    heap_start: usize,
    heap_end: usize,
    next: usize,
}

impl BumpAllocator {
    /// Create new bump allocator
    pub const fn new() -> Self {
        Self {
            heap_start: 0,
            heap_end: 0,
            next: 0,
        }
    }
    
    /// Initialize allocator with heap bounds
    pub fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.heap_start = heap_start;
        self.heap_end = heap_start + heap_size;
        self.next = heap_start;
    }
    
    /// Allocate memory
    pub fn allocate(&mut self, size: usize, align: usize) -> Option<*mut u8> {
        // Align the allocation
        let alloc_start = align_up(self.next, align);
        let alloc_end = alloc_start.checked_add(size)?;
        
        if alloc_end > self.heap_end {
            return None; // Out of memory
        }
        
        self.next = alloc_end;
        Some(alloc_start as *mut u8)
    }
    
    /// Deallocate memory (no-op for bump allocator)
    pub fn deallocate(&mut self, _ptr: *mut u8, _size: usize, _align: usize) {
        // Bump allocator doesn't support deallocation
    }
}

/// Align address up to alignment
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

/// Global kernel heap allocator
static ALLOCATOR: Mutex<BumpAllocator> = Mutex::new(BumpAllocator::new());

/// Initialize kernel heap
pub fn init_heap() {
    // Use a static heap (1MB) for kernel memory allocation
    const HEAP_SIZE: usize = 1024 * 1024;
    static HEAP: Mutex<[u8; HEAP_SIZE]> = Mutex::new([0; HEAP_SIZE]);
    
    let heap = HEAP.lock();
    let heap_ptr = heap.as_ptr() as usize;
    ALLOCATOR.lock().init(heap_ptr, HEAP_SIZE);
}

/// Allocate kernel memory
pub fn kalloc(size: usize, align: usize) -> Option<*mut u8> {
    ALLOCATOR.lock().allocate(size, align)
}

/// Free kernel memory
pub fn kfree(ptr: *mut u8, size: usize, align: usize) {
    ALLOCATOR.lock().deallocate(ptr, size, align);
}
