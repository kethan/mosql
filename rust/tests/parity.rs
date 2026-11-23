//! Differential parity: replay the JS-generated corpus through the Rust port
//! and require byte-identical SQL (or identical error messages).

use mosql::{parse_json, Builder, CrudOptions, Db, FindQuery, Value};

fn options(v: Option<&Value>) -> CrudOptions {
    let mut o = CrudOptions::default();
    if let Some(obj) = v.and_then(|v| v.as_object()) {
        if let Some(r) = obj.get("returning") {
            match r {
                Value::Array(items) => {
                    o.returning = Some(
                        items
                            .iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect(),
                    )
                }
                Value::Bool(true) | Value::Int(1) => o.returning = Some(Vec::new()),
                _ => {}
            }
        }
        if let Some(a) = obj.get("allowDeleteAll") {
            o.allow_delete_all = a.is_truthy();
        }
    }
    o
}

fn builder_for(variant: &str) -> Builder {
    match variant {
        "lite" => Builder::lite(),
        "tiny" => Builder::tiny(),
        _ => Builder::full(),
    }
}

fn chain(mut q: FindQuery, steps: Option<&Value>) -> mosql::Result<FindQuery> {
    if let Some(steps) = steps.and_then(|v| v.as_array()) {
        for step in steps {
            let method = step.get("m").and_then(|v| v.as_str()).unwrap_or("");
            let args = step
                .get("a")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            match method {
                "sort" => q = q.sort(&args[0])?,
                "limit" => q = q.limit(&args[0])?,
                "skip" => q = q.skip(&args[0])?,
                "distinct" => q = q.distinct()?,
                "count" => q = q.count()?,
                other => panic!("unknown chain step {}", other),
            }
        }
    }
    Ok(q)
}

#[test]
fn parity_with_javascript() {
    let raw = include_str!("fixtures.json");
    let fixtures = parse_json(raw).expect("fixtures.json must parse");
    let cases = fixtures.as_array().expect("array");

    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for case in cases {
        let name = case
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("<unnamed>");
        let variant = case
            .get("variant")
            .and_then(|v| v.as_str())
            .unwrap_or("full");
        let db = Db::parse(case.get("db").and_then(|v| v.as_str()).unwrap_or("sqlite"));
        let kind = case.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let table = case.get("table").and_then(|v| v.as_str()).unwrap_or("");
        let args = case
            .get("args")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let steps = case.get("chain");

        let builder = builder_for(variant);
        let coll = builder.collection(table, db);

        let result: mosql::Result<String> = (|| {
            let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
            match kind {
                "find" => {
                    let coll = coll?;
                    let q = coll.find(&arg(0), args.get(1))?;
                    chain(q, steps)?.to_sql()
                }
                "findOne" => {
                    let coll = coll?;
                    let q = coll.find_one(&arg(0), args.get(1))?;
                    chain(q, steps)?.to_sql()
                }
                "filter" => builder.filter(&arg(0), db),
                "expression" => builder.expression(&arg(0), db),
                "aggregate" => builder.aggregate(&arg(0), table, db),
                "insertMany" => builder.insert_many(table, &arg(0), db, &options(args.get(1))),
                "updateMany" => {
                    builder.update_many(table, &arg(0), &arg(1), db, &options(args.get(2)))
                }
                "deleteMany" => builder.delete_many(table, &arg(0), db, &options(args.get(1))),
                "countDocuments" => coll?.count_documents(&arg(0)),
                "distinct" => coll?.distinct(arg(0).as_str().unwrap_or(""), &arg(1)),
                other => panic!("unknown kind {}", other),
            }
        })();

        checked += 1;

        match (case.get("error"), &result) {
            (Some(expected), Err(actual)) => {
                let expected = expected.as_str().unwrap_or("");
                if expected != actual.0 {
                    failures.push(format!(
                        "{}: error mismatch\n  js:  {}\n  rust:{}",
                        name, expected, actual.0
                    ));
                }
            }
            (Some(expected), Ok(sql)) => failures.push(format!(
                "{}: expected error {:?}, got SQL {:?}",
                name,
                expected.as_str().unwrap_or(""),
                sql
            )),
            (None, Ok(sql)) => {
                let expected = case.get("sql").and_then(|v| v.as_str()).unwrap_or("");
                if expected != sql {
                    failures.push(format!(
                        "{}: sql mismatch\n  js:   {}\n  rust: {}",
                        name, expected, sql
                    ));
                }
            }
            (None, Err(e)) => failures.push(format!("{}: unexpected error: {}", name, e.0)),
        }
    }

    println!("checked {} fixtures", checked);
    assert!(
        failures.is_empty(),
        "{}/{} fixtures failed:\n{}",
        failures.len(),
        checked,
        failures.join("\n\n")
    );
}
