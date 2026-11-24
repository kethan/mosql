//! The error model: HTTP status, the contract's wire body, and the JavaScript
//! shapes the port must keep.

use mosql::Value;
use uflite::error::Error;

fn errors() -> Vec<(Error, u16, &'static str)> {
    vec![
        (Error::bad_request("nope"), 400, "BadRequest"),
        (Error::unauthorized("sign in"), 401, "NotAuthenticated"),
        (Error::forbidden("not yours"), 403, "Forbidden"),
        (Error::not_found("gone"), 404, "NotFound"),
        (Error::method_not_allowed("PATCH"), 405, "MethodNotAllowed"),
        (Error::conflict("exists"), 409, "Conflict"),
        (Error::too_large("25 MiB max"), 413, "PayloadTooLarge"),
        (Error::unprocessable("bad state"), 422, "Unprocessable"),
        (
            Error::too_many_requests("slow down"),
            429,
            "TooManyRequests",
        ),
        (Error::internal("boom"), 500, "GeneralError"),
        (Error::unavailable("down"), 503, "Unavailable"),
    ]
}

#[test]
fn every_variant_maps_to_its_status_and_name() {
    for (e, status, name) in errors() {
        assert_eq!(e.status(), status, "{name}");
        assert_eq!(e.wire_name(), name);
    }
}

#[test]
fn query_errors_are_internal() {
    let e = Error::Query(mosql::Error::new("unknown operator"));
    assert_eq!(e.status(), 500);
    assert_eq!(e.wire_name(), "GeneralError");
    assert_eq!(e.to_string(), "unknown operator");
}

#[test]
fn wire_body_is_the_contract_envelope() {
    let e = Error::conflict("Email is already taken");
    let v = e.to_wire();
    assert_eq!(v.get("name"), Some(&Value::str("Conflict")));
    assert_eq!(
        v.get("message"),
        Some(&Value::str("Email is already taken"))
    );
    assert_eq!(v.get("code"), Some(&Value::Int(409)));
    assert_eq!(v.get("className"), Some(&Value::str("conflict")));
    assert_eq!(v.get("errors"), Some(&Value::object()));
    // The contract has no `status` field; `code` carries it.
    assert_eq!(v.get("status"), None);
    // Exactly the five contract keys, nothing else.
    assert_eq!(v.as_object().map(|o| o.len()), Some(5));
}

#[test]
fn every_variant_produces_a_complete_wire_body() {
    for (e, status, name) in errors() {
        let v = e.to_wire();
        assert_eq!(v.get("name"), Some(&Value::str(name)), "{name}");
        assert_eq!(v.get("code"), Some(&Value::Int(status as i64)), "{name}");
        assert_eq!(v.get("className"), Some(&Value::str(e.class_name())));
        assert_eq!(v.get("errors"), Some(&Value::object()), "{name}");
        assert_eq!(v.get("message"), Some(&Value::str(e.to_string())));
    }
}

#[test]
fn class_names_are_the_kebab_case_forms() {
    let cases = [
        (Error::bad_request("x"), "bad-request"),
        (Error::unauthorized("x"), "not-authenticated"),
        (Error::forbidden("x"), "forbidden"),
        (Error::not_found("x"), "not-found"),
        (Error::method_not_allowed("x"), "method-not-allowed"),
        (Error::conflict("x"), "conflict"),
        (Error::too_large("x"), "payload-too-large"),
        (Error::unprocessable("x"), "unprocessable"),
        (Error::too_many_requests("x"), "too-many-requests"),
        (Error::internal("x"), "general"),
        (Error::unavailable("x"), "service-unavailable"),
    ];
    for (e, class) in cases {
        assert_eq!(e.class_name(), class);
    }
}

#[test]
fn the_javascript_shapes_are_unchanged() {
    // The five variants uflite also has keep their JavaScript `error.name`.
    assert_eq!(Error::bad_request("x").name(), "TypeError");
    assert_eq!(Error::not_found("x").name(), "NotFound");
    assert_eq!(Error::too_large("x").name(), "RangeError");
    assert_eq!(Error::internal("x").name(), "Error");
    assert_eq!(Error::Query(mosql::Error::new("x")).name(), "Error");
    // `From<Error> for Value` is the body uflite's REST layer serialises.
    let v = Value::from(Error::not_found("No record found for id '1'"));
    assert_eq!(v.get("name"), Some(&Value::str("NotFound")));
    assert_eq!(
        v.get("message"),
        Some(&Value::str("No record found for id '1'"))
    );
    assert_eq!(v.get("code"), Some(&Value::Int(404)));
    assert_eq!(v.get("status"), Some(&Value::Int(404)));
}

#[test]
fn platform_only_variants_fall_back_to_the_wire_name() {
    for (e, _, name) in errors() {
        if matches!(
            e,
            Error::BadRequest(_)
                | Error::NotFound(_)
                | Error::TooLarge(_)
                | Error::Internal(_)
                | Error::Query(_)
        ) {
            continue;
        }
        assert_eq!(e.name(), name, "no JavaScript counterpart");
    }
}

#[test]
fn record_not_found_message() {
    let e = Error::record_not_found(&Value::Int(7));
    assert_eq!(e.to_string(), "No record found for id '7'");
    assert!(e.is_not_found());
    assert_eq!(e.status(), 404);
}

#[test]
fn unsupported_is_internal() {
    let e = Error::unsupported("transactions");
    assert_eq!(e.status(), 500);
    assert_eq!(
        e.to_string(),
        "The database adapter does not support transactions"
    );
}

#[test]
fn display_is_the_message() {
    assert_eq!(Error::forbidden("no").to_string(), "no");
    assert_eq!(
        Error::Query(mosql::Error::new("bad query")).to_string(),
        "bad query"
    );
}

#[test]
fn mosql_errors_convert_into_service_errors() {
    let e: Error = mosql::Error::new("nope").into();
    assert!(matches!(e, Error::Query(_)));
    assert_eq!(e.status(), 500);
}

#[test]
fn std_error_source_is_the_inner_mosql_error() {
    use std::error::Error as _;
    let e = Error::Query(mosql::Error::new("inner"));
    assert!(e.source().is_some());
    assert!(Error::internal("outer").source().is_none());
}
