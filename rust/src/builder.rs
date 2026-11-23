//! The query builder — a direct port of `createQueryBuilder`, `FindQuery` and
//! the `collection()` API.

use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::eprintln;

use crate::error::{Error, Result};
use crate::escape::escape;
use crate::json_path::{json_path, json_update, Cast, JsonOp};
use crate::ops::{expr_op, filter_op, update_op};
use crate::stages::{stage, AggState};
use crate::validate;
use crate::value::Value;
use crate::Db;

fn set(items: &[&str]) -> Rc<Vec<String>> {
    Rc::new(items.iter().map(|s| s.to_string()).collect())
}

#[derive(Clone, Default)]
pub struct Builder {
    // `Rc` so that cloning a `Builder` is a few refcount bumps. That lets
    // `Collection` and `FindQuery` own their builder instead of borrowing it,
    // which is what makes `Builder::full().collection(..)?.find(..)` chain.
    filter_allow: Option<Rc<Vec<String>>>,
    expr_allow: Option<Rc<Vec<String>>>,
    update_allow: Option<Rc<Vec<String>>>,
    stage_allow: Option<Rc<Vec<String>>>,
    debug: bool,
}

/// Options shared by the CRUD helpers.
#[derive(Clone, Debug, Default)]
pub struct CrudOptions {
    /// `Some(&[])` means `RETURNING *`; `Some(&["id"])` means `RETURNING "id"`.
    pub returning: Option<Vec<String>>,
    pub allow_delete_all: bool,
}

impl Builder {
    /// Every operator (`umosql`).
    pub fn full() -> Builder {
        Builder::default()
    }

    /// All filters and updates, trimmed expressions and 7 stages (`umosql/lite`).
    pub fn lite() -> Builder {
        Builder {
            expr_allow: Some(set(&[
                "$add",
                "$subtract",
                "$multiply",
                "$divide",
                "$mod",
                "$abs",
                "$ceil",
                "$floor",
                "$round",
                "$pow",
                "$sqrt",
                "$concat",
                "$upper",
                "$lower",
                "$substr",
                "$trim",
                "$ltrim",
                "$rtrim",
                "$strLen",
                "$replace",
                "$eq",
                "$ne",
                "$gt",
                "$gte",
                "$lt",
                "$lte",
                "$in",
                "$nin",
                "$and",
                "$or",
                "$not",
                "$cond",
                "$ifNull",
                "$exists",
                "$toString",
                "$toInt",
                "$toDouble",
                "$sum",
                "$avg",
                "$min",
                "$max",
                "$count",
            ])),
            stage_allow: Some(set(&[
                "$match", "$project", "$group", "$sort", "$limit", "$skip", "$count",
            ])),
            ..Default::default()
        }
    }

    /// 10 filters, 13 expressions, `$set` only (`umosql/tiny`).
    pub fn tiny() -> Builder {
        Builder {
            filter_allow: Some(set(&[
                "$eq", "$ne", "$gt", "$gte", "$lt", "$lte", "$in", "$nin", "$like", "$exists",
            ])),
            expr_allow: Some(set(&[
                "$add",
                "$subtract",
                "$multiply",
                "$divide",
                "$concat",
                "$upper",
                "$lower",
                "$eq",
                "$cond",
                "$sum",
                "$avg",
                "$min",
                "$max",
            ])),
            update_allow: Some(set(&["$set"])),
            stage_allow: Some(set(&[
                "$match", "$project", "$group", "$sort", "$limit", "$skip", "$count",
            ])),
            ..Default::default()
        }
    }

    pub fn with_debug(mut self, on: bool) -> Builder {
        self.debug = on;
        self
    }

    fn check_filter_op(&self, op: &str) -> Result<()> {
        if let Some(allow) = &self.filter_allow {
            if !allow.iter().any(|x| x == op) {
                return Err(Error::new(format!("Unknown operator: {}", op)));
            }
        }
        Ok(())
    }

    fn check_expr_op(&self, op: &str) -> Result<()> {
        if let Some(allow) = &self.expr_allow {
            if !allow.iter().any(|x| x == op) {
                return Err(Error::new(format!("Unknown expression operator: {}", op)));
            }
        }
        Ok(())
    }

    fn check_update_op(&self, op: &str) -> Result<()> {
        if let Some(allow) = &self.update_allow {
            if !allow.iter().any(|x| x == op) {
                return Err(Error::new(format!("Unknown update operator: {}", op)));
            }
        }
        Ok(())
    }

    fn check_stage_op(&self, op: &str) -> Result<()> {
        if let Some(allow) = &self.stage_allow {
            if !allow.iter().any(|x| x == op) {
                return Err(Error::new(format!("Unknown pipeline operator: {}", op)));
            }
        }
        Ok(())
    }

    fn log_query(&self, sql: &str) {
        #[cfg(feature = "std")]
        if self.debug {
            eprintln!("[SQL Query] {}", sql);
        }
        #[cfg(not(feature = "std"))]
        let _ = sql;
    }

    pub(crate) fn wrap(&self, s: &mut AggState) -> String {
        s.counter += 1;
        format!("({}) AS t{}", s.sql, s.counter)
    }

    pub(crate) fn apply_where(&self, s: &mut AggState) {
        if !s.wheres.is_empty() {
            s.sql
                .push_str(&format!(" WHERE {}", s.wheres.join(" AND ")));
            s.wheres.clear();
        }
    }

    // -----------------------------------------------------------------------
    // FILTER
    // -----------------------------------------------------------------------

    pub fn filter(&self, q: &Value, db: Db) -> Result<String> {
        let obj = match q {
            Value::Object(o) => o,
            other => return Ok(other.to_js_string()),
        };

        let mut parts: Vec<String> = Vec::new();

        for (k, v) in obj.iter() {
            if k.strip_prefix('$').is_some() {
                match k.as_str() {
                    "$and" | "$or" => {
                        let arr = validate::arr(v, k)?;
                        let join = if k == "$and" { " AND " } else { " OR " };
                        let mut inner = Vec::with_capacity(arr.len());
                        for item in arr {
                            inner.push(self.filter(item, db)?);
                        }
                        parts.push(format!("({})", inner.join(join)));
                    }
                    "$nor" => {
                        let arr = validate::arr(v, k)?;
                        let mut inner = Vec::with_capacity(arr.len());
                        for item in arr {
                            inner.push(self.filter(item, db)?);
                        }
                        parts.push(format!("NOT ({})", inner.join(" OR ")));
                    }
                    "$not" => parts.push(format!("NOT ({})", self.filter(v, db)?)),
                    "$expr" => parts.push(self.expr(v, db)?),
                    _ => return Err(Error::new(format!("Unknown filter operator: {}", k))),
                }
                continue;
            }

            let is_json = k.contains('.');
            let f = if is_json {
                json_path(k, db, Cast::None)?
            } else {
                validate::col(k, db)?
            };

            if let Some(conditions) = v.as_object() {
                let mut conds: Vec<String> = Vec::new();
                for (op, val) in conditions.iter() {
                    let cast = match val {
                        Value::Int(_) | Value::Float(_) => Cast::Numeric,
                        Value::Bool(_) => Cast::Boolean,
                        _ => Cast::None,
                    };
                    let f2 = if is_json && cast != Cast::None {
                        json_path(k, db, cast)?
                    } else {
                        f.clone()
                    };

                    if op == "$not" && val.is_object() {
                        let mut inner = Vec::new();
                        for (op2, val2) in val.as_object().unwrap().iter() {
                            self.check_filter_op(op2)?;
                            inner.push(format!("{} {}", f2, filter_op(op2, val2, db, &f2)?));
                        }
                        conds.push(format!("NOT ({})", inner.join(" AND ")));
                        continue;
                    }

                    self.check_filter_op(op)?;

                    if op == "$ilike" || op == "$nilike" {
                        if db == Db::Pg {
                            conds.push(format!("{} {}", f, filter_op(op, val, db, &f)?));
                        } else {
                            conds.push(format!("LOWER({}) {}", f, filter_op(op, val, db, &f)?));
                        }
                    } else {
                        conds.push(format!("{} {}", f2, filter_op(op, val, db, &f2)?));
                    }
                }
                if conds.len() > 1 {
                    parts.push(format!("({})", conds.join(" AND ")));
                } else if let Some(first) = conds.into_iter().next() {
                    parts.push(first);
                }
            } else {
                let cast = match v {
                    Value::Int(_) | Value::Float(_) => Cast::Numeric,
                    Value::Bool(_) => Cast::Boolean,
                    _ => Cast::None,
                };
                let f2 = if is_json && cast != Cast::None {
                    json_path(k, db, cast)?
                } else {
                    f
                };
                parts.push(format!("{} = {}", f2, escape(v, db)?));
            }
        }

        Ok(parts.join(" AND "))
    }

    // -----------------------------------------------------------------------
    // EXPRESSION
    // -----------------------------------------------------------------------

    /// Alias matching the JS export name (`expression`).
    pub fn expression(&self, x: &Value, db: Db) -> Result<String> {
        self.expr(x, db)
    }

    pub fn expr(&self, x: &Value, db: Db) -> Result<String> {
        if let Value::Str(s) = x {
            if let Some(v) = s.strip_prefix('$') {
                return if v.contains('.') {
                    json_path(v, db, Cast::None)
                } else {
                    validate::col(v, db)
                };
            }
            return if s.contains('.') {
                json_path(s, db, Cast::None)
            } else {
                escape(x, db)
            };
        }

        if let Value::Object(o) = x {
            let (op, args) = match o.first() {
                Some(entry) => entry,
                None => return Err(Error::new("Unknown expression operator: (empty object)")),
            };
            self.check_expr_op(op)?;
            let list: Vec<Value> = match args {
                Value::Array(items) => items.clone(),
                other => vec![other.clone()],
            };
            let mut rec = |e: &Value| self.expr(e, db);
            return expr_op(op, &list, db, &mut rec);
        }

        escape(x, db)
    }

    // -----------------------------------------------------------------------
    // AGGREGATE
    // -----------------------------------------------------------------------

    pub fn aggregate(&self, pipeline: &Value, table: &str, db: Db) -> Result<String> {
        let stages = pipeline
            .as_array()
            .ok_or_else(|| Error::new("aggregate requires array"))?;

        let mut s = AggState {
            sql: format!("SELECT * FROM {}", validate::col(table, db)?),
            ..Default::default()
        };

        for (i, st) in stages.iter().enumerate() {
            let obj = st
                .as_object()
                .ok_or_else(|| Error::new(format!("Stage {} must be an object", i)))?;
            let (op, args) = obj
                .first()
                .ok_or_else(|| Error::new(format!("Stage {} must have one operator", i)))?;
            self.check_stage_op(op)?;
            stage(self, op, args, &mut s, db)?;
        }

        let mut sql = s.sql;
        if !s.wheres.is_empty() {
            sql.push_str(&format!(" WHERE {}", s.wheres.join(" AND ")));
        }
        if let Some(group) = &s.group_by {
            sql.push_str(&format!(" GROUP BY {}", group));
        }
        if !s.having.is_empty() {
            sql.push_str(&format!(" HAVING {}", s.having.join(" AND ")));
        }
        sql.push_str(&s.order);
        sql.push_str(&s.limit);
        sql.push_str(&s.offset);

        self.log_query(&sql);
        Ok(sql.trim().to_string())
    }

    // -----------------------------------------------------------------------
    // CRUD
    // -----------------------------------------------------------------------

    pub fn insert_many(
        &self,
        table: &str,
        docs: &Value,
        db: Db,
        options: &CrudOptions,
    ) -> Result<String> {
        let docs = docs
            .as_array()
            .ok_or_else(|| Error::new("insertMany requires array"))?;
        if docs.is_empty() {
            return Err(Error::new("insertMany requires at least one document"));
        }

        let mut keys: Vec<String> = Vec::new();
        for doc in docs {
            if let Some(obj) = doc.as_object() {
                for k in obj.keys() {
                    if !keys.iter().any(|x| x == k) {
                        keys.push(k.to_string());
                    }
                }
            }
        }
        if keys.is_empty() {
            return Err(Error::new("Documents must have at least one field"));
        }

        let columns = keys
            .iter()
            .map(|k| validate::col(k, db))
            .collect::<Result<Vec<_>>>()?
            .join(", ");

        let mut rows = Vec::with_capacity(docs.len());
        for doc in docs {
            let mut values = Vec::with_capacity(keys.len());
            for k in &keys {
                match doc.get(k) {
                    Some(v) => values.push(escape(v, db)?),
                    None => values.push("NULL".to_string()),
                }
            }
            rows.push(format!("({})", values.join(", ")));
        }

        let mut sql = format!(
            "INSERT INTO {} ({}) VALUES {}",
            validate::col(table, db)?,
            columns,
            rows.join(", ")
        );

        if db == Db::Pg {
            if let Some(returning) = &options.returning {
                if returning.is_empty() {
                    sql.push_str(" RETURNING *");
                } else {
                    let cols = returning
                        .iter()
                        .map(|k| validate::col(k, db))
                        .collect::<Result<Vec<_>>>()?
                        .join(", ");
                    sql.push_str(&format!(" RETURNING {}", cols));
                }
            }
        }

        self.log_query(&sql);
        Ok(sql)
    }

    pub fn update_many(
        &self,
        table: &str,
        query: &Value,
        update: &Value,
        db: Db,
        options: &CrudOptions,
    ) -> Result<String> {
        let update = update
            .as_object()
            .ok_or_else(|| Error::new("Update must be an object"))?;

        let mut set_clauses: Vec<String> = Vec::new();

        for (op, fields) in update.iter() {
            if op.starts_with('$') {
                self.check_update_op(op)?;
                let fields = fields
                    .as_object()
                    .ok_or_else(|| Error::new(format!("{} requires an object", op)))?;
                set_clauses.extend(update_op(op, fields, db)?);
            } else {
                let dotted = op.contains('.');
                if dotted {
                    if let Some(sql) = json_update(op, fields, db, JsonOp::Set)? {
                        set_clauses.push(sql);
                    }
                } else {
                    let k = validate::col(op, db)?;
                    set_clauses.push(format!("{} = {}", k, escape(fields, db)?));
                }
            }
        }

        if set_clauses.is_empty() {
            return Err(Error::new("Update requires at least one operation"));
        }

        let mut sql = format!(
            "UPDATE {} SET {}",
            validate::col(table, db)?,
            set_clauses.join(", ")
        );

        if has_entries(query) {
            sql.push_str(&format!(" WHERE {}", self.filter(query, db)?));
        }

        if db == Db::Pg {
            if let Some(returning) = &options.returning {
                if returning.is_empty() {
                    sql.push_str(" RETURNING *");
                } else {
                    let cols = returning
                        .iter()
                        .map(|k| validate::col(k, db))
                        .collect::<Result<Vec<_>>>()?
                        .join(", ");
                    sql.push_str(&format!(" RETURNING {}", cols));
                }
            }
        }

        self.log_query(&sql);
        Ok(sql)
    }

    pub fn delete_many(
        &self,
        table: &str,
        query: &Value,
        db: Db,
        options: &CrudOptions,
    ) -> Result<String> {
        let mut sql = format!("DELETE FROM {}", validate::col(table, db)?);

        if has_entries(query) {
            sql.push_str(&format!(" WHERE {}", self.filter(query, db)?));
        } else if !options.allow_delete_all {
            return Err(Error::new(
                "deleteMany requires a filter or allowDeleteAll option",
            ));
        }

        if db == Db::Pg {
            if let Some(returning) = &options.returning {
                if returning.is_empty() {
                    sql.push_str(" RETURNING *");
                } else {
                    let cols = returning
                        .iter()
                        .map(|k| validate::col(k, db))
                        .collect::<Result<Vec<_>>>()?
                        .join(", ");
                    sql.push_str(&format!(" RETURNING {}", cols));
                }
            }
        }

        self.log_query(&sql);
        Ok(sql)
    }

    // -----------------------------------------------------------------------
    // COLLECTION
    // -----------------------------------------------------------------------

    pub fn collection(&self, name: &str, db: Db) -> Result<Collection> {
        Ok(Collection {
            builder: self.clone(),
            table: validate::col(name, db)?,
            db,
        })
    }

    pub fn db(&self, name: &str, database: Db) -> Result<Collection> {
        self.collection(name, database)
    }
}

fn has_entries(v: &Value) -> bool {
    match v {
        Value::Object(o) => !o.is_empty(),
        Value::Array(a) => !a.is_empty(),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// COLLECTION
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Collection {
    pub(crate) builder: Builder,
    pub(crate) table: String,
    pub(crate) db: Db,
}

impl Collection {
    pub fn name(&self) -> &str {
        &self.table
    }

    pub fn database(&self) -> Db {
        self.db
    }

    pub fn find(&self, query: &Value, projection: Option<&Value>) -> Result<FindQuery> {
        let mut q = FindQuery {
            builder: self.builder.clone(),
            table: self.table.clone(),
            db: self.db,
            query: query.clone(),
            fields: None,
            sort_obj: None,
            skip: String::new(),
            limit: String::new(),
            distinct: false,
        };
        if let Some(proj) = projection {
            q = q.select(proj)?;
        }
        Ok(q)
    }

    pub fn find_one(&self, query: &Value, projection: Option<&Value>) -> Result<FindQuery> {
        self.find(query, projection)?.limit(&Value::Int(1))
    }

    pub fn insert_one(&self, doc: &Value, options: &CrudOptions) -> Result<String> {
        self.builder.insert_many(
            &self.table,
            &Value::Array(vec![doc.clone()]),
            self.db,
            options,
        )
    }

    pub fn insert_many(&self, docs: &Value, options: &CrudOptions) -> Result<String> {
        self.builder
            .insert_many(&self.table, docs, self.db, options)
    }

    pub fn update_one(
        &self,
        query: &Value,
        update: &Value,
        options: &CrudOptions,
    ) -> Result<String> {
        let mut sql = self
            .builder
            .update_many(&self.table, query, update, self.db, options)?;
        if self.db != Db::Pg {
            sql.push_str(" LIMIT 1");
        }
        Ok(sql)
    }

    pub fn update_many(
        &self,
        query: &Value,
        update: &Value,
        options: &CrudOptions,
    ) -> Result<String> {
        self.builder
            .update_many(&self.table, query, update, self.db, options)
    }

    pub fn delete_one(&self, query: &Value, options: &CrudOptions) -> Result<String> {
        let mut sql = self
            .builder
            .delete_many(&self.table, query, self.db, options)?;
        if self.db != Db::Pg {
            sql.push_str(" LIMIT 1");
        }
        Ok(sql)
    }

    pub fn delete_many(&self, query: &Value, options: &CrudOptions) -> Result<String> {
        self.builder
            .delete_many(&self.table, query, self.db, options)
    }

    pub fn count_documents(&self, query: &Value) -> Result<String> {
        let mut sql = format!("SELECT COUNT(*) AS count FROM {}", self.table);
        if has_entries(query) {
            sql.push_str(&format!(" WHERE {}", self.builder.filter(query, self.db)?));
        }
        Ok(sql)
    }

    pub fn distinct(&self, field: &str, query: &Value) -> Result<String> {
        let f = if field.contains('.') {
            json_path(field, self.db, Cast::None)?
        } else {
            validate::col(field, self.db)?
        };
        let mut sql = format!("SELECT DISTINCT {} FROM {}", f, self.table);
        if has_entries(query) {
            sql.push_str(&format!(" WHERE {}", self.builder.filter(query, self.db)?));
        }
        Ok(sql)
    }

    pub fn aggregate(&self, pipeline: &Value) -> Result<String> {
        self.builder.aggregate(pipeline, &self.table, self.db)
    }
}

// ---------------------------------------------------------------------------
// FIND QUERY
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct FindQuery {
    builder: Builder,
    table: String,
    db: Db,
    query: Value,
    fields: Option<String>,
    sort_obj: Option<Value>,
    skip: String,
    limit: String,
    distinct: bool,
}

fn is_dollar(v: &Value) -> bool {
    matches!(v, Value::Str(s) if s.starts_with('$'))
}

impl FindQuery {
    pub fn select(mut self, proj: &Value) -> Result<Self> {
        if let Some(obj) = proj.as_object() {
            let mut included = Vec::new();
            let mut computed = Vec::new();
            for (k, v) in obj.iter() {
                if matches!(v, Value::Int(1) | Value::Bool(true)) {
                    included.push(if k.contains('.') {
                        json_path(k, self.db, Cast::None)?
                    } else {
                        validate::col(k, self.db)?
                    });
                } else if v.is_object() || is_dollar(v) {
                    computed.push(format!(
                        "{} AS {}",
                        self.builder.expr(v, self.db)?,
                        validate::alias(k)
                    ));
                }
            }
            let mut all = included;
            all.extend(computed);
            self.fields = if all.is_empty() {
                None
            } else {
                Some(all.join(", "))
            };
        } else if let Some(items) = proj.as_array() {
            let mut parts = Vec::new();
            for item in items {
                let field = item.to_js_string();
                parts.push(if field.contains('.') {
                    json_path(&field, self.db, Cast::None)?
                } else {
                    validate::col(&field, self.db)?
                });
            }
            self.fields = Some(parts.join(", "));
        } else if let Value::Str(s) = proj {
            self.fields = Some(s.clone());
        }
        Ok(self)
    }

    pub fn sort(mut self, obj: &Value) -> Result<Self> {
        self.sort_obj = Some(obj.clone());
        Ok(self)
    }

    pub fn skip(mut self, n: &Value) -> Result<Self> {
        self.skip = format!(" OFFSET {}", validate::int(n, "skip")?);
        Ok(self)
    }

    pub fn limit(mut self, n: &Value) -> Result<Self> {
        self.limit = format!(" LIMIT {}", validate::int(n, "limit")?);
        Ok(self)
    }

    pub fn distinct(mut self) -> Result<Self> {
        self.distinct = true;
        Ok(self)
    }

    pub fn count(mut self) -> Result<Self> {
        self.fields = Some("COUNT(*) AS count".to_string());
        self.sort_obj = None;
        self.skip = String::new();
        self.limit = String::new();
        self.distinct = false;
        Ok(self)
    }

    pub fn to_sql(&self) -> Result<String> {
        let fields = self.fields.clone().unwrap_or_else(|| "*".to_string());
        let distinct = if self.distinct { "DISTINCT " } else { "" };
        let mut sql = format!("SELECT {}{} FROM {}", distinct, fields, self.table);

        if has_entries(&self.query) {
            sql.push_str(&format!(
                " WHERE {}",
                self.builder.filter(&self.query, self.db)?
            ));
        }

        if let Some(sort_obj) = &self.sort_obj {
            let mut clauses = Vec::new();
            if let Some(obj) = sort_obj.as_object() {
                for (k, ord) in obj.iter() {
                    let f = if k.contains('.') {
                        json_path(k, self.db, Cast::None)?
                    } else {
                        validate::col(k, self.db)?
                    };
                    let asc = matches!(ord, Value::Int(1)) || ord.as_str() == Some("asc");
                    if self.db == Db::Pg {
                        clauses.push(format!(
                            "{} {}",
                            f,
                            if asc {
                                "ASC NULLS FIRST"
                            } else {
                                "DESC NULLS LAST"
                            }
                        ));
                    } else if self.db == Db::Mysql && self.distinct {
                        clauses.push(format!("{} {}", f, if asc { "ASC" } else { "DESC" }));
                    } else {
                        let nulls = if asc {
                            format!("CASE WHEN {} IS NULL THEN 0 ELSE 1 END", f)
                        } else {
                            format!("CASE WHEN {} IS NULL THEN 1 ELSE 0 END", f)
                        };
                        clauses.push(format!(
                            "{}, {} {}",
                            nulls,
                            f,
                            if asc { "ASC" } else { "DESC" }
                        ));
                    }
                }
            }
            if !clauses.is_empty() {
                sql.push_str(&format!(" ORDER BY {}", clauses.join(", ")));
            }
        }

        sql.push_str(&self.limit);
        sql.push_str(&self.skip);

        self.builder.log_query(&sql);
        Ok(sql.trim().to_string())
    }

    /// JS calls this `toString()`.
    pub fn to_string_value(&self) -> String {
        self.to_sql().unwrap_or_default()
    }
}

impl core::fmt::Display for FindQuery {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.to_string_value())
    }
}
