//! The protocol, end to end.
//!
//! The test plays the part of the host: it starts the engine, answers the
//! questions the engine asks, and reads the response. The answers come
//! from a real SQLite database, so the statements the engine writes are
//! the statements a deployment runs — and the rows it reads back are the
//! rows it would read from D1.
//!
//! This is the same conversation the Worker script has, minus the
//! platform: the module asks, the host answers, the module continues.

use std::sync::Mutex;

use mosql::Value;
use mosql_worker::{
    mosql_worker_alloc, mosql_worker_free, mosql_worker_init, mosql_worker_resume,
    mosql_worker_run, mosql_worker_take,
};

/// The database the host answers from.
static DB: Mutex<Option<nanobase::sqlite::Connection>> = Mutex::new(None);

/// The statuses the module answers with.
const DONE: i32 = 0;
const ASK: i32 = 1;

/// Read what the module last said.
fn read() -> String {
    let length = mosql_worker_alloc(4) as *mut u32;
    let pointer = unsafe { mosql_worker_take(length) };
    let size = unsafe { std::ptr::read(length) };
    let text = if size == 0 {
        String::new()
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(pointer, size as usize) };
        String::from_utf8_lossy(bytes).into_owned()
    };
    unsafe { mosql_worker_free(length as *mut u8, 4) };
    text
}

/// Hand the module a string, and say what it said.
fn call(function: extern "C" fn(*const u8, u32) -> i32, text: &str) -> (i32, String) {
    let bytes = text.as_bytes();
    let pointer = mosql_worker_alloc(bytes.len() as u32);
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len()) };
    let status = function(pointer, bytes.len() as u32);
    unsafe { mosql_worker_free(pointer, bytes.len() as u32) };
    (status, read())
}

/// Answer the engine's question the way the Worker script does.
fn answer(op: &Value) -> Value {
    let mut answer = mosql::Object::new();
    let kind = op.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "query" => {
            let sql = op.get("sql").and_then(|v| v.as_str()).unwrap_or("");
            let params: Vec<Value> = op
                .get("params")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let wants_rows = op.get("rows").and_then(|v| v.as_bool()).unwrap_or(false);
            let mut guard = DB.lock().expect("db");
            let Some(connection) = guard.as_mut() else {
                answer.set("error", Value::str("the database is not open"));
                return Value::Object(answer);
            };
            if wants_rows {
                match connection.query(sql, &params) {
                    Ok(rows) => {
                        let items = rows
                            .iter()
                            .map(|row| {
                                let mut item = mosql::Object::new();
                                item.set(
                                    "columns",
                                    Value::Array(row.columns.iter().map(Value::str).collect()),
                                );
                                item.set("values", Value::Array(row.values.clone()));
                                Value::Object(item)
                            })
                            .collect();
                        answer.set("rows", Value::Array(items));
                        answer.set("changes", Value::Int(0));
                    }
                    Err(error) => {
                        answer.set("error", Value::str(error.to_string()));
                    }
                }
            } else {
                match connection.execute(sql, &params) {
                    Ok(changes) => {
                        answer.set("rows", Value::Array(Vec::new()));
                        answer.set("changes", Value::Int(changes as i64));
                    }
                    Err(error) => {
                        answer.set("error", Value::str(error.to_string()));
                    }
                }
            }
        }
        other => {
            answer.set("error", Value::str(format!("unknown question: {other}")));
        }
    }
    Value::Object(answer)
}

/// Run one request through the module, answering as it goes.
fn run(request: &str) -> (i32, String) {
    let (status, text) = call(mosql_worker_run, request);
    let mut status = status;
    let mut text = text;
    let mut guard = 0;
    while status == ASK {
        guard += 1;
        assert!(guard < 100, "the engine is asking forever");
        let op = mosql::parse_json(&text).expect("the question is JSON");
        let reply = answer(&op);
        let next = call(mosql_worker_resume, &mosql::to_json(&reply));
        status = next.0;
        text = next.1;
    }
    (status, text)
}

/// The whole conversation, once: the engine is built, and the tests that
/// follow use it.
#[test]
fn the_engine_answers_a_sign_up_and_a_sign_in() {
    // ---- the host is ready ---------------------------------------------
    let mut guard = DB.lock().expect("db");
    let connection = nanobase::sqlite::Connection::open_memory().expect("a database");
    connection
        .execute_batch(&better_auth_core::models::schema_sql())
        .expect("the schema");
    *guard = Some(connection);
    drop(guard);

    // ---- the engine starts ---------------------------------------------
    let mut config = mosql::Object::new();
    config.set("baseUrl", Value::str("https://auth.test"));
    config.set(
        "secret",
        Value::str("a secret of at least thirty-two bytes"),
    );
    config.set("cache", Value::Bool(false));
    let (status, text) = call(mosql_worker_init, &mosql::to_json(&Value::Object(config)));
    assert_eq!(status, DONE, "the engine did not start: {text}");

    // ---- a request that needs no database ------------------------------
    let (status, text) = run(r#"{"method":"GET","path":"/api/auth/ok"}"#);
    assert_eq!(status, DONE, "{text}");
    let response = mosql::parse_json(&text).expect("the response is JSON");
    assert_eq!(response.get("status").and_then(|v| v.as_f64()), Some(200.0));

    // ---- a sign-up, which does ------------------------------------------
    let mut body = mosql::Object::new();
    body.set("email", Value::str("ada@example.com"));
    body.set("password", Value::str("correct-horse-battery"));
    body.set("name", Value::str("Ada"));
    let request = format!(
        r#"{{"method":"POST","path":"/api/auth/sign-up/email","headers":[["content-type","application/json"]],"body":{}}}"#,
        mosql::to_json(&Value::str(mosql::to_json(&Value::Object(body))))
    );
    let (status, text) = run(&request);
    assert_eq!(status, DONE, "{text}");
    let response = mosql::parse_json(&text).expect("the response is JSON");
    assert_eq!(response.get("status").and_then(|v| v.as_f64()), Some(200.0));
    let payload = mosql::parse_json(
        response
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("{}"),
    )
    .expect("the body is JSON");
    assert_eq!(
        payload
            .get("user")
            .and_then(|u| u.get("email"))
            .and_then(|v| v.as_str()),
        Some("ada@example.com")
    );
    let token = payload
        .get("token")
        .and_then(|v| v.as_str())
        .expect("a token")
        .to_string();

    // ---- the session is in the database ---------------------------------
    let mut guard = DB.lock().expect("db");
    let connection = guard.as_mut().expect("a database");
    let rows = connection
        .query(
            "SELECT * FROM \"user\" WHERE \"email\" = ?",
            &[Value::str("ada@example.com")],
        )
        .expect("the query");
    assert_eq!(rows.len(), 1, "the user was written");
    assert_eq!(rows[0].values.len(), rows[0].columns.len());
    let sessions = connection
        .query("SELECT COUNT(*) AS n FROM \"session\"", &[])
        .expect("the query");
    assert_eq!(
        sessions[0].get("n"),
        Some(&Value::Int(1)),
        "the session was written"
    );
    drop(guard);

    // ---- a sign-in, which finds what the sign-up wrote ------------------
    let mut body = mosql::Object::new();
    body.set("email", Value::str("ada@example.com"));
    body.set("password", Value::str("correct-horse-battery"));
    let request = format!(
        r#"{{"method":"POST","path":"/api/auth/sign-in/email","headers":[["content-type","application/json"]],"body":{}}}"#,
        mosql::to_json(&Value::str(mosql::to_json(&Value::Object(body))))
    );
    let (status, text) = run(&request);
    assert_eq!(status, DONE, "{text}");
    let response = mosql::parse_json(&text).expect("the response is JSON");
    assert_eq!(response.get("status").and_then(|v| v.as_f64()), Some(200.0));

    // ---- the session the token names can be read back -------------------
    let request = format!(
        r#"{{"method":"GET","path":"/api/auth/get-session","headers":[["authorization","Bearer {token}"]]}}"#
    );
    let (status, text) = run(&request);
    assert_eq!(status, DONE, "{text}");
    let response = mosql::parse_json(&text).expect("the response is JSON");
    assert_eq!(response.get("status").and_then(|v| v.as_f64()), Some(200.0));
    let payload = mosql::parse_json(
        response
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("{}"),
    )
    .expect("the body is JSON");
    assert!(
        payload.get("session").is_some(),
        "the session came back: {payload:?}"
    );
}
