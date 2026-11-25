//! Same workload as the JavaScript benchmark in the README: run with
//! `cargo run --release --example bench`.

use std::time::Instant;

use mosql::{parse_json, Value};
use uflite::acl::Acl;
use uflite::db::{block_on, Collection, MemoryCollection};
use uflite::{DbOptions, DbService, Params};

fn docs(n: i64) -> Vec<Value> {
    (0..n)
        .map(|i| {
            parse_json(&format!(
                r#"{{"_id": {}, "name": "user{}", "age": {}, "active": {}, "city": "{}"}}"#,
                i + 1,
                i,
                (i * 7) % 80,
                i % 3 == 0,
                ["NY", "SF", "LA"][(i % 3) as usize]
            ))
            .unwrap()
        })
        .collect()
}

fn time(label: &str, iters: usize, f: impl Fn()) {
    for _ in 0..iters.min(2000) {
        f();
    }
    let t0 = Instant::now();
    for _ in 0..iters {
        f();
    }
    let us = t0.elapsed().as_secs_f64() * 1e6 / iters as f64;
    println!("{label:<10} {:>9.0} ops/s   ({us:.2} us/op)", 1e6 / us);
}

fn main() {
    let service = DbService::new(
        MemoryCollection::new("bench", docs(1000)),
        DbOptions::default(),
    );
    let query = Params::new()
        .query(parse_json(r#"{"age": {"$gte": 30}, "$sort": {"age": 1}, "$limit": 20}"#).unwrap())
        .unwrap();

    time("find", 2000, || {
        let page = block_on(service.find(&query)).unwrap();
        assert!(matches!(page, uflite::FindResult::Paginated(_)));
    });
    time("get", 20000, || {
        block_on(service.get(&Value::Int(500), &Params::new())).unwrap();
    });
    time("create", 2000, || {
        let c = block_on(service.create(
            parse_json(r#"{"name": "x", "age": 1}"#).unwrap(),
            &Params::new(),
        ))
        .unwrap();
        block_on(service.remove(c.get("_id").unwrap(), &Params::new())).unwrap();
    });

    let acl = Acl::new(
        &parse_json(
            r#"{"enabled": true, "rules": [
            {"id": "admin", "permissions": ["admin:*:*"]},
            {"id": "owner", "permissions": ["user:posts:update", "user:posts:remove"],
             "check": {"$expr": {"$eq": ["$user.id", "$post.authorId"]}}},
            {"id": "published", "permissions": ["*:posts:read"], "check": {"post.published": true}}
        ]}"#,
        )
        .unwrap(),
    )
    .unwrap();
    let ctx = parse_json(
        r#"{"user": {"id": 10, "role": "owner"}, "post": {"authorId": 10, "published": true}}"#,
    )
    .unwrap();
    let iters = 200_000;
    let t0 = Instant::now();
    let mut ok = 0;
    for _ in 0..iters {
        if acl.can(&ctx, "user:posts:update").unwrap() {
            ok += 1;
        }
    }
    let us = t0.elapsed().as_secs_f64() * 1e6 / iters as f64;
    println!(
        "{:<10} {:>9.0} ops/s   ({us:.2} us/op)  allowed={ok}",
        "acl.can",
        1e6 / us
    );

    // Raw collection matching, for comparison with the JS memory engine probe.
    let coll = MemoryCollection::new("raw", docs(1000));
    let q = parse_json(r#"{"age": {"$gte": 30}}"#).unwrap();
    let sort = parse_json(r#"{"age": 1}"#).unwrap();
    time("find+sort", 2000, || {
        block_on(coll.find(
            &q,
            None,
            uflite::FindOptions {
                sort: Some(sort.clone()),
                ..Default::default()
            },
        ))
        .unwrap();
    });
    time("find plain", 20000, || {
        block_on(coll.find(&q, None, uflite::FindOptions::default())).unwrap();
    });
    time("count", 20000, || {
        block_on(coll.count_documents(&q)).unwrap();
    });
    let by_id = parse_json(r#"{"_id": 500}"#).unwrap();
    time("findOne", 20000, || {
        block_on(coll.find_one(&by_id, None)).unwrap();
    });
}
