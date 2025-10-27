import mysql from "mysql2/promise";
import assert from "assert";
import { filter, aggregate, expression } from "../src/index.js";

import dotenv from "dotenv";
dotenv.config();

// === Connect to PlanetScale (set these as env vars) ===
const conn = await mysql.createConnection({
    host: process.env.MYSQL_HOST,       // e.g. 'aws.connect.psdb.cloud'
    user: process.env.MYSQL_USER,       // your PlanetScale username
    password: process.env.MYSQL_PASS,   // service password
    database: process.env.MYSQL_DB,     // e.g. 'mosql_tests'
    ssl: { rejectUnauthorized: false },
});

// Reset schema
await conn.query(`DROP TABLE IF EXISTS orders`);
await conn.query(`DROP TABLE IF EXISTS users`);

await conn.query(`
  CREATE TABLE users (
    id INT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255),
    age INT,
    city VARCHAR(255),
    active BOOLEAN,
    profile JSON
  );
`);

await conn.query(`
  CREATE TABLE orders (
    id INT AUTO_INCREMENT PRIMARY KEY,
    user_id INT,
    amount DECIMAL(10,2),
    status VARCHAR(255),
    FOREIGN KEY (user_id) REFERENCES users(id)
  );
`);

// Insert data
const users = [
    ["Alice", 25, "Paris", 1, JSON.stringify({ country: "France", score: 85 })],
    ["Bob", 30, "London", 1, JSON.stringify({ country: "UK", score: 90 })],
    ["Charlie", 22, "Berlin", 0, JSON.stringify({ country: "Germany", score: 60 })],
    ["David", 40, "Paris", 1, JSON.stringify({ country: "France", score: 95 })],
    ["Eve", 35, "Berlin", 1, JSON.stringify({ country: "Germany", score: 88 })],
];
await conn.query(
    `INSERT INTO users (name, age, city, active, profile) VALUES ?`,
    [users]
);

const orders = [
    [1, 120.5, "paid"],
    [1, 80.0, "pending"],
    [2, 200.0, "paid"],
    [3, 150.0, "cancelled"],
    [4, 300.0, "paid"],
    [5, 180.0, "paid"],
];
await conn.query(`INSERT INTO orders (user_id, amount, status) VALUES ?`, [orders]);

// === Normalize numeric output ===
const normalize = (rows) =>
    rows.map((r) =>
        Object.fromEntries(
            Object.entries(r).map(([k, v]) => {
                // Convert string numbers to actual numbers
                if (typeof v === 'string' && /^-?\d+(\.\d+)?$/.test(v)) {
                    return [k, parseFloat(v)];
                }
                // Round existing numbers
                if (typeof v === 'number') {
                    return [k, Math.round(v * 1000) / 1000];
                }
                return [k, v];
            })
        )
    );
// === Test runner ===
async function runTest(title, queryFn, expected) {
    const sql = typeof queryFn === "function" ? queryFn() : queryFn;
    const [rows] = await conn.query(sql);
    const actual = normalize(rows);
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

// === Same tests as SQLite ===

// 1️⃣ Filter simple
await runTest("Filter: active users in Paris", () => {
    const sql = `SELECT name, age, city FROM users WHERE ${filter({ city: "Paris", active: true }, "mysql")}`;
    return sql;
}, [
    { name: "Alice", age: 25, city: "Paris" },
    { name: "David", age: 40, city: "Paris" },
]);

// 2️⃣ Filter with JSON path
await runTest("Filter: users from France (JSON field)", () => {
    const sql = `SELECT name FROM users WHERE ${filter({ "profile.country": "France" }, "mysql")}`;
    return sql;
}, [
    { name: "Alice" },
    { name: "David" },
]);

// 3️⃣ Expression test
await runTest("Expression: computed age + 5", () => {
    const sql = `SELECT ${expression({ $add: ["$age", 5] }, "mysql")} AS next_age FROM users WHERE name = 'Bob'`;
    return sql;
}, [{ next_age: 35 }]);

// 4️⃣ $group aggregate
await runTest("Aggregate: group by city avgAge", () => {
    const sql = aggregate([
        { $group: { _id: "$city", avgAge: { $avg: "$age" } } },
        { $sort: { avgAge: 1 } },
    ])("users", "mysql");
    return sql;
}, [
    { _id: "Berlin", avgAge: 28.5 },
    { _id: "London", avgAge: 30 },
    { _id: "Paris", avgAge: 32.5 },
]);

// 5️⃣ $match before $group
await runTest("Aggregate: active users count per city", () => {
    const sql = aggregate([
        { $match: { active: true } },
        { $group: { _id: "$city", total: { $sum: 1 } } },
        { $sort: { _id: 1 } },
    ])("users", "mysql");
    return sql;
}, [
    { _id: "Berlin", total: 1 },
    { _id: "London", total: 1 },
    { _id: "Paris", total: 2 },
]);

// 6️⃣ $match after $group (HAVING)
await runTest("Aggregate: HAVING avgAge >= 30", () => {
    const sql = aggregate([
        { $group: { _id: "$city", avgAge: { $avg: "$age" } } },
        { $match: { avgAge: { $gte: 30 } } },
        { $sort: { avgAge: 1 } },
    ])("users", "mysql");
    return sql;
}, [
    { _id: "London", avgAge: 30 },
    { _id: "Paris", avgAge: 32.5 },
]);

// 7️⃣ JSON field aggregation
await runTest("Aggregate: avg JSON score by country", () => {
    const sql = aggregate([
        { $group: { _id: "$profile.country", avgScore: { $avg: "$profile.score" } } },
        { $sort: { avgScore: 1 } },
    ])("users", "mysql");
    return sql;
}, [
    { _id: "Germany", avgScore: 74 },
    { _id: "France", avgScore: 90 },
    { _id: "UK", avgScore: 90 },
]);

// 8️⃣ $count test
await runTest("Aggregate: count adult users", () => {
    const sql = aggregate([
        { $match: { age: { $gte: 25 } } },
        { $count: "totalAdults" },
    ])("users", "mysql");
    return sql;
}, [{ totalAdults: 4 }]);

// 9️⃣ $limit + $skip + $sort
await runTest("Aggregate: youngest users sorted", () => {
    const sql = aggregate([
        { $sort: { age: 1 } },
        { $skip: 1 },
        { $limit: 2 },
    ])("users", "mysql");
    return sql;
}, [
    { id: 1, name: "Alice", age: 25, city: "Paris", active: 1, profile: { "country": "France", "score": 85 } },
    { id: 2, name: "Bob", age: 30, city: "London", active: 1, profile: { "country": "UK", "score": 90 } },
]);

// 🔟 Complex with JSON HAVING
await runTest("Aggregate: countries with avgScore >= 80", () => {
    const sql = aggregate([
        { $group: { _id: "$profile.country", avgScore: { $avg: "$profile.score" } } },
        { $match: { avgScore: { $gte: 80 } } },
    ])("users", "mysql");
    return sql;
}, [
    { _id: "France", avgScore: 90 },
    { _id: "UK", avgScore: 90 },
]);

console.log("\n✅ All MySQL/PlanetScale aggregate and JSON tests completed.\n");

await conn.end();
