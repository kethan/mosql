## UMoSQL

# Mongo To SQL Query

[![tests](https://github.com/kethan/mosql/actions/workflows/node.js.yml/badge.svg)](https://github.com/kethan/mosql/actions/workflows/node.js.yml) [![Version](https://img.shields.io/npm/v/umosql.svg?color=success&style=flat-square)](https://www.npmjs.com/package/umosql) [![Badge size](https://deno.bundlejs.com/badge?q=umosql&treeshake=[*]&config={"compression":"brotli"})](https://unpkg.com/umosql)

[![Version](https://img.shields.io/npm/v/umosql.svg?color=success&style=flat-square)](https://www.npmjs.com/package/umosql) [![Badge size](https://deno.bundlejs.com/badge?q=umosql/lite&treeshake=[*]&config={"compression":"brotli"})](https://unpkg.com/umosql/lite)

[![Version](https://img.shields.io/npm/v/umosql.svg?color=success&style=flat-square)](https://www.npmjs.com/package/umosql) [![Badge size](https://deno.bundlejs.com/badge?q=umosql/tiny&treeshake=[*]&config={"compression":"brotli"})](https://unpkg.com/umosql/tiny)

---

## Install

```bash
npm i umosql
```

Every entry point ships in that one package — import the ones you need:

| Entry point    | Import                                                     | What you get                                                                  |
| -------------- | ---------------------------------------------------------- | ----------------------------------------------------------------------------- |
| **Full**       | `import { collection } from "umosql"`                      | MongoDB → SQL query builder (SQL strings, no driver)                          |
| **Lite**       | `import lite from "umosql/lite"`                           | Smaller builder: JSON paths + basic aggregation                               |
| **Tiny**       | `import tiny from "umosql/tiny"`                           | Smallest builder: basic filters/expressions, `$set` only (dotted paths work)  |
| **Schemaless** | `import { createSchemalessAdapter } from "umosql/schemaless"` | Executes queries: wraps a `better-sqlite3` / `pg` / `mysql2` handle        |
| **Memory**     | `import { collection } from "umosql/memory"`               | In-memory MongoDB-style engine (no SQL, no driver)                            |
| **Client**     | `import { createSchemalessClient } from "umosql/client"`   | One factory for `memory` / `sqlite` / `pg` / `mysql` / `mongodb` / any executor |

> **umosql has zero runtime dependencies.** No driver is bundled, and none is declared as a
> dependency or peer dependency — so nothing gets auto-installed into your project. You install
> the driver you want, and it is loaded lazily only when you request that backend.
> `dotenv` is optional in the same way: `.env` is read when it is installed, silently skipped when it is not.

## 📊 Feature Comparison

| Feature                  | Full               | Lite               | Tiny               |
| ------------------------ | ------------------ | ------------------ | ------------------ |
| **Size (gzip)**          | [![Full](https://deno.bundlejs.com/badge?q=umosql/lite&treeshake=[*]&config={"compression":"brotli"})](https://unpkg.com/umosql) | [![Lite](https://deno.bundlejs.com/badge?q=umosql/lite&treeshake=[*]&config={"compression":"brotli"})](https://unpkg.com/umosql/lite) | [![Tiny](https://deno.bundlejs.com/badge?q=umosql/tiny&treeshake=[*]&config={"compression":"brotli"})](https://unpkg.com/umosql/tiny) |
| **JSON Paths**           | ✅                 | ✅                 | ✅ *             |
| **JSON Updates**         | ✅ 8 ops           | ✅ 8 ops           | ✅ `$set` only * |
| **Aggregation**          | ✅ 12 stages       | ✅ 7 stages        | ✅ 7 stages      |
| **Filter Operators**     | ✅ All (16 + 5 structural) | ✅ All     | ✅ 10 + 5 structural |
| **Expression Operators** | ✅ All (58)        | ✅ 42              | ✅ 13            |
| **Update Operators**     | ✅ All (8)         | ✅ All (8)         | ✅ $set          |
| **Collection API**       | ✅                 | ✅                 | ✅                 |
| **FindQuery**            | ✅                 | ✅                 | ✅                 |
| **Extend/Add**           | ✅                 | ✅                 | ✅                 |
| **Multi-DB**             | ✅ PG/MySQL/SQLite | ✅ PG/MySQL/SQLite | ✅ PG/MySQL/SQLite |


## 🎯 When to Use Which Version?

### Use **FULL** when:

- You need JSON field support (`profile.score`)
- You need aggregation pipelines ($group, $match, etc.)
- Complex analytics queries
- MongoDB-to-SQL migration

* Tiny still resolves dotted paths (`profile.score`) in filters, `$set`, and sorting — path handling is core, not an operator. What tiny drops is operator breadth (no `$regex`/`$ilike`/`$between`/`$mod`, no `$inc`/`$mul`/…, 13 expression ops). See [Lite & Tiny operator lists](#-lite--tiny-operator-lists) for the exact sets.

### Use **LITE** when:

- You want every filter and update operator, but a trimmed expression set (no `$switch`, date parts, `$toBool`/`$toDate`, `$cmp`/`$size`/`$stdDev*`, `$literal`)
- Basic 7-stage aggregation (`$match`/`$project`/`$group`/`$sort`/`$limit`/`$skip`/`$count`) is enough — no `$addFields`/`$set`, `$sample`, `$sortByCount`, `$bucket`
- Medium complexity apps and REST APIs with JSON columns
- Balance between features and size

### Use **TINY** when:

- Simple CRUD operations with 10 filters, 13 expressions, `$set`, and the same basic 7 stages
- Dotted JSON paths are fine, but you don't need JSON-adjacent operators
- Smallest bundle size required
- Simple web apps or microservices

All three versions are production-ready! 🚀

# 📦 umosql

> **MongoDB-style queries for SQL databases and more**

Transform MongoDB queries into SQL (PostgreSQL, MySQL, SQLite) with a universal adapter system. Build REST APIs, serverless functions, or use in-memory storage with the same familiar MongoDB syntax.

---

## 🌟 Features

- ✅ **MongoDB-compatible query syntax**
- ✅ **SQL generation** for PostgreSQL, MySQL, SQLite
- ✅ **JSON field support** for nested objects (Full & Lite)
- ✅ **Aggregation pipelines** ($group, $match, $project, etc. in Full)
- ✅ **Schemaless adapters** for memory, MongoDB, SQLite, PostgreSQL, MySQL
- ✅ **Auto ID creation strategies** (`auto`, `mongo`, `custom`) with default `_id`
- ✅ **Serverless friendly** — works anywhere JavaScript runs
- ✅ **Three versions** — choose your feature set and bundle size

---

## 📦 Three Versions

| Version  | Size (gzip) | JSON Support | Aggregation | Use Case                        |
| -------- | ----------- | ------------ | ----------- | ------------------------------- |
| **Full** | ~6.61 kB    | ✅ Paths + all ops | ✅ 12 stages | Complete MongoDB compatibility |
| **Lite** | ~5.99 kB    | ✅ Paths + all updates | ✅ Basic (7 stages) | Full filters/updates, trimmed expressions |
| **Tiny** | ~4.95 kB    | ✅ Paths + `$set` | ✅ Basic (7 stages) | Minimal ops, smallest bundle   |

---

## 🚀 Quick Start

### Installation

```bash
npm i umosql
```

```javascript
import { collection } from "umosql";                         // query builder
import lite from "umosql/lite";                              // smaller builder
import tiny from "umosql/tiny";                              // smallest builder
import { createSchemalessAdapter } from "umosql/schemaless"; // adapters
import { collection as memCollection } from "umosql/memory"; // in-memory engine
import { createSchemalessClient } from "umosql/client";      // unified client
```

### Basic Usage

```javascript
import { collection } from "umosql";

const users = collection("users", "pg"); // 'pg', 'mysql', or 'sqlite'

// Generate SQL from MongoDB-style queries
users.find({ age: { $gte: 18 } }).toSQL();
// SELECT * FROM users WHERE age >= 18

users.updateOne(
    { email: "alice@example.com" },
    { $set: { status: "active" }, $inc: { loginCount: 1 } }
);
// UPDATE users SET status = 'active', loginCount = loginCount + 1
// WHERE email = 'alice@example.com'
// (MySQL/SQLite append `LIMIT 1`; PostgreSQL has no LIMIT on UPDATE)

users.insertMany([
	{ name: "Alice", age: 25 },
	{ name: "Bob", age: 30 },
]);
// INSERT INTO users (name, age) VALUES ('Alice', 25), ('Bob', 30)
```

### Small Examples

PostgreSQL (JSON and estimated count):

```javascript
import pkg from 'pg';
import { createSchemalessAdapter } from 'umosql/schemaless';
const client = new pkg.Client({ host, user, password, database });
await client.connect();
const { adapter } = createSchemalessAdapter(client, 'pg');
const users = adapter.collection('users');
await users.insertOne({ name: 'Alice', profile: { score: 85 } });
await users.updateOne({ name: 'Alice' }, { $inc: { 'profile.score': 5 } });
console.log(await users.estimatedDocumentCount());
await client.end();
```

MySQL (JSON update and count):

```javascript
import mysql from 'mysql2/promise';
import { createSchemalessAdapter } from 'umosql/schemaless';
const conn = await mysql.createConnection({ host, user, password, database });
const { adapter } = createSchemalessAdapter(conn, 'mysql');
const users = adapter.collection('users');
await users.insertOne({ name: 'Alice', profile: { score: 85 } });
await users.updateOne({ name: 'Alice' }, { $set: { 'profile.score': 90 } });
console.log(await users.estimatedDocumentCount());
await conn.end();
```

MongoDB (drop-in behavior):

```javascript
import { createSchemalessClient } from 'umosql/client';
const client = await createSchemalessClient('mongodb', { host, user, password, database });
const users = client.db(database).collection('users');
await users.insertOne({ name: 'Alice', profile: { score: 85 } });
await users.updateOne({ name: 'Alice' }, { $inc: { 'profile.score': 5 } });
console.log(await users.estimatedDocumentCount());
await client.close();
```

In-memory (no driver, great for tests and edge runtimes):

```javascript
import { createSchemalessClient } from 'umosql/client';
const client = await createSchemalessClient('memory');
const users = client.db('app').collection('users');
await users.insertOne({ name: 'Alice', profile: { score: 85 } });
console.log(await (await users.find({ 'profile.score': { $gte: 80 } })).toArray());
await client.close();
```

SQLite (in-memory):

```javascript
import Database from 'better-sqlite3';
import { createSchemalessAdapter } from 'umosql/schemaless';
const { adapter } = createSchemalessAdapter(new Database(':memory:'), 'sqlite');
const users = adapter.collection('users');
await users.insertOne({ name: 'Alice', profile: { score: 85 } });
await users.updateOne({ name: 'Alice' }, { $inc: { 'profile.score': 5 } });
console.log(await users.estimatedDocumentCount());
```

### Non‑Mongo Methods (SQL adapter helpers)

- `findMany({ filter, projection, sort, limit, skip, page, pageSize, includeTotal })`
  - Returns `{ items, total?, page?, pageSize? }`
  - Convenience pagination with optional total computation
- `createTableWithSchema(tableName, jsonSchema)`
  - Creates a table from JSON Schema properties and `required`
- `getTableSchema(tableName)`
  - Reads schema from information_schema / PRAGMA
- `addColumn(table, name, type, { required, unique, default })`
- `renameColumn(table, old, new)`
- `modifyColumn(table, name, newType, { required, unique, default })`
- `listCollections()`
- `dropColumn(table, name)`
- `dropIndex(table, indexName)`

APIs above mirror common SQL DDL. See:
- PostgreSQL: https://www.postgresql.org/docs/current/sql-commands.html
- MySQL: https://dev.mysql.com/doc/refman/8.0/en/sql-statements.html
- SQLite: https://sqlite.org/lang.html

## Serverless Examples

- Neon (Postgres over HTTP): `examples/serverless-neon.js`
- Turso (SQLite over HTTP): `examples/serverless-turso.js`

Env vars:
- Neon: `NEON_HTTP_URL`, `NEON_API_KEY`
- Turso: `TURSO_HTTP_URL`, `TURSO_TOKEN`

Notes:
- Uses `createSQLAdapter` with a custom `execute(sql, params)` that calls the provider’s HTTP API.
- Compose queries using the QueryBuilder: `qb.collection('users', 'pg'|'mysql'|'sqlite')`.
- DDL and JSON operators vary by backend; see vendor docs above.

### Drizzle Serverless

- Neon (drizzle‑orm/neon‑http): `examples/serverless-neon-drizzle.js`
- Turso (drizzle‑orm/libsql/http): `examples/serverless-turso-drizzle.js`

Examples mirror the provider docs; install drizzle adapters to run them.

## Husky

- Install: add dev dep `husky` and ensure `"prepare": "husky install"` in `package.json` (already set).
- Initialize: run `npx husky init` or `npx husky install` after install.
- Add a pre-commit hook:
  - `npx husky add .husky/pre-commit "npm run test"`
  - Optionally include lint/typecheck commands.
- Windows PowerShell: if scripts are blocked, enable with `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` then rerun the Husky commands.
Tiny/Lite usage:

```javascript
import tiny from 'umosql/tiny';
import lite from 'umosql/lite';
tiny.filter({ age: { $eq: 25 } }, 'sqlite');
lite.filter({ 'profile.country': 'FR' }, 'pg');
```

## ⚙️ Custom Builds (Ultra-Minimal)

- You can create your own builder with only the operators you need to reduce bundle size.
- Example:

```javascript
import { createQueryBuilder, filterOps, exprOps, updateOps } from 'umosql';

const custom = createQueryBuilder({
  filterOps: { $eq: filterOps.$eq, $in: filterOps.$in },
  exprOps: { $add: exprOps.$add, $upper: exprOps.$upper },
  updateOps: { $set: updateOps.$set },
  stageHandlers: {} // no aggregation
});

export const { collection, filter } = custom;
```

This approach lets you tailor the library to your use case and keep bundles extremely small.

---

## 📖 Table of Contents

- [Installation](#installation)
- [Query Examples](#query-examples)
  - [Basic Queries](#basic-queries)
  - [Array Operators ($in, $nin)](#array-operators-in-nin)
  - [Logical Operators](#logical-operators)
  - [Pattern Matching](#pattern-matching)
  - [JSON Field Queries](#json-field-queries)
- [CRUD Operations](#crud-operations)
  - [Find Operations](#find-operations)
  - [Insert Operations](#insert-operations)
  - [Update Operations](#update-operations)
  - [Delete Operations](#delete-operations)
- [Aggregation Pipeline](#aggregation-pipeline-full-version)
- [Filter Operators](#filter-operators)
- [Update Operators](#update-operators)
- [Expression Operators](#expression-operators)
- [Custom Operators](#custom-operators)
- [Operator Support Matrix](#operator-support-matrix)
- [Universal Adapters](#-universal-adapters)
- [Schemaless Adapters](#schemaless-adapters)
- [SQL Adapter Reference](#-sql-adapter-reference-createsqladapter)
- [In-Memory Engine Reference](#-in-memory-engine-reference-umosqlmemory)
- [MongoDB Adapter Reference](#-mongodb-adapter-reference)
- [Builder Utilities](#-builder-utilities--standalone-functions-umosql)
- [Lite & Tiny Lists](#-lite--tiny-operator-lists)
- [Backend Quirks](#️-backend-quirks--differences)
- [Connection Lifecycle](#-connection-lifecycle)
- [Examples Tour](#-examples-tour-examples)
- [Errors Reference](#-errors-reference)
- [TypeScript Types](#-typescript-types)
- [API Reference](#api-reference)

---

## 🔍 Query Examples

### Basic Queries

```javascript
import { collection } from "umosql";

const users = collection("users", "pg");

// Equality
users.find({ status: "active" }).toSQL();
// SELECT * FROM users WHERE status = 'active'

// Greater than
users.find({ age: { $gt: 18 } }).toSQL();
// SELECT * FROM users WHERE age > 18

// Greater than or equal
users.find({ age: { $gte: 18 } }).toSQL();
// SELECT * FROM users WHERE age >= 18

// Less than
users.find({ age: { $lt: 65 } }).toSQL();
// SELECT * FROM users WHERE age < 65

// Less than or equal
users.find({ age: { $lte: 65 } }).toSQL();
// SELECT * FROM users WHERE age <= 65

// Not equal
users.find({ status: { $ne: "banned" } }).toSQL();
// SELECT * FROM users WHERE status != 'banned'

// Multiple conditions (implicit AND)
users.find({ age: { $gte: 18 }, status: "active" }).toSQL();
// SELECT * FROM users WHERE age >= 18 AND status = 'active'
```

### Array Operators ($in, $nin)

```javascript
// IN - Match any value in array
users.find({ status: { $in: ["active", "pending", "verified"] } }).toSQL();
// SELECT * FROM users WHERE status IN ('active', 'pending', 'verified')

users.find({ age: { $in: [18, 21, 25, 30] } }).toSQL();
// SELECT * FROM users WHERE age IN (18, 21, 25, 30)

// NIN - Not in array
users.find({ role: { $nin: ["admin", "moderator"] } }).toSQL();
// SELECT * FROM users WHERE role NOT IN ('admin', 'moderator')

users.find({ status: { $nin: ["banned", "suspended", "deleted"] } }).toSQL();
// SELECT * FROM users WHERE status NOT IN ('banned', 'suspended', 'deleted')

// Empty array edge cases
users.find({ status: { $in: [] } }).toSQL();
// SELECT * FROM users WHERE status = 1 AND 1 = 0 (always false)

users.find({ status: { $nin: [] } }).toSQL();
// SELECT * FROM users WHERE status = 1 OR 1 = 1 (always true)

// Combined with other operators
users
	.find({
		age: { $gte: 18 },
		status: { $in: ["active", "verified"] },
	})
	.toSQL();
// SELECT * FROM users WHERE age >= 18 AND status IN ('active', 'verified')
```

### Logical Operators

```javascript
// $and - All conditions must be true
users
	.find({
		$and: [{ age: { $gte: 18 } }, { age: { $lte: 65 } }, { status: "active" }],
	})
	.toSQL();
// SELECT * FROM users WHERE (age >= 18 AND age <= 65 AND status = 'active')

// $or - Any condition must be true
users
	.find({
		$or: [
			{ role: "admin" },
			{ role: "moderator" },
			{ permissions: { $in: ["write", "delete"] } },
		],
	})
	.toSQL();
// SELECT * FROM users WHERE (role = 'admin' OR role = 'moderator' OR permissions IN ('write', 'delete'))

// $not - Negate condition
users
	.find({
		$not: { status: "banned" },
	})
	.toSQL();
// SELECT * FROM users WHERE NOT (status = 'banned')

// Complex nested logic
users
	.find({
		$and: [
			{ age: { $gte: 18 } },
			{ $or: [{ status: "active" }, { status: "verified" }] },
		],
	})
	.toSQL();
// SELECT * FROM users WHERE (age >= 18 AND (status = 'active' OR status = 'verified'))

// $nor - None of the conditions may be true
users.find({ $nor: [{ role: "admin" }, { role: "moderator" }] }).toSQL();
// SELECT * FROM users WHERE NOT (role = 'admin' OR role = 'moderator')

// $not on a single field - negate one operator
users.find({ age: { $not: { $gte: 18 } } }).toSQL();
// SELECT * FROM users WHERE NOT (age >= 18)

// $between - inclusive range (SQL-side extension)
users.find({ age: { $between: [18, 65] } }).toSQL();
// SELECT * FROM users WHERE age BETWEEN 18 AND 65

// $mod - divisor / remainder (SQL-side extension)
users.find({ age: { $mod: [10, 0] } }).toSQL();
// SELECT * FROM users WHERE age % 10 = 0
```

### Pattern Matching

```javascript
// LIKE - Pattern matching
users.find({ email: { $like: "%@gmail.com" } }).toSQL();
// SELECT * FROM users WHERE email LIKE '%@gmail.com'

users.find({ name: { $like: "John%" } }).toSQL();
// SELECT * FROM users WHERE name LIKE 'John%'

// ILIKE - Case-insensitive (PostgreSQL) / LOWER LIKE (others)
users.find({ name: { $ilike: "alice" } }).toSQL();
// PostgreSQL: SELECT * FROM users WHERE name ILIKE 'alice'
// Others: SELECT * FROM users WHERE LOWER(name) LIKE LOWER('alice')

// NOT LIKE
users.find({ email: { $nlike: "%@temporary.com" } }).toSQL();
// SELECT * FROM users WHERE email NOT LIKE '%@temporary.com'

// NOT ILIKE
users.find({ username: { $nilike: "admin%" } }).toSQL();
// PostgreSQL: SELECT * FROM users WHERE username NOT ILIKE 'admin%'

// REGEX - Regular expression matching
users.find({ code: { $regex: /^[A-Z]{3}\d{3}$/ } }).toSQL();
// PostgreSQL: SELECT * FROM users WHERE code ~ '^[A-Z]{3}\d{3}$'
// MySQL: SELECT * FROM users WHERE code REGEXP '^[A-Z]{3}\d{3}$'

users
	.find({ email: { $regex: "^[a-z0-9._%+-]+@[a-z0-9.-]+.[a-z]{2,}$" } })
	.toSQL();
```

### JSON Field Queries (Full & Lite)

```javascript
// Query nested JSON fields
users.find({ "profile.country": "France" }).toSQL();
// PostgreSQL: SELECT * FROM users WHERE (profile::jsonb #>> '{country}') = 'France'
// MySQL: SELECT * FROM users WHERE json_extract(profile, '$.country') = 'France'
// SQLite: SELECT * FROM users WHERE json_extract(profile, '$.country') = 'France'

// Deep nested paths
users.find({ "profile.address.city": "Paris" }).toSQL();
// PostgreSQL: WHERE (profile::jsonb #>> '{address,city}') = 'Paris'
// MySQL: WHERE json_extract(profile, '$.address.city') = 'Paris'
// SQLite: WHERE json_extract(profile, '$.address.city') = 'Paris'

// Array index access
users.find({ "orders.0.status": "completed" }).toSQL();
// PostgreSQL: WHERE (orders::jsonb #>> '{0,status}') = 'completed'
// MySQL: WHERE json_extract(orders, '$.0.status') = 'completed'
// SQLite: WHERE json_extract(orders, '$.0.status') = 'completed'

// JSON field with operators
users.find({ "profile.score": { $gte: 80 } }).toSQL();
// PostgreSQL: WHERE (profile::jsonb #>> '{score}')::numeric >= 80

users.find({ "tags.0": { $in: ["featured", "premium"] } }).toSQL();

// Multiple JSON conditions
users
	.find({
		"profile.country": "USA",
		"profile.age": { $gte: 18 },
		"settings.notifications": true,
	})
	.toSQL();
```

---

## 📝 CRUD Operations

### Find Operations

```javascript
const users = collection("users", "pg");

// Simple find
users.find({ city: "Paris" }).toSQL();
// SELECT * FROM users WHERE city = 'Paris'

// With projection (select specific fields)
users.find({ active: true }, { name: 1, email: 1, age: 1 }).toSQL();
// SELECT name, email, age FROM users WHERE active = TRUE

// Projection as array
users.find({ active: true }, ["name", "email"]).toSQL();
// SELECT name, email FROM users WHERE active = TRUE

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

// Sort only
users.find({}).sort({ createdAt: -1 }).toSQL();
// SELECT * FROM users ORDER BY createdAt DESC

// DISTINCT
users.find({ country: "USA" }).select(["state"]).distinct().toSQL();
// SELECT DISTINCT state FROM users WHERE country = 'USA'

// Find one
users.findOne({ email: "alice@example.com" }).toSQL();
// SELECT * FROM users WHERE email = 'alice@example.com' LIMIT 1

// Find by ID
users.findOne({ id: 123 }).toSQL();
// SELECT * FROM users WHERE id = 123 LIMIT 1

// Count documents
users.find({ status: "active" }).count().toSQL();
// SELECT COUNT(*) AS count FROM users WHERE status = 'active'

users.countDocuments({ age: { $gte: 18 } });
// SELECT COUNT(*) AS count FROM users WHERE age >= 18

// Distinct values
users.distinct("city", { country: "USA" });
// SELECT DISTINCT city FROM users WHERE country = 'USA'

users.distinct("status");
// SELECT DISTINCT status FROM users
```

### Insert Operations

```javascript
// Insert one document
users.insertOne({
	name: "Alice",
	age: 25,
	email: "alice@example.com",
	status: "active",
});
// INSERT INTO users (name, age, email, status)
// VALUES ('Alice', 25, 'alice@example.com', 'active')

// Insert with nested JSON
users.insertOne({
	name: "Bob",
	profile: {
		country: "USA",
		city: "New York",
		score: 95,
	},
});
// PostgreSQL: profile stored as JSONB
// Others: profile stored as JSON string

// Insert many documents
users.insertMany([
	{ name: "Charlie", age: 22, status: "pending" },
	{ name: "David", age: 28, status: "active" },
	{ name: "Eve", age: 35, status: "active" },
]);
// INSERT INTO users (name, age, status) VALUES
// ('Charlie', 22, 'pending'),
// ('David', 28, 'active'),
// ('Eve', 35, 'active')

// Documents with different fields (NULL for missing)
users.insertMany([
	{ name: "Frank", age: 30 },
	{ name: "Grace", email: "grace@example.com" },
	{ name: "Henry", age: 40, email: "henry@example.com" },
]);
// INSERT INTO users (name, age, email) VALUES
// ('Frank', 30, NULL),
// ('Grace', NULL, 'grace@example.com'),
// ('Henry', 40, 'henry@example.com')

// With RETURNING clause (PostgreSQL)
users.insertOne({ name: "Ivan" }, { returning: ["id", "name", "createdAt"] });
// INSERT INTO users (name) VALUES ('Ivan') RETURNING id, name, createdAt

users.insertMany([{ name: "Jack" }, { name: "Kate" }], { returning: "*" });
// INSERT INTO users (name) VALUES ('Jack'), ('Kate') RETURNING *
```

### Update Operations

```javascript
// Update one document
users.updateOne(
	{ email: "alice@example.com" },
	{ $set: { status: "verified", verifiedAt: new Date() } }
);
// UPDATE users SET status = 'verified', verifiedAt = '2024-01-01 12:00:00'
// WHERE email = 'alice@example.com'
// (MySQL/SQLite append `LIMIT 1`; PostgreSQL has no LIMIT on UPDATE)

// Update many documents
users.updateMany(
	{ age: { $lt: 18 } },
	{ $set: { role: "minor", permissions: [] } }
);
// UPDATE users SET role = 'minor', permissions = '[]' WHERE age < 18

// Increment values
users.updateOne({ id: 1 }, { $inc: { loginCount: 1, points: 10 } });
// UPDATE users SET loginCount = loginCount + 1, points = points + 10
// WHERE id = 1

// Multiply values
users.updateOne({ id: 1 }, { $mul: { score: 1.1 } });
// UPDATE users SET score = score * 1.1 WHERE id = 1

// Set to minimum
users.updateMany({}, { $min: { minPrice: 10 } });
// UPDATE users SET minPrice = LEAST(minPrice, 10)

// Set to maximum
users.updateMany({}, { $max: { maxDiscount: 50 } });
// UPDATE users SET maxDiscount = GREATEST(maxDiscount, 50)

// Unset fields (set to NULL)
users.updateOne({ id: 1 }, { $unset: { tempToken: "", tempData: "" } });
// UPDATE users SET tempToken = NULL, tempData = NULL WHERE id = 1

// Rename fields
users.updateMany({}, { $rename: { oldField: "newField" } });
// UPDATE users SET newField = oldField, oldField = NULL

// Current timestamp
users.updateOne(
	{ id: 1 },
	{ $currentDate: { lastLogin: true, updatedAt: true } }
);
// PostgreSQL: UPDATE users SET lastLogin = CURRENT_TIMESTAMP, updatedAt = CURRENT_TIMESTAMP WHERE id = 1
// MySQL: UPDATE users SET lastLogin = NOW(), updatedAt = NOW() WHERE id = 1 LIMIT 1
// SQLite: UPDATE users SET lastLogin = datetime('now'), updatedAt = datetime('now') WHERE id = 1 LIMIT 1

// Multiple operators combined
users.updateOne(
	{ id: 1 },
	{
		$set: { status: "active", lastLogin: new Date() },
		$inc: { loginCount: 1, points: 5 },
		$unset: { resetToken: "" },
		$currentDate: { updatedAt: true },
	}
);

// Update with $in filter
users.updateMany(
	{ status: { $in: ["pending", "unverified"] } },
	{ $set: { needsVerification: true } }
);
// UPDATE users SET needsVerification = TRUE
// WHERE status IN ('pending', 'unverified')

// Update JSON fields (Full & Lite)
users.updateOne(
	{ id: 1 },
	{ $set: { "profile.score": 95, "profile.level": 5 } }
);
// PostgreSQL: jsonb_set for efficient nested updates
// MySQL: JSON_SET
// SQLite: json_set

// Increment JSON numeric field
users.updateOne({ id: 1 }, { $inc: { "stats.views": 1, "stats.likes": 1 } });

// Remove JSON field
users.updateOne({ id: 1 }, { $unset: { "profile.tempField": "" } });

// With RETURNING (PostgreSQL)
users.updateMany(
	{ city: "Paris" },
	{ $inc: { points: 10 } },
	{ returning: ["id", "name", "points"] }
);
// UPDATE users SET points = points + 10 WHERE city = 'Paris' RETURNING id, name, points
```

### Delete Operations

```javascript
// Delete one document
users.deleteOne({ email: "old@example.com" });
// DELETE FROM users WHERE email = 'old@example.com'
// (MySQL/SQLite append `LIMIT 1`; PostgreSQL has no LIMIT on DELETE)

// Delete many documents
users.deleteMany({ active: false });
// DELETE FROM users WHERE active = FALSE

users.deleteMany({ lastLogin: { $lt: "2023-01-01" } });
// DELETE FROM users WHERE lastLogin < '2023-01-01'

// Delete with $in
users.deleteMany({ status: { $in: ["banned", "deleted", "suspended"] } });
// DELETE FROM users WHERE status IN ('banned', 'deleted', 'suspended')

// Delete with complex conditions
users.deleteMany({
	$and: [{ createdAt: { $lt: "2020-01-01" } }, { loginCount: { $eq: 0 } }],
});

// Delete all (requires explicit permission)
users.deleteMany({}, { allowDeleteAll: true });
// DELETE FROM users

// Attempting to delete all without permission throws error
users.deleteMany({}); // ❌ Error: deleteMany requires a filter or allowDeleteAll option

// With RETURNING (PostgreSQL)
users.deleteMany({ age: { $lt: 13 } }, { returning: ["id", "name", "email"] });
// DELETE FROM users WHERE age < 13 RETURNING id, name, email

users.deleteOne({ id: 123 }, { returning: "*" });
// DELETE FROM users WHERE id = 123 RETURNING *
```

---

## 📊 Aggregation Pipeline (Full Version)

### Basic Aggregation

```javascript
const orders = collection("orders", "pg");

// Group by and count
orders.aggregate([
	{
		$group: {
			_id: "$status",
			count: { $sum: 1 },
		},
	},
]);
// SELECT status AS _id, SUM(1) AS count
// FROM (SELECT * FROM orders) AS t1
// GROUP BY status

// Group with average
orders.aggregate([
	{
		$group: {
			_id: "$customerId",
			avgAmount: { $avg: "$amount" },
			totalOrders: { $sum: 1 },
		},
	},
]);
// SELECT customerId AS _id, AVG(amount) AS avgAmount, SUM(1) AS totalOrders
// FROM (SELECT * FROM orders) AS t1
// GROUP BY customerId

// Group with min/max
orders.aggregate([
	{
		$group: {
			_id: "$category",
			minPrice: { $min: "$price" },
			maxPrice: { $max: "$price" },
			avgPrice: { $avg: "$price" },
		},
	},
]);
```

### Match Before Group

```javascript
// Filter then aggregate
orders.aggregate([
	{ $match: { status: "completed" } },
	{
		$group: {
			_id: "$customerId",
			totalSpent: { $sum: "$amount" },
			orderCount: { $sum: 1 },
		},
	},
]);
// SELECT customerId AS _id, SUM(amount) AS totalSpent, SUM(1) AS orderCount
// FROM (SELECT * FROM orders WHERE status = 'completed') AS t1
// GROUP BY customerId

// Match with $in
orders.aggregate([
	{ $match: { status: { $in: ["completed", "shipped"] } } },
	{
		$group: {
			_id: "$category",
			total: { $sum: "$amount" },
		},
	},
]);
```

### Match After Group (HAVING)

```javascript
// Aggregate then filter results
orders.aggregate([
	{
		$group: {
			_id: "$customerId",
			totalSpent: { $sum: "$amount" },
			orderCount: { $sum: 1 },
		},
	},
	{ $match: { totalSpent: { $gte: 1000 } } },
]);
// SELECT customerId AS _id, SUM(amount) AS totalSpent, SUM(1) AS orderCount
// FROM (SELECT * FROM orders) AS t1
// GROUP BY customerId
// HAVING totalSpent >= 1000

// Multiple HAVING conditions
orders.aggregate([
	{
		$group: {
			_id: "$customerId",
			avgAmount: { $avg: "$amount" },
			count: { $sum: 1 },
		},
	},
	{
		$match: {
			avgAmount: { $gte: 100 },
			count: { $gte: 5 },
		},
	},
]);
```

### Project Stage

```javascript
// Select and transform fields
orders.aggregate([
	{
		$project: {
			orderId: "$id",
			customer: "$customerId",
			total: "$amount",
			status: 1,
		},
	},
]);
// SELECT id AS orderId, customerId AS customer, amount AS total, status AS status
// FROM (SELECT * FROM orders) AS t1

// With expressions
orders.aggregate([
	{
		$project: {
			orderId: "$id",
			total: { $multiply: ["$quantity", "$price"] },
			discount: { $divide: ["$amount", 10] },
		},
	},
]);
```

### Sort, Skip, Limit

```javascript
// Sort aggregation results
orders.aggregate([
	{
		$group: {
			_id: "$customerId",
			totalSpent: { $sum: "$amount" },
		},
	},
	{ $sort: { totalSpent: -1 } },
	{ $limit: 10 },
]);
// Top 10 customers by spending

// Pagination
orders.aggregate([
	{
		$group: {
			_id: "$category",
			count: { $sum: 1 },
		},
	},
	{ $sort: { count: -1 } },
	{ $skip: 20 },
	{ $limit: 10 },
]);
// Page 3 of categories (offset 20, limit 10)
```

### Count Stage

```javascript
// Count filtered results
orders.aggregate([
	{ $match: { status: "completed", amount: { $gte: 100 } } },
	{ $count: "expensiveOrders" },
]);
// SELECT COUNT(*) AS expensiveOrders
// FROM (SELECT * FROM orders WHERE status = 'completed' AND amount >= 100) AS t1
```

### Complex Aggregation Examples

```javascript
const users = collection("users", "pg");

// Average age by city, sorted
users.aggregate([
	{ $match: { active: true } },
	{
		$group: {
			_id: "$city",
			avgAge: { $avg: "$age" },
			count: { $sum: 1 },
		},
	},
	{ $match: { count: { $gte: 10 } } },
	{ $sort: { avgAge: -1 } },
]);

// JSON field aggregation (Full version)
users.aggregate([
	{
		$group: {
			_id: "$profile.country",
			avgScore: { $avg: "$profile.score" },
			users: { $sum: 1 },
		},
	},
	{ $match: { avgScore: { $gte: 80 } } },
	{ $sort: { avgScore: -1 } },
]);

// Multi-stage pipeline
const products = collection("products", "pg");

products.aggregate([
	// Filter active products
	{ $match: { active: true, stock: { $gt: 0 } } },

	// Calculate revenue per category
	{
		$group: {
			_id: "$category",
			totalRevenue: { $sum: { $multiply: ["$price", "$stock"] } },
			avgPrice: { $avg: "$price" },
			productCount: { $sum: 1 },
		},
	},

	// Only categories with significant revenue
	{ $match: { totalRevenue: { $gte: 10000 } } },

	// Sort by revenue
	{ $sort: { totalRevenue: -1 } },

	// Top 5 categories
	{ $limit: 5 },

	// Project final shape
	{
		$project: {
			category: "$_id",
			revenue: "$totalRevenue",
			avgPrice: 1,
			products: "$productCount",
		},
	},
];
```

### More Stages ($addFields, $sample, $sortByCount, $bucket, composite _id)

```javascript
// $addFields / $set - keep every column, add computed ones
orders.aggregate([
	{ $match: { active: true } },
	{ $addFields: { isAdult: { $gte: ["$age", 18] } } },
]);
// SELECT *, (age >= 18) AS isAdult
// FROM (SELECT * FROM orders WHERE active = TRUE) AS t1

// $set is an alias of $addFields; later stages sort by the alias
orders.aggregate([
	{ $set: { next: { $add: ["$age", 1] } } },
	{ $sort: { next: -1 } },
]);
// SELECT *, (age + 1) AS next FROM (SELECT * FROM orders) AS t1
// ORDER BY next DESC

// $sample - random rows: { size }
orders.aggregate([{ $sample: { size: 5 } }]);
// PostgreSQL/SQLite: ... ORDER BY RANDOM() LIMIT 5
// MySQL:             ... ORDER BY RAND() LIMIT 5

// $sortByCount - group by an expression, ordered by frequency
orders.aggregate([{ $sortByCount: "$city" }]);
// SELECT city AS _id, COUNT(*) AS count
// FROM (SELECT * FROM orders) AS t1 GROUP BY city ORDER BY count DESC

// $bucket - numeric ranges via CASE
orders.aggregate([
	{ $bucket: { groupBy: "$age", boundaries: [0, 18, 65], default: "senior" } },
]);
// SELECT CASE WHEN age >= 0 AND age < 18 THEN 0
//            WHEN age >= 18 AND age < 65 THEN 18
//            ELSE 'senior' END AS _id, COUNT(*) AS count ...
// GROUP BY CASE WHEN ... END

// $bucket with output accumulators
orders.aggregate([
	{
		$bucket: {
			groupBy: "$age",
			boundaries: [0, 18, 65],
			output: { n: { $sum: 1 }, avgSpent: { $avg: "$amount" } },
		},
	},
]);

// Composite _id - group by several expressions at once
orders.aggregate([
	{ $group: { _id: { city: "$city", year: "$year" }, n: { $sum: 1 } } },
]);
// SELECT city AS city, year AS year, COUNT(*) AS n
// FROM (SELECT * FROM orders) AS t1 GROUP BY city, year

// $group accumulators on SQL: $sum / $avg / $min / $max / $count
// plus any scalar expression. ($push / $addToSet / $first / $last are
// memory-only; $unset is a memory-only stage; $sample is SQL-only.)
```

---

## 🎯 Filter Operators Reference

| Operator  | Description            | Example                                               | SQL Output                         |
| --------- | ---------------------- | ----------------------------------------------------- | ---------------------------------- |
| `$eq`     | Equals                 | `{ age: { $eq: 25 } }`                                | `age = 25`                         |
| `$ne`     | Not equals             | `{ status: { $ne: 'inactive' } }`                     | `status != 'inactive'`             |
| `$gt`     | Greater than           | `{ age: { $gt: 18 } }`                                | `age > 18`                         |
| `$gte`    | Greater than or equal  | `{ age: { $gte: 18 } }`                               | `age >= 18`                        |
| `$lt`     | Less than              | `{ age: { $lt: 65 } }`                                | `age < 65`                         |
| `$lte`    | Less than or equal     | `{ age: { $lte: 65 } }`                               | `age <= 65`                        |
| `$in`     | In array               | `{ status: { $in: ['active', 'pending'] } }`          | `status IN ('active', 'pending')`  |
| `$nin`    | Not in array           | `{ role: { $nin: ['admin', 'mod'] } }`                | `role NOT IN ('admin', 'mod')`     |
| `$like`   | Pattern match          | `{ email: { $like: '%@gmail.com' } }`                 | `email LIKE '%@gmail.com'`         |
| `$ilike`  | Case-insensitive match | `{ name: { $ilike: 'alice' } }`                       | `name ILIKE 'alice'` (PG)          |
| `$nlike`  | Not like               | `{ email: { $nlike: '%temp%' } }`                     | `email NOT LIKE '%temp%'`          |
| `$nilike` | Not ilike              | `{ name: { $nilike: 'admin%' } }`                     | `name NOT ILIKE 'admin%'`          |
| `$regex`  | Regular expression     | `{ code: { $regex: /^[A-Z]+$/ } }`                    | `code ~ '^[A-Z]+$'` (PG)           |
| `$exists` | Field exists           | `{ phone: { $exists: true } }`                        | `phone IS NOT NULL`                |
| `$and`    | Logical AND            | `{ $and: [{ age: { $gte: 18 } }, { active: true }] }` | `(age >= 18 AND active = TRUE)`    |
| `$or`     | Logical OR             | `{ $or: [{ role: 'admin' }, { role: 'mod' }] }`       | `(role = 'admin' OR role = 'mod')` |
| `$not`    | Logical NOT            | `{ $not: { status: 'banned' } }`                      | `NOT (status = 'banned')`          |
| `$expr`   | Expression             | `{ $expr: { $gt: ['$price', '$cost'] } }`             | `(price > cost)`                   |

---

## 🔧 Update Operators Reference

| Operator       | Description      | Example                                 | SQL Output                             |
| -------------- | ---------------- | --------------------------------------- | -------------------------------------- |
| `$set`         | Set field value  | `{ $set: { status: 'active' } }`        | `status = 'active'`                    |
| `$inc`         | Increment value  | `{ $inc: { views: 1 } }`                | `views = views + 1`                    |
| `$mul`         | Multiply value   | `{ $mul: { price: 1.1 } }`              | `price = price * 1.1`                  |
| `$min`         | Set to minimum   | `{ $min: { lowScore: 50 } }`            | `lowScore = LEAST(lowScore, 50)`       |
| `$max`         | Set to maximum   | `{ $max: { highScore: 100 } }`          | `highScore = GREATEST(highScore, 100)` |
| `$unset`       | Remove field     | `{ $unset: { tempField: '' } }`         | `tempField = NULL`                     |
| `$rename`      | Rename field     | `{ $rename: { old: 'new' } }`           | `new = old, old = NULL`                |
| `$currentDate` | Set current date | `{ $currentDate: { updatedAt: true } }` | `updatedAt = CURRENT_TIMESTAMP`        |

### Update Operator Examples

```javascript
const products = collection("products", "pg");

// Set multiple fields
products.updateOne(
	{ id: 1 },
	{
		$set: {
			name: "New Name",
			price: 99.99,
			stock: 100,
			updatedAt: new Date(),
		},
	}
);

// Increment multiple counters
products.updateOne(
	{ id: 1 },
	{
		$inc: {
			views: 1,
			sales: 1,
			stock: -1, // decrement
		},
	}
);

// Multiply for percentage increase
products.updateMany(
	{ category: "electronics" },
	{
		$mul: { price: 1.15 }, // 15% price increase
	}
);

// Ensure minimum/maximum values
products.updateMany(
	{},
	{
		$min: { price: 9.99 }, // No product less than $9.99
		$max: { discount: 50 }, // Max 50% discount
	}
);

// Complex update with multiple operators
products.updateOne(
	{ id: 1 },
	{
		$set: { status: "featured", featuredAt: new Date() },
		$inc: { promotionCount: 1 },
		$mul: { price: 0.9 }, // 10% discount
		$unset: { tempPromo: "" },
		$currentDate: { updatedAt: true },
	}
);

// Plain keys (no `$`) work as `$set` - handy shorthand
products.updateOne({ id: 1 }, { name: "New Name", stock: 100 });
// UPDATE products SET name = 'New Name', stock = 100 WHERE id = 1
```

---

## 📐 Expression Operators Reference

### Arithmetic Operators

```javascript
import { expression } from "umosql";

// Addition
expression({ $add: ["$price", 10] }, "pg");
// (price + 10)

expression({ $add: ["$subtotal", "$tax", "$shipping"] }, "pg");
// (subtotal + tax + shipping)

// Subtraction
expression({ $subtract: ["$total", "$discount"] }, "pg");
// (total - discount)

// Multiplication
expression({ $multiply: ["$quantity", "$price"] }, "pg");
// (quantity * price)

// Division
expression({ $divide: ["$total", "$count"] }, "pg");
// (total / count)

// Modulo
expression({ $mod: ["$value", 10] }, "pg");
// (value % 10)
```

### String Operators

```javascript
// Concatenate strings
expression({ $concat: ["$firstName", " ", "$lastName"] }, "pg");
// PostgreSQL: CONCAT(firstName, ' ', lastName)
// SQLite: firstName || ' ' || lastName

expression({ $concat: ["Order #", "$orderNumber"] }, "pg");
// CONCAT('Order #', orderNumber)

// Uppercase
expression({ $upper: "$email" }, "pg");
// UPPER(email)

// Lowercase
expression({ $lower: "$name" }, "pg");
// LOWER(name)

// Substring
expression({ $substr: ["$description", 0, 100] }, "pg");
// SUBSTRING(description, 0, 100)
```

### Comparison in Expressions

```javascript
// Equal
expression({ $eq: ["$price", "$msrp"] }, "pg");
// (price = msrp)

// Greater than
expression({ $gt: ["$stock", 10] }, "pg");
// (stock > 10)

// Check if in array
expression({ $in: ["$status", ["active", "verified"]] }, "pg");
// (status IN ('active', 'verified'))
```

### Conditional Operators

```javascript
// Simple if-then-else
expression(
	{
		$cond: [{ $gte: ["$age", 18] }, "adult", "minor"],
	},
	"pg"
);
// CASE WHEN (age >= 18) THEN 'adult' ELSE 'minor' END

// Nested conditions
expression(
	{
		$cond: [
			{ $gte: ["$score", 90] },
			"A",
			{ $cond: [{ $gte: ["$score", 80] }, "B", "C"] },
		],
	},
	"pg"
);

// Switch statement
expression(
	{
		$switch: {
			branches: [
				{ case: { $eq: ["$status", "pending"] }, then: "Processing" },
				{ case: { $eq: ["$status", "shipped"] }, then: "In Transit" },
				{ case: { $eq: ["$status", "delivered"] }, then: "Completed" },
			],
			default: "Unknown",
		},
	},
	"pg"
);
// CASE
//   WHEN (status = 'pending') THEN 'Processing'
//   WHEN (status = 'shipped') THEN 'In Transit'
//   WHEN (status = 'delivered') THEN 'Completed'
//   ELSE 'Unknown'
// END
```

### Aggregation Functions (Full version)

```javascript
// In aggregation pipeline
orders.aggregate([
	{
		$group: {
			_id: "$customerId",
			total: { $sum: "$amount" },
			avg: { $avg: "$amount" },
			min: { $min: "$amount" },
			max: { $max: "$amount" },
			count: { $sum: 1 },
		},
	},
];
```

### More Expression Operators

```javascript
import { expression } from "umosql";

// Three-way comparison -> -1 / 0 / 1
expression({ $cmp: ["$price", "$msrp"] }, "pg");
// CASE WHEN price < msrp THEN -1 WHEN price > msrp THEN 1 ELSE 0 END

// Length of a JSON array column
expression({ $size: ["$tags"] }, "pg"); // jsonb_array_length(tags::jsonb)
expression({ $size: ["$tags"] }, "mysql"); // JSON_LENGTH(tags)
expression({ $size: ["$tags"] }, "sqlite"); // json_array_length(tags)

// Null checks inside expressions: [$field, bool]
expression({ $exists: ["$phone", true] }, "pg");
// (phone IS NOT NULL)

// Date parts (per-dialect functions)
expression({ $year: "$createdAt" }, "mysql"); // YEAR(createdAt)
expression({ $month: "$createdAt" }, "sqlite"); // CAST(strftime('%m', createdAt) AS INTEGER)
expression({ $dayOfWeek: "$createdAt" }, "pg"); // EXTRACT(DOW FROM createdAt) + 1 (Sunday = 1)
// Also: $dayOfMonth, $hour, $minute, $second, $week

// Type casts (per-dialect CAST)
expression({ $toInt: "$age" }, "mysql"); // CAST(age AS SIGNED)
expression({ $toInt: "$age" }, "pg"); // CAST(age AS INTEGER)
expression({ $toBool: "$age" }, "mysql"); // IF(age, 1, 0)
expression({ $toString: "$age" }, "pg"); // CAST(age AS TEXT)
expression({ $toDouble: "$age" }, "sqlite"); // CAST(age AS REAL)
expression({ $toDate: "$createdAt" }, "sqlite"); // datetime(createdAt)

// $literal escapes instead of resolving - "$notAField" stays a string
expression({ $literal: ["$notAField"] }, "pg");
// '$notAField'

// String helpers
expression({ $strLen: "$name" }, "pg"); // LENGTH(name)
expression({ $replace: ["$name", "a", "o"] }, "pg"); // REPLACE(name, 'a', 'o')
expression({ $trim: "$name" }, "pg"); // TRIM(name) (also $ltrim / $rtrim)

// $substr is 0-based like MongoDB; SQL is 1-based, so +1 is added
expression({ $substr: ["$name", 0, 3] }, "pg");
// SUBSTRING(name, (0 + 1), 3)

// Statistics + safe division ($divide guards divide-by-zero with NULLIF)
expression({ $stdDevPop: "$age" }, "pg"); // STDDEV_POP(age) (also $stdDevSamp)
expression({ $divide: ["$total", "$count"] }, "pg");
// (total * 1.0 / NULLIF(count, 0))

// Rounding with optional precision, power/square-root, abs/ceil/floor
expression({ $round: ["$price", 2] }, "pg"); // ROUND(price, 2)
expression({ $pow: ["$age", 2] }, "pg"); // POWER(age, 2)
expression({ $sqrt: "$age" }, "pg"); // SQRT(age)
```

---
### Full REST API Example (Express + pg)

A complete generic CRUD server — any collection, Mongo-style query strings:

```javascript
import express from "express";
import pkg from "pg";
import { collection } from "umosql";

const app = express();
app.use(express.json());
const pool = new pkg.Pool({ connectionString: process.env.DATABASE_URL });

// Parse ?q={...}&fields={...}&sort={...}&limit=&skip= into Mongo-style parts
const parseQuery = (req, res, next) => {
	try {
		req.mongoQuery = req.query.q ? JSON.parse(req.query.q) : {};
		req.queryOptions = {
			projection: req.query.fields ? JSON.parse(req.query.fields) : null,
			sort: req.query.sort ? JSON.parse(req.query.sort) : null,
			limit: parseInt(req.query.limit) || 100,
			skip: parseInt(req.query.skip) || 0,
		};
		next();
	} catch (error) {
		res
			.status(400)
			.json({ error: "Invalid query format", details: error.message });
	}
};

// GET /:collection - List all documents
app.get("/:collection", parseQuery, async (req, res) => {
	try {
		const coll = collection(req.params.collection, "pg");

		let query = coll.find(req.mongoQuery, req.queryOptions.projection);

		if (req.queryOptions.sort) {
			query = query.sort(req.queryOptions.sort);
		}

		query = query.skip(req.queryOptions.skip).limit(req.queryOptions.limit);

		const sql = query.toSQL();
		const result = await pool.query(sql);

		res.json({
			data: result.rows,
			count: result.rows.length,
			skip: req.queryOptions.skip,
			limit: req.queryOptions.limit,
		});
	} catch (error) {
		res.status(500).json({ error: error.message });
	}
});

// GET /:collection/count - Count documents
app.get("/:collection/count", parseQuery, async (req, res) => {
	try {
		const coll = collection(req.params.collection, "pg");
		const sql = coll.countDocuments(req.mongoQuery);

		const result = await pool.query(sql);
		res.json({ count: parseInt(result.rows[0].count) });
	} catch (error) {
		res.status(500).json({ error: error.message });
	}
});

// GET /:collection/:id - Get single document
app.get("/:collection/:id", async (req, res) => {
	try {
		const coll = collection(req.params.collection, "pg");
		const sql = coll.findOne({ id: req.params.id }).toSQL();

		const result = await pool.query(sql);

		if (result.rows.length === 0) {
			return res.status(404).json({ error: "Document not found" });
		}

		res.json(result.rows[0]);
	} catch (error) {
		res.status(500).json({ error: error.message });
	}
});

// POST /:collection - Create document
app.post("/:collection", async (req, res) => {
	try {
		const coll = collection(req.params.collection, "pg");
		const sql = coll.insertOne(req.body, { returning: "*" });

		const result = await pool.query(sql);
		res.status(201).json(result.rows[0]);
	} catch (error) {
		res.status(500).json({ error: error.message });
	}
});

// POST /:collection/bulk - Bulk insert
app.post("/:collection/bulk", async (req, res) => {
	try {
		if (!Array.isArray(req.body)) {
			return res.status(400).json({ error: "Body must be an array" });
		}

		const coll = collection(req.params.collection, "pg");
		const sql = coll.insertMany(req.body, { returning: "*" });

		const result = await pool.query(sql);
		res.status(201).json({ inserted: result.rows.length, data: result.rows });
	} catch (error) {
		res.status(500).json({ error: error.message });
	}
});

// PATCH /:collection/:id - Update single document
app.patch("/:collection/:id", async (req, res) => {
	try {
		const coll = collection(req.params.collection, "pg");
		const sql = coll.updateOne(
			{ id: req.params.id },
			{ $set: req.body },
			{ returning: "*" }
		);

		const result = await pool.query(sql);

		if (result.rows.length === 0) {
			return res.status(404).json({ error: "Document not found" });
		}

		res.json(result.rows[0]);
	} catch (error) {
		res.status(500).json({ error: error.message });
	}
});

// PUT /:collection/:id - Replace document
app.put("/:collection/:id", async (req, res) => {
	try {
		const coll = collection(req.params.collection, "pg");
		const sql = coll.updateOne(
			{ id: req.params.id },
			{ $set: req.body },
			{ returning: "*" }
		);

		const result = await pool.query(sql);

		if (result.rows.length === 0) {
			return res.status(404).json({ error: "Document not found" });
		}

		res.json(result.rows[0]);
	} catch (error) {
		res.status(500).json({ error: error.message });
	}
});

// PATCH /:collection - Bulk update
app.patch("/:collection", parseQuery, async (req, res) => {
	try {
		const coll = collection(req.params.collection, "pg");
		const sql = coll.updateMany(
			req.mongoQuery,
			{ $set: req.body },
			{ returning: "*" }
		);

		const result = await pool.query(sql);
		res.json({ updated: result.rows.length, data: result.rows });
	} catch (error) {
		res.status(500).json({ error: error.message });
	}
});

// DELETE /:collection/:id - Delete single document
app.delete("/:collection/:id", async (req, res) => {
	try {
		const coll = collection(req.params.collection, "pg");
		const sql = coll.deleteOne({ id: req.params.id }, { returning: "*" });

		const result = await pool.query(sql);

		if (result.rows.length === 0) {
			return res.status(404).json({ error: "Document not found" });
		}

		res.json({ deleted: true, data: result.rows[0] });
	} catch (error) {
		res.status(500).json({ error: error.message });
	}
});

// DELETE /:collection - Bulk delete
app.delete("/:collection", parseQuery, async (req, res) => {
	try {
		if (Object.keys(req.mongoQuery).length === 0) {
			return res.status(400).json({
				error: 'Query required for bulk delete. Use ?q={"field":"value"}',
			});
		}

		const coll = collection(req.params.collection, "pg");
		const sql = coll.deleteMany(req.mongoQuery, { returning: "*" });

		const result = await pool.query(sql);
		res.json({ deleted: result.rows.length, data: result.rows });
	} catch (error) {
		res.status(500).json({ error: error.message });
	}
});

// Error handler
app.use((err, req, res, next) => {
	console.error(err.stack);
	res
		.status(500)
		.json({ error: "Internal server error", details: err.message });
});

// Start the server
app.listen(3000, () => console.log("API on http://localhost:3000"));
```

Try it:

```bash
curl 'http://localhost:3000/users?q={"age":{"$gte":18}}&sort={"age":-1}&limit=5'
curl -X POST http://localhost:3000/users -H 'Content-Type: application/json' -d '{"name":"Alice","age":25}'
```

---

## 🔌 Universal Adapters

### Unified Client (one factory, every backend)

```javascript
import { createSchemalessClient } from "umosql/client";

// 'memory' | 'sqlite' | 'pg' | 'mysql' | 'mongodb' | 'sql'
const client = await createSchemalessClient("pg", {
	host: "localhost",
	database: "mydb",
	user: "postgres",
	password: "password",
});

const users = client.db("mydb").collection("users");

// All operations return promises
const inserted = await users.insertOne({ name: "Alice", age: 25 });
await users.updateOne({ name: "Alice" }, { $inc: { loginCount: 1 } });
await users.deleteMany({ active: false });

// find() is async and returns a chainable cursor
const adults = await (await users.find({ age: { $gte: 18 } }))
	.sort({ age: -1 })
	.limit(10)
	.toArray();

// Aggregation
const stats = await users.aggregate([
	{ $group: { _id: "$city", count: { $sum: 1 } } },
]);

// client.raw is the underlying driver handle (pg.Client, mysql2 connection, ...)
await client.close();
```

Only the backend you ask for is imported, so the other drivers do not need to be installed.

#### Drivers are yours, not ours

| Backend   | Install yourself      | Or bring your own                                                        |
| --------- | --------------------- | ------------------------------------------------------------------------ |
| `memory`  | nothing               | —                                                                        |
| `sql`     | nothing (any driver)  | `executor(sql, params)` — Neon, Turso, PlanetScale, Hyperdrive, ...      |
| `sqlite`  | `npm i better-sqlite3`| `{ client: db }` or `{ driver: { default: Database } }`                  |
| `pg`      | `npm i pg`            | `{ client: pgClient }` (adopted as-is, never re-connected) or `{ driver }`|
| `mysql`   | `npm i mysql2`        | `{ conn }` / `{ client }` or `{ driver }`                                |
| `mongodb` | `npm i mongodb`       | `{ client: mongoClient }` or `{ driver }`                                |

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

### Memory Adapter

```javascript
import { createSchemalessClient } from "umosql/client";

const client = await createSchemalessClient("memory");
const users = client.db("app").collection("users");

// Works exactly like MongoDB
await users.insertMany([
	{ name: "Alice", age: 25, tags: ["premium"] },
	{ name: "Bob", age: 30, tags: ["basic"] },
	{ name: "Charlie", age: 22, tags: ["premium", "vip"] },
]);

const premium = await (await users.find({ tags: { $elemMatch: { $eq: "premium" } } })).toArray();
await users.updateMany({ tags: { $elemMatch: { $eq: "premium" } } }, { $inc: { points: 100 } });
const grouped = await users.aggregate([
	{ $group: { _id: "$age", count: { $sum: 1 } } },
]);

await client.close(); // drops every in-memory store
```

Each `client.db(name)` gets its own isolated store, so database names never share documents.

### In-memory engine without adapters (synchronous)

```javascript
import { collection, db } from "umosql/memory";

const users = collection("users", [
	{ name: "Alice", age: 25, tags: ["premium"] },
	{ name: "Bob", age: 30, tags: ["basic"] },
]);

users.find({ age: { $in: [25, 30] } }).toArray(); // no promises
users.updateMany({ age: { $gte: 18 } }, { $inc: { points: 100 } });
users.aggregate([{ $group: { _id: "$age", count: { $sum: 1 } } }]);

// Or several named databases:
const app = db("app");
const logs = app.collection("logs", [], { idStrategy: "mongo" });
```

### Bring your own driver (adapters only)

```javascript
import { createSchemalessAdapter } from "umosql/schemaless";
import Database from "better-sqlite3";

const { adapter } = createSchemalessAdapter(new Database(":memory:"), "sqlite");
const users = adapter.collection("users");
```

### Serverless / HTTP drivers

```javascript
import { createSchemalessClient } from "umosql/client";

// Neon, Turso, PlanetScale, Cloudflare Hyperdrive, ...
const client = await createSchemalessClient("sql", {
	database: "pg",
	executor: async (sql, params) => {
		const res = await fetch(url, {
			method: "POST",
			headers: { Authorization: `Bearer ${token}` },
			body: JSON.stringify({ sql, params }),
		});
		return { rows: (await res.json()).rows };
	},
});
```

### Switch Between Adapters

```javascript
import { createSchemalessClient } from "umosql/client";

// Use an environment variable to switch backends
const dbType = process.env.DB_TYPE || "memory";
const client = await createSchemalessClient(dbType, {
	host: process.env.DB_HOST,
	database: process.env.DB_NAME,
	user: process.env.DB_USER,
	password: process.env.DB_PASSWORD,
});

// Same code works with any backend
const users = client.db("app").collection("users");
await users.insertOne({ name: "Alice" });
const results = await (await users.find({ age: { $gte: 18 } })).toArray();
await client.close();
```

---

## 🎨 Custom Operators

### Adding Custom Filter Operators

```javascript
import { extend, escape } from "umosql";

// Between operator
extend.filter({
	$between: (value, db, field) => {
		if (!Array.isArray(value) || value.length !== 2) {
			throw new Error("$between requires [min, max]");
		}
		return `BETWEEN ${escape(value[0], db)} AND ${escape(value[1], db)}`;
	},

	// Starts with
	$startsWith: (value, db, field) => {
		return db === "pg"
			? `LIKE ${escape(value + "%", db)}`
			: `LIKE ${escape(value + "%", db)}`;
	},

	// Ends with
	$endsWith: (value, db, field) => {
		return `LIKE ${escape("%" + value, db)}`;
	},
});

// Usage
users.find({ age: { $between: [18, 65] } }).toSQL();
users.find({ email: { $startsWith: "admin" } }).toSQL();
users.find({ email: { $endsWith: "@company.com" } }).toSQL();
```

### Adding Custom Expression Operators

```javascript
extend.expression({
	// Absolute value
	$abs: (args, ctx) => `ABS(${ctx.expr(args[0])})`,

	// Power
	$power: (args, ctx) => {
		if (args.length !== 2) throw new Error("$power requires [base, exponent]");
		return `POWER(${ctx.expr(args[0])}, ${ctx.expr(args[1])})`;
	},

	// Round
	$round: (args, ctx) => `ROUND(${ctx.expr(args[0])})`,

	// Coalesce (null handling)
	$ifNull: (args, ctx) => {
		if (args.length !== 2) throw new Error("$ifNull requires [field, default]");
		return `COALESCE(${ctx.expr(args[0])}, ${ctx.expr(args[1])})`;
	},
});

// Usage in aggregation
orders.aggregate([
	{
		$project: {
			orderId: "$id",
			total: { $abs: { $subtract: ["$amount", "$discount"] } },
			squared: { $power: ["$value", 2] },
			displayName: { $ifNull: ["$nickname", "$name"] },
		},
	},
]);
```

### Adding Custom Update Operators

```javascript
extend.update({
	// Array push (PostgreSQL)
	$push: (fields, db) => {
		if (db !== "pg") throw new Error("$push only for PostgreSQL");
		return Object.entries(fields).map(
			([key, val]) => `${key} = array_append(${key}, ${escape(val, db)})`
		);
	},

	// Array pull (PostgreSQL)
	$pull: (fields, db) => {
		if (db !== "pg") throw new Error("$pull only for PostgreSQL");
		return Object.entries(fields).map(
			([key, val]) => `${key} = array_remove(${key}, ${escape(val, db)})`
		);
	},
});

// Usage
users.updateOne(
	{ id: 1 },
	{
		$push: { tags: "featured" },
	}
);
// UPDATE users SET tags = array_append(tags, 'featured') WHERE id = 1
```
## Operator Support Matrix

### Filter Operators

| Operator | Memory | SQLite | MySQL | PostgreSQL | Notes |
| --- | --- | --- | --- | --- | --- |
| `$eq` | ✅ | ✅ | ✅ | ✅ | `=` |
| `$ne` | ✅ | ✅ | ✅ | ✅ | `!=` / `<>` |
| `$gt` | ✅ | ✅ | ✅ | ✅ | `>` |
| `$gte` | ✅ | ✅ | ✅ | ✅ | `>=` |
| `$lt` | ✅ | ✅ | ✅ | ✅ | `<` |
| `$lte` | ✅ | ✅ | ✅ | ✅ | `<=` |
| `$in` | ✅ | ✅ | ✅ | ✅ | Empty array handled: `= 1 AND 1 = 0` |
| `$nin` | ✅ | ✅ | ✅ | ✅ | Empty array handled: `= 1 OR 1 = 1` |
| `$like` | ✅ | ✅ | ✅ | ✅ | `%` `_` patterns |
| `$ilike` | ✅ | ✅ | ✅ | ✅ | PG: `ILIKE`; others: `LOWER(field) LIKE LOWER(value)` |
| `$nlike` | ✅ | ✅ | ✅ | ✅ | `NOT LIKE` |
| `$nilike` | ✅ | ✅ | ✅ | ✅ | PG: `NOT ILIKE`; others: `NOT LIKE LOWER(...)` |
| `$regex` | ✅ | ✅ | ✅ | ✅ | PG: `~`; MySQL: `REGEXP`; SQLite: anchored patterns rewritten to `LIKE` (`^A` → `'A%'`, `x$` → `'%x'`, else `'%…%'`) |
| `$exists` | ✅ | ✅ | ✅ | ✅ | `IS NULL` / `IS NOT NULL` |
| `$between` | ✅ | ✅ | ✅ | ✅ | `BETWEEN a AND b` |
| `$mod` | ✅ | ✅ | ✅ | ✅ | `field % m = r` |
| `$and` | ✅ | ✅ | ✅ | ✅ | Parenthesized conjunctions |
| `$or` | ✅ | ✅ | ✅ | ✅ | Parenthesized disjunctions |
| `$not` | ✅ | ✅ | ✅ | ✅ | `NOT ( ... )` |
| `$nor` | ✅ | ✅ | ✅ | ✅ | `NOT ( ... OR ... )` |
| `$expr` | ✅ | ✅ | ✅ | ✅ | Embed expression in filter |
| `$type` | ✅ | — | — | — | Memory-only |
| `$elemMatch` | ✅ | — | — | — | Memory-only |
| `$all` | ✅ | — | — | — | Memory-only |
| `$size` | ✅ | — | — | — | Memory-only |
| `$where` | ✅ | — | — | — | Memory-only (JS predicate `function () { … }`, `this` = doc) |

### Expression Operators

| Operator | Memory | SQLite | MySQL | PostgreSQL | Notes |
| --- | --- | --- | --- | --- | --- |
| `$add` | ✅ | ✅ | ✅ | ✅ | `+` |
| `$subtract` | ✅ | ✅ | ✅ | ✅ | `-` |
| `$multiply` | ✅ | ✅ | ✅ | ✅ | `*` |
| `$divide` | ✅ | ✅ | ✅ | ✅ | `/ NULLIF(...,0)` |
| `$mod` | ✅ | ✅ | ✅ | ✅ | `%` |
| `$abs` | ✅ | ✅ | ✅ | ✅ | `ABS()` |
| `$ceil` | ✅ | ✅ | ✅ | ✅ | `CEIL()` |
| `$floor` | ✅ | ✅ | ✅ | ✅ | `FLOOR()` |
| `$round` | ✅ | ✅ | ✅ | ✅ | `ROUND(x, p)` |
| `$pow` | ✅ | ✅ | ✅ | ✅ | `POWER()` |
| `$sqrt` | ✅ | ✅ | ✅ | ✅ | `SQRT()` |
| `$concat` | ✅ | ✅ | ✅ | ✅ | PG/MySQL: `CONCAT`, SQLite: `||` |
| `$upper` | ✅ | ✅ | ✅ | ✅ | `UPPER()` |
| `$lower` | ✅ | ✅ | ✅ | ✅ | `LOWER()` |
| `$substr` | ✅ | ✅ | ✅ | ✅ | `SUBSTRING()` |
| `$trim`/`$ltrim`/`$rtrim` | ✅ | ✅ | ✅ | ✅ | `TRIM` variants |
| `$strLen` | ✅ | ✅ | ✅ | ✅ | `LENGTH()` |
| `$replace` | ✅ | ✅ | ✅ | ✅ | `REPLACE()` |
| `$sum` | ✅ | ✅ | ✅ | ✅ | Aggregates; `$sum: 1` maps to `COUNT(*)` |
| `$avg` | ✅ | ✅ | ✅ | ✅ | `AVG()` |
| `$min` | ✅ | ✅ | ✅ | ✅ | Single: `MIN()`; multi: `LEAST()` |
| `$max` | ✅ | ✅ | ✅ | ✅ | Single: `MAX()`; multi: `GREATEST()` |
| `$count` | ❌ | ✅ | ✅ | ✅ | SQL-only expression (`COUNT(*)`); the `$count` *stage* works on memory |
| `$stdDevPop`/`$stdDevSamp` | ❌ | ✅ | ✅ | ✅ | SQL-only (`STDDEV_POP` / `STDDEV_SAMP`) |
| `$eq`,`$ne`,`$gt`,`$gte`,`$lt`,`$lte` | ✅ | ✅ | ✅ | ✅ | Comparison in expressions |
| `$cmp` | ✅ | ✅ | ✅ | ✅ | Returns -1/0/1 |
| `$in`/`$nin` | ✅ | ✅ | ✅ | ✅ | Expression `IN`/`NOT IN` |
| `$size` (JSON array) | ✅ | ✅ | ✅ | ✅ | PG: `jsonb_array_length`, MySQL: `JSON_LENGTH`, SQLite: `json_array_length` |
| `$and`/`$or`/`$not` | ✅ | ✅ | ✅ | ✅ | Logical composition |
| `$cond` | ✅ | ✅ | ✅ | ✅ | `CASE WHEN ... THEN ... ELSE ... END` |
| `$ifNull` | ✅ | ✅ | ✅ | ✅ | `COALESCE()` |
| `$switch` | ✅ | ✅ | ✅ | ✅ | `CASE` branches |
| `$exists` | ❌ | ✅ | ✅ | ✅ | SQL-only as an expression (`$exists: [field, bool]`); as a *filter* it works everywhere |
| Date parts `$year`,`$month`,`$dayOfMonth`,`$dayOfWeek`,`$hour`,`$minute`,`$second`,`$week` | ✅ | ✅ | ✅ | ✅ | DB-specific functions (`EXTRACT`, `YEAR`, `strftime`) |
| Cast `$toString`,`$toInt`,`$toDouble`,`$toBool`,`$toDate` | ✅ | ✅ | ✅ | ✅ | DB-specific `CAST` |
| `$literal` | ✅ | ✅ | ✅ | ✅ | Escaped literal |
| `$split` | ✅ | — | — | — | Memory-only (`[str, separator]` → array) |
| `$arrayElemAt` | ✅ | — | — | — | Memory-only (`[array, index]`, negatives from end) |
| `$slice` | ✅ | — | — | — | Memory-only (`[array, n]` / `[array, skip, limit]`) |
| `$map` / `$filter` | ✅ | — | — | — | Memory-only (`[array, var, expr]`, `$$var` in expr) |
| `$reduce` | ✅ | — | — | — | Memory-only (`[array, initial, expr]`, `$$value` / `$$this`) |
| `$type` (expression) | ✅ | — | — | — | Memory-only (returns `'string'`/`'number'`/…; filter `$type` also memory-only) |
| `$millisecond` | ✅ | — | — | — | Memory-only (SQL has `$year`…`$week` but no `$millisecond`) |

### Update Operators

| Operator | Memory | SQLite | MySQL | PostgreSQL | Notes |
| --- | --- | --- | --- | --- | --- |
| `$set` | ✅ | ✅ | ✅ | ✅ | Scalar and JSON path updates |
| `$inc` | ✅ | ✅ | ✅ | ✅ | Scalar and JSON numeric JSON path |
| `$mul` | ✅ | ✅ | ✅ | ✅ | Scalar and JSON numeric JSON path |
| `$min` | ✅ | ✅ | ✅ | ✅ | SQLite uses `MIN`, others `LEAST` |
| `$max` | ✅ | ✅ | ✅ | ✅ | SQLite uses `MAX`, others `GREATEST` |
| `$unset` | ✅ | ✅ | ✅ | ✅ | JSON path remove or `NULL` for scalars |
| `$currentDate` | ✅ | ✅ | ✅ | ✅ | PG: `CURRENT_TIMESTAMP`; MySQL: `NOW()`; SQLite: `datetime('now')` |
| `$rename` | ✅ | ✅ | ✅ | ✅ | Non-JSON fields; sets new = old, old = NULL |
| `$push`/`$pull`/`$addToSet` | ✅ | — | — | — | Memory-only array mutations (`$push` supports `$each`/`$slice`/`$sort`/`$position`) |
| `$pullAll` | ✅ | — | — | — | Memory-only (remove every listed value) |
| `$pop` | ✅ | — | — | — | Memory-only (`1` = last, `-1` = first) |
| `$setOnInsert` | ✅ | — | — | — | Memory-only (with `updateOne(q, u, { upsert: true })`) |

### Aggregation Stages

| Stage | Memory | SQLite | MySQL | PostgreSQL | Notes |
| --- | --- | --- | --- | --- | --- |
| `$match` | ✅ | ✅ | ✅ | ✅ | WHERE/HAVING integration |
| `$project` | ✅ | ✅ | ✅ | ✅ | SELECT with expressions |
| `$addFields` / `$set` | ✅ | ✅ | ✅ | ✅ | Adds computed fields |
| `$group` | ✅ | ✅ | ✅ | ✅ | GROUP BY with aggregates |
| `$sort` | ✅ | ✅ | ✅ | ✅ | ORDER BY |
| `$limit` | ✅ | ✅ | ✅ | ✅ | LIMIT |
| `$skip` | ✅ | ✅ | ✅ | ✅ | OFFSET |
| `$count` | ✅ | ✅ | ✅ | ✅ | Aggregates count |
| `$sample` | ❌ | ✅ | ✅ | ✅ | SQL-only (`{ size }` → random `ORDER BY` + `LIMIT`) |
| `$unset` | ✅ | ❌ | ❌ | ❌ | Memory-only (remove fields, string or array) |
| `$sortByCount` | ✅ | ✅ | ✅ | ✅ | GROUP BY expr, order by count desc |
| `$bucket` | ✅ | ✅ | ✅ | ✅ | CASE-based bucketing |
| `$unwind` | ✅ | — | — | — | Memory-only (string or `{ path, preserveNullAndEmptyArrays, includeArrayIndex }`) |
| `$group` accumulators `$push`/`$addToSet`/`$first`/`$last` | ✅ | ❌ | ❌ | ❌ | Memory-only; SQL `$group` uses `$sum`/`$avg`/`$min`/`$max`/`$count` + scalar exprs |

#### Code References

- Filter operators: `src/index.js:196` and `src/adapter/memory/memory.js:74`
- Expression operators: `src/index.js:253` and `src/adapter/memory/memory.js:121`
- Update operators: `src/index.js:426` and `src/adapter/memory/memory.js:311`
- Aggregation stages: `src/index.js:477` and `src/adapter/memory/memory.js:419`
- JSON path extraction: `src/index.js:80`
- JSON updates (`$set`, `$inc`, `$mul`): `src/index.js:98`
- Aggregate builder and stage assembly: `src/index.js:717`

### Compatibility & Testing

- PostgreSQL: tested with 16; native `ILIKE` and regex `~` used.
- MySQL: tested with 8.x; uses `REGEXP`, `LOWER(...) LIKE LOWER(...)` for case-insensitive like.
- SQLite: tested with `better-sqlite3`; `$regex` patterns are rewritten to `LIKE` (`^A` → `LIKE 'A%'`, `x$` → `LIKE '%x'`, `^A$` → `LIKE 'A'`, anything else → `LIKE '%…%'` with `.*` stripped), so complex patterns only approximate a real regex.
- Memory: full operator coverage, including array and pipeline-only stages.
- Unified suite covers filters, expressions, updates, and aggregation across adapters where applicable.

## Schemaless Adapters

- Adapters infer and evolve table schemas automatically for memory, SQLite, PostgreSQL, and MySQL. MongoDB adapter is optional; for drop-in replacement needs against SQL, use the unified adapter.
- Chainable cursor API for `find()` supports `sort`, `skip`, `limit`, and `toArray()` consistently.

### Quick Start

- Default backend is memory when omitted.

```javascript
import { createSchemalessAdapter } from 'umosql/schemaless';

// Memory (default)
const { adapter } = createSchemalessAdapter();
const users = adapter.collection('users');
await users.insertOne({ name: 'Alice' });
console.log(await (await users.find({ name: 'Alice' })).toArray());
```

### Unified Adapter File

- Use a single factory to target SQL backends without per-backend adapter folders.

```javascript
import { createSchemalessAdapter } from 'umosql/schemaless';
import Database from 'better-sqlite3';

// SQLite
const { adapter } = createSchemalessAdapter(new Database(':memory:'), 'sqlite');
const users = adapter.collection('users');
await users.insertOne({ name: 'Alice', profile: { score: 85 } });

// PostgreSQL
import pkg from 'pg';
const { Client } = pkg;
const pg = new Client({ host, port: 5432, user, password, database });
await pg.connect();
const { adapter: pgAdapter } = createSchemalessAdapter(pg, 'pg');

// MySQL
import mysql from 'mysql2/promise';
const conn = await mysql.createConnection({ host, user, password, database });
const { adapter: myAdapter } = createSchemalessAdapter(conn, 'mysql');
```

### MongoDB

- Optional backend with the same collection API shape.
 - Configure via an options object (host, port, user, password, database) according to your environment. No fixed environment variable names are required.
- Example usage:

```javascript
import { createSchemalessClient } from 'umosql/client';

const client = await createSchemalessClient('mongodb', { host: 'localhost', port: 27017, database: 'test' });
const users = client.db('test').collection('users');
await users.insertOne({ name: 'Alice' });
const rows = await (await users.find({ name: 'Alice' })).toArray();
await client.close();
```

The raw adapter factory is re-exported from `umosql/client` if you prefer it:

```javascript
import { createMongoSchemaless } from 'umosql/client';

const { adapter, client } = await createMongoSchemaless({ host: 'localhost', database: 'test' });
```

### Serverless Examples (Drizzle)

- Neon: `examples/serverless-neon-drizzle.js` (requires `NEON_HTTP_URL`)
- Turso: `examples/serverless-turso-drizzle.js` (requires `TURSO_HTTP_URL`, `TURSO_TOKEN`)
- PlanetScale: `examples/serverless-planetscale.js` (requires `PSCALE_DATA_API_URL`, `PSCALE_TOKEN`)
- Runnable tour of every backend: `examples/client.js` (`node examples/client.js`)

### Auto ID Creation

- Strategies
  - `auto`: numeric autoincrement (SQL default per backend)
  - `mongo`: 24-character hex string (ObjectId-like)
  - `custom`: supply `idGenerator()`

- Defaults
  - Default id column is `_id`
  - Non-`auto` strategies generate ids when absent on insert

- Configure per database/collection

```javascript
import { createSchemalessClient } from 'umosql/client';

const client = await createSchemalessClient('pg', { host, port, user, password, database });
const db = client.db('test_database', { id: '_id', idStrategy: 'mongo' });
const users = db.collection('users');
await users.insertOne({ name: 'Alice' });
```

```javascript
import { createSchemalessClient } from 'umosql/client';

const client = await createSchemalessClient('sql', {
  database: 'sqlite',
  executor: async (sql, params) => {}
});
const db = client.db('mydb', { id: '_id', idStrategy: 'custom', idGenerator: () => crypto.randomUUID() });
const events = db.collection('events');
await events.insertOne({ type: 'click' });
```

Passing `idGenerator` without `idStrategy` implies `custom` — under `auto` adapters would ignore it.

### Serverless Behavior

- On missing table/column errors, adapters perform idempotent DDL (`CREATE TABLE IF NOT EXISTS`, `ALTER TABLE ... ADD COLUMN`) and retry once.
- Updates can optionally introduce new columns via `$set`; toggle with `migrateOnUpdate` in `collection(name, { migrateOnUpdate: false })`.
## MongoDB-Compatible Methods Support Matrix

| Method | Memory | SQLite | MySQL | PostgreSQL |
| --- | --- | --- | --- | --- |
| `insertOne` | ✅ | ✅ | ✅ | ✅ |
| `insertMany` | ✅ | ✅ | ✅ | ✅ |
| `find` | ✅ | ✅ | ✅ | ✅ |
| `findOne` | ✅ | ✅ | ✅ | ✅ |
| `findMany` (SQL-only helper) | ❌ | ✅ | ✅ | ✅ |
| `sort/skip/limit` in `find()` | ✅ | ✅ | ✅ | ✅ |
| `updateOne` | ✅ | ✅ | ✅ | ✅ |
| `updateMany` | ✅ | ✅ | ✅ | ✅ |
| `upsertOne` | ❌ | ✅ | ✅ | ✅ |
| `deleteOne` | ✅ | ✅ | ✅ | ✅ |
| `deleteMany` | ✅ | ✅ | ✅ | ✅ |
| `countDocuments` | ✅ | ✅ | ✅ | ✅ |
| `estimatedDocumentCount` | ✅ | ✅ | ✅ | ✅ |
| `distinct` | ✅ | ✅ | ✅ | ✅ |
| `aggregate` | ✅ | ✅ | ✅ | ✅ |
| `createIndex` | ✅ | ✅ | ✅ | ✅ |
| `dropIndex` | ✅ | ✅ | ✅ | ✅ |
| `dropColumn` | ❌ | ❌ | ✅ | ✅ |
| `listCollections` | ✅ | ✅ | ✅ | ✅ |
| `dropCollection` | ✅ | ✅ | ✅ | ✅ |
| `createTableWithSchema` | ❌ | ✅ | ✅ | ✅ |
| `getTableSchema` | ❌ | ✅ | ✅ | ✅ |
| `addColumn` | ❌ | ✅ | ✅ | ✅ |
| `renameColumn` | ❌ | ✅ | ✅ | ✅ |
| `modifyColumn` | ❌ | ❌ | ✅ | ✅ |

Notes:
- Memory adapter is a drop-in for core CRUD, query, and aggregation. Administrative DDL is SQL-only.
- `findMany` is a convenience on SQL adapters for pagination plus total count.
- Memory has no `upsertOne` method — use `updateOne(query, update, { upsert: true })` (plus `$setOnInsert`) or `replaceOne(query, doc, { upsert: true })` on the memory engine.
- The memory engine additionally exposes `findById`, `replaceOne`, `drop`, `getAll`, `size`, `stats`, `dropDatabase` and the standalone `project(doc, projection)` helper (see [In-memory engine](#-in-memory-engine-reference-umosqlmemory)).

## Caveats and Differences

- JSON storage and operators
  - PostgreSQL uses `JSONB` and `jsonb_set`/`#>>` for path reads/writes.
  - MySQL uses `JSON` with `JSON_EXTRACT` and `JSON_SET`.
  - SQLite stores JSON as `TEXT` and uses `json_extract` (requires `json1` extension).
- Boolean values
  - PostgreSQL uses `TRUE/FALSE`.
  - MySQL/SQLite often represent booleans as `TINYINT(1)`/`INTEGER` (1/0) at the SQL level.
- Column management
  - `DROP COLUMN` is not supported by SQLite.
  - `MODIFY COLUMN` is not supported by SQLite; use `ALTER TABLE ... RENAME COLUMN` or recreate.
- Upsert semantics
  - SQL `upsertOne` is implemented as update-then-insert and may not be atomic; add unique constraints to ensure correctness.
- Indexes
  - PostgreSQL supports index types (`USING BTREE`, etc.); others are simpler. The adapter uses sensible defaults.
- Pagination totals
  - `findMany({ includeTotal: true })` performs an additional `COUNT(*)` query.
- Transactions
  - Adapters do not expose transaction helpers; use your client directly if needed.

## Drop-in Replacement Scope

- Implemented as MongoDB-like collection methods across backends:
  - CRUD: `insertOne`, `insertMany`, `find`, `findOne`, `updateOne`, `updateMany`, `deleteOne`, `deleteMany`.
  - Query helpers: `countDocuments`, `distinct`, `aggregate`, projection and computed fields via `$project`, `$addFields`, `$set`.
  - SQL-only helpers: `findMany`, `createTableWithSchema`, `addColumn`, `renameColumn`, `modifyColumn`, `getTableSchema`, `listCollections`.
- For pure MongoDB replacement needs (without DDL), memory and SQL adapters are compatible at the collection method level.
---

## 🧱 SQL Adapter Reference (`createSQLAdapter`)

`createSQLAdapter` is the engine under every SQL backend: `createSchemalessAdapter`
(better-sqlite3 / `pg` / `mysql2`), `createSchemalessClient('sql', …)` (serverless HTTP
executors), and the runnable `examples/serverless-*.js`. Use it directly whenever you have
*any* function that can run SQL.

### Minimal setup

```javascript
import { createSQLAdapter } from "umosql/schemaless";
import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers } from "umosql";

const qb = createQueryBuilder({ filterOps, exprOps, updateOps, stageHandlers });

const adapter = createSQLAdapter({
	database: "pg", // 'pg' | 'mysql' | 'sqlite'
	execute: async (sql, params) => {
		const res = await pool.query(sql, params);
		return { rows: res.rows, rowCount: res.rowCount };
	},
	queryBuilder: qb, // required
});

const users = adapter.collection("users");
await users.insertOne({ name: "Alice", age: 25 });
console.log(await users.findOne({ name: "Alice" }));
```

### The `execute(sql, params)` contract

Your function receives the generated SQL string (values are inlined; `params` is reserved)
and may return any of these shapes — they are normalized internally:

| Return shape | Example source |
| ------------ | -------------- |
| `{ rows, rowCount? }` | `pg`, Neon, any `{ rows }` HTTP API |
| `[rows, fields]` tuple | `mysql2/promise` `.execute()` |
| `{ affectedRows, insertId? }` | `mysql2` writes |
| `{ changes, lastInsertRowid }` | better-sqlite3 `.run()` |
| plain array of rows | Turso/libsql-style clients |

After normalization the adapter reads `rows`, `rowCount`/`affectedRows`/`changes`,
`insertId`/`lastInsertRowid`, and `fields`. DDL statements may return anything.

### Raw SQL, introspection, and SQL builders on the adapter

```javascript
// Run anything yourself (still normalized)
await adapter.execute("DELETE FROM users WHERE age < 18");

// Does the table exist? (information_schema / sqlite_master)
await adapter.tableExists("users"); // true / false

// Live column map, e.g. { name: 'TEXT', age: 'INTEGER' }
// (_id and created_*/updated_* bookkeeping columns are excluded;
// types are upper-cased, MySQL lengths stripped: 'varchar' -> 'VARCHAR')
await adapter.getTableSchema("users"); // { columns: { ... } }

// Render SQL without running it - same strings the adapter executes
adapter.buildInsert("users", [{ name: "Al" }]);
adapter.buildFind("users", { age: { $gte: 18 } }, ["name"]);
adapter.buildFindWithOptions("users", {}, null, { sort: { age: -1 }, limit: 10, skip: 5 });
adapter.buildUpdateOne("users", { name: "Al" }, { $inc: { n: 1 } });
adapter.buildUpdateMany("users", { active: true }, { $set: { v: 1 } });
adapter.buildDeleteOne("users", { name: "Al" });
adapter.buildDeleteMany("users", { active: false }); // allowDeleteAll: true inside
adapter.buildCount("users", { age: { $gte: 18 } });
adapter.buildDistinct("users", "city", { active: true });
adapter.buildAggregate("users", [{ $group: { _id: "$city", n: { $sum: 1 } } }]);
```

### Collection options: `schema`, ids, `migrateOnUpdate`

```javascript
const users = adapter.collection("users", {
	// Typed columns. String form = bare type; object form adds constraints.
	schema: {
		email: { type: "VARCHAR(255)", required: true, unique: true },
		age: "INT",
		role: { type: "VARCHAR(32)", default: "user" },
		password: { type: "TEXT", hidden: true }, // stripped from every read
		serialNo: "SERIAL", // pg: NULLs dropped on insert so the sequence applies
	},
	migrateOnUpdate: true, // $set/$rename may ADD COLUMN on update (default true)
	idColumn: "_id", // custom id column name
	idStrategy: "auto", // 'auto' | 'mongo' | 'custom'
	idGenerator: () => crypto.randomUUID(), // required for 'custom'
});
```

What each piece does:

- `required: true` → `NOT NULL`, `unique: true` → `UNIQUE` in the generated DDL.
- `default: value | () => value` → a SQL `DEFAULT` in DDL **and** a client-side default:
  missing fields are filled before insert (functions run per document).
- `hidden: true` → the field is stored but deleted from every returned document.
- `SERIAL` (pg) / `AUTO_INCREMENT` (mysql) schema types: explicit `null` values are
  stripped on insert so the sequence default applies.

### Type inference (schemaless inserts)

Values with no schema entry get a column type from this table:

| JS value | PostgreSQL | MySQL | SQLite |
| -------- | ---------- | ----- | ------ |
| string ≤ 255 chars | `VARCHAR(255)` | `VARCHAR(255)` | `TEXT` |
| string > 255 chars | `TEXT` | `TEXT` | `TEXT` |
| integer | `INTEGER` | `INT` | `INTEGER` |
| float | `DECIMAL(20,6)` | `DECIMAL(20,6)` | `REAL` |
| boolean | `BOOLEAN` | `TINYINT(1)` | `INTEGER` |
| Date | `TIMESTAMP` | `DATETIME` | `TEXT` |
| object / array | `JSONB` | `JSON` | `TEXT` |
| null / undefined | `VARCHAR(255)` | `VARCHAR(255)` | `TEXT` |

If a later document needs a *wider* type, the column is widened automatically
(`INT → DECIMAL → VARCHAR → TEXT` ranks; pg uses `ALTER COLUMN … TYPE`, mysql uses
`MODIFY COLUMN`, sqlite keeps the original type since it is dynamically typed).

### Lifecycle: tables and columns create themselves

1. First touch calls `initialize()` → `tableExists()` + `getTableSchema()`.
2. Missing table → `CREATE TABLE IF NOT EXISTS` with just `_id` + `created_at` /
   `updated_at`; every other column is added with `ALTER TABLE … ADD COLUMN`.
3. Every write runs `migrate(doc)`: unknown keys become new columns, narrow columns widen.
4. If a statement still fails with *missing table/column*, it is classified by error
   code and retried once after creating/migrating (`42P01`/`42703` on pg, `1146`/`1054`
   on mysql, `no such table/column` on sqlite). Duplicate-column races are swallowed.

`_id` per strategy: `auto` → `SERIAL` / `INT AUTO_INCREMENT` / `INTEGER AUTOINCREMENT`
primary key; `mongo`/`custom` → `TEXT` / `VARCHAR(24)` / `TEXT` primary key, generated
client-side (24-char hex for `mongo`). `insertedId` comes from `RETURNING` (pg),
`insertId` (mysql), `lastInsertRowid` (sqlite), or the generated id.

MySQL identifiers that collide with reserved words are backtick-quoted automatically
(a column named `longText` is emitted as `` `longText` `` in DDL *and* DML); ordinary
identifiers are left untouched.

### `find()` cursor and options

`find()` is async and returns a chainable cursor — `await` it first:

```javascript
const cursor = await users.find(
	{ age: { $gte: 18 } }, // filter
	["name", "age"], // projection (object / array / raw string)
	{ sort: { age: -1 }, skip: 20, limit: 10 } // or { order, offset, select, distinct }
);
const rows = await cursor.sort({ name: 1 }).limit(5).toArray();
const n = await cursor.count(); // runs the query, returns row count
```

`findOne(query, projection)` returns the first row or `null`. `distinct(field, query)`
returns the raw values (`["Paris", "London"]`).

### `findMany()` — pagination in one call

```javascript
const { items, total, page, pageSize } = await users.findMany({
	filter: { active: true },
	select: ["name", "age"], // or `projection`
	sort: { age: -1 }, // or `order`
	page: 2, // 1-based; computed as OFFSET (page - 1) * pageSize
	pageSize: 10, // or `limit`; raw `skip`/`offset` also accepted
	distinct: false,
	includeTotal: true, // extra COUNT(*) query -> `total`
});
```

### `upsertOne(query, update, insertDoc?)`

Update-then-insert (not atomic — add a unique constraint for correctness):

```javascript
// If nobody named Henry exists, inserts { name: 'Henry', age: 45, city: 'Rome' }
await users.upsertOne({ name: "Henry" }, { $set: { age: 45 } }, { city: "Rome" });
// { acknowledged: true, upserted: true, insertedId: 12 }
```

Scalar equality parts of the query are merged into the inserted document automatically.

### Index helpers

```javascript
await users.createIndex("email", { unique: true }); // idx_users_email
await users.createIndex("email", { name: "by_email", unique: true });
await users.createIndex("location", { type: "gist" }); // pg only: USING GIST
await adapter.createIndex("users", "email", { unique: true }); // adapter-level twin
await adapter.dropIndex("users", "by_email");
```

Defaults: `CREATE [UNIQUE] INDEX IF NOT EXISTS <name> ON <table> [USING <type>] (<field>)`
on pg/sqlite (mysql checks `SHOW INDEX` first since it lacks `IF NOT EXISTS`).

`dropColumn` works on pg/mysql only (`adapter.dropColumn(table, col)` delegates to it);
sqlite throws — see [Caveats](#caveats-and-differences).

### `createTableWithSchema(table, jsonSchema)`

Accepts a JSON-Schema-ish `{ properties, required }` (with `default` passthrough) or an
already-mapped `{ col: { type, required, default } }` object. Property mapping:

| JSON Schema | PostgreSQL | MySQL | SQLite |
| ----------- | ---------- | ----- | ------ |
| `string` | `TEXT` | `VARCHAR(255)` | `TEXT` |
| `string` + `maxLength: N` | `VARCHAR(N)` | `VARCHAR(N)` | `TEXT` |
| `string` + `format: date-time` | `TIMESTAMP` | `DATETIME` | `TEXT` |
| `integer` | `INTEGER` | `INT` | `INTEGER` |
| `number` | `DECIMAL(20,6)` | `DECIMAL(20,6)` | `REAL` |
| `boolean` | `BOOLEAN` | `TINYINT(1)` | `INTEGER` |
| `array` / `object` | `JSONB` | `JSON` | `TEXT` |

```javascript
await adapter.createTableWithSchema("users", {
	properties: {
		name: { type: "string" },
		age: { type: "integer" },
		created: { type: "string", format: "date-time" },
		active: { type: "boolean", default: true },
		meta: { type: "object", default: {} },
	},
	required: ["name"],
});
// pg emits:    CREATE TABLE IF NOT EXISTS users (_id SERIAL PRIMARY KEY, ...)
//              ALTER TABLE users ADD COLUMN name TEXT NOT NULL
//              ALTER TABLE users ADD COLUMN age INTEGER
//              ALTER TABLE users ADD COLUMN created TIMESTAMP
//              ALTER TABLE users ADD COLUMN active BOOLEAN DEFAULT TRUE
//              ALTER TABLE users ADD COLUMN meta JSONB DEFAULT '{}'
```

### DDL helpers

```javascript
await adapter.addColumn("users", "nickname", "VARCHAR(64)");
await adapter.addColumn("users", "score", "INT", { default: 0 });
await adapter.addColumn("users", "email", "TEXT", { required: true, unique: true });
await adapter.renameColumn("users", "nickname", "handle");
await adapter.modifyColumn("users", "score", "BIGINT"); // pg/mysql only
await adapter.dropCollection("users"); // pg appends CASCADE
await adapter.listCollections(); // ["users", "orders"]
```

Pass `createSchemalessAdapter(client, db, { debug: true })` (or `debug` on
`createQueryBuilder` / the `'sql'` client) to log every generated statement.

---

## 🧠 In-Memory Engine Reference (`umosql/memory`)

Fully synchronous — no promises, no SQL, no driver. Ideal for tests, caches, edge
runtimes, and offline-first stores. Two layers: the **engine** (direct classes and
functions) and the **schemaless adapter** (async-compatible wrapper used by
`createSchemalessClient('memory')`).

```javascript
import { collection, db, Database, Collection } from "umosql/memory";

// A collection with seed data (synchronous - results are returned directly)
const users = collection("users", [
	{ _id: 1, name: "Alice", age: 25, tags: ["x", "y"] },
	{ _id: 2, name: "Bob", age: 30, tags: ["y"] },
]);

users.find({ age: { $in: [25, 30] } }).toArray(); // both docs
// NOTE: memory $in/$eq compare whole values - match array ELEMENTS with $elemMatch:
users.find({ tags: { $elemMatch: { $eq: "x" } } }).toArray(); // [{ Alice… }]
users.findOne({ name: "Bob" }); // { Bob… } (or null)
users.findById(1); // sugar for findOne({ _id: id })
users.updateOne({ name: "Bob" }, { $inc: { age: 1 } });
users.updateMany({ age: { $gte: 18 } }, { $set: { adult: true } });
users.deleteOne({ name: "Bob" });
users.countDocuments({ age: { $gte: 18 } }); // number
users.estimatedDocumentCount(); // collection size
users.distinct("age"); // [25, 30]
users.aggregate([{ $group: { _id: "$age", n: { $sum: 1 } } }]);

// Named databases isolate collections; seed data/options apply on first use
const app = db("app");
const logs = app.collection("logs", [], { idStrategy: "mongo" });
app.listCollections(); // ["logs"]
app.stats("logs"); // { name, count, size, avgObjSize }
app.dropCollection("logs"); // true
app.dropDatabase(); // { acknowledged: true }
```

### FindQuery, options, and `project()`

```javascript
users
	.find({ age: { $gte: 18 } }, null, { sort: { age: -1 }, skip: 5, limit: 10 })
	.sort({ name: 1 })
	.skip(0)
	.limit(5)
	.distinct() // dedupe whole documents
	.count() // toArray() then returns [{ count: n }]
	.toArray();
```

Note: the memory `find()` projection argument is accepted but **not applied** — use the
standalone `project(doc, projection)` (supports inclusion, exclusion, and computed
fields) or an aggregate `$project` stage:

```javascript
import { project } from "umosql/memory";
project({ name: "Bob", age: 30, _id: 2 }, { name: 1 }); // { name: 'Bob', _id: 2 }
```

`find()` returns live references to stored documents; `updateOne`/`updateMany` clone
before writing, so concurrent reads never see half-applied updates.

### Curried `filter` / `expression` / `aggregate`

```javascript
import { filter, expression, aggregate } from "umosql/memory";

const isAdult = filter({ age: { $gte: 18 } }); // (doc) => boolean
users.getAll().filter(isAdult);

const nextAge = expression({ $add: ["$age", 1] }); // (doc) => value
nextAge({ age: 30 }); // 31

const pipeline = aggregate([{ $match: { age: { $gte: 18 } } }, { $count: "n" }]);
pipeline(users.getAll()); // [{ n: 2 }]
```

### Memory-only operators

Filters — `$type`, `$elemMatch`, `$all`, `$size`, `$where`:

```javascript
users.find({ age: { $type: "number" } }); // string|number|boolean|array|object|null|…
users.find({ tags: { $elemMatch: { $eq: "x" } } }); // any element matches
users.find({ tags: { $all: ["x", "y"] } }); // contains every value
users.find({ tags: { $size: 2 } }); // array length
users.find({ $where: function () { return this.age > 26; } }); // `this` = doc
```

Updates — `$push` modifiers, `$pull`/`$pullAll`, `$addToSet`, `$pop`, `$setOnInsert`:

```javascript
users.updateOne({ name: "Bob" }, { $push: { tags: "z" } });
users.updateOne({ name: "Bob" }, { $push: { scores: { $each: [1, 2], $sort: -1, $slice: 5 } } });
users.updateOne({ name: "Bob" }, { $addToSet: { tags: { $each: ["y", "z"] } } });
users.updateOne({ name: "Al" }, { $pull: { tags: "x" } }); // or a filter object
users.updateOne({ name: "Al" }, { $pullAll: { tags: ["x", "y"] } });
users.updateOne({ name: "Al" }, { $pop: { tags: 1 } }); // 1 = last, -1 = first
// $position inserts $each items at an index: { $each: [...], $position: 0 }

// Upsert flavour: plain updateOne with { upsert: true } (+ $setOnInsert)
users.updateOne({ name: "New" }, { $setOnInsert: { age: 9 } }, { upsert: true });
// { modifiedCount: 0, upsertedCount: 1, acknowledged: true }
users.replaceOne({ name: "Al" }, { name: "Al", age: 26 }); // keeps _id
```

Expressions — `$split`, `$arrayElemAt`, `$slice`, `$map`, `$filter`, `$reduce`, `$type`, `$millisecond`:

```javascript
users.aggregate([{ $project: { up: { $map: ["$tags", "t", { $upper: "$$t" }] } } }]);
// [{ up: ['X', 'Y'], _id: 1 }, …]   ($$t reads the bound variable)
users.aggregate([{ $project: { first: { $arrayElemAt: ["$tags", 0] } } }]);
users.aggregate([{ $project: { parts: { $split: ["$name", ""] } } }]);
```

Stages — `$unwind` options, extra `$group`/`$bucket` accumulators, `$unset`:

```javascript
users.aggregate([{ $unwind: "$tags" }]); // one doc per element
users.aggregate([
	{ $unwind: { path: "$tags", preserveNullAndEmptyArrays: true, includeArrayIndex: "i" } },
]);
users.aggregate([{ $group: { _id: "$age", names: { $push: "$name" }, first: { $first: "$name" } } }]);
// $group/$bucket accumulators: $sum $avg $min $max $push $addToSet $first $last ($count too in $bucket)
users.aggregate([{ $unset: "password" }]); // or ["a", "b"]
```

### Custom engines and path utilities

```javascript
import {
	createMemoryDB,
	filterOps,
	exprOps,
	updateOps,
	stageOps,
	extend,
	deepEquals,
	getPath,
	setPath,
	deletePath,
	clone,
} from "umosql/memory";

// Your own engine with hand-picked operators (same idea as createQueryBuilder)
const mini = createMemoryDB({ filterOps, exprOps, updateOps, stageOps });
mini.collection("t").insertOne({ a: 1 });

// Or extend the shared one: filter / expression / update / stage
extend.filter({ $even: (q, v) => v % 2 === 0 });

getPath({ a: { b: 1 } }, "a.b"); // 1 (array paths accepted too)
const o = {};
setPath(o, "a.0.b", 5); // { a: [{ b: 5 }] } (numeric segments make arrays)
deletePath(o, "a.0.b"); // { a: [{}] }
clone(new Date(0)) instanceof Date; // true (Date/Array/Object aware)
deepEquals({ a: 1 }, { a: 1 }); // true
```

`collection(name, initData?, { idColumn?, idStrategy?, idGenerator? })` honors
`auto` (numeric counter, continues past seeded numeric `_id`s), `mongo` (24-char hex),
and `custom` (`idGenerator()`; without one the id stays `undefined`).
`drop()` empties a collection and resets its counter; `getAll()`/`size()` inspect it.

The adapter wrapper — `createMemorySchemaless()` from `umosql/memory`,
`umosql/schemaless`, or `umosql/client` — returns `{ adapter, database }` where
`database` is a `Database('unified')`. Its `createIndex`/`dropIndex` simply acknowledge;
there is no `upsertOne`/`findMany`/DDL (see the [methods matrix](#mongodb-compatible-methods-support-matrix)).

---

## 🍃 MongoDB Adapter Reference

A thin native-driver pass-through — queries run through the real `mongodb` driver, so
Mongo-only features (`$regex` with options, `ObjectId`, …) behave exactly like MongoDB.
No DDL translation happens: `$rename`/`$currentDate`/array operators are sent natively.

```javascript
import { createMongoSchemaless } from "umosql/client"; // also via createSchemalessClient('mongodb', …)

const { adapter, client } = await createMongoSchemaless({
	host: "localhost", // default 'localhost' (+ MONGO_HOST/MONGO_PORT/MONGO_USER/…)
	port: 27017,
	user: "root",
	password: "secret",
	database: "testdb",
	driverOptions: {}, // forwarded to `new MongoClient(uri, { serverSelectionTimeoutMS: 5000, … })`
	// driver: mongoModule,  // inject instead of importing 'mongodb'
	// client: mongoClient,  // adopt instead of connecting
});

const users = adapter.collection("users");
await users.insertOne({ name: "Alice", profile: { score: 85 } });
// { acknowledged: true, insertedId: ObjectId(...) }
await users.updateOne({ name: "Alice" }, { $inc: { "profile.score": 5 } });
// { acknowledged, matchedCount, modifiedCount, upsertedId, upsertedCount }
await users.upsertOne({ name: "Bob" }, { $set: { age: 30 } }); // native upsert: true
const cursor = users.find({ age: { $gte: 18 } }, null, { sort: { age: -1 }, limit: 5 });
await cursor.sort({ name: 1 }).skip(0).limit(10).toArray();
await users.createIndex("email", { unique: true });
await adapter.listCollections(); // ["users", …]
await adapter.dropCollection("users");
await client.close();
```

Collection methods: `insertOne`/`insertMany`, `find` (sync cursor with
`sort`/`skip`/`limit`/`toArray`/`count`), `findOne`, `updateOne`/`updateMany`,
`upsertOne(filter, update)` (native `{ upsert: true }`, returns
`{ acknowledged, upserted, upsertedId }`), `deleteOne`/`deleteMany`,
`countDocuments`, `estimatedDocumentCount`, `distinct`, `aggregate` (native pipeline),
`createIndex(field, options)`. Adapter-level: `listCollections`, `dropCollection`,
`createIndex(table, field, options)`. Connection failures surface after the
5s `serverSelectionTimeoutMS` (override via `driverOptions`).

---

## 🛠 Builder Utilities & Standalone Functions (`umosql`)

Everything below is exported from the root entry point (and mirrored by `lite`/`tiny`
with their smaller op sets). The builder only produces SQL strings — pair it with any
driver, or hand it to `createSQLAdapter` as `queryBuilder`.

```javascript
import {
	collection, // (name, db?) -> CollectionApi (also the default export)
	db, // (name, db?) -> CollectionApi, alias of collection
	filter, // (query, db?) -> WHERE fragment
	expression, // (expr, db?) -> SQL expression
	aggregate, // (pipeline) -> (table, db?) -> SELECT (curried!)
	insertMany, // (table, docs, db?, opts?) -> INSERT
	updateMany, // (table, query, update, db?, opts?) -> UPDATE
	deleteMany, // (table, query, db?, opts?) -> DELETE
	FindQuery, // class: new FindQuery(table, query, projection?, db?)
	extend, // { filter, expression, update, stage }
	createQueryBuilder, // (config) -> custom builder (see below)
	filterOps, // the 16 SQL filter operators (shareable / pickable)
	exprOps, // the 58 SQL expression operators
	updateOps, // the 8 SQL update operators
	stageHandlers, // the 12 SQL pipeline stages
	escape, // (value, db?) -> SQL literal
	jsonPath, // (path, db?, cast?) -> JSON read expression
	validate, // { col, alias, arr, int }
	isObject, // typeof obj === 'object' && non-null && non-array
	is$, // string starts with '$'
} from "umosql";
```

### Standalone fragments (no collection needed)

```javascript
filter({ age: { $gte: 18 }, status: "active" } }, "pg");
// age >= 18 AND status = 'active'

expression({ $add: ["$price", "$tax"] }, "sqlite");
// (price + tax)

aggregate([{ $match: { active: true } }, { $count: "n" }])("users", "pg");
// SELECT COUNT(*) AS n FROM (SELECT * FROM users WHERE active = TRUE) AS t1

insertMany("users", [{ name: "Al" }], "sqlite");
// INSERT INTO users (name) VALUES ('Al')

updateMany("users", { active: false }, { $set: { v: 1 } }, "pg");
// UPDATE users SET v = 1 WHERE active = FALSE

deleteMany("users", { active: false }, "pg");
// DELETE FROM users WHERE active = FALSE

db("users", "pg").findOne({ id: 1 }).toSQL();
// SELECT * FROM users WHERE id = 1 LIMIT 1

// FindQuery directly (table must already be a valid identifier)
new FindQuery("users", { age: { $gte: 18 } }, ["name"], "pg")
	.sort({ age: -1 })
	.limit(5)
	.toSQL();
// SELECT name FROM users WHERE age >= 18 ORDER BY age DESC NULLS LAST LIMIT 5
```

`opts` on insert/update/delete: `{ returning: ["id", …] | "*" }` (pg only) and
`{ allowDeleteAll: true }` for filter-less `deleteMany` (otherwise it throws).

### `escape(value, db?)` — SQL literals per dialect

```javascript
escape("o'clock", "pg"); // 'o''clock' (quotes doubled)
escape(true, "pg"); // TRUE      |  escape(true, "mysql"); // 1
escape(new Date("2024-01-02T03:04:05Z"), "mysql"); // '2024-01-02 03:04:05' (UTC)
escape(new Date("2024-01-02T03:04:05Z"), "pg"); // '…T…Z'::timestamp
escape({ a: 1 }, "pg"); // '{"a":1}'::jsonb
escape(null); // NULL
escape(undefined); // throws 'Cannot escape undefined'
escape(NaN); // throws 'Cannot escape non-finite number'
```

### `jsonPath(path, db?, cast?)` — JSON reads per dialect

```javascript
jsonPath("profile.score", "pg"); // (profile::jsonb #>> '{score}')
jsonPath("profile.score", "pg", "numeric"); // ((profile::jsonb #>> '{score}'))::numeric
jsonPath("profile.score", "mysql"); // JSON_UNQUOTE(JSON_EXTRACT(profile, '$.score'))
jsonPath("profile.score", "mysql", "numeric"); // CAST(… AS DECIMAL(20,6))
jsonPath("profile.score", "sqlite"); // json_extract(profile, '$.score')
// casts: 'numeric' | 'int' | 'boolean' (pg); 'numeric' | 'int' (mysql)
```

Filters apply casts automatically for number/boolean comparisons; dotted segments that
are numeric become array indexes (`orders.0.status` → `'$[0].status'`-style paths).

### `validate` — the guards behind every builder

```javascript
validate.col("users", "mysql"); // 'users' (reserved words get backticks: `longText`)
validate.col("a;b", "pg"); // throws 'Invalid column: a;b'
validate.alias("a-b c"); // 'a_b_c' (non-word chars -> _)
validate.arr([1], "$in"); // returns it; non-arrays throw '$in requires array'
validate.int("5", "$limit"); // 5; negatives/fractions throw
```

### `extend` — custom operators (all four kinds)

The README's [Custom Operators](#-custom-operators) section shows filter/expression/update
extensions; `extend.stage` adds pipeline stages the same way:

```javascript
import { extend } from "umosql";

// $limitOffset: { limit, offset } in one stage
extend.stage({
	$limitOffset: (a, s) => {
		s.limit = ` LIMIT ${a.limit}`;
		s.offset = ` OFFSET ${a.offset}`;
	},
});
collection("users", "pg").aggregate([{ $limitOffset: { limit: 5, offset: 10 } }]);
// SELECT * FROM users LIMIT 5 OFFSET 10
```

Each stage handler receives `(args, state, db, helpers)` where `helpers` exposes
`{ wrap, applyWhere, replace, filter, expr }` — see `stageHandlers` in `src/index.js`.
Note `extend` mutates the shared op maps, so custom operators are global to the process.

### `createQueryBuilder(config)` — custom builds

```javascript
import { createQueryBuilder, filterOps, exprOps, updateOps } from "umosql";

const custom = createQueryBuilder({
	filterOps: { $eq: filterOps.$eq, $in: filterOps.$in }, // only what you ship
	exprOps: { $add: exprOps.$add },
	updateOps: { $set: updateOps.$set },
	stageHandlers: {}, // no aggregation at all
	debug: true, // log every generated statement
});
export const { collection, filter } = custom;
```

`lite` and `tiny` are exactly this pattern, prebuilt.

---

## 📦 Lite & Tiny Operator Lists

Precise sets (structural pieces — `$and`/`$or`/`$nor`/`$not`/`$expr`, bare equality,
dotted paths, sorting — work identically in all three):

**Lite** (`umosql/lite`) — every filter op, every update op, basic stages, 42 expression ops:

- Expressions kept: `$add $subtract $multiply $divide $mod $abs $ceil $floor $round
  $pow $sqrt $concat $upper $lower $substr $trim $ltrim $rtrim $strLen $replace $eq $ne
  $gt $gte $lt $lte $in $nin $and $or $not $cond $ifNull $exists $toString $toInt
  $toDouble $sum $avg $min $max $count`
- Expressions dropped: `$cmp $size $stdDevPop $stdDevSamp $switch`, all date parts
  (`$year $month $dayOfMonth $dayOfWeek $hour $minute $second $week`), `$toBool $toDate`,
  `$literal`
- Stages kept: `$match $project $group $sort $limit $skip $count` (no `$addFields`/`$set`,
  `$sample`, `$sortByCount`, `$bucket`)

**Tiny** (`umosql/tiny`) — minimal everything, dotted JSON paths still resolve:

- Filters: `$eq $ne $gt $gte $lt $lte $in $nin $like $exists`
- Expressions: `$add $subtract $multiply $divide $concat $upper $lower $eq $cond $sum
  $avg $min $max`
- Updates: `$set` only (including dotted `$set: { "profile.score": 95 }`)
- Stages: `$match $project $group $sort $limit $skip $count`

```javascript
import tiny from "umosql/tiny";
import lite from "umosql/lite";

tiny.filter({ age: { $eq: 25 } }, "sqlite"); // age = 25
tiny.filter({ "profile.score": { $gte: 80 } }, "pg"); // ((profile::jsonb #>> '{score}'))::numeric >= 80
lite.filter({ "profile.country": "FR" }, "pg");
lite.collection("o", "pg").aggregate([{ $group: { _id: "$city", n: { $sum: 1 } } }]);
```

---

## ⚠️ Backend Quirks & Differences

Beyond [Caveats](#caveats-and-differences), these sharp edges are worth knowing up front:

- **PostgreSQL lower-cases unquoted aliases.** `aggregate([{ $group: { _id: null, avgAge: { $avg: "$age" } } }])`
  returns `{ avgage: … }` on pg but `{ avgAge: … }` elsewhere — read both
  (`row.avgAge ?? row.avgage`). Same for `$project`/`$addFields` aliases.
- **PostgreSQL has no `LIMIT` on `UPDATE`/`DELETE`.** `updateOne`/`deleteOne` append
  `LIMIT 1` on mysql/sqlite only; on pg they rely on the filter. (The adapters compute
  `matchedCount` with a pre-`COUNT(*)`, so the reported counts still match Mongo semantics.)
- **Booleans differ on the wire:** `TRUE`/`FALSE` on pg, `1`/`0` on mysql (`TINYINT(1)`)
  and sqlite (`INTEGER`). Returned rows may carry `1`/`0` — normalize at the boundary.
- **Dates differ:** pg `TIMESTAMP` (`'iso'::timestamp`), mysql `DATETIME`
  (`'YYYY-MM-DD HH:MM:SS'` UTC), sqlite `TEXT` (ISO string), memory/mongo native `Date`.
- **JSON storage differs:** pg `JSONB` (`#>>`, `jsonb_set`, `@>`-family), mysql `JSON`
  (`JSON_EXTRACT`/`JSON_SET`/`JSON_REMOVE`), sqlite `TEXT` (`json_extract`/`json_set`/
  `json_remove` — needs the `json1` extension, bundled with modern sqlite).
- **MySQL quoting:** identifiers equal to reserved words (`longText` = `LONGTEXT`, …)
  are backtick-quoted in DDL *and* DML automatically; everything else is unquoted.
- **MySQL `information_schema` reports `VARCHAR` without length** (`DATA_TYPE = 'varchar'`);
  the adapter compares base types so `VARCHAR` ≡ `VARCHAR(255)`, and `$rename` pre-adds
  the target with a valid `VARCHAR(255)` instead of copying the bare type.
- **`SERIAL` reports as `integer`.** pg `SERIAL` / mysql `AUTO_INCREMENT` columns read
  back as plain integers, so null-stripping consults your declared `schema`, not the
  live introspection.
- **Sort nulls:** pg sorts `ASC NULLS FIRST` / `DESC NULLS LAST`; mysql/sqlite emulate it
  with `CASE WHEN <col> IS NULL …` (plain `ASC`/`DESC` when mysql meets `DISTINCT`).
  Memory sorts nulls last in both directions.
- **`$substr` is 0-based** (Mongo semantics) — `+1` is added for SQL automatically.
- **Memory `find()` ignores its projection argument** — use `project(doc, proj)` or an
  aggregate `$project`. Memory `$in`/`$eq` compare whole values; use `$elemMatch` for
  array elements. Memory `find()` returns live references.
- **Memory ids:** `auto` continues past the largest seeded numeric `_id`; `custom`
  without an `idGenerator` leaves the id `undefined`.
- **`upsertOne` is update-then-insert on SQL** (two round-trips, not atomic); native on
  MongoDB (`{ upsert: true }`); `updateOne(…, { upsert: true })` on memory.

---

## 🔌 Connection Lifecycle

Every example in this README follows the same discipline — connect, `try/finally`,
release — because a leaked handle hangs the process:

| Backend | Open | Close | Notes |
| ------- | ---- | ----- | ----- |
| memory | — (sync) | `client.close()` drops stores | per-`db(name)` isolation |
| sqlite | `new Database(path?)` | `db.close()` | sync API, no sockets |
| pg | `new Client(…)` → **`await client.connect()`** | `await client.end()` | never `connect()` twice — adopted clients are used as-is |
| mysql | `await mysql.createConnection(…)` | `await conn.end()` | connects during creation |
| mongodb | `await createMongoSchemaless(…)` (connects inside) | `await client.close()` | fails fast after 5s `serverSelectionTimeoutMS` |
| `sql` executor | yours | `close` from config (or no-op) | `raw` is your `client` or `null` |

```javascript
import { createSchemalessClient } from "umosql/client";

const client = await createSchemalessClient("pg", { host, database, user, password });
try {
	const users = client.db("app").collection("users");
	await users.insertOne({ name: "Alice" });
} finally {
	await client.close(); // safe to call twice; never throws
}
```

`CLIENT_TYPES` (`['memory', 'sqlite', 'pg', 'mysql', 'mongodb', 'sql']`) enumerates the
valid `createSchemalessClient` types. `loadDriver(backend, specifier, () => import(…))`
(re-exported from `umosql/client`) gives custom backends the same
lazy-load-with-install-hint behavior, and every async factory calls `loadEnv()` first so
a local `.env` is picked up when `dotenv` happens to be installed (quietly skipped when
it is not). Pass `{ debug: true }` to log each generated statement.

---

## 📁 Examples Tour (`examples/`)

Runnable end-to-end scripts (all guarded — they no-op without their env vars):

| File | What it shows |
| ---- | ------------- |
| `client.js` | One API across every backend (`node examples/client.js` runs driver-free; `PG_*`/`MYSQL_*`/`MONGO_*` light up live backends) |
| `sqlite.js` / `pg.js` / `mysql.js` / `mongo.js` / `memory.js` | Per-backend `createSchemalessClient` tours |
| `lite.js` / `tiny.js` | The small builders (`lite` notes its 7 basic stages) |
| `serverless-neon.js` | `createSQLAdapter({ database: 'pg', execute: fetch→Neon, queryBuilder })` (`NEON_HTTP_URL`, `NEON_API_KEY`) |
| `serverless-turso.js` | Same pattern for Turso (`TURSO_HTTP_URL`, `TURSO_TOKEN`) |
| `serverless-planetscale.js` | Same pattern for PlanetScale (`PSCALE_DATA_API_URL`, `PSCALE_TOKEN`) |
| `serverless-neon-drizzle.js` | Drizzle `neon-http` (`drizzle-orm/neon-http` + `@neondatabase/serverless`) |
| `serverless-turso-drizzle.js` | Drizzle libsql (`drizzle-orm/libsql` + `@libsql/client`) |
| `dbdebug.pg.js` / `dbdebug.pg.insert.js` / `dbdebug.mysql.js` | Minimal live-DB probes used while developing the adapters |

The serverless trio shares one shape: build a `qb`, wrap an HTTP `execute`, call
`adapter.execute(sql)` / `adapter.collection(…)` exactly like a socketed backend.

---

## ❌ Errors Reference

Fail-fast validation — every message below is thrown (not returned) at build time:

| Trigger | Message |
| ------- | ------- |
| unknown `$op` in filter / expression / update / stage | `Unknown operator: $x` / `Unknown expression operator: $x` / `Unknown update operator: $x` / `Unknown pipeline operator: $x` |
| unknown memory accumulator | `Unknown accumulator: $x` / `Unknown stage: $x` |
| bad identifier | `Invalid column: …` (anything outside `[\w.]`, empty segments) |
| `deleteMany({})` without opt-in | `deleteMany requires a filter or allowDeleteAll option` |
| `insertMany([])` / field-less docs | `insertMany requires at least one document` / `Documents must have at least one field` |
| non-object `insertOne` / update | `insertOne requires a document object` / `Update must be an object` |
| `$in`/`$nin`/`aggregate` non-array | `$in requires array` (…), `aggregate` → `Stage N must be an object` / `Pipeline must be array` (memory) |
| `$between` / `$mod` shape | `$between requires [min, max]` / `$mod requires [divisor, remainder]` |
| `$cond` / `$switch` / `$exists`-expr shape | `$cond requires [condition, then, else]` / `$switch requires {branches: […], default: …}` (+ `Branch needs {case, then}`) / `$exists requires [field, boolean]` |
| `$in`/`$nin` expr shape | `$in requires [value, array]` |
| `$limit`/`$skip`/`$sample` shape | `$limit requires non-negative integer` (…`$skip`…) / `$sample` → size int / `$bucket requires boundaries array` |
| `$rename` on `a.b` paths | `$rename for JSON fields not supported` |
| `escape(undefined)` / `NaN` / `Infinity` | `Cannot escape undefined` / `Cannot escape non-finite number` |
| regex > 1000 chars | `Regex pattern too long (max 1000 chars)` |
| `createSQLAdapter` misconfig | `execute function is required` / `queryBuilder is required (pass createQueryBuilder({…}))` |
| `createSchemalessClient('sql', …)` w/o executor | `createSchemalessClient('sql', { executor }) requires an executor(sql, params) function` |
| unknown client type | `Unknown type: x. Expected one of: memory, sqlite, pg, mysql, mongodb, sql` |
| missing driver | `umosql: the "pg" backend needs the "pg" driver…` (+ install hint; never a bare module error) |
| sqlite DDL gaps | `SQLite does not support DROP COLUMN` / `SQLite does not support MODIFY COLUMN` |
| memory `insertMany` non-array | `insertMany requires array` / `Query must be object` |

---

## 📝 TypeScript Types

Each entry point ships a hand-written `.d.ts` next to its ESM/CJS/UMD/minified builds:

| Entry | Types | Highlights |
| ----- | ----- | ---------- |
| `umosql` | `index.d.ts` | `DBType`, `FindQuery`, `CollectionApi`, `ExtendApi`, `QueryBuilderApi`, `QueryBuilderConfig` |
| `umosql/schemaless` | `schemaless.d.ts` | `Schema`/`SchemaField`, `CollectionOptions`, `FindOptions`/`FindManyOptions` (+ `order` alias), `Cursor`, `WriteResult`, `SchemalessCollection`, `SchemalessAdapter` (incl. `build*`), `SQLAdapterConfig` |
| `umosql/memory` | `memory.d.ts` | engine classes, curried `filter`/`expression`/`aggregate`, `project`, utils |
| `umosql/client` | `client.d.ts` | `createSchemalessClient` config, `CLIENT_TYPES`, re-exported factories |
| `umosql/lite`, `umosql/tiny` | `lite/index.d.ts`, `tiny/index.d.ts` | trimmed builder surfaces |

## API Reference

### Entry points

| Module                | Exports                                                                                                                                                                                                                                       |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `umosql`              | `collection` (default), `filter`, `expression`, `aggregate`, `insertMany`, `updateMany`, `deleteMany`, `FindQuery`, `extend`, `db`, `createQueryBuilder`, `filterOps`, `exprOps`, `updateOps`, `stageHandlers`, `escape`, `jsonPath`, `validate` |
| `umosql/lite`         | default builder object + `collection`, `filter`, `expression`, `aggregate`, `extend`, `db`                                                                                                                                                    |
| `umosql/tiny`         | default builder object + `collection`, `filter`, `expression`, `aggregate`, `extend`, `db`                                                                                                                                                    |
| `umosql/schemaless`   | `createSchemalessAdapter`, `createSQLAdapter`, `createMemorySchemaless`                                                                                                                                                                       |
| `umosql/memory`       | `collection` (default), `db`, `Database`, `Collection`, `FindQuery`, `filter`, `expression`, `aggregate`, `project`, `extend`, `createMemoryDB`, `createMemorySchemaless`, `filterOps`, `exprOps`, `updateOps`, `stageOps`, `deepEquals`, `getPath`, `setPath`, `deletePath`, `clone` |
| `umosql/client`       | `createSchemalessClient` (default), `CLIENT_TYPES`, `createSchemalessAdapter`, `createSQLAdapter`, `createMemorySchemaless`, `createMongoSchemaless`                                                                                          |

Each entry point is published as ESM (`.es.js`), CommonJS (`.cjs`), UMD, minified IIFE, and a matching `.d.ts`.
The query-builder entries (`umosql`, `/lite`, `/tiny`) only produce SQL strings; the adapter entries
(`/schemaless`, `/memory`, `/client`) execute them.

### createSchemalessClient(type?, config?)

- `type`: `memory` (in-process), `sqlite` (`better-sqlite3`), `pg`, `mysql`, `mongodb`, or `sql`
  (any driver through your own `executor`). Defaults to `sqlite`.
- `config`
  - `client` / `conn`: reuse an existing driver instance instead of creating one — the driver package
    is then **never imported**. A `pg` client you pass in is **not** re-connected (pg throws on a second
    `connect()`).
  - `driver`: inject the driver module (e.g. `await import('pg')`, or a stub in tests) when you want
    umosql to construct the connection but not resolve the package.
  - `driverOptions`: extra options forwarded to the driver constructor / connection factory.
  - `filename`: sqlite file (default `:memory:`).
  - `host`, `port`, `user`, `password`, `database`: pg / mysql / mongodb.
  - `connectionString` | `url` (+ `ssl`): pg connection string (Neon, Supabase, ...).
  - `uri`: mysql connection URI.
  - `executor(sql, params)`: required for `type: 'sql'`; return driver rows (`{ rows }`, `mysql2`'s
    `[rows, fields]`, or a plain array). Optional `close()` for teardown.
  - `debug`: log every generated statement.
  - `id` | `idColumn`, `idStrategy`, `idGenerator`: defaults for collections created via `db()`.
- Returns `{ type, raw, adapter, db(name?, options?), close() }`
  - `raw`: the underlying driver instance (`pg.Client`, `mysql2` connection, `MongoClient`,
    `better-sqlite3` handle, in-memory `Database`); `null` for `type: 'sql'` unless you pass `client`.
  - `db(name, options?)` → `{ name, collection(name, opts?), adapter }`. For `memory`, each name gets an
    isolated store; for SQL/Mongo it scopes id options.
  - `close()`: releases the driver (ends connections, drops in-memory stores). Safe to call twice.

```javascript
import { createSchemalessClient } from 'umosql/client';

const client = await createSchemalessClient('sqlite', { filename: ':memory:' });
const users = client.db('app').collection('users');
await users.insertOne({ name: 'Alice', age: 25 });
console.log(await (await users.find({ age: { $gte: 18 } })).toArray());
await client.close();
```

### createSchemalessAdapter(client?, database?, options?)

- Parameters
  - `client` (optional): backend client instance. Omit for memory.
  - `database` (optional): one of `memory` | `sqlite` | `pg` | `mysql`. Defaults to `memory`.
  - `options` (optional): `{ debug?: boolean }`.
- Returns: `{ adapter, client? }`
  - `adapter`: unified interface with Mongo-compatible collection methods.
  - `client`: the raw client for SQL backends (not present for memory).
  - memory returns `{ adapter, database }` instead, where `database` is the in-memory `Database`.
- Use `createSchemalessClient` when you want the driver created/closed for you, or `mongodb` support.

### Drivers

- umosql declares **no** runtime dependencies and bundles **no** driver code (`pg`, `mysql2`,
  `better-sqlite3`, `mongodb` and `dotenv` stay external in every build).
- Install only what you use, or pass `{ client }` / `{ driver }` to skip resolution entirely.
- `loadDriver(backend, specifier, () => import(specifier))` is exported from `umosql/client` if you
  want the same lazy-load-with-hint behaviour in a custom backend.

### Adapter

- `collection(name, opts?)` → `Collection`
  - `opts` may include `{ id?: string, idStrategy?: 'auto'|'mongo'|'custom', idGenerator?: () => string }`.
- `listCollections()` → `string[]`
- `dropCollection(name)` → `{ acknowledged: boolean }`
- SQL-only helpers
  - `createTableWithSchema(name, jsonSchema)`
  - `getTableSchema(name)` → `{ columns: Record<string, any> }`
  - `addColumn(name, col, type, options?)`
  - `renameColumn(name, from, to)`
  - `modifyColumn(name, col, type, options?)` (pg/mysql)

### Collection

- CRUD
  - `insertOne(doc)` → `{ acknowledged: boolean, insertedId: any }`
  - `insertMany(docs)` → `{ acknowledged: boolean, insertedIds: any[] }`
  - `find(filter?, projection?, options?)` → `{ toArray(): Promise<any[]>, count(): Promise<number> }`
  - `findOne(filter?, projection?)` → `Promise<any | null>`
  - `updateOne(filter, update, options?)` → `{ acknowledged: boolean, matchedCount: number, modifiedCount: number, upsertedId?: any }`
  - `updateMany(filter, update, options?)` → `{ acknowledged: boolean, matchedCount: number, modifiedCount: number }`
  - `upsertOne(filter, update)` → `{ acknowledged: boolean, upserted: boolean, upsertedId?: any }`
  - `deleteOne(filter)` → `{ acknowledged: boolean, deletedCount: number }`
  - `deleteMany(filter?)` → `{ acknowledged: boolean, deletedCount: number }`
- Query helpers
  - `countDocuments(filter?)` → `number`
  - `estimatedDocumentCount()` → `number`
  - `distinct(field, filter?)` → `any[]`
  - `aggregate(pipeline)` → `any[]` (Mongo-like stages with SQL mapping under the hood)
- SQL-only convenience
  - `findMany({ filter, sort, page, pageSize, includeTotal })` → `{ items, total, page, pageSize }`

### ID Strategies

- `auto` (default): numeric autoincrement on SQL; in-memory numeric counter.
- `mongo`: 24-character hex string (`_id`) for parity.
- `custom`: provide `idGenerator()`; adapter respects provided id.


---

### Similar Projects

- [MongoDB](https://www.mongodb.com/) - The inspiration
- [Mongoose](https://mongoosejs.com/) - MongoDB object modeling
- [Knex.js](http://knexjs.org/) - SQL query builder

---

## 📞 Support

- 🐛 [Report bugs](https://github.com/kethan/umosql/issues)
- 💡 [Request features](https://github.com/kethan/umosql/issues)
- 📖 [Documentation](https://github.com/kethan/umosql)

---

## 🚀 Quick Links

- [NPM Package](https://www.npmjs.com/package/umosql)
- [GitHub Repository](https://github.com/kethan/umosql)

---

## 🙏 Acknowledgments

Inspired by MongoDB's intuitive query syntax and the need for SQL compatibility.

---

## 📄 License

MIT License - feel free to use in your projects!

---

## 🙏 Credits

Created for developers who love MongoDB syntax but need SQL databases.

---

**Made with ❤️ for the JavaScript community and for developers who love MongoDB syntax but need SQL databases**