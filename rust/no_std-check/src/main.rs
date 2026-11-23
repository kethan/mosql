//! Bare-metal consumer: no `std`, no allocator provided for us.
//!
//! `mosql` is `#![no_std]` + `alloc`, so a `no_std` consumer must supply the
//! allocator and panic handler — exactly what this does. If the crate ever
//! grows a hidden `std` dependency, this stops linking.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

use mosql::memory::Collection;
use mosql::{parse_json, Builder, Db, Value};

const HEAP_SIZE: usize = 96 * 1024;

/// A bump allocator: enough to prove the engine runs without `std`.
struct Bump {
    offset: AtomicUsize,
    heap: UnsafeCell<[u8; HEAP_SIZE]>,
}

unsafe impl Sync for Bump {}

unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = (layout.size() + 15) & !15;
        let base = self.heap.get() as usize;
        loop {
            let off = self.offset.load(Ordering::Relaxed);
            let start = (off + layout.align() - 1) & !(layout.align() - 1);
            let end = start + size;
            if end > HEAP_SIZE {
                return core::ptr::null_mut();
            }
            if self
                .offset
                .compare_exchange_weak(off, end, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return (base + start) as *mut u8;
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // A bump allocator never frees; fine for a link-time check.
    }
}

#[global_allocator]
static ALLOC: Bump = Bump {
    offset: AtomicUsize::new(0),
    heap: UnsafeCell::new([0; HEAP_SIZE]),
};

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// Touch both halves of the crate. The result is written to a `static` so the
/// optimizer cannot discard the calls — this must actually execute the code.
static mut SINK: u32 = 0;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut checksum = 0u32;

    if let Ok(q) = parse_json(r#"{"age":{"$gte":18}}"#) {
        if let Ok(coll) = Builder::full().collection("users", Db::Pg) {
            if let Ok(q2) = coll.find(&q, None) {
                if let Ok(q3) = q3_limit(q2) {
                    if let Ok(sql) = q3.to_sql() {
                        checksum = checksum.wrapping_add(sql.len() as u32);
                    }
                }
            }
        }
    }

    let docs = vec![
        parse_json(r#"{"name":"Ada","age":36}"#).unwrap_or(Value::Null),
        parse_json(r#"{"name":"Bob","age":17}"#).unwrap_or(Value::Null),
    ];
    let coll = Collection::new("users", docs, Default::default());
    if let Ok(q) = parse_json(r#"{"age":{"$gte":18}}"#) {
        if let Ok(hits) = coll.find(&q).to_vec() {
            checksum = checksum.wrapping_add(hits.len() as u32);
        }
    }

    unsafe { SINK = checksum };
    loop {
        core::hint::spin_loop();
    }
}

fn q3_limit(q: mosql::FindQuery) -> mosql::Result<mosql::FindQuery> {
    q.limit(&Value::Int(10))
}
