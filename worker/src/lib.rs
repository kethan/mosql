//! The authentication engine, as a Cloudflare Worker.
//!
//! This crate is the Rust half of the deployment. It is compiled to
//! WebAssembly and imported by the Worker script, which is the only other
//! half: the script holds the bindings — D1, KV, R2 — and the WebAssembly
//! module asks it to use them.
//!
//! # The protocol
//!
//! Six functions cross the line, all of them exported by the module:
//!
//! | export | what it does |
//! | --- | --- |
//! | `mosql_worker_alloc` / `mosql_worker_free` | memory for the host to write into |
//! | `mosql_worker_init` | read the configuration and build the engine |
//! | `mosql_worker_run` | start answering a request |
//! | `mosql_worker_resume` | hand over the answer to a question |
//! | `mosql_worker_take` | read the last thing the module said |
//!
//! `mosql_worker_run` and `mosql_worker_resume` answer with a status:
//! `1` means the module is asking the host to do something, `0` means the
//! response is ready, and `-1` means something went wrong. In every case
//! the message is read with `mosql_worker_take`.
//!
//! # Why it works this way
//!
//! A Worker is single-threaded, and the database is a promise. Rust cannot
//! await a JavaScript promise without a binding layer, so the engine does
//! not try: it *asks*. The request handling is an ordinary Rust future
//! that stops at the point where it needs the database, and the host —
//! which does have the binding, and does have an event loop — runs the
//! statement and continues the future. The engine is the same engine the
//! native binary runs, with the same routes, the same SQL and the same
//! records; only the way out to the world is different.

pub mod blob;
pub mod bus;
pub mod cache;
pub mod d1;
pub mod engine;
pub mod fetch;
pub mod host;

use std::alloc::{alloc, dealloc, Layout};
use std::sync::{Arc, Mutex, OnceLock};

use bus::{Bus, Root, Step};
use engine::Engine;

/// The status the module answers with.
mod status {
    /// The response is ready.
    pub const DONE: i32 = 0;
    /// The module is asking the host to do something.
    pub const ASK: i32 = 1;
    /// Something went wrong.
    pub const ERROR: i32 = -1;
}

static BUS: OnceLock<Bus> = OnceLock::new();
static ENGINE: OnceLock<Arc<Engine>> = OnceLock::new();
static ROOT: Mutex<Option<Root>> = Mutex::new(None);
static OUT: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// Memory for the host to write into.
///
/// The host writes a request, a configuration or an answer into it, and
/// hands the pointer back. It must be released with
/// [`mosql_worker_free`].
#[no_mangle]
pub extern "C" fn mosql_worker_alloc(len: u32) -> *mut u8 {
    if len == 0 {
        return std::ptr::null_mut();
    }
    // SAFETY: the layout is non-zero, and the allocation is released with
    // the same layout in `mosql_worker_free`.
    unsafe { alloc(Layout::array::<u8>(len as usize).expect("layout")) }
}

/// Release memory the host was given.
///
/// # Safety
///
/// The pointer and the length must come from [`mosql_worker_alloc`], and
/// the memory must not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn mosql_worker_free(ptr: *mut u8, len: u32) {
    if ptr.is_null() || len == 0 {
        return;
    }
    // SAFETY: the pointer and the length came from `mosql_worker_alloc`.
    dealloc(ptr, Layout::array::<u8>(len as usize).expect("layout"));
}

/// Build the engine from the host's configuration.
///
/// The configuration is a JSON object; see [`engine::build`]. Calling this
/// again does nothing: the engine is built once per isolate.
#[no_mangle]
pub extern "C" fn mosql_worker_init(ptr: *const u8, len: u32) -> i32 {
    host::install();
    let Some(config) = read_json(ptr, len) else {
        return status::ERROR;
    };
    let bus = BUS.get_or_init(Bus::new).clone();
    match engine::build(&config, bus) {
        Ok(engine) => {
            let _ = ENGINE.set(Arc::new(engine));
            say(&mosql::Value::ok());
            status::DONE
        }
        Err(message) => fail(&message),
    }
}

/// Start answering a request.
///
/// The request is the host's JSON; see [`engine::parse_request`]. The
/// answer is `1` when the module is asking the host to do something, `0`
/// when the response is ready, and `-1` when something went wrong.
#[no_mangle]
pub extern "C" fn mosql_worker_run(ptr: *const u8, len: u32) -> i32 {
    let Some(engine) = ENGINE.get() else {
        return fail("the engine is not initialised");
    };
    let Some(request) = read_json(ptr, len) else {
        return status::ERROR;
    };
    let request = match engine::parse_request(&request) {
        Ok(request) => request,
        Err(message) => return fail(&message),
    };
    let engine = Arc::clone(engine);
    let root: Root = Box::pin(async move {
        let response = engine::handle(&engine, request).await;
        engine::encode_response(&response)
    });
    {
        let mut guard = ROOT.lock().expect("root");
        *guard = Some(root);
    }
    drive()
}

/// Hand the host's answer to the module's question, and continue.
///
/// The answer is the host's JSON; its shape depends on what was asked
/// for. The answer is the status of the last call to
/// [`mosql_worker_run`] or [`mosql_worker_resume`].
#[no_mangle]
pub extern "C" fn mosql_worker_resume(ptr: *const u8, len: u32) -> i32 {
    let Some(bus) = BUS.get() else {
        return fail("the engine is not initialised");
    };
    let Some(answer) = read_json(ptr, len) else {
        return status::ERROR;
    };
    if let Err(message) = bus.deliver(answer) {
        return fail(&message);
    }
    drive()
}

/// The last thing the module said.
///
/// The length is written to `len`, and the bytes are in the module's
/// memory at the returned pointer. They stay there until the module says
/// something else.
///
/// # Safety
///
/// `len` must point at four bytes of writable memory.
#[no_mangle]
pub unsafe extern "C" fn mosql_worker_take(len: *mut u32) -> *const u8 {
    let out = OUT.lock().expect("out");
    // SAFETY: the host passes a pointer to four bytes of its own memory.
    unsafe {
        *len = out.len() as u32;
    }
    out.as_ptr()
}

/// Poll the engine and report what it wants.
fn drive() -> i32 {
    let Some(bus) = BUS.get() else {
        return fail("the engine is not initialised");
    };
    let mut guard = ROOT.lock().expect("root");
    let outcome = {
        let Some(root) = guard.as_mut() else {
            return fail("no request is running");
        };
        bus::step(root, bus)
    };
    match outcome {
        Step::Done(value) => {
            *guard = None;
            say(&value);
            status::DONE
        }
        Step::Ask(op) => {
            say(&op);
            status::ASK
        }
        Step::Stalled => {
            *guard = None;
            fail("the engine stopped without asking for anything")
        }
    }
}

/// Say something, and remember it for [`mosql_worker_take`].
fn say(value: &mosql::Value) {
    let text = mosql::to_json(value);
    let mut out = OUT.lock().expect("out");
    out.clear();
    out.extend_from_slice(text.as_bytes());
}

/// Say that something went wrong.
fn fail(message: &str) -> i32 {
    let mut value = mosql::Object::new();
    value.set("error", mosql::Value::str(message));
    say(&mosql::Value::Object(value));
    status::ERROR
}

/// Read the host's JSON.
fn read_json(ptr: *const u8, len: u32) -> Option<mosql::Value> {
    if ptr.is_null() || len == 0 {
        return None;
    }
    // SAFETY: the host wrote `len` bytes at `ptr` and has not freed them.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let text = String::from_utf8_lossy(bytes);
    match mosql::parse_json(&text) {
        Ok(value) => Some(value),
        Err(error) => {
            fail(&format!(
                "the host sent something that is not JSON: {error}"
            ));
            None
        }
    }
}

/// A tiny helper, so the module can say "all right" without a value.
trait ValueExt {
    fn ok() -> mosql::Value;
}

impl ValueExt for mosql::Value {
    fn ok() -> mosql::Value {
        let mut value = mosql::Object::new();
        value.set("ok", mosql::Value::Bool(true));
        mosql::Value::Object(value)
    }
}
