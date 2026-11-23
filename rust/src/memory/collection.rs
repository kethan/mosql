//! In-memory `Collection` / `Database` — a port of the JS classes.

use crate::error::{Error, Result};
use crate::value::{to_json, Object, Value};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::eval::{get_path, matches, set_path};
use super::stages::{aggregate, sort_values};
use super::update::apply_update;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdStrategy {
    Auto,
    Mongo,
}

impl IdStrategy {
    pub fn parse(s: &str) -> IdStrategy {
        if s == "mongo" {
            IdStrategy::Mongo
        } else {
            IdStrategy::Auto
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CollectionOptions {
    pub id_column: Option<String>,
    pub id_strategy: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Stats {
    pub name: String,
    pub count: usize,
    pub size: usize,
    pub avg_obj_size: f64,
}

#[derive(Clone, Debug)]
pub struct UpdateResult {
    pub matched_count: i64,
    pub modified_count: i64,
    pub upserted_count: i64,
}

#[derive(Clone, Debug)]
pub struct DeleteResult {
    pub deleted_count: i64,
}

pub struct Collection {
    pub name: String,
    data: Vec<Value>,
    next_id: i64,
    pub id_column: String,
    pub id_strategy: IdStrategy,
    mongo_seq: u64,
}

impl Collection {
    pub fn new(name: &str, data: Vec<Value>, options: CollectionOptions) -> Collection {
        let max = data
            .iter()
            .filter_map(|d| d.get("_id").and_then(|v| v.as_f64()))
            .filter(|v| v.is_finite())
            .fold(f64::NEG_INFINITY, f64::max);
        let next_id = if max.is_finite() { max as i64 + 1 } else { 1 };

        Collection {
            name: name.to_string(),
            data,
            next_id,
            id_column: options
                .id_column
                .clone()
                .unwrap_or_else(|| "_id".to_string()),
            id_strategy: IdStrategy::parse(options.id_strategy.as_deref().unwrap_or("auto")),
            mongo_seq: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn get_all(&self) -> Vec<Value> {
        self.data.clone()
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }

    fn gen_mongo_id(&mut self) -> Value {
        self.mongo_seq += 1;
        let ts = (1_704_067_200u64 + self.mongo_seq / 1000) & 0xffff_ffff;
        let rand = self.mongo_seq.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        Value::Str(format!("{:08x}{:016x}", ts, rand))
    }

    // -------------------------------------------------------------------
    // READ
    // -------------------------------------------------------------------

    pub fn find(&self, query: &Value) -> MemFind<'_> {
        MemFind {
            coll: self,
            query: query.clone(),
            projection: None,
            sort: None,
            skip: 0,
            limit: None,
            distinct: false,
            count: false,
        }
    }

    pub fn find_one(&self, query: &Value, projection: Option<&Value>) -> Result<Option<Value>> {
        let mut q = self.find(query);
        if let Some(p) = projection {
            q = q.select(p);
        }
        let mut out = q.limit(1).to_vec()?;
        Ok(if out.is_empty() {
            None
        } else {
            Some(out.remove(0))
        })
    }

    pub fn find_by_id(&self, id: &Value) -> Result<Option<Value>> {
        let mut q = Object::new();
        q.set("_id", id.clone());
        self.find_one(&Value::Object(q), None)
    }

    pub fn count_documents(&self, query: &Value) -> Result<i64> {
        let mut n = 0;
        for doc in &self.data {
            if matches(query, doc)? {
                n += 1;
            }
        }
        Ok(n)
    }

    pub fn estimated_document_count(&self) -> i64 {
        self.data.len() as i64
    }

    pub fn distinct(&self, field: &str, query: &Value) -> Result<Vec<Value>> {
        let mut out: Vec<Value> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for doc in &self.data {
            if !matches(query, doc)? {
                continue;
            }
            if let Some(v) = get_path(doc, field) {
                let key = to_json(v);
                if !seen.contains(&key) {
                    seen.push(key);
                    out.push(v.clone());
                }
            }
        }
        Ok(out)
    }

    pub fn aggregate(&self, pipeline: &Value) -> Result<Vec<Value>> {
        aggregate(pipeline, &self.data)
    }

    // -------------------------------------------------------------------
    // WRITE
    // -------------------------------------------------------------------

    pub fn insert_one(&mut self, doc: Value) -> Result<Value> {
        let mut n = doc;
        let id_col = self.id_column.clone();

        let missing = n.get(&id_col).map_or(true, |v| v.is_null());
        if missing {
            if self.id_strategy == IdStrategy::Mongo {
                set_path(&mut n, &id_col, self.gen_mongo_id());
            } else {
                set_path(&mut n, &id_col, Value::Int(self.next_id));
                self.next_id += 1;
            }
        }

        let inserted = n.get(&id_col).cloned().unwrap_or(Value::Null);
        self.data.push(n);
        Ok(inserted)
    }

    pub fn insert_many(&mut self, docs: &[Value]) -> Result<Vec<Value>> {
        let mut ids = Vec::with_capacity(docs.len());
        for doc in docs {
            ids.push(self.insert_one(doc.clone())?);
        }
        Ok(ids)
    }

    fn apply_update(&self, doc: &mut Value, update: &Value, is_insert: bool) -> Result<()> {
        let obj = update
            .as_object()
            .ok_or_else(|| Error::new("Update must be an object"))?;
        for (op, fields) in obj.iter() {
            if op.starts_with('$') {
                let fields = fields
                    .as_object()
                    .ok_or_else(|| Error::new(format!("{} requires an object", op)))?;
                apply_update(op, fields, doc, is_insert)?;
            } else {
                set_path(doc, op, fields.clone());
            }
        }
        Ok(())
    }

    fn position(&self, query: &Value) -> Result<Option<usize>> {
        for (i, doc) in self.data.iter().enumerate() {
            if matches(query, doc)? {
                return Ok(Some(i));
            }
        }
        Ok(None)
    }

    pub fn update_one(
        &mut self,
        query: &Value,
        update: &Value,
        upsert: bool,
    ) -> Result<UpdateResult> {
        match self.position(query)? {
            None => {
                if !upsert {
                    return Ok(UpdateResult {
                        matched_count: 0,
                        modified_count: 0,
                        upserted_count: 0,
                    });
                }
                let mut n = Value::object();
                self.apply_update(&mut n, update, true)?;
                let mut base = Value::object();
                if let Value::Object(q) = query {
                    for (k, v) in q.iter() {
                        if let Value::Object(o) = &mut base {
                            o.set(k.clone(), v.clone());
                        }
                    }
                }
                if let Value::Object(o) = &n {
                    for (k, v) in o.iter() {
                        if let Value::Object(b) = &mut base {
                            b.set(k.clone(), v.clone());
                        }
                    }
                }
                self.insert_one(base)?;
                Ok(UpdateResult {
                    matched_count: 0,
                    modified_count: 0,
                    upserted_count: 1,
                })
            }
            Some(idx) => {
                let mut c = self.data[idx].clone();
                self.apply_update(&mut c, update, false)?;
                self.data[idx] = c;
                Ok(UpdateResult {
                    matched_count: 1,
                    modified_count: 1,
                    upserted_count: 0,
                })
            }
        }
    }

    pub fn update_many(&mut self, query: &Value, update: &Value) -> Result<UpdateResult> {
        let mut modified = 0;
        for i in 0..self.data.len() {
            if matches(query, &self.data[i])? {
                let mut c = self.data[i].clone();
                self.apply_update(&mut c, update, false)?;
                self.data[i] = c;
                modified += 1;
            }
        }
        Ok(UpdateResult {
            matched_count: modified,
            modified_count: modified,
            upserted_count: 0,
        })
    }

    pub fn replace_one(
        &mut self,
        query: &Value,
        replacement: &Value,
        upsert: bool,
    ) -> Result<UpdateResult> {
        match self.position(query)? {
            None => {
                if !upsert {
                    return Ok(UpdateResult {
                        matched_count: 0,
                        modified_count: 0,
                        upserted_count: 0,
                    });
                }
                self.insert_one(replacement.clone())?;
                Ok(UpdateResult {
                    matched_count: 0,
                    modified_count: 0,
                    upserted_count: 1,
                })
            }
            Some(idx) => {
                let old_id = self.data[idx].get("_id").cloned();
                let mut next = replacement.clone();
                if let Some(id) = old_id {
                    if let Value::Object(o) = &mut next {
                        o.set("_id", id);
                    }
                }
                self.data[idx] = next;
                Ok(UpdateResult {
                    matched_count: 1,
                    modified_count: 1,
                    upserted_count: 0,
                })
            }
        }
    }

    pub fn delete_one(&mut self, query: &Value) -> Result<DeleteResult> {
        match self.position(query)? {
            None => Ok(DeleteResult { deleted_count: 0 }),
            Some(idx) => {
                self.data.remove(idx);
                Ok(DeleteResult { deleted_count: 1 })
            }
        }
    }

    pub fn delete_many(&mut self, query: &Value) -> Result<DeleteResult> {
        let before = self.data.len();
        let mut kept = Vec::with_capacity(before);
        for doc in self.data.drain(..) {
            if !matches(query, &doc)? {
                kept.push(doc);
            }
        }
        self.data = kept;
        Ok(DeleteResult {
            deleted_count: (before - self.data.len()) as i64,
        })
    }

    /// Remove every document and reset the id counter.
    ///
    /// Named `clear` rather than `drop`: a method called `drop` on a type is
    /// easily mistaken for `std::ops::Drop::drop`.
    pub fn clear(&mut self) {
        self.data.clear();
        self.next_id = 1;
    }
}

// ---------------------------------------------------------------------------
// FIND CURSOR
// ---------------------------------------------------------------------------

pub struct MemFind<'a> {
    coll: &'a Collection,
    query: Value,
    projection: Option<Value>,
    sort: Option<Value>,
    skip: i64,
    limit: Option<i64>,
    distinct: bool,
    count: bool,
}

impl<'a> MemFind<'a> {
    pub fn select(mut self, projection: &Value) -> Self {
        self.projection = Some(projection.clone());
        self
    }

    pub fn sort(mut self, sort: &Value) -> Self {
        self.sort = Some(sort.clone());
        self
    }

    pub fn skip(mut self, n: i64) -> Self {
        self.skip = n.max(0);
        self
    }

    pub fn limit(mut self, n: i64) -> Self {
        self.limit = Some(n.max(0));
        self
    }

    pub fn distinct(mut self) -> Self {
        self.distinct = true;
        self
    }

    pub fn count(mut self) -> Self {
        self.count = true;
        self
    }

    pub fn to_vec(&self) -> Result<Vec<Value>> {
        let mut res: Vec<Value> = Vec::new();
        for doc in &self.coll.data {
            if matches(&self.query, doc)? {
                res.push(doc.clone());
            }
        }

        if let Some(sort) = &self.sort {
            sort_values(&mut res, sort);
        }
        if self.skip > 0 {
            res = res.into_iter().skip(self.skip as usize).collect();
        }
        if let Some(limit) = self.limit {
            res = res.into_iter().take(limit as usize).collect();
        }
        if self.count {
            let mut o = Object::new();
            o.set("count", Value::Int(res.len() as i64));
            return Ok(vec![Value::Object(o)]);
        }
        if self.distinct {
            let mut seen: Vec<String> = Vec::new();
            res.retain(|d| {
                let key = to_json(d);
                if seen.contains(&key) {
                    false
                } else {
                    seen.push(key);
                    true
                }
            });
        }
        if let Some(proj) = &self.projection {
            let mut projected = Vec::with_capacity(res.len());
            for doc in &res {
                projected.push(crate::memory::stages::find_projection(doc, proj)?);
            }
            res = projected;
        }
        Ok(res)
    }
}

// ---------------------------------------------------------------------------
// DATABASE
// ---------------------------------------------------------------------------

pub struct Database {
    pub name: String,
    collections: Vec<(String, Collection)>,
}

impl Database {
    pub fn new(name: &str) -> Database {
        Database {
            name: name.to_string(),
            collections: Vec::new(),
        }
    }

    pub fn collection(
        &mut self,
        name: &str,
        init: Option<Vec<Value>>,
        options: CollectionOptions,
    ) -> &mut Collection {
        if !self.collections.iter().any(|(n, _)| n == name) {
            self.collections.push((
                name.to_string(),
                Collection::new(name, init.unwrap_or_default(), options),
            ));
        }
        let name = name.to_string();
        self.collections
            .iter_mut()
            .find(|(n, _)| *n == name)
            .map(|(_, c)| c)
            .unwrap()
    }

    pub fn drop_collection(&mut self, name: &str) -> bool {
        let before = self.collections.len();
        self.collections.retain(|(n, _)| n != name);
        before != self.collections.len()
    }

    pub fn list_collections(&self) -> Vec<String> {
        self.collections.iter().map(|(n, _)| n.clone()).collect()
    }

    pub fn stats(&self, name: &str) -> Option<Stats> {
        let coll = self
            .collections
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| c)?;
        let data = coll.get_all();
        let size = to_json(&Value::Array(data)).len();
        let count = coll.size();
        Some(Stats {
            name: coll.name.clone(),
            count,
            size,
            avg_obj_size: if count > 0 {
                size as f64 / count as f64
            } else {
                0.0
            },
        })
    }

    pub fn drop_database(&mut self) {
        self.collections.clear();
    }
}
