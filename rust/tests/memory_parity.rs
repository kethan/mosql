//! Differential parity for the in-memory engine: replays the JS-generated
//! corpus and requires identical JSON results.

use mosql::memory::{aggregate, apply_update, eval_expr, matches, set_path, Collection};
use mosql::{parse_json, to_json, Object, Value};

fn obj(entries: Vec<(&str, Value)>) -> Value {
    let mut o = Object::new();
    for (k, v) in entries {
        o.set(k, v);
    }
    Value::Object(o)
}

fn json_of(v: &Value) -> String {
    to_json(v)
}

fn compare(name: &str, computed: &Value, expected: &Value, failures: &mut Vec<String>) {
    let a = json_of(computed);
    let b = json_of(expected);
    if a != b {
        failures.push(format!("{}: mismatch\n  js:   {}\n  rust: {}", name, b, a));
    }
}

fn js_update_result(r: mosql::memory::UpdateResult) -> Value {
    if r.upserted_count > 0 {
        obj(vec![
            ("modifiedCount", Value::Int(0)),
            ("upsertedCount", Value::Int(1)),
            ("acknowledged", Value::Bool(true)),
        ])
    } else {
        obj(vec![
            ("modifiedCount", Value::Int(r.modified_count)),
            ("matchedCount", Value::Int(r.matched_count)),
            ("acknowledged", Value::Bool(true)),
        ])
    }
}

#[test]
fn memory_parity_with_javascript() {
    let raw = include_str!("memory-fixtures.json");
    let fixtures = parse_json(raw).expect("memory-fixtures.json must parse");
    let cases = fixtures.as_array().expect("array");

    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for case in cases {
        let name = case
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("<unnamed>");
        let kind = case.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        checked += 1;

        match kind {
            "filter" => {
                let docs = case.get("docs").and_then(|v| v.as_array()).unwrap();
                let query = case.get("query").unwrap();
                let mut out = Vec::new();
                for doc in docs {
                    if matches(query, doc).unwrap_or(false) {
                        out.push(doc.clone());
                    }
                }
                compare(
                    name,
                    &Value::Array(out),
                    case.get("result").unwrap(),
                    &mut failures,
                );
            }
            "expr" => {
                let doc = case.get("doc").unwrap();
                let expr = case.get("expr").unwrap();
                let value = eval_expr(expr, doc, &Vec::new()).unwrap_or(Value::Null);
                let expected = case.get("result").unwrap();
                let is_undefined = expected
                    .get("$undefined")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if is_undefined {
                    if !value.is_null() {
                        failures.push(format!(
                            "{}: expected undefined, got {}",
                            name,
                            json_of(&value)
                        ));
                    }
                } else {
                    compare(name, &value, expected, &mut failures);
                }
            }
            "update" => {
                let mut doc = case.get("doc").unwrap().clone();
                let update = case.get("update").unwrap();
                for (op, fields) in update.as_object().unwrap().iter() {
                    if op.starts_with('$') {
                        apply_update(op, fields.as_object().unwrap(), &mut doc, false)
                            .unwrap_or_else(|e| panic!("{}: {}", name, e));
                    } else {
                        set_path(&mut doc, op, fields.clone());
                    }
                }
                compare(name, &doc, case.get("result").unwrap(), &mut failures);
            }
            "aggregate" => {
                let docs = case.get("docs").and_then(|v| v.as_array()).unwrap();
                let out = aggregate(case.get("pipeline").unwrap(), docs)
                    .unwrap_or_else(|e| panic!("{}: {}", name, e));
                compare(
                    name,
                    &Value::Array(out),
                    case.get("result").unwrap(),
                    &mut failures,
                );
            }
            "collection" => {
                let results = case.get("results").and_then(|v| v.as_array()).unwrap();
                let mut coll = Collection::new("users", vec![], Default::default());
                for step in results {
                    let op = step.get("op").and_then(|v| v.as_str()).unwrap_or("");
                    let args = step
                        .get("args")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let label = format!("{}/{}", name, op);
                    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
                    let computed: Value = match op {
                        "insertOne" => obj(vec![
                            ("insertedId", coll.insert_one(arg(0)).unwrap_or(Value::Null)),
                            ("acknowledged", Value::Bool(true)),
                        ]),
                        "insertMany" => obj(vec![
                            (
                                "insertedIds",
                                Value::Array(
                                    coll.insert_many(args[0].as_array().unwrap())
                                        .unwrap_or_default(),
                                ),
                            ),
                            ("acknowledged", Value::Bool(true)),
                        ]),
                        "findOne" => coll
                            .find_one(&arg(0), args.get(1))
                            .unwrap_or(None)
                            .unwrap_or(Value::Null),
                        "findById" => coll
                            .find_by_id(&arg(0))
                            .unwrap_or(None)
                            .unwrap_or(Value::Null),
                        "find" => {
                            let mut q = coll.find(&arg(0));
                            if let Some(opts) = args.get(1) {
                                if let Some(s) = opts.get("sort") {
                                    q = q.sort(s);
                                }
                                if let Some(s) = opts.get("skip") {
                                    q = q.skip(s.to_number().unwrap_or(0.0) as i64);
                                }
                                if let Some(l) = opts.get("limit") {
                                    q = q.limit(l.to_number().unwrap_or(0.0) as i64);
                                }
                            }
                            let docs = q.to_vec().unwrap_or_default();
                            Value::Array(
                                docs.iter()
                                    .map(|d| d.get("name").cloned().unwrap_or(Value::Null))
                                    .collect(),
                            )
                        }
                        "countDocuments" => Value::Int(coll.count_documents(&arg(0)).unwrap_or(0)),
                        "estimatedDocumentCount" => Value::Int(coll.estimated_document_count()),
                        "distinct" => Value::Array(
                            coll.distinct(arg(0).as_str().unwrap_or(""), &arg(1))
                                .unwrap_or_default(),
                        ),
                        "aggregate" => Value::Array(coll.aggregate(&arg(0)).unwrap_or_default()),
                        "updateOne" => {
                            let upsert =
                                arg(2).get("upsert").map(|v| v.is_truthy()).unwrap_or(false);
                            let r = coll
                                .update_one(&arg(0), &arg(1), upsert)
                                .unwrap_or_else(|e| panic!("{}: {}", label, e));
                            js_update_result(r)
                        }
                        "updateMany" => {
                            let r = coll.update_many(&arg(0), &arg(1)).unwrap();
                            js_update_result(r)
                        }
                        "replaceOne" => {
                            let upsert =
                                arg(2).get("upsert").map(|v| v.is_truthy()).unwrap_or(false);
                            let r = coll.replace_one(&arg(0), &arg(1), upsert).unwrap();
                            js_update_result(r)
                        }
                        "deleteOne" => {
                            let r = coll.delete_one(&arg(0)).unwrap();
                            obj(vec![
                                ("deletedCount", Value::Int(r.deleted_count)),
                                ("acknowledged", Value::Bool(true)),
                            ])
                        }
                        "deleteMany" => {
                            let r = coll.delete_many(&arg(0)).unwrap();
                            obj(vec![
                                ("deletedCount", Value::Int(r.deleted_count)),
                                ("acknowledged", Value::Bool(true)),
                            ])
                        }
                        "size" => Value::Int(coll.size() as i64),
                        "getAll" => Value::Array(
                            coll.get_all()
                                .iter()
                                .map(|d| d.get("name").cloned().unwrap_or(Value::Null))
                                .collect(),
                        ),
                        other => panic!("unknown collection op {}", other),
                    };

                    if let Some(error) = step.get("error") {
                        failures.push(format!(
                            "{}: JS threw {:?}, rust returned {}",
                            label,
                            error,
                            json_of(&computed)
                        ));
                    } else {
                        compare(
                            &label,
                            &computed,
                            step.get("result").unwrap(),
                            &mut failures,
                        );
                    }
                }
            }
            other => panic!("unknown kind {}", other),
        }
    }

    println!("checked {} memory fixtures", checked);
    assert!(
        failures.is_empty(),
        "{}/{} memory fixtures failed:\n{}",
        failures.len(),
        checked,
        failures.join("\n\n")
    );
}

#[test]
fn regex_engine() {
    use mosql::memory::regex::Regex;
    let cases = [
        ("^a", "abc", true),
        ("^a", "bac", false),
        ("a.c", "abc", true),
        ("a.c", "ac", false),
        ("ab*", "a", true),
        ("ab+", "a", false),
        ("a|b", "b", true),
        ("[a-c]+", "bcd", true),
        ("\\d+", "abc123", true),
        ("\\d+", "abc", false),
        ("\\w+@\\w+", "a@b", true),
        ("^$", "", true),
        ("a{2,3}", "aa", true),
        ("a{2,3}", "a", false),
        ("(?i)A", "a", true),
    ];
    for (pattern, text, expected) in cases {
        let ci = pattern.starts_with("(?i)");
        let p = pattern.trim_start_matches("(?i)");
        let re = Regex::new(p, ci).unwrap();
        assert_eq!(
            re.is_match(text),
            expected,
            "pattern {} text {}",
            pattern,
            text
        );
    }
}
