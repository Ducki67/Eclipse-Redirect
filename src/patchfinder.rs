use core::arch::x86_64::*;
use core::ptr;

use crate::pe::ImageInfo;

pub struct ParsedPattern {
    pub bytes: [u8; 256],
    pub enabled: [u8; 32],
    pub count: usize,
}

impl ParsedPattern {
    pub const fn new() -> Self {
        Self {
            bytes: [0u8; 256],
            enabled: [0u8; 32],
            count: 0,
        }
    }
}

pub const fn parse_pattern(pattern: &str) -> ParsedPattern {
    let mut result = ParsedPattern::new();
    let bytes = pattern.as_bytes();
    let mut i = 0;
    let mut idx = 0;

    while i < bytes.len() && idx < 256 {
        if bytes[i] == b' ' {
            i += 1;
            continue;
        }

        let b1 = hex_to_nibble(bytes[i]);
        let b2 = if i + 1 < bytes.len() {
            hex_to_nibble(bytes[i + 1])
        } else {
            0xFF
        };

        if b1 == 0xFF || b2 == 0xFF {
            result.bytes[idx] = 0;
        } else {
            result.bytes[idx] = (b1 << 4) | b2;
            let byte_idx = idx / 8;
            let bit_idx = idx % 8;
            result.enabled[byte_idx] |= 1 << bit_idx;
        }

        idx += 1;
        i += 2;
        if i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
    }

    result.count = idx;
    result
}

const fn hex_to_nibble(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0xFF,
    }
}

pub fn find_pattern(text_start: u64, text_size: u32, pattern: &ParsedPattern) -> u64 {
    if pattern.count == 0 {
        return 0;
    }

    let scan_start = text_start as *const u8;
    let scan_size = text_size as usize;

    if scan_size < pattern.count {
        return 0;
    }
    let limit = scan_size - pattern.count;

    let mut first_enabled = 0;
    while first_enabled < pattern.count {
        let byte_idx = first_enabled / 8;
        let bit_idx = first_enabled % 8;
        if pattern.enabled[byte_idx] & (1 << bit_idx) != 0 {
            break;
        }
        first_enabled += 1;
    }

    if first_enabled >= pattern.count {
        return 0;
    }

    let target_byte = pattern.bytes[first_enabled];

    unsafe {
        let target_simd = _mm_set1_epi8(target_byte as i8);
        let mut i = 0;

        while i + 16 <= scan_size {
            let chunk = _mm_loadu_si128(scan_start.add(i) as *const __m128i);
            let cmp = _mm_cmpeq_epi8(chunk, target_simd);
            let mask = _mm_movemask_epi8(cmp) as u32;

            if mask != 0 {
                let mut bit = 0u32;
                while bit < 16 {
                    if mask & (1 << bit) != 0 {
                        let pos = i + bit as usize;
                        if pos >= first_enabled {
                            let offset = pos - first_enabled;
                            if offset <= limit && check_pattern(scan_start, offset, pattern) {
                                return scan_start.add(offset) as u64;
                            }
                        }
                    }
                    bit += 1;
                }
            }
            i += 16;
        }

        let mut j = if i > first_enabled { i - first_enabled } else { 0 };
        while j <= limit {
            if check_pattern(scan_start, j, pattern) {
                return scan_start.add(j) as u64;
            }
            j += 1;
        }
    }

    0
}

fn check_pattern(base: *const u8, offset: usize, pattern: &ParsedPattern) -> bool {
    for j in 0..pattern.count {
        let byte_idx = j / 8;
        let bit_idx = j % 8;
        if pattern.enabled[byte_idx] & (1 << bit_idx) != 0 {
            unsafe {
                if *base.add(offset + j) != pattern.bytes[j] {
                    return false;
                }
            }
        }
    }
    true
}

fn is_rip_lea_prefix(b: u8) -> bool {
    b & 0xFB == 0x48
}

pub fn find_string_ref_wide(image: &ImageInfo, string: &[u16]) -> u64 {
    find_string_ref(image, string.as_ptr() as *const u8, string.len() * 2)
}

pub fn find_string_ref_ascii(image: &ImageInfo, string: &[u8]) -> u64 {
    find_string_ref(image, string.as_ptr(), string.len())
}

fn find_string_ref(image: &ImageInfo, needle: *const u8, needle_len: usize) -> u64 {
    let (text_start, text_size) = match image.text_section() {
        Some(s) => s,
        None => return 0,
    };
    let (rdata_start, rdata_size) = match image.rdata_section() {
        Some(s) => s,
        None => return 0,
    };

    if needle_len == 0 || (rdata_size as usize) < needle_len {
        return 0;
    }

    let rdata_end = rdata_start + rdata_size as u64;
    let compare_limit = rdata_end - needle_len as u64;
    let text_ptr = text_start as *const u8;
    let text_size = text_size as usize;

    if text_size < 7 {
        return 0;
    }
    let last = text_size - 6;

    unsafe {
        let target_simd = _mm_set1_epi8(0x8Du8 as i8);
        let mut i = 0;

        while i + 16 <= text_size {
            let chunk = _mm_loadu_si128(text_ptr.add(i) as *const __m128i);
            let cmp = _mm_cmpeq_epi8(chunk, target_simd);
            let mask = _mm_movemask_epi8(cmp) as u32;

            if mask != 0 {
                let mut bit = 0u32;
                while bit < 16 {
                    if mask & (1 << bit) != 0 {
                        let hit = check_string_ref(
                            text_ptr,
                            i + bit as usize,
                            last,
                            rdata_start,
                            compare_limit,
                            needle,
                            needle_len,
                        );
                        if hit != 0 {
                            return hit;
                        }
                    }
                    bit += 1;
                }
            }
            i += 16;
        }

        while i < text_size {
            if *text_ptr.add(i) == 0x8D {
                let hit = check_string_ref(
                    text_ptr,
                    i,
                    last,
                    rdata_start,
                    compare_limit,
                    needle,
                    needle_len,
                );
                if hit != 0 {
                    return hit;
                }
            }
            i += 1;
        }
    }

    0
}

unsafe fn check_string_ref(
    text_ptr: *const u8,
    pos: usize,
    last: usize,
    rdata_start: u64,
    compare_limit: u64,
    needle: *const u8,
    needle_len: usize,
) -> u64 {
    if pos < 1 || pos >= last {
        return 0;
    }

    if !is_rip_lea_prefix(*text_ptr.add(pos - 1)) {
        return 0;
    }

    let displacement = ptr::read_unaligned(text_ptr.add(pos + 2) as *const i32);
    let string_addr = (text_ptr.add(pos + 6) as u64).wrapping_add(displacement as i64 as u64);

    if string_addr < rdata_start || string_addr > compare_limit {
        return 0;
    }

    if !bytes_equal(string_addr as *const u8, needle, needle_len) {
        return 0;
    }

    text_ptr.add(pos - 1) as u64
}

unsafe fn bytes_equal(a: *const u8, b: *const u8, len: usize) -> bool {
    for i in 0..len {
        if *a.add(i) != *b.add(i) {
            return false;
        }
    }
    true
}

pub fn check_bytes_at(base: u64, offset: i32, bytes: &[u8], upwards: bool) -> bool {
    let addr = if upwards {
        base.wrapping_sub(offset as u64)
    } else {
        base.wrapping_add(offset as u64)
    };
    let ptr = addr as *const u8;
    unsafe {
        for (i, &b) in bytes.iter().enumerate() {
            if *ptr.add(i) != b {
                return false;
            }
        }
    }
    true
}

pub fn find_function_prologue(str_ref: u64, is_eos: bool) -> u64 {
    for i in 0..4096 {
        let d = i as i32;

        if is_eos {
            if check_bytes_at(str_ref, d, &[0x48, 0x89, 0x5C], true) {
                return str_ref - i as u64;
            }
            continue;
        }

        if check_bytes_at(str_ref, d, &[0x4C, 0x8B, 0xDC], true)
            || check_bytes_at(str_ref, d, &[0x48, 0x8B, 0xC4], true)
            || check_bytes_at(str_ref, d, &[0x48, 0x89, 0x5C], true)
        {
            return str_ref - i as u64;
        }

        if check_bytes_at(str_ref, d, &[0x48, 0x81, 0xEC], true)
            || check_bytes_at(str_ref, d, &[0x48, 0x83, 0xEC], true)
        {
            for x in 0..50 {
                let inner = (i + x) as i32;
                if check_bytes_at(str_ref, inner, &[0x4C, 0x8B, 0xDC], true)
                    || check_bytes_at(str_ref, inner, &[0x48, 0x8B, 0xC4], true)
                    || check_bytes_at(str_ref, inner, &[0x48, 0x89, 0x5C], true)
                {
                    return str_ref - inner as u64;
                }
                if check_bytes_at(str_ref, inner, &[0x40], true) {
                    return str_ref - inner as u64;
                }
            }
        }
    }
    0
}

pub fn find_vtable_entry(rdata_start: u64, rdata_size: u32, func_addr: u64) -> u64 {
    let ptr = rdata_start as *const u64;
    let count = rdata_size as usize / 8;

    unsafe {
        let target = _mm_set1_epi32(func_addr as u32 as i32);
        let mut i = 0;

        while i + 2 <= count {
            let chunk = _mm_loadu_si128(ptr.add(i) as *const __m128i);
            let cmp = _mm_cmpeq_epi32(chunk, target);
            let mask = _mm_movemask_epi8(cmp) as u32;

            if mask != 0 {
                let mut bit = 0u32;
                while bit < 16 {
                    if mask & (1 << bit) != 0 {
                        let idx = i + (bit / 8) as usize;
                        if *ptr.add(idx) == func_addr {
                            return ptr.add(idx) as u64;
                        }
                    }
                    bit += 4;
                }
            }
            i += 2;
        }

        while i < count {
            if *ptr.add(i) == func_addr {
                return ptr.add(i) as u64;
            }
            i += 1;
        }
    }

    0
}
