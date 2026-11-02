//! Counting allocator for nDPI (research R9): every nDPI allocation carries
//! a 16-byte size header, and live bytes are tracked in an atomic so the
//! flow table can enforce `memory_limit_mb` by evicting flows.

// Without nDPI only the tests use the wrappers.
#![cfg_attr(not(feature = "ndpi"), allow(dead_code))]

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

/// Header size; keeps malloc's 16-byte alignment for the caller.
const HDR: usize = 16;

static LIVE: AtomicUsize = AtomicUsize::new(0);

/// Bytes currently allocated by nDPI through these wrappers.
pub fn live_bytes() -> usize {
    LIVE.load(Relaxed)
}

unsafe extern "C" fn counted_malloc(size: usize) -> *mut c_void {
    let Some(total) = size.checked_add(HDR) else {
        return std::ptr::null_mut();
    };
    // SAFETY: plain libc allocation of `total` bytes.
    let base = unsafe { libc::malloc(total) }.cast::<u8>();
    if base.is_null() {
        return base.cast();
    }
    // SAFETY: `base` has at least HDR bytes and malloc alignment ≥ usize's.
    unsafe { base.cast::<usize>().write(size) };
    LIVE.fetch_add(size, Relaxed);
    // SAFETY: `base + HDR` stays within the `total`-byte block.
    unsafe { base.add(HDR) }.cast()
}

unsafe extern "C" fn counted_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: `ptr` came from counted_malloc/realloc, so the header sits
    // HDR bytes before it and holds the user size.
    let base = unsafe { ptr.cast::<u8>().sub(HDR) };
    // SAFETY: as above.
    let size = unsafe { base.cast::<usize>().read() };
    LIVE.fetch_sub(size, Relaxed);
    // SAFETY: `base` is the pointer libc::malloc/realloc returned.
    unsafe { libc::free(base.cast()) };
}

unsafe extern "C" fn counted_calloc(n: usize, size: usize) -> *mut c_void {
    let Some(total) = n.checked_mul(size) else {
        return std::ptr::null_mut();
    };
    // SAFETY: delegates to counted_malloc.
    let p = unsafe { counted_malloc(total) };
    if !p.is_null() {
        // SAFETY: `p` is valid for `total` writable bytes.
        unsafe { std::ptr::write_bytes(p.cast::<u8>(), 0, total) };
    }
    p
}

unsafe extern "C" fn counted_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    if ptr.is_null() {
        // SAFETY: realloc(NULL, n) is malloc(n).
        return unsafe { counted_malloc(size) };
    }
    if size == 0 {
        // SAFETY: realloc(p, 0) frees p here.
        unsafe { counted_free(ptr) };
        return std::ptr::null_mut();
    }
    let Some(total) = size.checked_add(HDR) else {
        return std::ptr::null_mut();
    };
    // SAFETY: `ptr` came from these wrappers (header precedes it).
    let base = unsafe { ptr.cast::<u8>().sub(HDR) };
    // SAFETY: as above.
    let old = unsafe { base.cast::<usize>().read() };
    // SAFETY: `base` is a live libc allocation.
    let new = unsafe { libc::realloc(base.cast(), total) }.cast::<u8>();
    if new.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: `new` has at least HDR bytes.
    unsafe { new.cast::<usize>().write(size) };
    LIVE.fetch_add(size, Relaxed);
    LIVE.fetch_sub(old, Relaxed);
    // SAFETY: within the `total`-byte block.
    unsafe { new.add(HDR) }.cast()
}

/// Route nDPI's general and flow allocations through the counters. Must
/// run before any other nDPI call (ndpi_api.h).
#[cfg(feature = "ndpi")]
pub fn install() {
    // SAFETY: called once at startup before nDPI allocates anything; the
    // wrappers follow the malloc/free/calloc/realloc contracts.
    unsafe {
        ndpi_sys::ndpi_set_memory_alloction_functions(
            Some(counted_malloc),
            Some(counted_free),
            Some(counted_calloc),
            Some(counted_realloc),
            None,
            None,
            Some(counted_malloc),
            Some(counted_free),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_live_bytes_through_every_entry_point() {
        let before = live_bytes();
        // SAFETY: wrappers used as a matched malloc/realloc/free sequence.
        unsafe {
            let a = counted_malloc(100);
            let b = counted_calloc(10, 10);
            assert_eq!(live_bytes() - before, 200);
            assert_eq!(
                std::slice::from_raw_parts(b.cast::<u8>(), 100),
                &[0u8; 100][..]
            );
            let a = counted_realloc(a, 300);
            assert_eq!(live_bytes() - before, 400);
            counted_free(a);
            counted_free(b);
            counted_free(std::ptr::null_mut());
        }
        assert_eq!(live_bytes(), before);
    }
}
