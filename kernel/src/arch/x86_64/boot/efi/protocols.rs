//! UEFI Protocol definitions

/// EFI GUID
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

/// Loaded Image Protocol GUID
pub const LOADED_IMAGE_PROTOCOL_GUID: Guid = Guid {
    data1: 0x5B1B31A1,
    data2: 0x9562,
    data3: 0x11d2,
    data4: [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

/// EFI Loaded Image Protocol
#[repr(C)]
pub struct LoadedImageProtocol {
    pub revision: u32,
    pub parent_handle: usize,
    pub system_table: usize,
    pub device_handle: usize,
    pub file_path: usize,
    pub reserved: usize,
    pub load_options_size: u32,
    pub load_options: *const u8,
    pub image_base: *const u8,
    pub image_size: u64,
    pub image_code_type: u32,
    pub image_data_type: u32,
    pub unload: usize,
}

/// Simple File System Protocol GUID
pub const SIMPLE_FILE_SYSTEM_PROTOCOL_GUID: Guid = Guid {
    data1: 0x0964e5b22,
    data2: 0x6459,
    data3: 0x11d2,
    data4: [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};
