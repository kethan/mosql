//! Node-API addon: exposes the Rust core to JavaScript through a stable C ABI.
//!
//! Loaded with `require('./mosql.node')`. There is no `wasm-bindgen`, no
//! `napi-rs`, and no build script — the `napi_*` symbols are provided by the
//! Node process that loads the addon.
//!
//! The surface mirrors the wasm ABI so the same JS bridge can drive either:
//!
//! | function | purpose |
//! | --- | --- |
//! | `build(request)` | build SQL for one request object |
//! | `memoryOpen()` | create a collection, returns a handle |
//! | `memoryInsert(handle, docs)` | insert an array of documents |
//! | `memoryFind(handle, query, opts)` | query a collection |
//! | `memoryAggregate(handle, pipeline)` | run a pipeline |
//! | `memoryClose(handle)` | drop a collection |

#![allow(clippy::missing_safety_doc)]

use alloc::vec::Vec;

use crate::node_api::{self as napi, napi_callback_info, napi_env, napi_value};
use crate::Value;

#[cfg(feature = "memory")]
use crate::memory::Collection as MemCollection;

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

/// `args(0)` of the current call.
unsafe fn arg0(env: napi_env, info: napi_callback_info) -> crate::Result<Value> {
    let argv = napi::args(env, info, 1)?;
    napi::to_value(env, argv[0])
}

unsafe fn handle_of(env: napi_env, v: napi_value) -> crate::Result<u32> {
    let n = napi::to_value(env, v)?;
    Ok(match n {
        Value::Int(i) => i as u32,
        Value::Float(f) => f as u32,
        _ => 0,
    })
}

/// Read `argv[index]` as a collection handle.
unsafe fn handle_at(
    env: napi_env,
    info: napi_callback_info,
    argc: usize,
    index: usize,
) -> crate::Result<u32> {
    let argv = napi::args(env, info, argc)?;
    handle_of(env, argv[index])
}

unsafe fn ok(env: napi_env, v: Value) -> napi_value {
    match napi::from_value(env, &v) {
        Ok(out) => out,
        Err(e) => napi::throw(env, &e.0),
    }
}

unsafe fn fail(env: napi_env, e: crate::Error) -> napi_value {
    napi::throw(env, &e.0)
}

// ---------------------------------------------------------------------------
// Stateless builder
// ---------------------------------------------------------------------------

unsafe extern "C" fn js_build(env: napi_env, info: napi_callback_info) -> napi_value {
    let req = match arg0(env, info) {
        Ok(v) => v,
        Err(e) => return fail(env, e),
    };
    match crate::dispatch::build_request(&req) {
        Ok(v) => ok(env, v),
        Err(e) => fail(env, e),
    }
}

// ---------------------------------------------------------------------------
// Stateful memory collections
// ---------------------------------------------------------------------------

#[cfg(feature = "memory")]
mod mem_state {
    use super::*;

    // Single-threaded by construction: Node calls the addon on one isolate
    // thread, and the handles are never shared across workers.
    static mut MEM: Option<Vec<MemCollection>> = None;

    unsafe fn store() -> &'static mut Vec<MemCollection> {
        // `addr_of_mut!` keeps this a raw-pointer access, so no reference to the
        // static is ever created.
        let slot = core::ptr::addr_of_mut!(MEM);
        if (*slot).is_none() {
            *slot = Some(Vec::new());
        }
        (*slot).as_mut().unwrap()
    }

    unsafe fn coll(handle: u32) -> crate::Result<&'static mut MemCollection> {
        store()
            .get_mut(handle as usize)
            .ok_or_else(|| crate::Error::new(format!("invalid memory handle: {}", handle)))
    }

    pub(super) unsafe extern "C" fn open(env: napi_env, _info: napi_callback_info) -> napi_value {
        let store = store();
        store.push(MemCollection::new("mem", Vec::new(), Default::default()));
        ok(env, Value::Int((store.len() - 1) as i64))
    }

    pub(super) unsafe extern "C" fn insert(env: napi_env, info: napi_callback_info) -> napi_value {
        let handle = match handle_at(env, info, 2, 0) {
            Ok(h) => h,
            Err(e) => return fail(env, e),
        };
        let argv = match napi::args(env, info, 2) {
            Ok(v) => v,
            Err(e) => return fail(env, e),
        };
        let docs = match napi::to_value(env, argv[1]) {
            Ok(Value::Array(items)) => items,
            Ok(_) => Vec::new(),
            Err(e) => return fail(env, e),
        };
        match coll(handle).and_then(|c| c.insert_many(&docs)) {
            Ok(ids) => ok(env, Value::Array(ids)),
            Err(e) => fail(env, e),
        }
    }

    pub(super) unsafe extern "C" fn find(env: napi_env, info: napi_callback_info) -> napi_value {
        let argv = match napi::args(env, info, 3) {
            Ok(v) => v,
            Err(e) => return fail(env, e),
        };
        let handle = match handle_of(env, argv[0]) {
            Ok(h) => h,
            Err(e) => return fail(env, e),
        };
        let query = match napi::to_value(env, argv[1]) {
            Ok(v) => v,
            Err(e) => return fail(env, e),
        };
        let opts = match napi::to_value(env, argv[2]) {
            Ok(v) => v,
            Err(e) => return fail(env, e),
        };
        let res = coll(handle).and_then(|c| crate::dispatch::find_docs(c, &opts, &query));
        match res {
            Ok(out) => ok(env, Value::Array(out)),
            Err(e) => fail(env, e),
        }
    }

    pub(super) unsafe extern "C" fn aggregate(
        env: napi_env,
        info: napi_callback_info,
    ) -> napi_value {
        let argv = match napi::args(env, info, 2) {
            Ok(v) => v,
            Err(e) => return fail(env, e),
        };
        let handle = match handle_of(env, argv[0]) {
            Ok(h) => h,
            Err(e) => return fail(env, e),
        };
        let pipeline = match napi::to_value(env, argv[1]) {
            Ok(v) => v,
            Err(e) => return fail(env, e),
        };
        match coll(handle).and_then(|c| c.aggregate(&pipeline)) {
            Ok(out) => ok(env, Value::Array(out)),
            Err(e) => fail(env, e),
        }
    }

    pub(super) unsafe extern "C" fn close(env: napi_env, info: napi_callback_info) -> napi_value {
        let handle = match handle_at(env, info, 1, 0) {
            Ok(h) => h,
            Err(e) => return fail(env, e),
        };
        let store = store();
        if (handle as usize) < store.len() {
            store[handle as usize] = MemCollection::new("closed", Vec::new(), Default::default());
        }
        napi::from_value(env, &Value::Null).unwrap_or(core::ptr::null_mut())
    }
}

#[no_mangle]
pub extern "C" fn napi_register_module_v1(env: napi_env, exports: napi_value) -> napi_value {
    unsafe {
        napi::export(env, exports, "build", js_build);
        #[cfg(feature = "memory")]
        {
            napi::export(env, exports, "memoryOpen", mem_state::open);
            napi::export(env, exports, "memoryInsert", mem_state::insert);
            napi::export(env, exports, "memoryFind", mem_state::find);
            napi::export(env, exports, "memoryAggregate", mem_state::aggregate);
            napi::export(env, exports, "memoryClose", mem_state::close);
        }
    }
    exports
}
