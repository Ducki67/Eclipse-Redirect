use core::ptr;
use core::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use crate::opts;
use crate::patchfinder;
use crate::pe;

pub static F_MEMORY_REALLOC: AtomicU64 = AtomicU64::new(0);

pub struct FMemory;

impl FMemory {
    pub fn find_realloc(image: &crate::pe::ImageInfo) -> u64 {
        const PATTERN: patchfinder::ParsedPattern = patchfinder::parse_pattern(
            "48 89 5C 24 ? 48 89 74 24 10 57 48 83 EC ? 48 8B F1 41 8B D8 48 8B 0D ? ? ? ?",
        );
        let pattern = PATTERN;
        let (text_start, text_size) = match image.text_section() {
            Some(s) => s,
            None => return 0,
        };
        let addr = patchfinder::find_pattern(text_start, text_size, &pattern);
        if addr != 0 {
            F_MEMORY_REALLOC.store(addr, Ordering::Relaxed);
        }
        addr
    }

    pub fn realloc(ptr: *mut u8, size: u64, alignment: i32) -> *mut u8 {
        let func_addr = F_MEMORY_REALLOC.load(Ordering::Relaxed);
        if func_addr == 0 {
            return core::ptr::null_mut();
        }
        unsafe {
            let func: extern "system" fn(*mut u8, u64, i32) -> *mut u8 =
                core::mem::transmute(func_addr);
            func(ptr, size, alignment)
        }
    }

    pub fn malloc(size: u64) -> *mut u8 {
        Self::realloc(core::ptr::null_mut(), size, 0)
    }

    pub fn free(ptr: *mut u8) {
        if !ptr.is_null() {
            Self::realloc(ptr, 0, 0);
        }
    }
}

pub static SET_URL_IDX: AtomicI64 = AtomicI64::new(0);
pub static PROCESS_REQUEST_OG: AtomicU64 = AtomicU64::new(0);
pub static PROCESS_REQUEST_OG_EOS: AtomicU64 = AtomicU64::new(0);

pub const EOS_SET_URL_IDX: i64 = 10;
const DEFAULT_SET_URL_IDX: i64 = 10;

#[repr(C)]
pub struct FString {
    pub string: *mut u16,
    pub length: u32,
    pub max_size: u32,
}

impl Drop for FString {
    fn drop(&mut self) {
        if !self.string.is_null() {
            FMemory::free(self.string as *mut u8);
            self.string = core::ptr::null_mut();
        }
    }
}

impl FString {
    pub fn new() -> Self {
        Self {
            string: core::ptr::null_mut(),
            length: 0,
            max_size: 0,
        }
    }

    pub fn from_wide(ptr: *const u16) -> Self {
        if ptr.is_null() {
            return Self::new();
        }
        unsafe {
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            len += 1;

            let alloc = FMemory::malloc((len * 2) as u64) as *mut u16;
            if alloc.is_null() {
                return Self::new();
            }
            ptr::copy_nonoverlapping(ptr, alloc, len);

            Self {
                string: alloc,
                length: len as u32,
                max_size: len as u32,
            }
        }
    }

    pub fn to_wide(s: &str) -> Self {
        let wide: alloc::vec::Vec<u16> = s.encode_utf16().chain(core::iter::once(0)).collect();
        let len = wide.len() as u32;
        let alloc = FMemory::malloc((wide.len() * 2) as u64) as *mut u16;
        if alloc.is_null() {
            return Self::new();
        }
        unsafe {
            ptr::copy_nonoverlapping(wide.as_ptr(), alloc, wide.len());
        }
        Self {
            string: alloc,
            length: len,
            max_size: len,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.string.is_null() || self.length <= 1
    }

    pub fn to_utf8(&self) -> alloc::string::String {
        if self.string.is_null() || self.length == 0 {
            return alloc::string::String::new();
        }
        unsafe {
            let mut len = 0usize;
            let max = (self.length - 1) as usize;
            while len < max && *self.string.add(len) != 0 {
                len += 1;
            }
            alloc::string::String::from_utf16_lossy(core::slice::from_raw_parts(self.string, len))
        }
    }

    pub fn raw(&self) -> *mut u16 {
        self.string
    }
}

pub struct FCurlHttpRequest {
    pub vtable: *mut *mut u8,
}

impl FCurlHttpRequest {
    pub fn get_url(&self) -> FString {
        unsafe {
            if self.vtable.is_null() || (*self.vtable).is_null() {
                return FString::new();
            }
            let vtable_func: extern "system" fn(*mut FCurlHttpRequest, *mut FString) =
                core::mem::transmute(*self.vtable);
            let mut result = FString::new();
            vtable_func(self as *const Self as *mut FCurlHttpRequest, &mut result);
            result
        }
    }

    pub fn set_url(&self, url: &FString, is_eos: bool) {
        unsafe {
            let idx = if is_eos {
                EOS_SET_URL_IDX
            } else {
                SET_URL_IDX.load(Ordering::Relaxed)
            };
            if idx <= 0 || self.vtable.is_null() {
                return;
            }
            let slot = *self.vtable.add(idx as usize);
            if slot.is_null() {
                return;
            }
            let vtable_func: extern "system" fn(*mut FCurlHttpRequest, *mut FString) =
                core::mem::transmute(slot);
            vtable_func(
                self as *const Self as *mut FCurlHttpRequest,
                url as *const FString as *mut FString,
            );
        }
    }

    pub fn initialize_url_index(&mut self) {
        if opts::SET_URL_INDEX != 0 {
            SET_URL_IDX.store(opts::SET_URL_INDEX, Ordering::Relaxed);
            return;
        }

        unsafe {
            if self.vtable.is_null() {
                SET_URL_IDX.store(DEFAULT_SET_URL_IDX, Ordering::Relaxed);
                return;
            }

            let get_func = *self.vtable;
            if get_func.is_null() || !pe::is_readable(get_func as u64, 0x80) {
                SET_URL_IDX.store(DEFAULT_SET_URL_IDX, Ordering::Relaxed);
                return;
            }

            let mut url_offset: u32 = 0;
            for i in 0..100u64 {
                let addr = get_func as u64 + i;
                if patchfinder::check_bytes_at(addr, 0, &[0x48, 0x8D, 0x91], false) {
                    url_offset = ptr::read_unaligned((addr + 3) as *const u32);
                    break;
                }
                if patchfinder::check_bytes_at(addr, 0, &[0x48, 0x8D, 0x51], false) {
                    url_offset = *((addr + 3) as *const u8) as u32;
                    break;
                }
            }

            if url_offset == 0 {
                SET_URL_IDX.store(DEFAULT_SET_URL_IDX, Ordering::Relaxed);
                return;
            }

            for i in 1..0x20usize {
                let func = *self.vtable.add(i);
                if func.is_null() || !pe::is_readable(func as u64, 0x80) {
                    continue;
                }
                for j in 0..0x60u64 {
                    let addr = func as u64 + j;
                    if patchfinder::check_bytes_at(addr, 0, &[0x48, 0x81, 0xC1], false)
                        && ptr::read_unaligned((addr + 3) as *const u32) == url_offset
                    {
                        SET_URL_IDX.store(i as i64, Ordering::Relaxed);
                        return;
                    }
                    if patchfinder::check_bytes_at(addr, 0, &[0x48, 0x83, 0xC1], false)
                        && *((addr + 3) as *const u8) as u32 == url_offset
                    {
                        SET_URL_IDX.store(i as i64, Ordering::Relaxed);
                        return;
                    }
                }
            }

            SET_URL_IDX.store(DEFAULT_SET_URL_IDX, Ordering::Relaxed);
        }
    }
}
