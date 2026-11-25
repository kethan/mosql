//! The query builder — a direct port of `createQueryBuilder`, `FindQuery` and
//! the `collection()` API.

use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::eprintln;

use crate::error::{Error, Result};
use crate::escape::{escape, write_escaped};
use crate::json_path::{json_path, json_update, write_json_path, Cast, JsonOp};
use crate::ops::{expr_op, update_op, write_filter_op};
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
        let mut out = String::new();
        self.write_filter(&mut out, q, db)?;
        Ok(out)
    }

    /// The filter, written into a buffer the caller already owns.
    ///
    /// The SQL builder writes into one string rather than making one per
    /// fragment: a query with five conditions used to make a dozen
    /// intermediate strings, and the allocations cost more than the
    /// parsing.
    pub fn write_filter(&self, out: &mut String, q: &Value, db: Db) -> Result<()> {
        let obj = match q {
            Value::Object(o) => o,
            other => {
                out.push_str(&other.to_js_string());
                return Ok(());
            }
        };

        let mut first = true;

        for (k, v) in obj.iter() {
            if k.strip_prefix('$').is_some() {
                match k.as_str() {
                    "$and" | "$or" => {
                        let arr = validate::arr(v, k)?;
                        let join = if k == "$and" { " AND " } else { " OR " };
                        if !first {
                            out.push_str(" AND ");
                        }
                        first = false;
                        out.push('(');
                        let mut inner_first = true;
                        for item in arr {
                            if !inner_first {
                                out.push_str(join);
                            }
                            inner_first = false;
                            self.write_filter(out, item, db)?;
                        }
                        out.push(')');
                    }
                    "$nor" => {
                        let arr = validate::arr(v, k)?;
                        if !first {
                            out.push_str(" AND ");
                        }
                        first = false;
                        out.push_str("NOT (");
                        let mut inner_first = true;
                        for item in arr {
                            if !inner_first {
                                out.push_str(" OR ");
                            }
                            inner_first = false;
                            self.write_filter(out, item, db)?;
                        }
                        out.push(')');
                    }
                    "$not" => {
                        if !first {
                            out.push_str(" AND ");
                        }
                        first = false;
                        out.push_str("NOT (");
                        self.write_filter(out, v, db)?;
                        out.push(')');
                    }
                    "$expr" => {
                        if !first {
                            out.push_str(" AND ");
                        }
                        first = false;
                        out.push_str(&self.expr(v, db)?);
                    }
                    _ => return Err(Error::new(format!("Unknown filter operator: {}", k))),
                }
                continue;
            }

            let is_json = k.contains('.');

            if let Some(conditions) = v.as_object() {
                // Every condition contributes exactly one clause, so the
                // count is known before the first one is written — and
                // the count decides whether the group is wrapped.
                let total = conditions.entries.len();
                if !first {
                    out.push_str(" AND ");
                }
                first = false;
                if total > 1 {
                    out.push('(');
                }
                let mut written = 0;
                for (op, val) in conditions.iter() {
                    if written > 0 {
                        out.push_str(" AND ");
                    }
                    written += 1;
                    let cast = match val {
                        Value::Int(_) | Value::Float(_) => Cast::Numeric,
                        Value::Bool(_) => Cast::Boolean,
                        _ => Cast::None,
                    };

                    if op == "$not" && val.is_object() {
                        out.push_str("NOT (");
                        let mut inner_first = true;
                        for (op2, val2) in val.as_object().unwrap().iter() {
                            self.check_filter_op(op2)?;
                            if !inner_first {
                                out.push_str(" AND ");
                            }
                            inner_first = false;
                            write_column(out, k, is_json, db, cast)?;
                            out.push(' ');
                            write_filter_op(out, op2, val2, db, "")?;
                        }
                        out.push(')');
                        continue;
                    }

                    self.check_filter_op(op)?;

                    if op == "$ilike" || op == "$nilike" {
                        if db == Db::Pg {
                            write_column(out, k, is_json, db, Cast::None)?;
                            out.push(' ');
                        } else {
                            out.push_str("LOWER(");
                            write_column(out, k, is_json, db, Cast::None)?;
                            out.push_str(") ");
                        }
                        write_filter_op(out, op, val, db, "")?;
                    } else {
                        write_column(out, k, is_json, db, cast)?;
                        out.push(' ');
                        write_filter_op(out, op, val, db, "")?;
                    }
                }
                if total > 1 {
                    out.push(')');
                }
            } else {
                let cast = match v {
                    Value::Int(_) | Value::Float(_) => Cast::Numeric,
                    Value::Bool(_) => Cast::Boolean,
                    _ => Cast::None,
                };
                if !first {
                    out.push_str(" AND ");
                }
                first = false;
                write_column(out, k, is_json, db, cast)?;
                out.push_str(" = ");
                write_escaped(out, v, db)?;
            }
        }

        Ok(())
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
            sql.push_str(" WHERE ");
            for (index, clause) in s.wheres.iter().enumerate() {
                if index > 0 {
                    sql.push_str(" AND ");
                }
                sql.push_str(clause);
            }
        }
        if let Some(group) = &s.group_by {
            sql.push_str(" GROUP BY ");
            sql.push_str(group);
        }
        if !s.having.is_empty() {
            sql.push_str(" HAVING ");
            for (index, clause) in s.having.iter().enumerate() {
                if index > 0 {
                    sql.push_str(" AND ");
                }
                sql.push_str(clause);
            }
        }
        sql.push_str(&s.order);
        sql.push_str(&s.limit);
        sql.push_str(&s.offset);

        self.log_query(&sql);
        let trimmed = sql.trim();
        Ok(if trimmed.len() == sql.len() {
            sql
        } else {
            trimmed.to_string()
        })
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

        // The column names are borrowed from the documents, and the
        // statement is written into one string: an insert of fifty
        // documents used to make a string for every value and another
        // for every row.
        let mut keys: Vec<&str> = Vec::new();
        for doc in docs {
            if let Some(obj) = doc.as_object() {
                for k in obj.keys() {
                    if !keys.iter().any(|x| *x == k) {
                        keys.push(k);
                    }
                }
            }
        }
        if keys.is_empty() {
            return Err(Error::new("Documents must have at least one field"));
        }

        let mut sql = String::with_capacity(64 + docs.len() * 32);
        sql.push_str("INSERT INTO ");
        validate::write_col(&mut sql, table, db)?;
        sql.push_str(" (");
        for (index, k) in keys.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            validate::write_col(&mut sql, k, db)?;
        }
        sql.push_str(") VALUES ");
        for (index, doc) in docs.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            sql.push('(');
            for (column, k) in keys.iter().enumerate() {
                if column > 0 {
                    sql.push_str(", ");
                }
                match doc.get(k) {
                    Some(v) => write_escaped(&mut sql, v, db)?,
                    None => sql.push_str("NULL"),
                }
            }
            sql.push(')');
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

        let mut sql = String::with_capacity(96);
        sql.push_str("UPDATE ");
        validate::write_col(&mut sql, table, db)?;
        sql.push_str(" SET ");
        for (index, clause) in set_clauses.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            sql.push_str(clause);
        }

        if has_entries(query) {
            sql.push_str(" WHERE ");
            self.write_filter(&mut sql, query, db)?;
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
        // The statement is written into one string. It used to be made
        // out of a dozen pieces, each its own allocation, and the
        // allocations cost more than the parsing.
        let mut sql = String::with_capacity(96);
        sql.push_str("SELECT ");
        if self.distinct {
            sql.push_str("DISTINCT ");
        }
        match &self.fields {
            Some(fields) => sql.push_str(fields),
            None => sql.push('*'),
        }
        sql.push_str(" FROM ");
        sql.push_str(&self.table);

        if has_entries(&self.query) {
            sql.push_str(" WHERE ");
            self.builder.write_filter(&mut sql, &self.query, self.db)?;
        }

        if let Some(sort_obj) = &self.sort_obj {
            let mut clauses = String::new();
            let mut f = String::new();
            if let Some(obj) = sort_obj.as_object() {
                for (k, ord) in obj.iter() {
                    f.clear();
                    if k.contains('.') {
                        write_json_path(&mut f, k, self.db, Cast::None)?;
                    } else {
                        validate::write_col(&mut f, k, self.db)?;
                    }
                    let asc = matches!(ord, Value::Int(1)) || ord.as_str() == Some("asc");
                    if !clauses.is_empty() {
                        clauses.push_str(", ");
                    }
                    if self.db == Db::Pg {
                        clauses.push_str(&f);
                        clauses.push(' ');
                        clauses.push_str(if asc {
                            "ASC NULLS FIRST"
                        } else {
                            "DESC NULLS LAST"
                        });
                    } else if self.db == Db::Mysql && self.distinct {
                        clauses.push_str(&f);
                        clauses.push(' ');
                        clauses.push_str(if asc { "ASC" } else { "DESC" });
                    } else {
                        clauses.push_str("CASE WHEN ");
                        clauses.push_str(&f);
                        clauses.push_str(if asc {
                            " IS NULL THEN 0 ELSE 1 END, "
                        } else {
                            " IS NULL THEN 1 ELSE 0 END, "
                        });
                        clauses.push_str(&f);
                        clauses.push(' ');
                        clauses.push_str(if asc { "ASC" } else { "DESC" });
                    }
                }
            }
            if !clauses.is_empty() {
                sql.push_str(" ORDER BY ");
                sql.push_str(&clauses);
            }
        }

        sql.push_str(&self.limit);
        sql.push_str(&self.skip);

        self.builder.log_query(&sql);
        // The statement is trimmed, but it rarely has anything to trim,
        // and then the string it already has is the answer.
        let trimmed = sql.trim();
        Ok(if trimmed.len() == sql.len() {
            sql
        } else {
            trimmed.to_string()
        })
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

/// The column expression, written into a buffer the caller already owns.
///
/// A dotted path is a JSON path, and a plain name is an identifier. The
/// cast applies to the JSON case only, which is what the original did.
fn write_column(out: &mut String, key: &str, is_json: bool, db: Db, cast: Cast) -> Result<()> {
    if is_json {
        write_json_path(out, key, db, cast)
    } else {
        validate::write_col(out, key, db)
    }
}
