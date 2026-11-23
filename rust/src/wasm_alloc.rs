//! Allocator for the freestanding wasm build.
//!
//! `wasm32-unknown-unknown` ships no allocator, so this provides one. Two
//! allocation lifetimes matter here:
//!
//! * **persistent** — documents kept in a memory collection across calls,
//! * **transient** — request/response buffers and the `Value`s built while
//!   evaluating a query.
//!
//! A pure bump allocator leaks the transient buffers, so this is a bump
//! allocator with an **exact-size free cache**: every block is rounded to a
//! 16-byte granule and bucketed by that granule count, so a lookup is a single
//! array index and can never return a block of the wrong size. The first 16
//! bytes of a free block hold the link to the next free block of that size.
//! Blocks larger than [`LARGEST_BUCKET`] share one overflow list.

use core::alloc::{GlobalAlloc, Layout};
use core::arch::wasm32::{memory_grow, memory_size};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const PAGE: usize = 65536;
const GRANULE: usize = 16;
/// Buckets 0..=LARGEST_BUCKET/16 hold blocks of exactly that many granules.
const LARGEST_BUCKET: usize = 16 * 1024;
const NUM_BUCKETS: usize = LARGEST_BUCKET / GRANULE + 1;

static mut HEAP_BASE: usize = 0;
static OFFSET: AtomicUsize = AtomicUsize::new(0);
/// One free list per exact size, plus a final overflow list.
static mut FREE_HEAD: [usize; NUM_BUCKETS + 1] = [0; NUM_BUCKETS + 1];
/// When set, every allocation is transient: the bump pointer is all the
/// bookkeeping needed, so the free lists are bypassed entirely.
static TRANSIENT: AtomicBool = AtomicBool::new(true);
/// Highest bucket index ever used, so [`reset`] only clears what it must.
static mut FREE_HIGH: usize = 0;
const OVERFLOW: usize = NUM_BUCKETS;

#[inline]
fn round_up(n: usize) -> usize {
    (n + GRANULE - 1) & !(GRANULE - 1)
}

/// Index of the bucket that owns blocks of exactly `size` bytes.
///
/// `size` must be a non-zero multiple of [`GRANULE`].
#[inline]
fn bucket_of(size: usize) -> usize {
    let g = size / GRANULE;
    if g < NUM_BUCKETS {
        g
    } else {
        OVERFLOW
    }
}

pub struct WasmAlloc;

/// Drop every transient allocation.
///
/// Only safe when nothing is meant to survive the call, i.e. when no memory
/// collection has been opened. The JS bridge calls this automatically in that
/// case, which keeps stateless SQL building on the fast bump path.
pub fn reset() {
    OFFSET.store(0, Ordering::Relaxed);
    TRANSIENT.store(true, Ordering::Relaxed);
    unsafe {
        for i in 0..=FREE_HIGH {
            FREE_HEAD[i] = 0;
        }
        FREE_HIGH = 0;
    }
}

/// Switch to persistent mode: allocations may outlive the current call.
///
/// Only reachable from the memory-collection API, so it is compiled out of a
/// builder-only wasm binary.
#[cfg(feature = "memory")]
pub fn persist() {
    TRANSIENT.store(false, Ordering::Relaxed);
}

unsafe fn heap_base() -> usize {
    let base = HEAP_BASE;
    if base != 0 {
        return base;
    }
    let base = memory_size(0) as usize * PAGE;
    HEAP_BASE = base;
    base
}

/// Push a block onto the free list for its exact size.
///
/// # Safety
/// `ptr` must be 16-byte aligned, `size` a positive multiple of 16, and the
/// block must not be in use.
unsafe fn push_free(ptr: usize, size: usize) {
    let b = bucket_of(size);
    // Every block is at least one granule, so both words always fit.
    unsafe {
        (ptr as *mut usize).write(FREE_HEAD[b]);
        (ptr as *mut usize).add(1).write(size);
    }
    FREE_HEAD[b] = ptr;
    if b > FREE_HIGH {
        FREE_HIGH = b;
    }
}

/// Pop a free block of exactly `size` bytes, or search the overflow list.
///
/// # Safety
/// Caller must not hold references into the free lists.
unsafe fn take_free(size: usize) -> *mut u8 {
    let b = bucket_of(size);
    if b != OVERFLOW {
        // Exact bucket first, then a few larger ones: a larger block can always
        // be split, and its exact size is known from its bucket index.
        let last = (b + 8).min(NUM_BUCKETS);
        let mut i = b;
        while i < last {
            let head = FREE_HEAD[i];
            if head != 0 {
                FREE_HEAD[i] = unsafe { *(head as *const usize) };
                let have = i * GRANULE;
                if have >= size + GRANULE {
                    unsafe { push_free(head + size, have - size) };
                }
                return head as *mut u8;
            }
            i += 1;
        }
        return core::ptr::null_mut();
    }

    // Overflow: walk the list for the first block that is large enough, then
    // split the tail back onto the list.
    let mut prev = 0usize;
    let mut cur = FREE_HEAD[OVERFLOW];
    while cur != 0 {
        let next = unsafe { *(cur as *const usize) };
        let sz = unsafe { *((cur as *const usize).add(1)) };
        if sz >= size {
            if prev == 0 {
                FREE_HEAD[OVERFLOW] = next;
            } else {
                unsafe { *(prev as *mut usize) = next };
            }
            if sz >= size + GRANULE {
                unsafe { push_free(cur + size, sz - size) };
            }
            return cur as *mut u8;
        }
        prev = cur;
        cur = next;
    }
    core::ptr::null_mut()
}

unsafe fn bump(align: usize, size: usize) -> *mut u8 {
    let base = heap_base();
    loop {
        let off = OFFSET.load(Ordering::Relaxed);
        let start = (off + align - 1) & !(align - 1);
        let end = start + size;
        let limit = memory_size(0) as usize * PAGE;
        if base + end > limit {
            let need = (base + end) - limit;
            let pages = (need + PAGE - 1) / PAGE;
            if memory_grow(0, pages) == usize::MAX {
                return core::ptr::null_mut();
            }
            continue;
        }
        match OFFSET.compare_exchange_weak(off, end, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return (base + start) as *mut u8,
            Err(_) => continue,
        }
    }
}

unsafe impl GlobalAlloc for WasmAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = round_up(layout.size().max(1));
        let align = layout.align();

        if align <= GRANULE && !TRANSIENT.load(Ordering::Relaxed) {
            let reused = take_free(size);
            if !reused.is_null() {
                return reused;
            }
        }

        // `GRANULE` is a multiple of every alignment <= 16, so forcing the
        // minimum alignment keeps every block reusable.
        let align = if align <= GRANULE { GRANULE } else { align };
        bump(align, size)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() || layout.size() == 0 || TRANSIENT.load(Ordering::Relaxed) {
            // A zero-sized allocation owns no memory; its pointer is dangling.
            return;
        }
        if layout.align() <= GRANULE {
            push_free(ptr as usize, round_up(layout.size()));
        }
        // Over-aligned blocks are not recycled; they are rare and small.
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size == 0 {
            self.dealloc(ptr, layout);
            return core::ptr::null_mut();
        }
        let new = self.alloc(Layout::from_size_align_unchecked(new_size, layout.align()));
        if new.is_null() {
            return new;
        }
        core::ptr::copy_nonoverlapping(ptr, new, layout.size().min(new_size));
        self.dealloc(ptr, layout);
        new
    }
}

#[global_allocator]
static ALLOC: WasmAlloc = WasmAlloc;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
