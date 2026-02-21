//! Multiboot2 boot information parsing
//! 
//! The bootloader passes information about loaded modules, memory map, etc.
//! via the multiboot2 info structure.

/// Multiboot2 magic number
pub const MULTIBOOT2_MAGIC: u32 = 0x36d76289;

/// Boot information passed by bootloader
#[repr(C)]
pub struct MultibootInfo {
    pub total_size: u32,
    pub reserved: u32,
}

/// Multiboot2 tag types
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagType {
    End = 0,
    Module = 3,
    MemoryMap = 6,
    Framebuffer = 8,
}

/// Generic multiboot2 tag header
#[repr(C)]
pub struct Tag {
    pub tag_type: u32,
    pub size: u32,
}

/// Module tag - contains loaded module info
#[repr(C)]
pub struct ModuleTag {
    pub tag_type: u32,
    pub size: u32,
    pub mod_start: u32,
    pub mod_end: u32,
    // Followed by null-terminated string (module name)
}

/// Boot module information
#[derive(Debug, Clone, Copy)]
pub struct BootModule {
    pub start: usize,
    pub end: usize,
    pub name: &'static str,
}

const EMPTY_MODULE: BootModule = BootModule {
    start: 0,
    end: 0,
    name: "",
};

/// Lightweight validation that `addr` points to a multiboot2 info block.
pub fn looks_like_multiboot2(addr: usize) -> bool {
    if addr == 0 || (addr & 0x7) != 0 {
        return false;
    }

    let info = unsafe { &*(addr as *const MultibootInfo) };
    if info.total_size < 16 || info.reserved != 0 {
        return false;
    }

    let end = addr.saturating_add(info.total_size as usize);
    if end <= addr {
        return false;
    }

    true
}

/// Parse multiboot2 info structure
pub fn parse_multiboot_info(multiboot_addr: usize) -> Option<MultibootIterator> {
    if multiboot_addr == 0 {
        return None;
    }
    
    let info = unsafe { &*(multiboot_addr as *const MultibootInfo) };
    
    Some(MultibootIterator {
        current: multiboot_addr + 8, // Skip total_size and reserved
        end: multiboot_addr + info.total_size as usize,
    })
}

/// Iterator over multiboot2 tags
pub struct MultibootIterator {
    current: usize,
    end: usize,
}

impl Iterator for MultibootIterator {
    type Item = &'static Tag;
    
    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.end {
            return None;
        }
        
        let tag = unsafe { &*(self.current as *const Tag) };
        
        if tag.tag_type == TagType::End as u32 {
            return None;
        }
        
        // Align to 8-byte boundary
        let size = (tag.size + 7) & !7;
        self.current += size as usize;
        
        Some(tag)
    }
}

impl Tag {
    /// Check if this is a module tag
    pub fn as_module(&self) -> Option<&ModuleTag> {
        if self.tag_type == TagType::Module as u32 {
            Some(unsafe { &*(self as *const Tag as *const ModuleTag) })
        } else {
            None
        }
    }
}

impl ModuleTag {
    /// Get module name
    pub fn name(&self) -> &'static str {
        unsafe {
            let name_ptr = (self as *const ModuleTag).add(1) as *const u8;
            let header_size = core::mem::size_of::<ModuleTag>();
            let max_len = (self.size as usize).saturating_sub(header_size);
            let mut len = 0usize;
            while len < max_len && *name_ptr.add(len) != 0 {
                len += 1;
            }
            let slice = core::slice::from_raw_parts(name_ptr, len);
            match core::str::from_utf8(slice) {
                Ok(s) => s,
                Err(_) => "<invalid-module-name>",
            }
        }
    }
    
    /// Get module data
    pub fn data(&self) -> &'static [u8] {
        unsafe {
            let start = self.mod_start as usize;
            let len = (self.mod_end - self.mod_start) as usize;
            core::slice::from_raw_parts(start as *const u8, len)
        }
    }
}

/// Get all boot modules from multiboot info
pub fn get_boot_modules(multiboot_addr: usize) -> &'static [BootModule] {
    use spin::Mutex;
    
    static MODULES: Mutex<([BootModule; 16], usize)> = Mutex::new(([EMPTY_MODULE; 16], 0));
    
    let mut modules = MODULES.lock();
    let (ref mut array, ref mut count) = *modules;

    *count = 0;
    if let Some(iter) = parse_multiboot_info(multiboot_addr) {
        for tag in iter {
            if let Some(module) = tag.as_module() {
                if *count < array.len() {
                    array[*count] = BootModule {
                        start: module.mod_start as usize,
                        end: module.mod_end as usize,
                        name: module.name(),
                    };
                    *count += 1;
                }
            }
        }
    }

    let ptr = array.as_ptr();
    let len = *count;
    drop(modules);

    unsafe { core::slice::from_raw_parts(ptr, len) }
}
