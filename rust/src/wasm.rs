//! Hand-written WebAssembly ABI (no `wasm-bindgen`, no dependencies).
//!
//! Two surfaces:
//!
//! * **stateless SQL builder** — [`mosql_build`] takes a JSON request and
//!   returns a JSON response (`{"ok":true,"sql":"..."}`),
//! * **stateful memory engine** (with the `memory` feature) —
//!   `mosql_mem_open` / `mosql_mem_insert` / `mosql_mem_find` /
//!   `mosql_mem_aggregate` / `mosql_mem_close` keep documents inside wasm
//!   memory so only the query and the result cross the boundary.
//!
//! Both use a 4-byte little-endian length prefix followed by UTF-8 JSON.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::{parse_json, to_json, Value};

#[cfg(feature = "memory")]
use crate::memory::Collection as MemCollection;

fn respond(payload: String) -> *mut u8 {
    let bytes = payload.into_bytes();
    let total = bytes.len();
    let mut out = Vec::with_capacity(4 + total);
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&bytes);
    let ptr = out.as_mut_ptr();
    core::mem::forget(out);
    ptr
}

fn json_string(s: &str) -> String {
    to_json(&Value::Str(s.to_string()))
}

/// Release every transient allocation.
///
/// Only valid when no memory collection is open — see [`crate::wasm_alloc`].
#[no_mangle]
pub extern "C" fn mosql_reset() {
    #[cfg(all(target_arch = "wasm32", not(feature = "std")))]
    crate::wasm_alloc::reset();
}

/// Allocate `len` bytes that JS can write into.
#[no_mangle]
pub extern "C" fn mosql_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    core::mem::forget(buf);
    ptr
}

/// Free a buffer previously returned by [`mosql_alloc`] or any build function.
///
/// # Safety
/// `ptr` must have come from this module and `len` must be its capacity.
#[no_mangle]
pub unsafe extern "C" fn mosql_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    drop(unsafe { Vec::from_raw_parts(ptr, len, len) });
}

unsafe fn read_request(ptr: *const u8, len: usize) -> crate::Result<Value> {
    if ptr.is_null() || len == 0 {
        return Err(crate::Error::new("empty request"));
    }
    let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
    match core::str::from_utf8(slice) {
        Ok(s) => parse_json(s),
        Err(e) => Err(crate::Error::new(format!(
            "request is not valid UTF-8: {}",
            e
        ))),
    }
}

fn build_response(request: crate::Result<Value>) -> *mut u8 {
    let payload = match request {
        Ok(Value::Str(sql)) => format!("{{\"ok\":true,\"sql\":{}}}", json_string(&sql)),
        Ok(other) => format!("{{\"ok\":true,\"result\":{}}}", to_json(&other)),
        Err(e) => format!("{{\"ok\":false,\"error\":{}}}", json_string(&e.0)),
    };
    respond(payload)
}

/// Build SQL for one request. Errors are returned as JSON, never panics.
///
/// # Safety
/// `ptr` must point to `len` valid UTF-8 bytes.
#[no_mangle]
pub unsafe extern "C" fn mosql_build(ptr: *const u8, len: usize) -> *mut u8 {
    let request = match read_request(ptr, len) {
        Ok(req) => crate::dispatch::build_request(&req),
        Err(e) => Err(e),
    };
    build_response(request)
}

// ---------------------------------------------------------------------------
// Stateful memory collections
// ---------------------------------------------------------------------------
//
// A `static mut` is the only way to hold state across ABI calls without a
// dependency, and this module is single-threaded by construction (wasm has no
// threads here), so the accesses below do not alias.

#[cfg(feature = "memory")]
mod mem_state {
    use super::*;

    static mut MEM: Option<Vec<MemCollection>> = None;

    unsafe fn mem_store() -> &'static mut Vec<MemCollection> {
        let slot = core::ptr::addr_of_mut!(MEM);
        if (*slot).is_none() {
            *slot = Some(Vec::new());
        }
        (*slot).as_mut().unwrap()
    }

    unsafe fn mem_coll(handle: u32) -> crate::Result<&'static mut MemCollection> {
        let store = mem_store();
        store
            .get_mut(handle as usize)
            .ok_or_else(|| crate::Error::new(format!("invalid memory handle: {}", handle)))
    }

    /// Create an empty memory collection and return its handle.
    #[no_mangle]
    pub extern "C" fn mosql_mem_open() -> u32 {
        #[cfg(all(target_arch = "wasm32", not(feature = "std")))]
        crate::wasm_alloc::persist();
        unsafe {
            let store = mem_store();
            store.push(MemCollection::new("mem", Vec::new(), Default::default()));
            (store.len() - 1) as u32
        }
    }

    /// Insert a JSON array of documents.
    ///
    /// # Safety
    /// `ptr` must point to `len` valid UTF-8 bytes.
    #[no_mangle]
    pub unsafe extern "C" fn mosql_mem_insert(handle: u32, ptr: *const u8, len: usize) -> *mut u8 {
        let result = (|| -> crate::Result<Value> {
            let req = read_request(ptr, len)?;
            let docs = req
                .get("docs")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let ids = mem_coll(handle)?.insert_many(&docs)?;
            Ok(Value::Array(ids))
        })();
        build_response(result)
    }

    /// Query a memory collection.
    ///
    /// # Safety
    /// `ptr` must point to `len` valid UTF-8 bytes.
    #[no_mangle]
    pub unsafe extern "C" fn mosql_mem_find(handle: u32, ptr: *const u8, len: usize) -> *mut u8 {
        let result = (|| -> crate::Result<Value> {
            let req = read_request(ptr, len)?;
            let query = req
                .get("query")
                .cloned()
                .unwrap_or_else(|| Value::Object(Default::default()));
            let coll = mem_coll(handle)?;
            let out = crate::dispatch::find_docs(coll, &req, &query)?;
            Ok(Value::Array(out))
        })();
        build_response(result)
    }

    /// Run an aggregation pipeline over a memory collection.
    ///
    /// # Safety
    /// `ptr` must point to `len` valid UTF-8 bytes.
    #[no_mangle]
    pub unsafe extern "C" fn mosql_mem_aggregate(
        handle: u32,
        ptr: *const u8,
        len: usize,
    ) -> *mut u8 {
        let result = (|| -> crate::Result<Value> {
            let req = read_request(ptr, len)?;
            let pipeline = req
                .get("pipeline")
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new()));
            let coll = mem_coll(handle)?;
            let out = coll.aggregate(&pipeline)?;
            Ok(Value::Array(out))
        })();
        build_response(result)
    }

    /// Drop a memory collection.
    #[no_mangle]
    pub extern "C" fn mosql_mem_close(handle: u32) {
        unsafe {
            let store = mem_store();
            if (handle as usize) < store.len() {
                store[handle as usize] =
                    MemCollection::new("closed", Vec::new(), Default::default());
            }
        }
    }
}
