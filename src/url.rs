use core::ptr;

use crate::unreal::{FMemory, FString};

pub struct ParsedUrl {
    pub protocol: FString,
    pub separator: FString,
    pub domain: FString,
    pub port: FString,
    pub path: FString,
    pub query: FString,
}

impl Drop for ParsedUrl {
    fn drop(&mut self) {
    }
}

impl ParsedUrl {
    pub fn new() -> Self {
        Self {
            protocol: FString::new(),
            separator: FString::new(),
            domain: FString::new(),
            port: FString::new(),
            path: FString::new(),
            query: FString::new(),
        }
    }

    pub fn parse(url: &FString) -> Self {
        if url.string.is_null() {
            return Self::new();
        }

        let mut result = Self::new();
        let s = url.string;
        let len = url.length as usize;

        let mut proto_end = 0;
        while proto_end < len && unsafe { *s.add(proto_end) } != b':' as u16 {
            proto_end += 1;
        }
        result.protocol = substring(url, 0, proto_end);

        let sep_size =
            if proto_end + 2 < len && unsafe { *s.add(proto_end + 1) == '/' as u16 }
                && unsafe { *s.add(proto_end + 2) == '/' as u16 }
            {
                3
            } else {
                1
            };
        result.separator = substring(url, proto_end, sep_size);

        let domain_start = proto_end + sep_size;
        let mut domain_and_port_end = domain_start;
        while domain_and_port_end < len && unsafe { *s.add(domain_and_port_end) } != b'/' as u16 {
            domain_and_port_end += 1;
        }

        let domain_and_port = substring(url, domain_start, domain_and_port_end - domain_start);
        let path_str = substring(url, domain_and_port_end, len - domain_and_port_end);

        let mut port_off = 0;
        while port_off < domain_and_port.length as usize
            && unsafe { *domain_and_port.string.add(port_off) } != b':' as u16
        {
            port_off += 1;
        }

        if port_off < domain_and_port.length as usize {
            result.domain = substring(&domain_and_port, 0, port_off);
            result.port = substring(
                &domain_and_port,
                port_off,
                domain_and_port.length as usize - port_off,
            );
        } else {
            result.domain = domain_and_port;
        }

        let mut query_off = 0;
        while query_off < path_str.length as usize
            && unsafe { *path_str.string.add(query_off) } != b'?' as u16
        {
            query_off += 1;
        }

        result.path = substring(&path_str, 0, query_off);
        if query_off < path_str.length as usize {
            result.query = substring(
                &path_str,
                query_off,
                path_str.length as usize - query_off,
            );
        }

        result
    }

    pub fn set_host(&mut self, host: &FString) {
        if host.string.is_null() {
            return;
        }

        let s = host.string;
        let len = host.length as usize;

        let mut proto_end = 0;
        while proto_end < len && unsafe { *s.add(proto_end) } != b':' as u16 {
            proto_end += 1;
        }

        drop(core::mem::replace(&mut self.protocol, substring(host, 0, proto_end)));

        let sep_size =
            if proto_end + 2 < len && unsafe { *s.add(proto_end + 1) == '/' as u16 }
                && unsafe { *s.add(proto_end + 2) == '/' as u16 }
            {
                3
            } else {
                1
            };

        drop(core::mem::replace(&mut self.separator, substring(host, proto_end, sep_size)));

        let domain_start = proto_end + sep_size;
        let mut domain_end = domain_start;
        while domain_end < len && unsafe { *s.add(domain_end) } != b'/' as u16 {
            domain_end += 1;
        }

        let domain_and_port = substring(host, domain_start, domain_end - domain_start);

        let mut port_off = 0;
        while port_off < domain_and_port.length as usize
            && unsafe { *domain_and_port.string.add(port_off) } != b':' as u16
        {
            port_off += 1;
        }

        if port_off < domain_and_port.length as usize {
            drop(core::mem::replace(&mut self.domain, substring(&domain_and_port, 0, port_off)));
            drop(core::mem::replace(&mut self.port, substring(
                &domain_and_port,
                port_off,
                domain_and_port.length as usize - port_off,
            )));
        } else {
            drop(core::mem::replace(&mut self.domain, domain_and_port));
        }
    }

    pub fn get_url(&self) -> FString {
        let proto_len = if !self.protocol.string.is_null() && self.protocol.length > 0 {
            (self.protocol.length - 1) as usize
        } else {
            0
        };
        let sep_len = if !self.separator.string.is_null() && self.separator.length > 0 {
            (self.separator.length - 1) as usize
        } else {
            0
        };
        let domain_len = if !self.domain.string.is_null() && self.domain.length > 0 {
            (self.domain.length - 1) as usize
        } else {
            0
        };
        let port_len = if !self.port.string.is_null() && self.port.length > 0 {
            (self.port.length - 1) as usize
        } else {
            0
        };
        let path_len = if !self.path.string.is_null() && self.path.length > 0 {
            (self.path.length - 1) as usize
        } else {
            0
        };
        let query_len = if !self.query.string.is_null() && self.query.length > 0 {
            (self.query.length - 1) as usize
        } else {
            0
        };

        let total = proto_len + sep_len + domain_len + port_len + path_len + query_len + 1;
        let alloc = FMemory::malloc((total * 2) as u64) as *mut u16;
        if alloc.is_null() {
            return FString::new();
        }

        unsafe {
            let mut pos = 0;
            if !self.protocol.string.is_null() {
                ptr::copy_nonoverlapping(self.protocol.string, alloc.add(pos), proto_len);
                pos += proto_len;
            }
            if !self.separator.string.is_null() {
                ptr::copy_nonoverlapping(self.separator.string, alloc.add(pos), sep_len);
                pos += sep_len;
            }
            if !self.domain.string.is_null() {
                ptr::copy_nonoverlapping(self.domain.string, alloc.add(pos), domain_len);
                pos += domain_len;
            }
            if !self.port.string.is_null() {
                ptr::copy_nonoverlapping(self.port.string, alloc.add(pos), port_len);
                pos += port_len;
            }
            if !self.path.string.is_null() {
                ptr::copy_nonoverlapping(self.path.string, alloc.add(pos), path_len);
                pos += path_len;
            }
            if !self.query.string.is_null() {
                ptr::copy_nonoverlapping(self.query.string, alloc.add(pos), query_len);
                pos += query_len;
            }
            *alloc.add(pos) = 0;
        }

        FString {
            string: alloc,
            length: total as u32,
            max_size: total as u32,
        }
    }
}

fn substring(s: &FString, offset: usize, count: usize) -> FString {
    if s.string.is_null() || offset >= s.length as usize {
        return FString::new();
    }
    let actual_count = count.min(s.length as usize - offset - 1);
    let alloc = FMemory::malloc(((actual_count + 1) * 2) as u64) as *mut u16;
    if alloc.is_null() {
        return FString::new();
    }
    unsafe {
        ptr::copy_nonoverlapping(s.string.add(offset), alloc, actual_count);
        *alloc.add(actual_count) = 0;
    }
    FString {
        string: alloc,
        length: (actual_count + 1) as u32,
        max_size: (actual_count + 1) as u32,
    }
}
