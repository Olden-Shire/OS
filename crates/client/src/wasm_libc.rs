// custom — C symbol shims for dear imgui on wasm32-unknown-unknown.
//
// imgui-sys compiles imgui's C++ with clang against the freestanding
// headers in crates/client/wasm-libc/include; the symbols those headers
// declare resolve here. mem* (memset/memcpy/memmove/memcmp) and the math
// library (cosf/powf/...) already ship in Rust's compiler-builtins for
// wasm, and the printf family is stb_sprintf (IMGUI_USE_STB_SPRINTF), so
// what's left is strings, ctype, qsort, malloc and number parsing.
//
// malloc routes through std::alloc, i.e. through perf::CountingAllocator —
// imgui's heap shows up in the Mem: counter like everything else.

#![cfg(target_arch = "wasm32")]
#![allow(clippy::missing_safety_doc)]

use std::alloc::{alloc, dealloc, Layout};

// 16 bytes in front of every allocation to remember the full layout size
// (C free() doesn't pass one back).
const HDR: usize = 16;

fn layout(total: usize) -> Layout {
    Layout::from_size_align(total, 16).expect("wasm_libc layout")
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn malloc(size: usize) -> *mut u8 {
    let total = size.saturating_add(HDR);
    let p = unsafe { alloc(layout(total)) };
    if p.is_null() {
        return p;
    }
    unsafe {
        (p as *mut usize).write(total);
        p.add(HDR)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let base = ptr.sub(HDR);
        let total = (base as *const usize).read();
        dealloc(base, layout(total));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn realloc(ptr: *mut u8, size: usize) -> *mut u8 {
    if ptr.is_null() {
        return unsafe { malloc(size) };
    }
    unsafe {
        let base = ptr.sub(HDR);
        let old_total = (base as *const usize).read();
        let new = malloc(size);
        if !new.is_null() {
            let copy = size.min(old_total - HDR);
            std::ptr::copy_nonoverlapping(ptr, new, copy);
        }
        free(ptr);
        new
    }
}

// ── string.h ────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strlen(mut s: *const u8) -> usize {
    let mut n = 0;
    unsafe {
        while *s != 0 {
            s = s.add(1);
            n += 1;
        }
    }
    n
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcmp(a: *const u8, b: *const u8) -> i32 {
    unsafe { strncmp(a, b, usize::MAX) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strncmp(mut a: *const u8, mut b: *const u8, mut n: usize) -> i32 {
    unsafe {
        while n > 0 {
            let (ca, cb) = (*a, *b);
            if ca != cb || ca == 0 {
                return ca as i32 - cb as i32;
            }
            a = a.add(1);
            b = b.add(1);
            n -= 1;
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcpy(dst: *mut u8, src: *const u8) -> *mut u8 {
    unsafe {
        let n = strlen(src);
        std::ptr::copy_nonoverlapping(src, dst, n + 1);
    }
    dst
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strncpy(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        let len = strlen(src).min(n);
        std::ptr::copy_nonoverlapping(src, dst, len);
        std::ptr::write_bytes(dst.add(len), 0, n - len);
    }
    dst
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcat(dst: *mut u8, src: *const u8) -> *mut u8 {
    unsafe {
        strcpy(dst.add(strlen(dst)), src);
    }
    dst
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strchr(mut s: *const u8, c: i32) -> *const u8 {
    let c = c as u8;
    unsafe {
        loop {
            if *s == c {
                return s;
            }
            if *s == 0 {
                return std::ptr::null();
            }
            s = s.add(1);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strrchr(s: *const u8, c: i32) -> *const u8 {
    let c = c as u8;
    let mut found = std::ptr::null();
    let mut p = s;
    unsafe {
        loop {
            if *p == c {
                found = p;
            }
            if *p == 0 {
                return found;
            }
            p = p.add(1);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strstr(hay: *const u8, needle: *const u8) -> *const u8 {
    unsafe {
        let nlen = strlen(needle);
        if nlen == 0 {
            return hay;
        }
        let hlen = strlen(hay);
        if hlen < nlen {
            return std::ptr::null();
        }
        for i in 0..=(hlen - nlen) {
            if strncmp(hay.add(i), needle, nlen) == 0 {
                return hay.add(i);
            }
        }
        std::ptr::null()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memchr(s: *const u8, c: i32, n: usize) -> *const u8 {
    let c = c as u8;
    unsafe {
        for i in 0..n {
            if *s.add(i) == c {
                return s.add(i);
            }
        }
    }
    std::ptr::null()
}

// ── ctype.h ─────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn toupper(c: i32) -> i32 {
    (c as u8 as char).to_ascii_uppercase() as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn tolower(c: i32) -> i32 {
    (c as u8 as char).to_ascii_lowercase() as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn isspace(c: i32) -> i32 {
    matches!(c as u8, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn isdigit(c: i32) -> i32 {
    (c as u8).is_ascii_digit() as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn isalnum(c: i32) -> i32 {
    (c as u8).is_ascii_alphanumeric() as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn isprint(c: i32) -> i32 {
    (0x20..0x7F).contains(&c) as i32
}

// ── stdlib.h ────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub unsafe extern "C" fn qsort(
    base: *mut u8,
    count: usize,
    size: usize,
    cmp: unsafe extern "C" fn(*const u8, *const u8) -> i32,
) {
    // Insertion sort — imgui sorts tiny arrays (settings, columns).
    unsafe {
        let mut tmp = vec![0u8; size];
        for i in 1..count {
            let mut j = i;
            std::ptr::copy_nonoverlapping(base.add(i * size), tmp.as_mut_ptr(), size);
            while j > 0 && cmp(base.add((j - 1) * size), tmp.as_ptr()) > 0 {
                std::ptr::copy_nonoverlapping(base.add((j - 1) * size), base.add(j * size), size);
                j -= 1;
            }
            std::ptr::copy_nonoverlapping(tmp.as_ptr(), base.add(j * size), size);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtod(s: *const u8, end: *mut *const u8) -> f64 {
    unsafe {
        let mut p = s;
        while isspace(*p as i32) != 0 {
            p = p.add(1);
        }
        let start = p;
        if *p == b'+' || *p == b'-' {
            p = p.add(1);
        }
        while (*p).is_ascii_digit() {
            p = p.add(1);
        }
        if *p == b'.' {
            p = p.add(1);
            while (*p).is_ascii_digit() {
                p = p.add(1);
            }
        }
        if *p == b'e' || *p == b'E' {
            let mut q = p.add(1);
            if *q == b'+' || *q == b'-' {
                q = q.add(1);
            }
            if (*q).is_ascii_digit() {
                while (*q).is_ascii_digit() {
                    q = q.add(1);
                }
                p = q;
            }
        }
        let len = p.offset_from(start) as usize;
        let txt = std::str::from_utf8_unchecked(std::slice::from_raw_parts(start, len));
        if !end.is_null() {
            *end = p;
        }
        txt.parse::<f64>().unwrap_or(0.0)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn atof(s: *const u8) -> f64 {
    unsafe { strtod(s, std::ptr::null_mut()) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn atoi(s: *const u8) -> i32 {
    unsafe { strtod(s, std::ptr::null_mut()) as i32 }
}

#[unsafe(no_mangle)]
pub extern "C" fn abs(v: i32) -> i32 {
    v.wrapping_abs()
}

// ── stdio.h ─────────────────────────────────────────────────────────────

// imgui references sscanf through the ImGuiDataTypeInfo table (InputScalar
// text parsing). The overlay has no input widgets; parse nothing. The wasm
// C ABI lowers `...` to one trailing pointer arg, so these non-variadic
// signatures link-match the variadic declarations.
#[unsafe(no_mangle)]
pub extern "C" fn sscanf(_s: *const u8, _fmt: *const u8, _va: *const u8) -> i32 {
    0
}

// stb_sprintf is compiled into imgui-sys (IMGUI_USE_STB_SPRINTF), and its
// va_list on wasm IS the spilled-args pointer — delegate the printf family
// to it instead of porting a formatter.
unsafe extern "C" {
    fn stbsp_vsnprintf(buf: *mut u8, count: i32, fmt: *const u8, va: *const u8) -> i32;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vsnprintf(buf: *mut u8, n: usize, fmt: *const u8, va: *const u8) -> i32 {
    unsafe { stbsp_vsnprintf(buf, n as i32, fmt, va) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn snprintf(buf: *mut u8, n: usize, fmt: *const u8, va: *const u8) -> i32 {
    unsafe { stbsp_vsnprintf(buf, n as i32, fmt, va) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sprintf(buf: *mut u8, fmt: *const u8, va: *const u8) -> i32 {
    unsafe { stbsp_vsnprintf(buf, i32::MAX, fmt, va) }
}

// Debug-TTY logging (IMGUI_DEBUG_PRINTF path) — drop it.
#[unsafe(no_mangle)]
pub extern "C" fn printf(_fmt: *const u8, _va: *const u8) -> i32 {
    0
}
