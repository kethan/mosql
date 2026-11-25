//! Micro-benchmark for the in-memory engine.
//!
//! Run with `cargo run --release --bin mem-bench`.

use std::time::Instant;

use mosql::memory::Collection;
use mosql::{parse_json, Object, Value};

fn doc(i: i64) -> Value {
    let mut o = Object::new();
    o.set("_id", Value::Int(i));
    o.set("name", Value::str(format!("user{}", i)));
    o.set("age", Value::Int(18 + (i % 60)));
    o.set("active", Value::Bool(i % 3 == 0));
    o.set(
        "tags",
        Value::Array(vec![Value::str("t1"), Value::str("t2")]),
    );
    let mut profile = Object::new();
    profile.set("score", Value::Int((i * 7) % 100));
    profile.set(
        "city",
        Value::str(if i % 2 == 0 { "London" } else { "Paris" }),
    );
    o.set("profile", Value::Object(profile));
    Value::Object(o)
}

fn q(json: &str) -> Value {
    parse_json(json).unwrap()
}

fn main() {
    let n = 10_000i64;
    let docs: Vec<Value> = (0..n).map(doc).collect();

    let mut coll = Collection::new("users", docs, Default::default());
    let query = q(r#"{"age":{"$gte":40},"$or":[{"active":true},{"profile.score":{"$gt":80}}]}"#);
    let pipeline = q(
        r#"[{"$match":{"active":true}},{"$group":{"_id":"$profile.city","n":{"$sum":1},"avg":{"$avg":"$age"}}},{"$sort":{"n":-1}}]"#,
    );
    let update = q(r#"[{"$set":{"verified":true}},{"$inc":{"profile.score":1}}]"#);
    let update = match update {
        Value::Array(items) => items,
        _ => unreachable!(),
    };

    // Best-of-N: the machine is shared, so the minimum is the only reading
    // that reflects the code rather than whatever else was running.
    let rounds = std::env::var("ROUNDS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(3);

    let run = |label: &str, iters: u32, f: &mut dyn FnMut()| {
        let mut best = f64::INFINITY;
        for _ in 0..rounds.max(1) {
            let t = Instant::now();
            for _ in 0..iters {
                f();
            }
            let ns = t.elapsed().as_nanos() as f64 / iters as f64;
            if ns < best {
                best = ns;
            }
        }
        println!(
            "{:<12} {:>10.1} ns/op {:>12.0} ops/s   ({:.2} ms/op)",
            label,
            best,
            1e9 / best,
            best / 1e6
        );
    };

    // Warm up.
    for _ in 0..3 {
        let _ = coll.find(&query).to_vec().unwrap();
        let _ = coll.aggregate(&pipeline).unwrap();
    }

    run("find", 200, &mut || {
        let out = coll.find(&query).to_vec().unwrap();
        assert!(!out.is_empty());
    });

    run("find+sort", 200, &mut || {
        let out = coll
            .find(&query)
            .sort(&q(r#"{"age":-1}"#))
            .limit(10)
            .to_vec()
            .unwrap();
        assert!(!out.is_empty());
    });

    run("aggregate", 100, &mut || {
        let out = coll.aggregate(&pipeline).unwrap();
        assert!(!out.is_empty());
    });

    run("updateMany", 100, &mut || {
        let r = coll.update_many(&query, &update[0]).unwrap();
        assert!(r.matched_count > 0);
    });

    // A separate collection, so this case does not grow the one the other
    // cases are measured against.
    let mut sink = Collection::new("sink", Vec::new(), Default::default());
    run("insert1", 2_000, &mut || {
        let _ = sink.insert_one(doc(999_999)).unwrap();
    });

    println!("\ncollection size: {}", coll.size());

    // The check mode prints what the engine actually computed, so the
    // JavaScript and the Go baselines can be compared against it. A
    // timing comparison only means something if the three are doing the
    // same work.
    if std::env::var("CHECK").is_ok() {
        let matched = coll.find(&query).to_vec().unwrap().len();
        let groups = coll.aggregate(&pipeline).unwrap();
        let first = groups.first().cloned().unwrap_or(Value::Null);
        let city = first
            .get("_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let n = first.get("n").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let avg = first.get("avg").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let updated = coll.update_many(&query, &update[0]).unwrap().matched_count;
        println!(
            "CHECK find={matched} groups={} city={city} n={n} avg={avg} updated={updated}",
            groups.len()
        );
    }
}
