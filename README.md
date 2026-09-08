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
| **Tiny**       | `import tiny from "umosql/tiny"`                           | Smallest builder: basic filters/expressions, `$set` only                      |
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
| **JSON Paths**           | ✅                 | ✅                 | ❌                 |
| **JSON Updates**         | ✅                 | ✅ Basic           | ❌                 |
| **Aggregation**          | ✅                 | ✅ Basic           | ✅ Basic           |
| **Filter Operators**     | ✅ All             | ✅ All             | ✅ Basic           |
| **Expression Operators** | ✅ All             | ✅ Basic           | ✅ Basic           |
| **Update Operators**     | ✅ All             | ✅ Basic           | ✅ $set            |
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

### Use **LITE** when:

- You need JSON fields but not aggregation
- Medium complexity apps
- REST APIs with JSON columns
- Balance between features and size

### Use **TINY** when:

- Simple CRUD operations only
- No JSON columns needed
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
| **Full** | ~6.61 kB    | ✅ Yes       | ✅ Yes      | Complete MongoDB compatibility  |
| **Lite** | ~5.99 kB    | ✅ Yes       | ❌ No       | JSON without aggregation        |
| **Tiny** | ~4.95 kB    | ❌ No        | ✅ Basic    | Minimal ops, smallest bundle    |

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
//   AND ctid IN (SELECT ctid FROM users WHERE email = 'alice@example.com' LIMIT 1)

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
// Others: SELECT * FROM users WHERE LOWER(username) NOT LIKE LOWER('admin%')

// $not on a field negates the conditions nested in it
users.find({ age: { $not: { $lt: 23 } } }).toSQL();
// SELECT * FROM users WHERE NOT (age < 23)

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
//   AND ctid IN (SELECT ctid FROM users WHERE email = 'alice@example.com' LIMIT 1)

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
//   AND ctid IN (SELECT ctid FROM users WHERE id = 1 LIMIT 1)

// Multiply values
users.updateOne({ id: 1 }, { $mul: { score: 1.1 } });
// UPDATE users SET score = score * 1.1 WHERE id = 1
//   AND ctid IN (SELECT ctid FROM users WHERE id = 1 LIMIT 1)

// Set to minimum
users.updateMany({}, { $min: { minPrice: 10 } });
// UPDATE users SET minPrice = LEAST(minPrice, 10)

// Set to maximum
users.updateMany({}, { $max: { maxDiscount: 50 } });
// UPDATE users SET maxDiscount = GREATEST(maxDiscount, 50)

// Unset fields (set to NULL)
users.updateOne({ id: 1 }, { $unset: { tempToken: "", tempData: "" } });
// UPDATE users SET tempToken = NULL, tempData = NULL WHERE id = 1
//   AND ctid IN (SELECT ctid FROM users WHERE id = 1 LIMIT 1)

// Rename fields
users.updateMany({}, { $rename: { oldField: "newField" } });
// UPDATE users SET newField = oldField, oldField = NULL

// Current timestamp
users.updateOne(
	{ id: 1 },
	{ $currentDate: { lastLogin: true, updatedAt: true } }
);
// PostgreSQL: UPDATE users SET lastLogin = CURRENT_TIMESTAMP, updatedAt = CURRENT_TIMESTAMP WHERE id = 1 AND ctid IN (SELECT ctid FROM users WHERE id = 1 LIMIT 1)
// MySQL: UPDATE users SET lastLogin = NOW(), updatedAt = NOW() WHERE id = 1 LIMIT 1
// SQLite: UPDATE users SET lastLogin = datetime('now'), updatedAt = datetime('now') WHERE id = 1 AND rowid IN (SELECT rowid FROM users WHERE id = 1 LIMIT 1)

> `updateOne` and `deleteOne` affect at most one row on every dialect. MySQL gets
> `LIMIT 1`; SQLite and PostgreSQL have no LIMIT on DML, so the statement is
> narrowed with `rowid` / `ctid` instead (the row identifier both support).
> `updateMany` and `deleteMany` never carry the narrowing.

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
//   AND ctid IN (SELECT ctid FROM users WHERE email = 'old@example.com' LIMIT 1)

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
// DELETE FROM users WHERE id = 123
//   AND ctid IN (SELECT ctid FROM users WHERE id = 123 LIMIT 1) RETURNING *
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
]);
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
]);
```

---
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

const premium = await (await users.find({ tags: { $in: ["premium"] } })).toArray();
await users.updateMany({ tags: { $in: ["premium"] } }, { $inc: { points: 100 } });
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

users.find({ tags: { $in: ["premium"] } }).toArray(); // no promises
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
| `$regex` | ✅ | ⚠️ | ✅ | ✅ | PG: `~`; MySQL: `REGEXP`; SQLite: rewritten to `LIKE`, exact only for `^…$` anchored patterns |
| `$exists` | ✅ | ✅ | ✅ | ✅ | `IS NULL` / `IS NOT NULL` |
| `$between` | ✅ | ✅ | ✅ | ✅ | `BETWEEN a AND b` |
| `$mod` | ✅ | ✅ | ✅ | ✅ | `field % m = r`; both operands must be finite numbers |
| `$and` | ✅ | ✅ | ✅ | ✅ | Parenthesized conjunctions |
| `$or` | ✅ | ✅ | ✅ | ✅ | Parenthesized disjunctions |
| `$not` | ✅ | ✅ | ✅ | ✅ | `NOT ( ... )`, top level or per field: `{ age: { $not: { $lt: 23 } } }` |
| `$nor` | ✅ | ✅ | ✅ | ✅ | `NOT ( ... OR ... )` |
| `$expr` | ✅ | ✅ | ✅ | ✅ | Embed expression in filter |
| `$type` | ✅ | — | — | — | Memory-only |
| `$elemMatch` | ✅ | — | — | — | Memory-only |
| `$all` | ✅ | — | — | — | Memory-only |
| `$size` | ✅ | — | — | — | Memory-only |

### Expression Operators

| Operator | Memory | SQLite | MySQL | PostgreSQL | Notes |
| --- | --- | --- | --- | --- | --- |
| `$add` | ✅ | ✅ | ✅ | ✅ | `+` |
| `$subtract` | ✅ | ✅ | ✅ | ✅ | `-` |
| `$multiply` | ✅ | ✅ | ✅ | ✅ | `*` |
| `$divide` | ✅ | ✅ | ✅ | ✅ | `/ NULLIF(...,0)`; SQLite/PostgreSQL cast to REAL/numeric first, so `25 / 2` is `12.5` |
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
| `$substr` | ✅ | ✅ | ✅ | ✅ | `SUBSTRING()` with a 0 based start index, like MongoDB |
| `$trim`/`$ltrim`/`$rtrim` | ✅ | ✅ | ✅ | ✅ | `TRIM` variants |
| `$strLen` | ✅ | ✅ | ✅ | ✅ | `LENGTH()` |
| `$replace` | ✅ | ✅ | ✅ | ✅ | `REPLACE()` |
| `$sum` | ✅ | ✅ | ✅ | ✅ | Aggregates; `$sum: 1` maps to `COUNT(*)` |
| `$avg` | ✅ | ✅ | ✅ | ✅ | `AVG()` |
| `$min` | ✅ | ✅ | ✅ | ✅ | Single: `MIN()`; multi: `LEAST()` |
| `$max` | ✅ | ✅ | ✅ | ✅ | Single: `MAX()`; multi: `GREATEST()` |
| `$count` | ✅ | ✅ | ✅ | ✅ | `COUNT(*)` |
| `$stdDevPop`/`$stdDevSamp` | ✅ | ✅ | ✅ | ✅ | `STDDEV_*()` |
| `$eq`,`$ne`,`$gt`,`$gte`,`$lt`,`$lte` | ✅ | ✅ | ✅ | ✅ | Comparison in expressions |
| `$cmp` | ✅ | ✅ | ✅ | ✅ | Returns -1/0/1 |
| `$in`/`$nin` | ✅ | ✅ | ✅ | ✅ | Expression `IN`/`NOT IN` |
| `$size` (JSON array) | ✅ | ✅ | ✅ | ✅ | PG: `jsonb_array_length`, MySQL: `JSON_LENGTH`, SQLite: `json_array_length` |
| `$and`/`$or`/`$not` | ✅ | ✅ | ✅ | ✅ | Logical composition |
| `$cond` | ✅ | ✅ | ✅ | ✅ | `CASE WHEN ... THEN ... ELSE ... END` |
| `$ifNull` | ✅ | ✅ | ✅ | ✅ | `COALESCE()` |
| `$switch` | ✅ | ✅ | ✅ | ✅ | `CASE` branches |
| `$exists` | ✅ | ✅ | ✅ | ✅ | `IS NULL` / `IS NOT NULL` |
| Date parts `$year`,`$month`,`$dayOfMonth`,`$dayOfWeek`,`$hour`,`$minute`,`$second`,`$week` | ✅ | ✅ | ✅ | ✅ | DB-specific functions (`EXTRACT`, `YEAR`, `strftime`) |
| Cast `$toString`,`$toInt`,`$toDouble`,`$toBool`,`$toDate` | ✅ | ✅ | ✅ | ✅ | DB-specific `CAST` |
| `$literal` | ✅ | ✅ | ✅ | ✅ | Escaped literal |

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
| `$push`/`$pull`/`$addToSet` | ✅ | — | — | — | Memory-only array mutations |

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
| `$sample` | ✅ | ✅ | ✅ | ✅ | Random ordering + LIMIT |
| `$sortByCount` | ✅ | ✅ | ✅ | ✅ | GROUP BY expr, order by count desc |
| `$bucket` | ✅ | ✅ | ✅ | ✅ | CASE-based bucketing |
| `$unwind` | ✅ | — | — | — | Memory-only |

#### Code References

- Filter operators: `FILTER OPERATORS` in `src/index.js`, `filterOps` in `src/adapter/memory/memory.js`
- Expression operators: `EXPRESSION OPERATORS` in `src/index.js`, `exprOps` in the memory engine
- Update operators: `UPDATE OPERATORS` in `src/index.js`, `updateOps` in the memory engine
- Aggregation stages: `AGGREGATE PIPELINE` in `src/index.js`, `stageOps` in the memory engine
- JSON path extraction and JSON updates: `jsonPath()` / `jsonUpdate()` in `src/index.js`
- Single-row narrowing for `updateOne`/`deleteOne`: `whereClause()` in `src/index.js`
- CRUD SQL: `collection()` in `src/index.js`, executed by `SQLCollection` in `src/schemaless.js`

### Compatibility & Testing

- PostgreSQL: tested with 16; native `ILIKE` and regex `~` used.
- MySQL: tested with 8.x; uses `REGEXP`, `LOWER(...) LIKE LOWER(...)` for case-insensitive like.
- SQLite: tested with `better-sqlite3`; `$regex` is rewritten to `LIKE`, so only `^`/`$` anchored
  patterns map exactly (a `REGEXP` UDF can be registered for full fidelity). `UPDATE/DELETE … LIMIT`
  is not available, so single-row statements are narrowed through `rowid`.
- Memory: full operator coverage, including array and pipeline-only stages.
- Unified suite covers filters, expressions, updates, and aggregation across adapters where applicable.

### Running the Tests

```bash
npm test        # every tests/*.spec.js file
npm run test:db # only the specs that talk to a real database
npm run db:up   # docker compose: postgres + mysql + mongodb, then `npm run coverage:db`
```

`tests/run-all.js` runs each spec file in its own Node process and fails as soon as one of them
exits non-zero, so an assertion error cannot be printed and ignored. The specs that need a server
(`pg`, `mysql`, `mongodb`) check their environment first (`PG_HOST`, `MYSQL_HOST`, …) and skip
without it, which keeps `npm test` meaningful offline; the CI `db` job provides the variables and
runs them for real.

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
| `upsertOne` | ✅ | ✅ | ✅ | ✅ |
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
    (`count()` runs a `SELECT COUNT(*)` for the filter - it does not fetch rows; a `limit`/`skip`
    on the cursor is applied to the returned number)
  - `findOne(filter?, projection?)` → `Promise<any | null>`
  - `updateOne(filter, update, options?)` → `{ acknowledged: boolean, matchedCount: number, modifiedCount: number, upsertedId?: any }` (at most one row, on every dialect)
  - `updateMany(filter, update, options?)` → `{ acknowledged: boolean, matchedCount: number, modifiedCount: number }`
  - `upsertOne(filter, update)` → `{ acknowledged: boolean, upserted: boolean, upsertedId?: any }`
  - `deleteOne(filter)` → `{ acknowledged: boolean, deletedCount: number }` (at most one row, on every dialect)
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