//! Micro-benchmark: build N queries and report throughput.
//!
//! Run with `cargo run --release --bin bench`.

use std::time::Instant;

use mosql::{Builder, CrudOptions, Db, Value};

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut o = mosql::Object::new();
    for (k, v) in pairs {
        o.set(*k, v.clone());
    }
    Value::Object(o)
}

fn query() -> Value {
    obj(&[
        ("age", obj(&[("$gte", Value::Int(18))])),
        ("status", Value::str("active")),
        (
            "$or",
            Value::Array(vec![
                obj(&[("role", Value::str("admin"))]),
                obj(&[("profile.score", obj(&[("$gt", Value::Int(90))]))]),
            ]),
        ),
    ])
}

fn main() {
    let b = Builder::full();
    let users = b.collection("users", Db::Pg).unwrap();
    let q = query();
    let opts = CrudOptions::default();
    let pipeline = Value::Array(vec![
        obj(&[("$match", query())]),
        obj(&[(
            "$group",
            obj(&[
                ("_id", Value::str("$status")),
                ("total", obj(&[("$sum", Value::Int(1))])),
            ]),
        )]),
        obj(&[("$sort", obj(&[("total", Value::Int(-1))]))]),
    ]);

    let n = 20_000u32;

    // Warm up
    for _ in 0..1_000 {
        let _ = users.find(&q, None).unwrap().to_sql().unwrap();
    }

    let rounds = std::env::var("ROUNDS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(3);

    // Best-of-N: the machine is shared, so the minimum is the only reading that
    // reflects the code rather than whatever else was running.
    let run = |label: &str, f: &mut dyn FnMut()| {
        let mut best = f64::INFINITY;
        for _ in 0..rounds.max(1) {
            let t = Instant::now();
            for _ in 0..n {
                f();
            }
            let ns = t.elapsed().as_nanos() as f64 / n as f64;
            if ns < best {
                best = ns;
            }
        }
        println!(
            "{:<9} {:8.1} ns/op  {:>10.0} ops/s",
            label,
            best,
            1e9 / best
        );
    };

    run("find", &mut || {
        let sql = users.find(&q, None).unwrap().to_sql().unwrap();
        assert!(!sql.is_empty());
    });
    run("update", &mut || {
        let sql = users
            .update_many(
                &q,
                &obj(&[("$set", obj(&[("status", Value::str("done"))]))]),
                &opts,
            )
            .unwrap();
        assert!(!sql.is_empty());
    });
    run("aggregate", &mut || {
        let sql = users.aggregate(&pipeline).unwrap();
        assert!(!sql.is_empty());
    });
    run("insert50", &mut || {
        let sql = users
            .insert_many(
                &Value::Array(
                    (0..50)
                        .map(|i| {
                            obj(&[
                                ("name", Value::str(format!("u{}", i))),
                                ("age", Value::Int(i)),
                            ])
                        })
                        .collect(),
                ),
                &opts,
            )
            .unwrap();
        assert!(!sql.is_empty());
    });
}
