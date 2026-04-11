//! Simple bump allocator for DS main RAM.
//!
//! Allocates from a 512 KB static buffer. Never frees — acceptable for
//! single-allocation use cases like the Framebuffer (192 KB at 256x192).
//!
//! Uses a plain `UnsafeCell<usize>` offset instead of atomics because
//! `armv5te-none-eabi` has `max-atomic-width: 0` (no hardware CAS)
//! and the DS ARM9 is single-threaded.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;

/// 512 KB heap — enough for Framebuffer (192 KB) + overhead.
const HEAP_SIZE: usize = 512 * 1024;

#[repr(C, align(8))]
struct HeapStorage {
    data: UnsafeCell<[u8; HEAP_SIZE]>,
    offset: UnsafeCell<usize>,
}

// SAFETY: DS is single-threaded (single ARM9 core, no preemption in our code).
unsafe impl Sync for HeapStorage {}

static HEAP: HeapStorage = HeapStorage {
    data: UnsafeCell::new([0u8; HEAP_SIZE]),
    offset: UnsafeCell::new(0),
};

pub struct BumpAlloc;

unsafe impl GlobalAlloc for BumpAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align();
        let size = layout.size();

        unsafe {
            let current = *HEAP.offset.get();
            // Align up
            let aligned = (current + align - 1) & !(align - 1);
            let new_offset = aligned + size;

            if new_offset > HEAP_SIZE {
                return core::ptr::null_mut(); // OOM
            }

            *HEAP.offset.get() = new_offset;
            let base = HEAP.data.get() as *mut u8;
            base.add(aligned)
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator never frees.
    }
}
