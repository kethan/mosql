//! Hand-written Node-API bindings — no `napi` crate, no `node_api.h`, no
//! `napi-rs`, no build script.
//!
//! Node-API is a stable C ABI: the `napi_*` symbols are exported by the Node
//! process itself and resolved when the addon is `dlopen`ed, so declaring them
//! as `extern "C"` is enough. The opaque handle types are modelled as
//! `*mut c_void`, which is ABI-correct on every platform Node supports.
//!
//! This module also holds the two converters that matter for performance:
//! [`to_value`] reads a JS value straight into a [`Value`] with **no JSON
//! round-trip**, and [`from_value`] writes one back the same way. Skipping
//! `JSON.stringify`/`parse` at the boundary is most of the reason the addon is
//! faster than the wasm build.

#![allow(non_camel_case_types)]

use alloc::string::String;
use alloc::vec::Vec;

use crate::value::from_f64;
use crate::Value;

pub type napi_env = *mut core::ffi::c_void;
pub type napi_value = *mut core::ffi::c_void;
pub type napi_callback_info = *mut core::ffi::c_void;
pub type napi_status = i32;
pub type napi_valuetype = i32;

pub type napi_callback = Option<unsafe extern "C" fn(napi_env, napi_callback_info) -> napi_value>;

pub const NAPI_AUTO_LENGTH: usize = usize::MAX;

pub const NAPI_UNDEFINED: napi_valuetype = 0;
pub const NAPI_NULL: napi_valuetype = 1;
pub const NAPI_BOOLEAN: napi_valuetype = 2;
pub const NAPI_NUMBER: napi_valuetype = 3;
pub const NAPI_STRING: napi_valuetype = 4;
pub const NAPI_SYMBOL: napi_valuetype = 5;
pub const NAPI_OBJECT: napi_valuetype = 6;
pub const NAPI_FUNCTION: napi_valuetype = 7;
pub const NAPI_EXTERNAL: napi_valuetype = 8;
pub const NAPI_BIGINT: napi_valuetype = 9;

extern "C" {
    fn napi_get_cb_info(
        env: napi_env,
        info: napi_callback_info,
        argc: *mut usize,
        argv: *mut napi_value,
        this_arg: *mut napi_value,
        data: *mut *mut core::ffi::c_void,
    ) -> napi_status;
    fn napi_typeof(env: napi_env, value: napi_value, result: *mut napi_valuetype) -> napi_status;
    fn napi_get_null(env: napi_env, result: *mut napi_value) -> napi_status;
    fn napi_get_boolean(env: napi_env, value: bool, result: *mut napi_value) -> napi_status;
    fn napi_get_value_bool(env: napi_env, value: napi_value, result: *mut bool) -> napi_status;
    fn napi_get_value_double(env: napi_env, value: napi_value, result: *mut f64) -> napi_status;
    fn napi_create_double(env: napi_env, value: f64, result: *mut napi_value) -> napi_status;
    fn napi_create_int64(env: napi_env, value: i64, result: *mut napi_value) -> napi_status;
    fn napi_get_value_string_utf8(
        env: napi_env,
        value: napi_value,
        buf: *mut u8,
        bufsize: usize,
        result: *mut usize,
    ) -> napi_status;
    fn napi_create_string_utf8(
        env: napi_env,
        str_: *const u8,
        length: usize,
        result: *mut napi_value,
    ) -> napi_status;
    fn napi_create_object(env: napi_env, result: *mut napi_value) -> napi_status;
    fn napi_create_array_with_length(
        env: napi_env,
        length: usize,
        result: *mut napi_value,
    ) -> napi_status;
    fn napi_set_named_property(
        env: napi_env,
        object: napi_value,
        utf8name: *const u8,
        value: napi_value,
    ) -> napi_status;
    fn napi_set_element(
        env: napi_env,
        object: napi_value,
        index: u32,
        value: napi_value,
    ) -> napi_status;
    fn napi_get_array_length(env: napi_env, value: napi_value, result: *mut u32) -> napi_status;
    fn napi_get_element(
        env: napi_env,
        object: napi_value,
        index: u32,
        result: *mut napi_value,
    ) -> napi_status;
    fn napi_get_property_names(
        env: napi_env,
        object: napi_value,
        result: *mut napi_value,
    ) -> napi_status;
    fn napi_get_property(
        env: napi_env,
        object: napi_value,
        key: napi_value,
        result: *mut napi_value,
    ) -> napi_status;
    fn napi_is_array(env: napi_env, value: napi_value, result: *mut bool) -> napi_status;
    fn napi_is_date(env: napi_env, value: napi_value, result: *mut bool) -> napi_status;
    fn napi_get_date_value(env: napi_env, value: napi_value, result: *mut f64) -> napi_status;
    fn napi_create_function(
        env: napi_env,
        utf8name: *const u8,
        length: usize,
        cb: napi_callback,
        data: *mut core::ffi::c_void,
        result: *mut napi_value,
    ) -> napi_status;
    fn napi_throw_error(env: napi_env, code: *const u8, msg: *const u8) -> napi_status;
}

/// Build a NUL-terminated copy of `s`.
///
/// `napi_set_named_property` and the `utf8name` argument of
/// `napi_create_function` are C strings, not counted strings.
fn cstr(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 1);
    v.extend_from_slice(s.as_bytes());
    v.push(0);
    v
}

/// A N-API call that can fail.
macro_rules! napi {
    ($($t:tt)*) => {
        if unsafe { $($t)* } != 0 {
            return Err(crate::Error::new("napi call failed"));
        }
    };
}

fn js_string(env: napi_env, value: napi_value) -> crate::Result<String> {
    let mut len = 0usize;
    napi!(napi_get_value_string_utf8(
        env,
        value,
        core::ptr::null_mut(),
        0,
        &mut len
    ));
    let mut buf = Vec::<u8>::with_capacity(len + 1);
    let mut written = 0usize;
    napi!(napi_get_value_string_utf8(
        env,
        value,
        buf.as_mut_ptr(),
        buf.capacity(),
        &mut written
    ));
    unsafe { buf.set_len(written) };
    String::from_utf8(buf).map_err(|_| crate::Error::new("string is not valid UTF-8"))
}

/// Read a JS value into a [`Value`] without a JSON round-trip.
///
/// # Safety
/// `value` must be a valid `napi_value` from `env`.
pub unsafe fn to_value(env: napi_env, value: napi_value) -> crate::Result<Value> {
    let mut ty = NAPI_UNDEFINED;
    napi!(napi_typeof(env, value, &mut ty));

    match ty {
        NAPI_UNDEFINED | NAPI_NULL | NAPI_SYMBOL => Ok(Value::Null),
        NAPI_BOOLEAN => {
            let mut b = false;
            napi!(napi_get_value_bool(env, value, &mut b));
            Ok(Value::Bool(b))
        }
        NAPI_NUMBER => {
            let mut d = 0.0f64;
            napi!(napi_get_value_double(env, value, &mut d));
            Ok(from_f64(d))
        }
        NAPI_STRING => Ok(Value::Str(js_string(env, value)?)),
        NAPI_OBJECT => {
            let mut is_array = false;
            napi!(napi_is_array(env, value, &mut is_array));
            if is_array {
                let mut len = 0u32;
                napi!(napi_get_array_length(env, value, &mut len));
                let mut items = Vec::with_capacity(len as usize);
                for i in 0..len {
                    let mut el: napi_value = core::ptr::null_mut();
                    napi!(napi_get_element(env, value, i, &mut el));
                    items.push(to_value(env, el)?);
                }
                return Ok(Value::Array(items));
            }

            let mut is_date = false;
            napi!(napi_is_date(env, value, &mut is_date));
            if is_date {
                let mut ms = 0.0f64;
                napi!(napi_get_date_value(env, value, &mut ms));
                let days = crate::float::floor(ms / 86_400_000.0) as i64;
                let rem = crate::float::rem_euclid(ms, 86_400_000.0);
                let (y, mo, d) = crate::memory::civil_from_days(days);
                let h = crate::float::floor(rem / 3_600_000.0) as u32;
                let mi = crate::float::floor((rem % 3_600_000.0) / 60_000.0) as u32;
                let s = crate::float::floor((rem % 60_000.0) / 1_000.0) as u32;
                let msec = crate::float::floor(rem % 1_000.0) as u32;
                return Ok(Value::Date(format!(
                    "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
                    y, mo, d, h, mi, s, msec
                )));
            }

            // Plain object: enumerate its own keys.
            let mut names: napi_value = core::ptr::null_mut();
            napi!(napi_get_property_names(env, value, &mut names));
            let mut count = 0u32;
            napi!(napi_get_array_length(env, names, &mut count));

            let mut out = crate::Object::new();
            for i in 0..count {
                let mut key: napi_value = core::ptr::null_mut();
                napi!(napi_get_element(env, names, i, &mut key));
                let name = js_string(env, key)?;
                let mut el: napi_value = core::ptr::null_mut();
                napi!(napi_get_property(env, value, key, &mut el));
                out.set(name, to_value(env, el)?);
            }
            Ok(Value::Object(out))
        }
        _ => Ok(Value::Null),
    }
}

/// Write a [`Value`] back to JS without a JSON round-trip.
///
/// # Safety
/// `env` must be a valid `napi_env`.
pub unsafe fn from_value(env: napi_env, value: &Value) -> crate::Result<napi_value> {
    let mut out: napi_value = core::ptr::null_mut();
    match value {
        Value::Null => napi!(napi_get_null(env, &mut out)),
        Value::Bool(b) => napi!(napi_get_boolean(env, *b, &mut out)),
        Value::Int(i) => napi!(napi_create_int64(env, *i, &mut out)),
        Value::Float(f) => napi!(napi_create_double(env, *f, &mut out)),
        Value::Str(s) => napi!(napi_create_string_utf8(env, s.as_ptr(), s.len(), &mut out)),
        Value::Date(d) => napi!(napi_create_string_utf8(env, d.as_ptr(), d.len(), &mut out)),
        Value::Array(items) => {
            napi!(napi_create_array_with_length(env, items.len(), &mut out));
            for (i, item) in items.iter().enumerate() {
                let el = from_value(env, item)?;
                napi!(napi_set_element(env, out, i as u32, el));
            }
        }
        Value::Object(obj) => {
            napi!(napi_create_object(env, &mut out));
            for (k, v) in obj.iter() {
                let el = from_value(env, v)?;
                let name = cstr(k);
                napi!(napi_set_named_property(env, out, name.as_ptr(), el));
            }
        }
    }
    Ok(out)
}

/// Read the arguments of the current call.
///
/// # Safety
/// Must be called from inside a `napi_callback`.
pub unsafe fn args(
    env: napi_env,
    info: napi_callback_info,
    n: usize,
) -> crate::Result<Vec<napi_value>> {
    let mut argc = n;
    let mut argv: Vec<napi_value> = vec![core::ptr::null_mut(); n];
    napi!(napi_get_cb_info(
        env,
        info,
        &mut argc,
        argv.as_mut_ptr(),
        core::ptr::null_mut(),
        core::ptr::null_mut()
    ));
    Ok(argv)
}

/// Register `[name, callback]` pairs on `exports`.
///
/// # Safety
/// `exports` must be a valid JS object.
pub unsafe fn export(
    env: napi_env,
    exports: napi_value,
    name: &str,
    cb: unsafe extern "C" fn(napi_env, napi_callback_info) -> napi_value,
) {
    let cname = cstr(name);
    let mut f: napi_value = core::ptr::null_mut();
    if napi_create_function(
        env,
        cname.as_ptr(),
        NAPI_AUTO_LENGTH,
        Some(cb),
        core::ptr::null_mut(),
        &mut f,
    ) != 0
    {
        return;
    }
    napi_set_named_property(env, exports, cname.as_ptr(), f);
}

/// Throw a JS `Error` with `msg`.
///
/// # Safety
/// `env` must be valid.
pub unsafe fn throw(env: napi_env, msg: &str) -> napi_value {
    let cmsg = cstr(msg);
    napi_throw_error(env, core::ptr::null(), cmsg.as_ptr());
    core::ptr::null_mut()
}
