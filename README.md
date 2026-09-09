# UMoSQL

**MongoDB-style queries for SQL databases (and memory, and MongoDB itself).**

Write MongoDB queries — `find({ age: { $gte: 18 } })`, `$group`, `$set`, JSON paths — and run them on **PostgreSQL, MySQL, SQLite**, a zero-driver **in-memory engine**, or real **MongoDB** with the same collection API.

[![tests](https://github.com/kethan/mosql/actions/workflows/node.js.yml/badge.svg)](https://github.com/kethan/mosql/actions/workflows/node.js.yml)
[![Version](https://img.shields.io/npm/v/umosql.svg?color=success&style=flat-square)](https://www.npmjs.com/package/umosql)
[![Full](https://deno.bundlejs.com/badge?q=umosql&treeshake=[*]&config={"compression":"brotli"})](https://unpkg.com/umosql)
[![Lite](https://deno.bundlejs.com/badge?q=umosql/lite&treeshake=[*]&config={"compression":"brotli"})](https://unpkg.com/umosql/lite)
[![Tiny](https://deno.bundlejs.com/badge?q=umosql/tiny&treeshake=[*]&config={"compression":"brotli"})](https://unpkg.com/umosql/tiny)

---

## Table of Contents

- [Install](#install)
- [Entry points](#entry-points)
- [Quick start](#quick-start)
- [Choosing a version (Full / Lite / Tiny)](#choosing-a-version-full--lite--tiny)
- [Query examples](#query-examples)
  - [Basic queries](#basic-queries)
  - [Array operators ($in, $nin)](#array-operators-in-nin)
  - [Logical operators](#logical-operators)
  - [Pattern matching](#pattern-matching)
  - [JSON field queries](#json-field-queries)
- [CRUD operations](#crud-operations)
  - [Find operations](#find-operations)
  - [Insert operations](#insert-operations)
  - [Update operations](#update-operations)
  - [Delete operations](#delete-operations)
- [Aggregation pipeline](#aggregation-pipeline)
- [Operator reference](#operator-reference)
  - [Filter operators](#filter-operators)
  - [Update operators](#update-operators)
  - [Expression operators](#expression-operators)
- [Custom operators (extend)](#custom-operators-extend)
- [Custom builds (ultra-minimal)](#custom-builds-ultra-minimal)
- [Schemaless adapters](#schemaless-adapters)
  - [Unified client (memory / sqlite / pg / mysql / mongodb / sql)](#unified-client-memory--sqlite--pg--mysql--mongodb--sql)
  - [Bring your own driver](#bring-your-own-driver)
  - [Auto DDL, schema inference and migration](#auto-ddl-schema-inference-and-migration)
  - [Schemas, defaults, hidden fields](#schemas-defaults-hidden-fields)
  - [Auto ID creation](#auto-id-creation)
  - [DDL helpers](#ddl-helpers)
  - [Method support matrix](#method-support-matrix)
- [Serverless (Neon, Turso, PlanetScale, Drizzle, Hyperdrive, ...)](#serverless-neon-turso-planetscale-drizzle-hyperdrive-)
- [Express / REST integration](#express--rest-integration)
- [In-memory engine](#in-memory-engine)
- [Operator support matrix](#operator-support-matrix)
- [Compatibility and caveats](#compatibility-and-caveats)
- [Testing](#testing)
- [API reference](#api-reference)
- [Examples directory](#examples-directory)
- [License](#license)

---

## Install

```bash
npm i umosql
```

> **umosql has zero runtime dependencies.** No database driver is bundled, and none is declared as a
> dependency or peer dependency — nothing gets auto-installed into your project. You install the driver
> you want (`pg`, `mysql2`, `better-sqlite3`, `mongodb`), and it is loaded lazily only when you request
> that backend. `dotenv` is optional the same way: `.env` is read when it is installed, silently skipped
> when it is not.
>
> Requires Node.js ≥ 18. Each entry point ships as ESM, CommonJS, UMD, a minified IIFE, and a `.d.ts`.

## Entry points

Everything below ships in the single `umosql` package — import only what you need:

| Entry point | Import | Size (gzip) | What you get |
| --- | --- | --- | --- |
| **Full** | `import { createQueryBuilder, ... } from "umosql"` | ~8.6 kB | Complete operator packs + `createQueryBuilder` (SQL strings, no driver) |
| **Lite** | `import lite from "umosql/lite"` | ~8.4 kB | Prebuilt builder: all filter ops, most expression ops, all update ops, basic stages |
| **Tiny** | `import tiny from "umosql/tiny"` | ~6.6 kB | Prebuilt builder: 10 filter ops, 13 expression ops, `$set` only |
| **Schemaless** | `import { createSchemalessAdapter } from "umosql/schemaless"` | ~22 kB | Executes queries: wraps a `better-sqlite3` / `pg` / `mysql2` handle, or any `execute(sql, params)` |
| **Memory** | `import { collection } from "umosql/memory"` | ~8.3 kB | In-memory MongoDB-style engine (no SQL, no driver) |
| **Client** | `import { createSchemalessClient } from "umosql/client"` | ~25.6 kB | One async factory for `memory` / `sqlite` / `pg` / `mysql` / `mongodb` / `sql` |

The query-builder entries (`umosql`, `/lite`, `/tiny`) only **produce SQL strings**. The adapter entries
(`/schemaless`, `/memory`, `/client`) **execute** them against a database.

> **Why is the full builder not pre-wired?** The `umosql` entry exports the raw operator packs
> (`filterOps`, `exprOps`, `updateOps`, `stageHandlers`) plus `createQueryBuilder` so bundlers can
> tree-shake every operator you don't use. `/lite` and `/tiny` are pre-wired convenience builds.
> Calling `collection()` from the bare `umosql` entry throws `Unknown operator: …` by design —
> see [Quick start](#quick-start) for the one-liner that wires it up.

## Quick start

### 1. Query builder → SQL strings (no database required)

```javascript
import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers } from "umosql";

// Wire up the full operator set once (tree-shakeable: pick only what you need)
const qb = createQueryBuilder({ filterOps, exprOps, updateOps, stageHandlers });

const users = qb.collection("users", "pg"); // 'pg' | 'mysql' | 'sqlite'

users.find({ age: { $gte: 18 } }).toSQL();
// SELECT * FROM users WHERE age >= 18

users.find({ status: "active" }, { name: 1, email: 1 }).sort({ age: -1 }).skip(10).limit(5).toSQL();
// SELECT name, email FROM users WHERE status = 'active' ORDER BY age DESC LIMIT 5 OFFSET 10

users.updateOne(
    { email: "alice@example.com" },
    { $set: { status: "active" }, $inc: { loginCount: 1 } }
);
// UPDATE users SET status = 'active', loginCount = loginCount + 1
// WHERE email = 'alice@example.com' LIMIT 1

users.insertMany([{ name: "Alice", age: 25 }, { name: "Bob", age: 30 }]);
// INSERT INTO users (name, age) VALUES ('Alice', 25), ('Bob', 30)

// find/findOne return a chainable FindQuery; every other method returns the SQL string directly.
```

Or use a prebuilt variant:

```javascript
import lite from "umosql/lite";
import tiny from "umosql/tiny";

lite.collection("users", "pg").find({ "profile.country": "FR" }).toSQL();
tiny.collection("users", "sqlite").find({ age: { $gte: 18 } }).toSQL();
```

### 2. Schemaless adapter — executes against a real database

Tables, columns and JSON columns are created and evolved automatically from your documents:

```javascript
import Database from "better-sqlite3";
import { createSchemalessAdapter } from "umosql/schemaless";

const { adapter } = createSchemalessAdapter(new Database(":memory:"), "sqlite");
const users = adapter.collection("users");

await users.insertOne({ name: "Alice", profile: { score: 85 } }); // creates table + columns
await users.updateOne({ name: "Alice" }, { $inc: { "profile.score": 5 } });
console.log(await users.estimatedDocumentCount()); // 1
```

### 3. Unified client — one factory, every backend

```javascript
import { createSchemalessClient } from "umosql/client";

const client = await createSchemalessClient("memory"); // or 'sqlite' | 'pg' | 'mysql' | 'mongodb' | 'sql'
const users = client.db("app").collection("users");

await users.insertOne({ name: "Alice", age: 25 });
const adults = await (await users.find({ age: { $gte: 18 } })).sort({ age: -1 }).toArray();
console.log(adults, await users.countDocuments({}));
await client.close();
```

Runnable tours: `node examples/client.js` (works with zero configuration — it falls back to memory/SQLite
and exercises live backends only when their env vars are set).

## Choosing a version (Full / Lite / Tiny)

| Feature | Full | Lite | Tiny |
| --- | --- | --- | --- |
| **Filter operators** | ✅ All 16 | ✅ All 16 | ✅ 10 (`$eq`, `$ne`, `$gt`, `$gte`, `$lt`, `$lte`, `$in`, `$nin`, `$like`, `$exists`) |
| **JSON paths** (`profile.score`) | ✅ | ✅ | ✅ |
| **Update operators** | ✅ All 8 | ✅ All 8 | ✅ `$set` only |
| **Expression operators** | ✅ All 50 | ✅ 36 | ✅ 13 |
| **Aggregation stages** | ✅ All 12 | ✅ 7 basic | ✅ 7 basic |
| **Custom operator packs** | ✅ `createQueryBuilder` | via `extend` | via `extend` |
| **Size (gzip)** | ~8.6 kB | ~8.4 kB | ~6.6 kB |

Stages included in Lite/Tiny: `$match`, `$project`, `$group`, `$sort`, `$limit`, `$skip`, `$count`.
Full adds `$addFields`/`$set`, `$sample`, `$sortByCount`, `$bucket`, `$unwind` (memory).

### Use **Full** when

- You want every operator, including `$regex`, `$ilike`, `$between`, `$mod`, `$switch`, date parts and casts
- You build custom bundles with only the operators you need (`createQueryBuilder`)
- You are migrating from MongoDB to SQL

### Use **Lite** when

- You want the full filter/update surface with a smaller expression set
- You need JSON columns and basic aggregation without the long tail of expression operators

### Use **Tiny** when

- Simple CRUD: equality/range/`$in`/`$like` filters and `$set` updates
- Smallest bundle size matters (edge functions, embeds)

All three produce SQL for PostgreSQL, MySQL and SQLite, and all three support JSON paths.

## Query examples

All examples below use the wired builder from [Quick start](#quick-start):

```javascript
const users = qb.collection("users", "pg");
```

### Basic queries

```javascript
// Equality
users.find({ status: "active" }).toSQL();
// SELECT * FROM users WHERE status = 'active'

// Range + comparison operators
users.find({ age: { $gte: 18 } }).toSQL();
users.find({ age: { $gt: 18, $lt: 65 } }).toSQL();
users.find({ status: { $ne: "banned" } }).toSQL();

// Multiple conditions (implicit AND)
users.find({ age: { $gte: 18 }, status: "active" }).toSQL();
// SELECT * FROM users WHERE age >= 18 AND status = 'active'

// Between and modulo (Full/Lite)
users.find({ age: { $between: [18, 65] } }).toSQL();
// age BETWEEN 18 AND 65
users.find({ id: { $mod: [10, 0] } }).toSQL();
// (id % 10) = 0
```

### Array operators ($in, $nin)

```javascript
users.find({ status: { $in: ["active", "pending", "verified"] } }).toSQL();
// SELECT * FROM users WHERE status IN ('active', 'pending', 'verified')

users.find({ role: { $nin: ["admin", "moderator"] } }).toSQL();
// SELECT * FROM users WHERE role NOT IN ('admin', 'moderator')

// Empty-array edge cases are handled
users.find({ status: { $in: [] } }).toSQL();
// ... WHERE status = 1 AND 1 = 0 (always false)

users.find({ status: { $nin: [] } }).toSQL();
// ... WHERE status = 1 OR 1 = 1 (always true)
```

### Logical operators

```javascript
users.find({ $and: [{ age: { $gte: 18 } }, { status: "active" }] }).toSQL();
// SELECT * FROM users WHERE (age >= 18 AND status = 'active')

users.find({ $or: [{ role: "admin" }, { role: "moderator" }] }).toSQL();
// SELECT * FROM users WHERE (role = 'admin' OR role = 'moderator')

users.find({ $not: { status: "banned" } }).toSQL();
// SELECT * FROM users WHERE NOT (status = 'banned')

// Nested logic
users.find({
    $and: [
        { age: { $gte: 18 } },
        { $or: [{ status: "active" }, { status: "verified" }] },
    ],
}).toSQL();
// SELECT * FROM users WHERE (age >= 18 AND (status = 'active' OR status = 'verified'))
```

### Pattern matching

```javascript
users.find({ email: { $like: "%@gmail.com" } }).toSQL();
// SELECT * FROM users WHERE email LIKE '%@gmail.com'

users.find({ name: { $ilike: "alice" } }).toSQL();
// PostgreSQL: SELECT * FROM users WHERE name ILIKE 'alice'
// MySQL/SQLite: LOWER(name) LIKE LOWER('alice')

users.find({ email: { $nlike: "%@temporary.com" } }).toSQL();
users.find({ username: { $nilike: "admin%" } }).toSQL();

// Regex: PG uses `~`, MySQL uses REGEXP, SQLite needs a REGEXP UDF (Full/Lite)
users.find({ code: { $regex: /^[A-Z]{3}\d{3}$/ } }).toSQL();
// PostgreSQL: SELECT * FROM users WHERE code ~ '^[A-Z]{3}\d{3}$'
// MySQL: SELECT * FROM users WHERE code REGEXP '^[A-Z]{3}\d{3}$'
```

### JSON field queries

All three builders support dotted JSON paths; each dialect uses its native JSON functions:

```javascript
users.find({ "profile.country": "France" }).toSQL();
// PostgreSQL: WHERE (profile::jsonb #>> '{country}') = 'France'
// MySQL:      WHERE JSON_UNQUOTE(JSON_EXTRACT(profile, '$.country')) = 'France'
// SQLite:     WHERE json_extract(profile, '$.country') = 'France'

users.find({ "profile.address.city": "Paris" }).toSQL(); // deep paths
users.find({ "orders.0.status": "completed" }).toSQL();  // array index access
users.find({ "profile.score": { $gte: 80 } }).toSQL();   // with operators
// PostgreSQL: WHERE ((profile::jsonb #>> '{score}'))::numeric >= 80

users.find({
    "profile.country": "USA",
    "profile.age": { $gte: 18 },
}).toSQL();
```

---

## CRUD operations

### Find operations

```javascript
// Projection (object or array)
users.find({ active: true }, { name: 1, email: 1 }).toSQL();
// SELECT name, email FROM users WHERE active = TRUE
users.find({ active: true }, ["name", "email"]).toSQL();

// Chainable query builder
users
    .find({ age: { $gte: 18 } })
    .select({ name: 1, email: 1 })
    .sort({ age: -1, name: 1 })
    .skip(10)
    .limit(5)
    .toSQL();
// SELECT name, email FROM users WHERE age >= 18
// ORDER BY age DESC, name ASC LIMIT 5 OFFSET 10

// Distinct
users.find({ country: "USA" }).select(["state"]).distinct().toSQL();
// SELECT DISTINCT state FROM users WHERE country = 'USA'

// Find one / count
users.findOne({ email: "alice@example.com" }).toSQL();
// SELECT * FROM users WHERE email = 'alice@example.com' LIMIT 1

users.countDocuments({ age: { $gte: 18 } });
// SELECT COUNT(*) AS count FROM users WHERE age >= 18

// Distinct values
users.distinct("city", { country: "USA" });
// SELECT DISTINCT city FROM users WHERE country = 'USA'
```

### Insert operations

```javascript
users.insertOne({ name: "Alice", age: 25, email: "alice@example.com" });
// INSERT INTO users (name, age, email) VALUES ('Alice', 25, 'alice@example.com')

// Nested JSON is serialized per dialect (JSONB on PG, JSON string elsewhere)
users.insertOne({ name: "Bob", profile: { country: "USA", score: 95 } });

// Missing fields become NULL
users.insertMany([
    { name: "Frank", age: 30 },
    { name: "Grace", email: "grace@example.com" },
]);
// INSERT INTO users (name, age, email) VALUES
// ('Frank', 30, NULL), ('Grace', NULL, 'grace@example.com')

// RETURNING (PostgreSQL)
users.insertOne({ name: "Ivan" }, { returning: ["id", "name"] });
// INSERT INTO users (name) VALUES ('Ivan') RETURNING id, name
users.insertMany([{ name: "Jack" }], { returning: "*" });
```

### Update operations

```javascript
users.updateOne({ email: "alice@example.com" }, { $set: { status: "verified" } });
// UPDATE users SET status = 'verified' WHERE email = 'alice@example.com' LIMIT 1

users.updateMany({ age: { $lt: 18 } }, { $set: { role: "minor" } });

// Arithmetic / bounds
users.updateOne({ id: 1 }, { $inc: { loginCount: 1, points: 10 } });
// UPDATE users SET loginCount = loginCount + 1, points = points + 10 WHERE id = 1 LIMIT 1
users.updateOne({ id: 1 }, { $mul: { score: 1.1 } });
users.updateMany({}, { $min: { price: 9.99 } });   // LEAST/MIN
users.updateMany({}, { $max: { discount: 50 } });  // GREATEST/MAX

// Null out or rename fields
users.updateOne({ id: 1 }, { $unset: { tempToken: "" } });
users.updateMany({}, { $rename: { oldField: "newField" } });

// Current timestamp per dialect
users.updateOne({ id: 1 }, { $currentDate: { updatedAt: true } });
// PG: CURRENT_TIMESTAMP  |  MySQL: NOW()  |  SQLite: datetime('now')

// Combined operators
users.updateOne(
    { id: 1 },
    {
        $set: { status: "active" },
        $inc: { loginCount: 1 },
        $unset: { resetToken: "" },
        $currentDate: { updatedAt: true },
    }
);

// JSON path updates (Full & Lite)
users.updateOne({ id: 1 }, { $set: { "profile.score": 95 } });
// PG: jsonb_set  |  MySQL: JSON_SET  |  SQLite: json_set
users.updateOne({ id: 1 }, { $inc: { "stats.views": 1 } });
users.updateOne({ id: 1 }, { $unset: { "profile.tempField": "" } });

// RETURNING (PostgreSQL)
users.updateMany({ city: "Paris" }, { $inc: { points: 10 } }, { returning: ["id", "points"] });
```

### Delete operations

```javascript
users.deleteOne({ email: "old@example.com" });
// DELETE FROM users WHERE email = 'old@example.com' LIMIT 1

users.deleteMany({ active: false });

users.deleteMany({ status: { $in: ["banned", "deleted"] } });

// Deleting everything is guarded behind an explicit option
users.deleteMany({}); // ❌ throws: requires a filter or allowDeleteAll
users.deleteMany({}, { allowDeleteAll: true });
// DELETE FROM users

// RETURNING (PostgreSQL)
users.deleteMany({ age: { $lt: 13 } }, { returning: ["id", "name"] });
```

---

## Aggregation pipeline

```javascript
const orders = qb.collection("orders", "pg");

// Group by and count (aggregate returns the SQL string directly)
orders.aggregate([{ $group: { _id: "$status", count: { $sum: 1 } } }]);
// SELECT status AS _id, COUNT(*) AS count FROM (SELECT * FROM orders) AS t1 GROUP BY status// Match before group (WHERE)
orders.aggregate([
    { $match: { status: "completed" } },
    { $group: { _id: "$customerId", totalSpent: { $sum: "$amount" } } },
]);

// Match after group (HAVING)
orders.aggregate([
    { $group: { _id: "$customerId", totalSpent: { $sum: "$amount" } } },
    { $match: { totalSpent: { $gte: 1000 } } },
]);

// Project with expressions
orders.aggregate([
    {
        $project: {
            orderId: "$id",
            total: { $multiply: ["$quantity", "$price"] },
            discount: { $divide: ["$amount", 10] },
        },
    },
]);

// Sort / skip / limit
orders.aggregate([
    { $group: { _id: "$customerId", totalSpent: { $sum: "$amount" } } },
    { $sort: { totalSpent: -1 } },
    { $limit: 10 },
]);

// Count stage
orders.aggregate([
    { $match: { status: "completed", amount: { $gte: 100 } } },
    { $count: "expensiveOrders" },
]);

// JSON field aggregation (Full/Lite)
users.aggregate([
    { $group: { _id: "$profile.country", avgScore: { $avg: "$profile.score" } } },
    { $sort: { avgScore: -1 } },
]);
```

Full stages: `$match`, `$project`, `$addFields`/`$set`, `$group`, `$sort`, `$limit`, `$skip`, `$count`,
`$sample`, `$sortByCount`, `$bucket` (and `$unwind` on the memory engine). Lite/Tiny include the seven
basic stages.

## Operator reference

### Filter operators

| Operator | Description | Example | SQL output |
| --- | --- | --- | --- |
| `$eq` | Equals | `{ age: { $eq: 25 } }` | `age = 25` |
| `$ne` | Not equals | `{ status: { $ne: 'inactive' } }` | `status != 'inactive'` |
| `$gt` / `$gte` | Greater than (or equal) | `{ age: { $gte: 18 } }` | `age >= 18` |
| `$lt` / `$lte` | Less than (or equal) | `{ age: { $lt: 65 } }` | `age < 65` |
| `$in` | In array | `{ status: { $in: ['a','b'] } }` | `status IN ('a','b')` |
| `$nin` | Not in array | `{ role: { $nin: ['admin'] } }` | `role NOT IN ('admin')` |
| `$like` | Pattern match | `{ email: { $like: '%@gmail.com' } }` | `email LIKE '%@gmail.com'` |
| `$ilike` | Case-insensitive like | `{ name: { $ilike: 'alice' } }` | `name ILIKE 'alice'` (PG) |
| `$nlike` | Not like | `{ email: { $nlike: '%temp%' } }` | `email NOT LIKE '%temp%'` |
| `$nilike` | Not ilike | `{ name: { $nilike: 'admin%' } }` | `name NOT ILIKE 'admin%'` |
| `$regex` | Regex match | `{ code: { $regex: /^[A-Z]+$/ } }` | `code ~ '...'` (PG) / `REGEXP` (MySQL) |
| `$exists` | Field exists | `{ phone: { $exists: true } }` | `phone IS NOT NULL` |
| `$between` | Range check | `{ age: { $between: [18, 65] } }` | `age BETWEEN 18 AND 65` |
| `$mod` | Modulo match | `{ id: { $mod: [10, 0] } }` | `(id % 10) = 0` |
| `$and` / `$or` / `$not` / `$nor` | Logical combinators | `{ $or: [...] }` | parenthesized SQL |
| `$expr` | Embedded expression | `{ $expr: { $gt: ['$price', '$cost'] } }` | `(price > cost)` |

Memory-only extras: `$type`, `$elemMatch`, `$all`, `$size`.

### Update operators

| Operator | Description | Example | SQL output |
| --- | --- | --- | --- |
| `$set` | Set field value | `{ $set: { status: 'active' } }` | `status = 'active'` |
| `$inc` | Increment | `{ $inc: { views: 1 } }` | `views = views + 1` |
| `$mul` | Multiply | `{ $mul: { price: 1.1 } }` | `price = price * 1.1` |
| `$min` | Set to minimum | `{ $min: { lowScore: 50 } }` | `LEAST(lowScore, 50)` |
| `$max` | Set to maximum | `{ $max: { highScore: 100 } }` | `GREATEST(highScore, 100)` |
| `$unset` | Remove field (NULL) | `{ $unset: { tempField: '' } }` | `tempField = NULL` |
| `$rename` | Rename field | `{ $rename: { old: 'new' } }` | `new = old, old = NULL` |
| `$currentDate` | Current date | `{ $currentDate: { updatedAt: true } }` | `CURRENT_TIMESTAMP` / `NOW()` / `datetime('now')` |

JSON paths work inside every operator above (`$set`/`$inc`/`$unset` …), e.g. `{ $inc: { 'stats.views': 1 } }`.

### Expression operators

Use inside `$project`, `$addFields`, `$group`, or standalone via `qb.expression(...)`:

```javascript
qb.expression({ $add: ["$price", 10] }, "pg"); // (price + 10)
qb.expression({ $concat: ["$firstName", " ", "$lastName"] }, "pg");
qb.expression({ $cond: [{ $gte: ["$age", 18] }, "adult", "minor"] }, "pg");
// CASE WHEN (age >= 18) THEN 'adult' ELSE 'minor' END
qb.expression({ $switch: { branches: [...], default: "Unknown" } }, "pg");
```

Available (Full):

- **Arithmetic**: `$add`, `$subtract`, `$multiply`, `$divide`, `$mod`, `$abs`, `$ceil`, `$floor`, `$round`, `$pow`, `$sqrt`
- **String**: `$concat`, `$upper`, `$lower`, `$substr`, `$trim`, `$ltrim`, `$rtrim`, `$strLen`, `$replace`
- **Aggregates**: `$sum`, `$avg`, `$min`, `$max`, `$count`, `$stdDevPop`, `$stdDevSamp`
- **Comparison**: `$eq`, `$ne`, `$gt`, `$gte`, `$lt`, `$lte`, `$cmp`, `$in`, `$nin`, `$size`
- **Logic/conditional**: `$and`, `$or`, `$not`, `$cond`, `$ifNull`, `$switch`, `$exists`
- **Date parts**: `$year`, `$month`, `$dayOfMonth`, `$dayOfWeek`, `$hour`, `$minute`, `$second`, `$week`
- **Casts**: `$toString`, `$toInt`, `$toDouble`, `$toBool`, `$toDate`
- **Misc**: `$literal`

## Custom operators (extend)

Every builder exposes `extend` to register operators at runtime — no fork required:

```javascript
import { escape } from "umosql";

qb.extend.filter({
    // BETWEEN a AND b
    $between: (value, db, field) =>
        `${field} BETWEEN ${escape(value[0], db)} AND ${escape(value[1], db)}`,
});

qb.extend.expression({
    $power: (args, ctx) => `POWER(${ctx.expr(args[0])}, ${ctx.expr(args[1])})`,
});

qb.extend.update({
    // PostgreSQL array push
    $push: (fields, db) =>
        Object.entries(fields).map(
            ([key, val]) => `${key} = array_append(${key}, ${escape(val, db)})`
        ),
});

// Usage
users.find({ age: { $between: [18, 65] } }).toSQL();
```

The memory engine has its own `extend` with the same shape:
`umosql/memory` → `extend.filter / extend.expression / extend.update / extend.stage`.

## Custom builds (ultra-minimal)

Build a builder with only the operators you need — unused operators are tree-shaken from the bundle:

```javascript
import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers } from "umosql";

const custom = createQueryBuilder({
    filterOps: { $eq: filterOps.$eq, $in: filterOps.$in },
    exprOps: { $add: exprOps.$add, $upper: exprOps.$upper },
    updateOps: { $set: updateOps.$set },
    stageHandlers: {}, // no aggregation
});

export const { collection, filter, expression } = custom;
```

## Schemaless adapters

The `/schemaless` and `/client` entries wrap a driver in a Mongo-like collection API that also
**manages the SQL schema for you**: tables, columns and JSON columns are inferred from your documents,
created lazily, and widened when types grow. The adapter builds and executes the SQL itself via an
internal query builder (wired with the full operator set).

### Unified client (memory / sqlite / pg / mysql / mongodb / sql)

```javascript
import { createSchemalessClient } from "umosql/client";

const client = await createSchemalessClient("pg", {
    host: "localhost",
    database: "mydb",
    user: "postgres",
    password: "password",
});

const users = client.db("mydb").collection("users");

await users.insertOne({ name: "Alice", age: 25 });
await users.updateOne({ name: "Alice" }, { $inc: { loginCount: 1 } });
await users.deleteMany({ active: false });

// find() is async and returns a chainable cursor
const adults = await (await users.find({ age: { $gte: 18 } })).sort({ age: -1 }).limit(10).toArray();

// Aggregation works on every backend
const stats = await users.aggregate([{ $group: { _id: "$city", count: { $sum: 1 } } }]);

client.raw; // underlying driver handle: pg.Client, mysql2 connection, MongoClient, ...
await client.close();
```

`type` accepts `memory` (default for `createSchemalessAdapter`, zero deps), `sqlite`
(default for `createSchemalessClient`), `pg`, `mysql`, `mongodb`, or `sql` — any backend driven by
your own `executor(sql, params)`.

#### Bring your own driver

| Backend | Install yourself | Or bring your own |
| --- | --- | --- |
| `memory` | nothing | — |
| `sql` | nothing (any driver) | `executor(sql, params)` — Neon, Turso, PlanetScale, Hyperdrive, D1, ... |
| `sqlite` | `npm i better-sqlite3` | `{ client: db }` or `{ driver: { default: Database } }` |
| `pg` | `npm i pg` | `{ client: pgClient }` (adopted as-is, never re-connected) or `{ driver }` |
| `mysql` | `npm i mysql2` | `{ conn }` / `{ client }` or `{ driver }` |
| `mongodb` | `npm i mongodb` | `{ client: mongoClient }` or `{ driver }` |

```javascript
// umosql never resolves the package itself:
import { Client } from "pg";
const pgClient = new Client({ connectionString: process.env.DATABASE_URL });
await pgClient.connect();

const client = await createSchemalessClient("pg", { client: pgClient });
```

If a driver is missing you get an actionable error instead of a module-resolution stack:

```
umosql: the "pg" backend needs the "pg" driver, which is not installed.
  -> install it yourself: `npm i pg`
  -> or bring your own: createSchemalessClient("pg", { client }) / { driver }
umosql never bundles drivers.
```

### MongoDB backend

Same collection API, backed by a real MongoDB server:

```javascript
const client = await createSchemalessClient("mongodb", {
    host: "localhost",
    port: 27017,
    database: "test",
});
const users = client.db("test").collection("users");
await users.insertOne({ name: "Alice" });
const rows = await (await users.find({ name: "Alice" })).toArray();
await client.close();

// Or use the raw adapter factory:
import { createMongoSchemaless } from "umosql/client";
const { adapter, client: mongoClient } = await createMongoSchemaless({ host: "localhost", database: "test" });
```

### Auto DDL, schema inference and migration

- On the first operation, the adapter reads `information_schema` / `PRAGMA` and, if the table is
  missing, creates it (`CREATE TABLE IF NOT EXISTS`) with:
  - an id column (`_id SERIAL PRIMARY KEY` on PG, `INT AUTO_INCREMENT` on MySQL, `INTEGER ... AUTOINCREMENT` on SQLite)
  - `created_at` / `updated_at` timestamp columns (server-side defaults per dialect)
- Insert/update documents **add missing columns** automatically (`ALTER TABLE ... ADD COLUMN`),
  inferring types from values: `INTEGER`/`INT`, `DECIMAL(20,6)`, `VARCHAR(255)`→`TEXT`,
  `BOOLEAN`/`TINYINT(1)`, `TIMESTAMP`, `JSONB`/`JSON`/`TEXT` for objects and arrays.
- Types are widened when a value no longer fits (e.g. `INTEGER` → `DECIMAL`), never narrowed.
- Failed statements are retried once after idempotent DDL, so the whole flow is race-safe.
- Updates can also introduce new columns via `$set`; opt out per collection with
  `collection(name, { migrateOnUpdate: false })`.

```javascript
import { createSchemalessAdapter } from "umosql/schemaless";
import Database from "better-sqlite3";

// SQLite
const { adapter } = createSchemalessAdapter(new Database(":memory:"), "sqlite");

// PostgreSQL
import pg from "pg";
const pgClient = new pg.Client({ host: "localhost", database: "mydb" });
await pgClient.connect();
const { adapter: pgAdapter } = createSchemalessAdapter(pgClient, "pg");

// MySQL
import mysql from "mysql2/promise";
const conn = await mysql.createConnection({ host: "localhost", database: "mydb" });
const { adapter: myAdapter } = createSchemalessAdapter(conn, "mysql");
```

### Schemas, defaults, hidden fields

Pass a partial schema to `collection()` to pin column types, `NOT NULL`, `UNIQUE`, defaults
(static or factory), or to keep fields out of every returned document:

```javascript
const users = adapter.collection("users", {
    schema: {
        email: { type: "VARCHAR(320)", required: true, unique: true },
        role: { type: "VARCHAR(32)", default: "member" },
        createdAt: { type: "TIMESTAMP", default: () => new Date() },
        passwordHash: { hidden: true }, // stripped from every find*() result
    },
});
```

### Auto ID creation

| Strategy | Behavior |
| --- | --- |
| `auto` (default) | SQL autoincrement: `_id SERIAL` (PG) / `AUTO_INCREMENT` (MySQL) / `AUTOINCREMENT` (SQLite); numeric counter in memory |
| `mongo` | 24-character hex ObjectId-like string generated on insert |
| `custom` | your `idGenerator()` is called when `_id` is absent (passing `idGenerator` alone implies `custom`) |

```javascript
const db = client.db("test_database", { idColumn: "_id", idStrategy: "mongo" });
const users = db.collection("users");
await users.insertOne({ name: "Alice" }); // _id = 24-char hex
```

### DDL helpers

SQL adapters expose administrative helpers (SQL-only; memory is CRUD/aggregate only):

```javascript
await adapter.listCollections(); // string[]
await adapter.dropCollection("users");
await adapter.createTableWithSchema("posts", { title: "TEXT", views: "INTEGER" });
await adapter.getTableSchema("posts"); // { columns: { title: 'TEXT', ... } } — raw dialect names
await adapter.addColumn("posts", "slug", "TEXT", { unique: true });
await adapter.renameColumn("posts", "slug", "path");
await adapter.modifyColumn("posts", "views", "BIGINT"); // pg/mysql
await adapter.createIndex("posts", "title", { unique: true });
await adapter.dropIndex("posts", "posts_title_idx");
await users.dropColumn("views"); // pg/mysql only (SQLite cannot DROP COLUMN)
```

### findMany — SQL pagination helper

```javascript
const page = await users.findMany({
    filter: { active: true },
    sort: { createdAt: -1 },
    page: 2,
    pageSize: 20,
    includeTotal: true, // adds one COUNT(*) query
});
// { items: [...], total: 137, page: 2, pageSize: 20 }
```

### Method support matrix

| Method | Memory | SQLite | MySQL | PostgreSQL |
| --- | --- | --- | --- | --- |
| `insertOne` / `insertMany` | ✅ | ✅ | ✅ | ✅ |
| `find` (async cursor) | ✅ | ✅ | ✅ | ✅ |
| `findOne` | ✅ | ✅ | ✅ | ✅ |
| `findMany` (SQL-only helper) | ❌ | ✅ | ✅ | ✅ |
| `updateOne` / `updateMany` | ✅ | ✅ | ✅ | ✅ |
| `upsertOne` | ✅ | ✅ | ✅ | ✅ |
| `deleteOne` / `deleteMany` | ✅ | ✅ | ✅ | ✅ |
| `countDocuments` / `estimatedDocumentCount` | ✅ | ✅ | ✅ | ✅ |
| `distinct` | ✅ | ✅ | ✅ | ✅ |
| `aggregate` | ✅ | ✅ | ✅ | ✅ |
| `createIndex` / `dropIndex` | ✅ | ✅ | ✅ | ✅ |
| `dropColumn` | ❌ | ❌ | ✅ | ✅ |
| `listCollections` / `dropCollection` | ✅ | ✅ | ✅ | ✅ |
| `createTableWithSchema` / `getTableSchema` | ❌ | ✅ | ✅ | ✅ |
| `addColumn` / `renameColumn` | ❌ | ✅ | ✅ | ✅ |
| `modifyColumn` | ❌ | ❌ | ✅ | ✅ |

## Serverless (Neon, Turso, PlanetScale, Drizzle, Hyperdrive, ...)

Any HTTP SQL driver works through `createSQLAdapter({ database, execute, queryBuilder })` or
`createSchemalessClient('sql', { database, executor })` — umosql generates the SQL, you ship it to
the provider:

```javascript
import { createSchemalessClient } from "umosql/client";

const client = await createSchemalessClient("sql", {
    database: "pg",
    executor: async (sql, params) => {
        const res = await fetch(process.env.NEON_HTTP_URL, {
            method: "POST",
            headers: { Authorization: `Bearer ${process.env.NEON_API_KEY}`, "Content-Type": "application/json" },
            body: JSON.stringify({ sql, params }),
        });
        return res.json();
    },
});
```

Working examples in [`examples/`](examples/):

| Example | Provider | Env vars |
| --- | --- | --- |
| `examples/serverless-neon.js` | Neon (Postgres over HTTP) | `NEON_HTTP_URL` (or `DATABASE_URL`), `NEON_API_KEY` |
| `examples/serverless-turso.js` | Turso (SQLite over HTTP) | `TURSO_HTTP_URL` (or `DATABASE_URL`), `TURSO_TOKEN` (or `DATABASE_AUTH_TOKEN`) |
| `examples/serverless-planetscale.js` | PlanetScale Data API | `PSCALE_DATA_API_URL`, `PSCALE_TOKEN` |
| `examples/serverless-neon-drizzle.js` | Neon via `drizzle-orm/neon-http` | `NEON_HTTP_URL` |
| `examples/serverless-turso-drizzle.js` | Turso via `drizzle-orm/libsql/http` | `TURSO_HTTP_URL` |

Notes:

- Compose with the QueryBuilder (`qb.collection('users', 'pg' \| 'mysql' \| 'sqlite')`) and execute with your provider.
- DDL and JSON operators vary by provider; the schemaless adapter's auto-DDL works wherever the
  provider executes plain SQL.

## Express / REST integration

The builder is a pure function from Mongo queries to SQL, so it maps cleanly onto request handling.
Minimal pattern (adapt to your framework):

```javascript
import express from "express";
import { pool } from "./db.js"; // your pg Pool
import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers } from "umosql";

const qb = createQueryBuilder({ filterOps, exprOps, updateOps, stageHandlers });
const app = express();
app.use(express.json());

// GET /users?q={"age":{"$gte":18}}&sort={"age":-1}&limit=10&skip=0
app.get("/:collection", async (req, res) => {
    try {
        const coll = qb.collection(req.params.collection, "pg");
        const query = coll
            .find(req.query.q ? JSON.parse(req.query.q) : {})
            .sort(req.query.sort ? JSON.parse(req.query.sort) : {})
            .skip(parseInt(req.query.skip) || 0)
            .limit(parseInt(req.query.limit) || 100);
        const sql = query.toSQL();
        const result = await pool.query(sql);
        res.json({ data: result.rows, count: result.rows.length });
    } catch (err) {
        res.status(400).json({ error: "Invalid query", details: err.message });
    }
});

// POST /users — insertOne with RETURNING
app.post("/:collection", async (req, res) => {
    const sql = qb.collection(req.params.collection, "pg").insertOne(req.body, { returning: "*" });
    const result = await pool.query(sql);
    res.status(201).json(result.rows[0] ?? null);
});

app.listen(3000);
```

The same pattern works with the schemaless adapter — then table/column creation and JSON columns
are handled for you, and `findMany` gives you pagination with totals out of the box.

## In-memory engine

`umosql/memory` is a standalone MongoDB-style engine — no SQL, no driver, sync API, great for tests
and edge runtimes. It supports the full operator set plus memory-only features (`$type`, `$elemMatch`,
`$all`, `$size`, `$unwind`):

```javascript
import { collection as memCollection } from "umosql/memory";

const users = memCollection("users", [
    { name: "Alice", age: 25, city: "Paris", profile: { score: 85 } },
    { name: "Bob", age: 30, city: "London", profile: { score: 90 } },
]);

users.find({ city: "Paris" }).toArray(); // sync
users.aggregate([
    { $group: { _id: "$city", avgAge: { $avg: "$age" } } },
    { $sort: { avgAge: 1 } },
]);
users.updateOne({ name: "Alice" }, { $inc: { "profile.score": 5 } });
```

For an async, schemaless-flavored in-memory store with the full adapter API, use the memory backend
of the unified client (each database name gets an isolated store):

```javascript
const client = await createSchemalessClient("memory");
const users = client.db("app", { idStrategy: "mongo" }).collection("users");
```

## Operator support matrix

### Filter operators

| Operator | Memory | SQLite | MySQL | PostgreSQL | Notes |
| --- | --- | --- | --- | --- | --- |
| `$eq` | ✅ | ✅ | ✅ | ✅ | `=` |
| `$ne` | ✅ | ✅ | ✅ | ✅ | `!=` / `<>` |
| `$gt` / `$gte` / `$lt` / `$lte` | ✅ | ✅ | ✅ | ✅ | comparison |
| `$in` / `$nin` | ✅ | ✅ | ✅ | ✅ | empty arrays handled |
| `$like` / `$nlike` | ✅ | ✅ | ✅ | ✅ | `%` `_` patterns |
| `$ilike` / `$nilike` | ✅ | ✅ | ✅ | ✅ | PG: `ILIKE`; others: `LOWER(field) LIKE LOWER(value)` |
| `$regex` | ✅ | ⚠️ | ✅ | ✅ | PG: `~`; MySQL: `REGEXP`; SQLite: needs `REGEXP` UDF |
| `$exists` | ✅ | ✅ | ✅ | ✅ | `IS NULL` / `IS NOT NULL` |
| `$between` / `$mod` | ✅ | ✅ | ✅ | ✅ | `BETWEEN`, `%` |
| `$and` / `$or` / `$not` / `$nor` | ✅ | ✅ | ✅ | ✅ | parenthesized conjunctions/disjunctions |
| `$expr` | ✅ | ✅ | ✅ | ✅ | embed expression in filter |
| `$type` / `$elemMatch` / `$all` / `$size` | ✅ | — | — | — | memory-only |

### Expression operators

Full list in [Expression operators](#expression-operators). Highlights per dialect:

- `$concat`: PG/MySQL `CONCAT`, SQLite `||`
- `$min`/`$max` (multi-arg): `LEAST`/`GREATEST` (SQLite `MIN`/`MAX`)
- `$size` (JSON array length): PG `jsonb_array_length`, MySQL `JSON_LENGTH`, SQLite `json_array_length`
- Date parts: `EXTRACT` (PG), `YEAR()`/etc. (MySQL), `strftime` (SQLite)
- Casts: DB-specific `CAST`

### Update operators

| Operator | Memory | SQLite | MySQL | PostgreSQL | Notes |
| --- | --- | --- | --- | --- | --- |
| `$set` / `$inc` / `$mul` | ✅ | ✅ | ✅ | ✅ | scalar and JSON path updates |
| `$min` / `$max` | ✅ | ✅ | ✅ | ✅ | SQLite `MIN`/`MAX`, others `LEAST`/`GREATEST` |
| `$unset` | ✅ | ✅ | ✅ | ✅ | JSON path remove or `NULL` for scalars |
| `$currentDate` | ✅ | ✅ | ✅ | ✅ | `CURRENT_TIMESTAMP` / `NOW()` / `datetime('now')` |
| `$rename` | ✅ | ✅ | ✅ | ✅ | non-JSON fields; sets new = old, old = NULL |
| `$push` / `$pull` / `$addToSet` | ✅ | — | — | — | memory-only array mutations (or via `extend.update`) |

### Aggregation stages

| Stage | Memory | SQLite | MySQL | PostgreSQL | Notes |
| --- | --- | --- | --- | --- | --- |
| `$match` | ✅ | ✅ | ✅ | ✅ | WHERE/HAVING integration |
| `$project` | ✅ | ✅ | ✅ | ✅ | SELECT with expressions |
| `$addFields` / `$set` | ✅ | ✅ | ✅ | ✅ | adds computed fields |
| `$group` | ✅ | ✅ | ✅ | ✅ | GROUP BY with aggregates |
| `$sort` / `$limit` / `$skip` | ✅ | ✅ | ✅ | ✅ | ORDER BY / LIMIT / OFFSET |
| `$count` | ✅ | ✅ | ✅ | ✅ | aggregate count |
| `$sample` | ✅ | ✅ | ✅ | ✅ | random ordering + LIMIT |
| `$sortByCount` | ✅ | ✅ | ✅ | ✅ | GROUP BY expr, order by count desc |
| `$bucket` | ✅ | ✅ | ✅ | ✅ | CASE-based bucketing |
| `$unwind` | ✅ | — | — | — | memory-only |

## Compatibility and caveats

- **JSON storage**: PostgreSQL `JSONB` with `jsonb_set`/`#>>`; MySQL `JSON` with `JSON_EXTRACT`/`JSON_SET`; SQLite `TEXT` with `json_extract`.
- **Booleans**: PostgreSQL `TRUE/FALSE`; MySQL/SQLite use `TINYINT(1)`/`INTEGER` (1/0).
- **Column management**: SQLite cannot `DROP COLUMN` or `MODIFY COLUMN` (rename or recreate instead).
- **Upserts**: SQL `upsertOne` is update-then-insert and not atomic — add unique constraints.
- **Indexes**: PostgreSQL supports index types (`USING BTREE`, ...); the adapter uses sensible defaults elsewhere.
- **Pagination totals**: `findMany({ includeTotal: true })` performs one extra `COUNT(*)`.
- **Transactions**: adapters don't expose transaction helpers — use `client.raw` directly.
- **Identifiers**: table/column names are used unquoted; Postgres folds unquoted identifiers to
  lowercase (so `getTableSchema` may return lowercase column names for camelCase fields).
- **`$regex` on SQLite**: requires a `REGEXP` UDF (or use `$like`).

## Testing

```bash
npm test          # full suite — runs offline: memory + SQLite + PGlite (WASM Postgres)
npm run test:db   # only the db-backed specs
npm run coverage  # c8 coverage over the full suite
```

The suite executes **787 assertions across every backend** and needs no external services:

- **PostgreSQL** → `@electric-sql/pglite` (in-memory WASM Postgres), or a real server via `PG_HOST`/`PG_PORT`/`PG_USER`/`PG_PASSWORD`/`PG_DB`
- **MySQL** → `mysql-memory-server` (embedded real `mysqld`, no Docker), or `MYSQL_HOST`/`MYSQL_PORT`/`MYSQL_USER`/`MYSQL_PASS`/`MYSQL_DB`
- **SQLite** → `better-sqlite3` `:memory:`

```bash
docker compose up -d   # optional: real PG + MySQL for CI parity (npm run db:up)
```

## API reference

### Entry points

| Module | Key exports |
| --- | --- |
| `umosql` | `createQueryBuilder`, `filterOps`, `exprOps`, `updateOps`, `stageHandlers`, `collection`, `filter`, `expression`, `aggregate`, `insertMany`, `updateMany`, `deleteMany`, `db`, `FindQuery`, `extend`, `escape`, `jsonPath`, `validate`, `is$`, `isObject` |
| `umosql/lite` | default builder + `collection`, `filter`, `expression`, `aggregate`, `extend`, `db` |
| `umosql/tiny` | default builder + `collection`, `filter`, `expression`, `aggregate`, `extend`, `db` |
| `umosql/schemaless` | `createSchemalessAdapter`, `createSQLAdapter`, `createMemorySchemaless` |
| `umosql/memory` | `collection`, `db`, `Database`, `Collection`, `FindQuery`, `filter`, `expression`, `aggregate`, `project`, `extend`, `createMemoryDB`, `createMemorySchemaless`, `filterOps`, `exprOps`, `updateOps`, `stageOps`, `deepEquals`, `getPath`, `setPath`, `deletePath`, `clone` |
| `umosql/client` | `createSchemalessClient`, `CLIENT_TYPES`, `loadDriver`, `createSchemalessAdapter`, `createSQLAdapter`, `createMemorySchemaless`, `createMongoSchemaless` |

### createSchemalessClient(type?, config?)

- `type`: `memory` \| `sqlite` (default) \| `pg` \| `mysql` \| `mongodb` \| `sql`.
- `config`:
  - `client` / `conn` — reuse an existing driver instance; the driver package is then **never imported**
    (a `pg` client you pass in is not re-connected — pg throws on a second `connect()`).
  - `driver` — inject the driver module (e.g. `await import('pg')`, or a stub in tests) so umosql
    constructs the connection but never resolves the package.
  - `driverOptions` — forwarded to the driver constructor / connection factory.
  - `filename` — sqlite file (default `:memory:`).
  - `host`, `port`, `user`, `password`, `database` — pg / mysql / mongodb.
  - `connectionString` \| `url` (+ `ssl`) — pg connection string (Neon, Supabase, ...).
  - `uri` — mysql connection URI.
  - `executor(sql, params)` — required for `type: 'sql'`; return driver rows (`{ rows }`, mysql2's
    `[rows, fields]`, or a plain array). Optional `close()` for teardown.
  - `debug` — log every generated statement.
  - `id` \| `idColumn`, `idStrategy`, `idGenerator` — defaults for collections created via `db()`.
- Returns `{ type, raw, adapter, db(name?, options?), close() }`:
  - `raw` — the underlying driver instance (`null` for `type: 'sql'` unless you pass `client`).
  - `db(name, options?)` → `{ name, collection(name, opts?), adapter }`; for `memory` each name gets an
    isolated store.
  - `close()` — releases the driver; safe to call twice.

### createSchemalessAdapter(client?, database?, options?)

- `client` — driver instance; omit for memory.
- `database` — `memory` (default) \| `sqlite` \| `pg` \| `mysql`.
- `options` — `{ debug?: boolean }`.
- Returns `{ adapter, client? }` (memory returns `{ adapter, database }`).
- Use `createSchemalessClient` when you want the driver created/closed for you, or `mongodb` support.

### createSQLAdapter({ database, execute, queryBuilder, debug? })

- `database` — `'pg'` \| `'mysql'` \| `'sqlite'` (controls SQL dialect).
- `execute(sql, params)` — your serverless/HTTP transport; return driver rows.
- `queryBuilder` — pass `createQueryBuilder({ filterOps, exprOps, updateOps, stageHandlers })`.

### Adapter and Collection

Adapter: `collection(name, options?)`, `listCollections()`, `dropCollection(name)`,
`tableExists(name)`, `getTableSchema(name)`, `createTableWithSchema(name, schema)`, `addColumn()`,
`renameColumn()`, `modifyColumn()` (pg/mysql), `createIndex()`, `dropIndex()`, `dropColumn()` (pg/mysql),
plus `buildInsert/buildFind/buildUpdateOne/...` for custom pipelines.

Collection options: `{ schema?, migrateOnUpdate?, idColumn?, idStrategy?, idGenerator? }`.

Collection methods: `insertOne`, `insertMany`, `find` (await → cursor with `sort`/`skip`/`limit`/
`distinct`/`toArray`/`count`), `findOne`, `findMany` (SQL), `updateOne`, `updateMany`, `upsertOne`,
`deleteOne`, `deleteMany`, `countDocuments`, `estimatedDocumentCount`, `distinct`, `aggregate`,
`createIndex`, `dropColumn` (pg/mysql).

Write results follow Mongo conventions:
`{ acknowledged, insertedId?, insertedIds?, matchedCount?, modifiedCount?, deletedCount?, upserted?, upsertedId? }`.

## Examples directory

| File | What it shows |
| --- | --- |
| `examples/client.js` | Runnable tour of every backend (memory → sql → sqlite → pg/mysql/mongo when configured) |
| `examples/memory.js` | In-memory engine: filters, aggregation, nested updates |
| `examples/lite.js` / `examples/tiny.js` | Prebuilt lite/tiny builders |
| `examples/pg.js`, `examples/mysql.js`, `examples/sqlite.js` | Schemaless adapters on each SQL backend |
| `examples/mongo.js` | MongoDB backend |
| `examples/serverless-*.js` | Neon, Turso, PlanetScale, Drizzle (see [Serverless](#serverless-neon-turso-planetscale-drizzle-hyperdrive-)) |

Run any of them directly, e.g. `node examples/client.js`.

## License

[ISC](LICENSE) © Kethan Surana

---

Inspired by MongoDB's query syntax; built for the SQL world. Similar projects:
[MongoDB](https://www.mongodb.com/) (the inspiration), [Mongoose](https://mongoosejs.com/)
(object modeling), [Knex.js](http://knexjs.org/) (SQL query builder).

