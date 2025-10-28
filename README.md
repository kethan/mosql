## UMoSQL

# Mongo To SQL Query

[![tests](https://github.com/kethan/mosql/actions/workflows/node.js.yml/badge.svg)](https://github.com/kethan/mosql/actions/workflows/node.js.yml) [![Version](https://img.shields.io/npm/v/umosql.svg?color=success&style=flat-square)](https://www.npmjs.com/package/umosql) [![Badge size](https://deno.bundlejs.com/badge?q=umosql&treeshake=[*]&config={"compression":"brotli"})](https://unpkg.com/umosql)

[![Version](https://img.shields.io/npm/v/umosql.svg?color=success&style=flat-square)](https://www.npmjs.com/package/umosql) [![Badge size](https://deno.bundlejs.com/badge?q=umosq/lite&treeshake=[*]&config={"compression":"brotli"})](https://unpkg.com/umosql/lite)

[![Version](https://img.shields.io/npm/v/umosql.svg?color=success&style=flat-square)](https://www.npmjs.com/package/umosql) [![Badge size](https://deno.bundlejs.com/badge?q=umosql/timy&treeshake=[*]&config={"compression":"brotli"})](https://unpkg.com/umosql/tiny)

---

## Install

```
npm i umosql
npm i umosql/lite
npm i umosql/tiny

```

## 📊 Feature Comparison

| Feature                  | Full               | Lite               | Tiny               |
| ------------------------ | ------------------ | ------------------ | ------------------ |
| **Size**                 | ~600 lines         | ~450 lines         | ~300 lines         |
| **JSON Paths**           | ✅                 | ✅                 | ❌                 |
| **JSON Updates**         | ✅                 | ✅                 | ❌                 |
| **Aggregation**          | ✅                 | ❌                 | ❌                 |
| **Filter Operators**     | ✅ All             | ✅ All             | ✅ All             |
| **Expression Operators** | ✅ All             | ✅ Basic           | ✅ Basic           |
| **Update Operators**     | ✅ All             | ✅ All             | ✅ All             |
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

- ✅ **MongoDB-compatible query syntax** - Use what you already know
- ✅ **SQL generation** - PostgreSQL, MySQL, SQLite support
- ✅ **JSON field support** - Query nested objects (Full & Lite versions)
- ✅ **Aggregation pipelines** - $group, $match, $project, etc. (Full version)
- ✅ **Universal adapters** - SQL, Memory, PouchDB, Firebase
- ✅ **REST API ready** - Build APIs in minutes
- ✅ **Serverless friendly** - Works anywhere JavaScript runs
- ✅ **Three versions** - Choose your feature set and bundle size

---

## 📦 Three Versions

| Version  | Size       | JSON Support | Aggregation | Use Case                        |
| -------- | ---------- | ------------ | ----------- | ------------------------------- |
| **Full** | ~600 lines | ✅ Yes       | ✅ Yes      | Complete MongoDB compatibility  |
| **Lite** | ~450 lines | ✅ Yes       | ❌ No       | JSON fields without aggregation |
| **Tiny** | ~300 lines | ❌ No        | ❌ No       | Simple CRUD, smallest bundle    |

---

## 🚀 Quick Start

### Installation

```bash
pm i umosql
npm i umosql/lite
npm i umosql/tiny
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
// WHERE email = 'alice@example.com' LIMIT 1

users.insertMany([
	{ name: "Alice", age: 25 },
	{ name: "Bob", age: 30 },
]);
// INSERT INTO users (name, age) VALUES ('Alice', 25), ('Bob', 30)
```

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
- [REST API Examples](#rest-api-examples)
- [Universal Adapters](#universal-adapters)
- [Custom Operators](#custom-operators)

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
// WHERE email = 'alice@example.com' LIMIT 1

// Update many documents
users.updateMany(
	{ age: { $lt: 18 } },
	{ $set: { role: "minor", permissions: [] } }
);
// UPDATE users SET role = 'minor', permissions = '[]' WHERE age < 18

// Increment values
users.updateOne({ id: 1 }, { $inc: { loginCount: 1, points: 10 } });
// UPDATE users SET loginCount = loginCount + 1, points = points + 10
// WHERE id = 1 LIMIT 1

// Multiply values
users.updateOne({ id: 1 }, { $mul: { score: 1.1 } });
// UPDATE users SET score = score * 1.1 WHERE id = 1 LIMIT 1

// Set to minimum
users.updateMany({}, { $min: { minPrice: 10 } });
// UPDATE users SET minPrice = LEAST(minPrice, 10)

// Set to maximum
users.updateMany({}, { $max: { maxDiscount: 50 } });
// UPDATE users SET maxDiscount = GREATEST(maxDiscount, 50)

// Unset fields (set to NULL)
users.updateOne({ id: 1 }, { $unset: { tempToken: "", tempData: "" } });
// UPDATE users SET tempToken = NULL, tempData = NULL WHERE id = 1 LIMIT 1

// Rename fields
users.updateMany({}, { $rename: { oldField: "newField" } });
// UPDATE users SET newField = oldField, oldField = NULL

// Current timestamp
users.updateOne(
	{ id: 1 },
	{ $currentDate: { lastLogin: true, updatedAt: true } }
);
// PostgreSQL: UPDATE users SET lastLogin = CURRENT_TIMESTAMP, updatedAt = CURRENT_TIMESTAMP WHERE id = 1 LIMIT 1
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
// DELETE FROM users WHERE email = 'old@example.com' LIMIT 1

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
// DELETE FROM users WHERE id = 123 LIMIT 1 RETURNING *
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

## 🌐 REST API Examples

### Complete Express Server

```javascript
import express from "express";
import { collection } from "umosql";
import pg from "pg";

const app = express();
app.use(express.json());

// Database connection
const pool = new pg.Pool({
	host: process.env.DB_HOST || "localhost",
	database: process.env.DB_NAME || "mydb",
	user: process.env.DB_USER || "postgres",
	password: process.env.DB_PASSWORD,
});

// Middleware to parse MongoDB query
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

const PORT = process.env.PORT || 3000;
app.listen(PORT, () => {
	console.log(`🚀 API running on http://localhost:${PORT}`);
	console.log(`\nExamples:`);
	console.log(`  GET  /messages`);
	console.log(`  GET  /messages?q={"unread":true}`);
	console.log(
		`  GET  /messages?q={"priority":{"$gte":5}}&sort={"createdAt":-1}`
	);
	console.log(`  GET  /messages/123`);
	console.log(`  POST /messages`);
	console.log(`  PUT  /messages/123`);
	console.log(`  DELETE /messages/123`);
});
```

### API Usage Examples

```bash
# List all messages
curl http://localhost:3000/messages

# Filter unread messages
curl "http://localhost:3000/messages?q={\"unread\":true}"

# Filter with $in
curl "http://localhost:3000/messages?q={\"status\":{\"\\$in\":[\"pending\",\"active\"]}}"

# Filter with $gte and $lte
curl "http://localhost:3000/messages?q={\"priority\":{\"\\$gte\":5,\"\\$lte\":10}}"

# Filter with multiple conditions
curl "http://localhost:3000/messages?q={\"unread\":true,\"priority\":{\"\\$gte\":5}}"

# Filter with $or
curl "http://localhost:3000/messages?q={\"\\$or\":[{\"priority\":10},{\"urgent\":true}]}"

# With pagination
curl "http://localhost:3000/messages?limit=10&skip=20"

# With sorting
curl "http://localhost:3000/messages?sort={\"createdAt\":-1,\"priority\":-1}"

# With field projection
curl "http://localhost:3000/messages?fields={\"title\":1,\"body\":1,\"createdAt\":1}"

# Combined query
curl "http://localhost:3000/messages?q={\"unread\":true}&sort={\"priority\":-1}&limit=5&fields={\"title\":1}"

# Count documents
curl "http://localhost:3000/messages/count"
curl "http://localhost:3000/messages/count?q={\"unread\":true}"

# Get single message
curl http://localhost:3000/messages/123

# Create message
curl -X POST http://localhost:3000/messages \
  -H "Content-Type: application/json" \
  -d '{"title":"Hello","body":"World","priority":5,"unread":true}'

# Bulk create
curl -X POST http://localhost:3000/messages/bulk \
  -H "Content-Type: application/json" \
  -d '[
    {"title":"Message 1","priority":5},
    {"title":"Message 2","priority":3}
  ]'

# Update message (partial)
curl -X PATCH http://localhost:3000/messages/123 \
  -H "Content-Type: application/json" \
  -d '{"unread":false,"readAt":"2024-01-01T12:00:00Z"}'

# Replace message
curl -X PUT http://localhost:3000/messages/123 \
  -H "Content-Type: application/json" \
  -d '{"title":"Updated","body":"New content","priority":10}'

# Bulk update
curl -X PATCH "http://localhost:3000/messages?q={\"unread\":true}" \
  -H "Content-Type: application/json" \
  -d '{"unread":false}'

# Delete message
curl -X DELETE http://localhost:3000/messages/123

# Bulk delete
curl -X DELETE "http://localhost:3000/messages?q={\"priority\":{\"\\$lt\":3}}"
```

### Frontend Usage (JavaScript)

```javascript
// Fetch API wrapper
class APIClient {
	constructor(baseURL) {
		this.baseURL = baseURL;
	}

	async find(collection, query = {}, options = {}) {
		const params = new URLSearchParams();
		if (Object.keys(query).length) params.append("q", JSON.stringify(query));
		if (options.sort) params.append("sort", JSON.stringify(options.sort));
		if (options.limit) params.append("limit", options.limit);
		if (options.skip) params.append("skip", options.skip);
		if (options.fields) params.append("fields", JSON.stringify(options.fields));

		const response = await fetch(`${this.baseURL}/${collection}?${params}`);
		return response.json();
	}

	async findOne(collection, id) {
		const response = await fetch(`${this.baseURL}/${collection}/${id}`);
		if (!response.ok) throw new Error("Not found");
		return response.json();
	}

	async create(collection, data) {
		const response = await fetch(`${this.baseURL}/${collection}`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(data),
		});
		return response.json();
	}

	async update(collection, id, data) {
		const response = await fetch(`${this.baseURL}/${collection}/${id}`, {
			method: "PATCH",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(data),
		});
		return response.json();
	}

	async delete(collection, id) {
		const response = await fetch(`${this.baseURL}/${collection}/${id}`, {
			method: "DELETE",
		});
		return response.json();
	}

	async count(collection, query = {}) {
		const params = new URLSearchParams();
		if (Object.keys(query).length) params.append("q", JSON.stringify(query));

		const response = await fetch(
			`${this.baseURL}/${collection}/count?${params}`
		);
		return response.json();
	}
}

// Usage
const api = new APIClient("http://localhost:3000");

// Get unread messages with high priority
const messages = await api.find(
	"messages",
	{ unread: true, priority: { $gte: 5 } },
	{ sort: { createdAt: -1 }, limit: 10 }
);

// Get messages by status
const pending = await api.find("messages", {
	status: { $in: ["pending", "processing"] },
});

// Create message
const newMessage = await api.create("messages", {
	title: "New Message",
	body: "Content here",
	priority: 5,
});

// Update message
await api.update("messages", "123", {
	unread: false,
	readAt: new Date().toISOString(),
});

// Count unread
const { count } = await api.count("messages", { unread: true });
```

---

## 🔌 Universal Adapters (TODO)

### SQL Adapter (with execution)

```javascript
import { SQLAdapter } from "umosql/adapters";

const db = new SQLAdapter({
	type: "pg", // 'pg', 'mysql', or 'sqlite'
	host: "localhost",
	database: "mydb",
	user: "postgres",
	password: "password",
});

await db.connect();

// Use collection interface
const users = db.collection("users");

// All operations return promises
const adults = await users.find({ age: { $gte: 18 } });
const newUser = await users.insertOne({ name: "Alice", age: 25 });
await users.updateOne({ id: 1 }, { $inc: { loginCount: 1 } });
await users.deleteMany({ active: false });

// Aggregation
const stats = await users.aggregate([
	{ $group: { _id: "$city", count: { $sum: 1 } } },
]);

await db.disconnect();
```

### Memory Adapter

```javascript
import { MemoryAdapter } from "umosql/adapters";

const db = new MemoryAdapter();
await db.connect();

const users = db.collection("users");

// Works exactly like MongoDB
await users.insertMany([
	{ name: "Alice", age: 25, tags: ["premium"] },
	{ name: "Bob", age: 30, tags: ["basic"] },
	{ name: "Charlie", age: 22, tags: ["premium", "vip"] },
]);

// All MongoDB query features work
const premium = await users.find({ tags: { $in: ["premium"] } });
const adults = await users.find({ age: { $gte: 18 } });

// Update with operators
await users.updateMany(
	{ tags: { $in: ["premium"] } },
	{ $inc: { points: 100 } }
);

// Aggregation
const grouped = await users.aggregate([
	{ $group: { _id: "$age", count: { $sum: 1 } } },
]);
```

### Switch Between Adapters

```javascript
import { connect } from "umosql/adapters";

// Use environment variable to switch
const dbType = process.env.DB_TYPE || "memory";
const db = await connect(dbType, {
	// PostgreSQL
	host: process.env.DB_HOST,
	database: process.env.DB_NAME,
	user: process.env.DB_USER,
	password: process.env.DB_PASSWORD,
});

// Same code works with any adapter!
const users = db.collection("users");
await users.insertOne({ name: "Alice" });
const results = await users.find({ age: { $gte: 18 } });
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
