import Database from "better-sqlite3";
import assert from "assert";
import { filter, aggregate, expression } from "../src/index.js";

// === Initialize SQLite Memory DB ===
const db = new Database(":memory:");

// Create schema
db.exec(`
  CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT,
    age INTEGER,
    city TEXT,
    active BOOLEAN,
    profile TEXT -- JSON field
  );

  CREATE TABLE orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER,
    amount REAL,
    status TEXT,
    FOREIGN KEY (user_id) REFERENCES users(id)
  );
`);

// Insert users (with JSON)
const users = [
  ["Alice", 25, "Paris", 1, JSON.stringify({ country: "France", score: 85 })],
  ["Bob", 30, "London", 1, JSON.stringify({ country: "UK", score: 90 })],
  ["Charlie", 22, "Berlin", 0, JSON.stringify({ country: "Germany", score: 60 })],
  ["David", 40, "Paris", 1, JSON.stringify({ country: "France", score: 95 })],
  ["Eve", 35, "Berlin", 1, JSON.stringify({ country: "Germany", score: 88 })],
];
const insertUser = db.prepare("INSERT INTO users (name, age, city, active, profile) VALUES (?, ?, ?, ?, ?)");
users.forEach((u) => insertUser.run(...u));

// Insert orders
const orders = [
  [1, 120.5, "paid"],
  [1, 80.0, "pending"],
  [2, 200.0, "paid"],
  [3, 150.0, "cancelled"],
  [4, 300.0, "paid"],
  [5, 180.0, "paid"],
];
const insertOrder = db.prepare("INSERT INTO orders (user_id, amount, status) VALUES (?, ?, ?)");
orders.forEach((o) => insertOrder.run(...o));

// === Helper to normalize numeric fields for deep equality ===
function normalize(rows) {
  return rows.map((r) => {
    const obj = {};
    for (const [k, v] of Object.entries(r)) {
      obj[k] = typeof v === "number" ? Math.round(v * 1000) / 1000 : v;
    }
    return obj;
  });
}

// === Test Runner ===
function runTest(title, queryFn, expected) {
  const sql = typeof queryFn === "function" ? queryFn() : queryFn;
  const actual = normalize(db.prepare(sql).all());
  try {
    assert.deepStrictEqual(actual, expected);
    console.log(`✅ PASS: ${title}`);
  } catch (err) {
    console.error(`❌ FAIL: ${title}`);
    console.error("SQL:", sql);
    console.error("Expected:", expected);
    console.error("Actual:", actual);
    process.exitCode = 1;
  }
}

// === Tests ===

// 1️⃣ Filter simple
runTest("Filter: active users in Paris", () => {
  const sql = `SELECT name, age, city FROM users WHERE ${filter({ city: "Paris", active: true })}`;
  return sql;
}, [
  { name: "Alice", age: 25, city: "Paris" },
  { name: "David", age: 40, city: "Paris" },
]);

// 2️⃣ Filter with JSON path
runTest("Filter: users from France (JSON field)", () => {
  const sql = `SELECT name FROM users WHERE ${filter({ "profile.country": "France" })}`;
  return sql;
}, [
  { name: "Alice" },
  { name: "David" },
]);

// 3️⃣ Expression test
runTest("Expression: computed age + 5", () => {
  const sql = `SELECT ${expression({ $add: ["$age", 5] })} AS next_age FROM users WHERE name = 'Bob'`;
  return sql;
}, [{ next_age: 35 }]);

// 4️⃣ $group aggregate
runTest("Aggregate: group by city avgAge", () => {
  const sql = aggregate([
    { $group: { _id: "$city", avgAge: { $avg: "$age" } } },
    { $sort: { avgAge: 1 } },
  ])("users");
  return sql;
}, [
  { _id: "Berlin", avgAge: 28.5 },
  { _id: "London", avgAge: 30 },
  { _id: "Paris", avgAge: 32.5 },
]);

// 5️⃣ $match before $group
runTest("Aggregate: active users count per city", () => {
  const sql = aggregate([
    { $match: { active: true } },
    { $group: { _id: "$city", total: { $sum: 1 } } },
    { $sort: { _id: 1 } },
  ])("users");
  return sql;
}, [
  { _id: "Berlin", total: 1 },
  { _id: "London", total: 1 },
  { _id: "Paris", total: 2 },
]);

// 6️⃣ $match after $group (HAVING)
runTest("Aggregate: HAVING avgAge > 30", () => {
  const sql = aggregate([
    { $group: { _id: "$city", avgAge: { $avg: "$age" } } },
    { $match: { avgAge: { $gt: 30 } } },
    { $sort: { avgAge: 1 } },
  ])("users");
  return sql;
}, [
  { _id: "Paris", avgAge: 32.5 }
]);

// 7️⃣ JSON field aggregation
runTest("Aggregate: avg JSON score by country", () => {
  const sql = aggregate([
    { $group: { _id: "$profile.country", avgScore: { $avg: "$profile.score" } } },
    { $sort: { avgScore: 1 } },
  ])("users");
  return sql;
}, [
  { _id: "Germany", avgScore: 74 },
  { _id: "France", avgScore: 90 },
  { _id: "UK", avgScore: 90 },
]);

// 8️⃣ $count test
runTest("Aggregate: count adult users", () => {
  const sql = aggregate([
    { $match: { age: { $gte: 25 } } },
    { $count: "totalAdults" },
  ])("users");
  return sql;
}, [{ totalAdults: 4 }]);

// 9️⃣ $limit + $skip + $sort
runTest("Aggregate: youngest users sorted", () => {
  const sql = aggregate([
    { $sort: { age: 1 } },
    { $skip: 1 },
    { $limit: 2 },
  ])("users");
  return sql;
}, [
  { id: 1, name: "Alice", age: 25, city: "Paris", active: 1, profile: '{"country":"France","score":85}' },
  { id: 2, name: "Bob", age: 30, city: "London", active: 1, profile: '{"country":"UK","score":90}' },
]);

// 🔟 Complex with JSON HAVING
runTest("Aggregate: countries with avgScore >= 80", () => {
  const sql = aggregate([
    { $group: { _id: "$profile.country", avgScore: { $avg: "$profile.score" } } },
    { $match: { avgScore: { $gte: 80 } } },
  ])("users");
  return sql;
}, [
  { _id: "France", avgScore: 90 },
  { _id: "UK", avgScore: 90 },
]);

console.log("\n✅ All SQLite aggregate and JSON tests completed.\n");
