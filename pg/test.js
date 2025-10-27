import pkg from "pg";
const { Client } = pkg;
import assert from "assert";
import { filter, aggregate, expression } from "../src/index.js";
import dotenv from "dotenv";
dotenv.config();

// === Connect ===
const client = new Client({
    host: process.env.PG_HOST,
    port: process.env.PG_PORT || 5432,
    user: process.env.PG_USER,
    password: process.env.PG_PASSWORD,
    database: process.env.PG_DB,
});
await client.connect();

// === Reset Schema ===
await client.query(`DROP TABLE IF EXISTS orders`);
await client.query(`DROP TABLE IF EXISTS users`);

await client.query(`
  CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    name TEXT,
    age INT,
    city TEXT,
    active BOOLEAN,
    profile JSONB
  );
`);

await client.query(`
  CREATE TABLE orders (
    id SERIAL PRIMARY KEY,
    user_id INT REFERENCES users(id),
    amount NUMERIC,
    status TEXT
  );
`);

// === Insert users ===
const users = [
    ["Alice", 25, "Paris", true, { country: "France", score: 85 }],
    ["Bob", 30, "London", true, { country: "UK", score: 90 }],
    ["Charlie", 22, "Berlin", false, { country: "Germany", score: 60 }],
    ["David", 40, "Paris", true, { country: "France", score: 95 }],
    ["Eve", 35, "Berlin", true, { country: "Germany", score: 88 }],
];
for (const [name, age, city, active, profile] of users) {
    await client.query(
        `INSERT INTO users (name, age, city, active, profile) VALUES ($1, $2, $3, $4, $5)`,
        [name, age, city, active, profile]
    );
}

// === Insert orders ===
const orders = [
    [1, 120.5, "paid"],
    [1, 80.0, "pending"],
    [2, 200.0, "paid"],
    [3, 150.0, "cancelled"],
    [4, 300.0, "paid"],
    [5, 180.0, "paid"],
];
for (const [user_id, amount, status] of orders) {
    await client.query(
        `INSERT INTO orders (user_id, amount, status) VALUES ($1, $2, $3)`,
        [user_id, amount, status]
    );
}

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
// === Test Runner ===
async function runTest(title, queryFn, expected) {
    const sql = typeof queryFn === "function" ? queryFn() : queryFn;
    try {
        const res = await client.query(sql);
        const actual = normalize(res.rows);
        assert.deepStrictEqual(actual, expected);
        console.log(`✅ PASS: ${title}`);
    } catch (err) {
        console.error(`❌ FAIL: ${title}`);
        console.error("SQL:", sql);
        console.error("Expected:", expected);
        console.error("Actual:", err.rows || err.message);
        process.exitCode = 1;
    }
}

// === Tests ===

// 1️⃣ Filter simple
await runTest("Filter: active users in Paris", () => {
    const sql = `SELECT name, age, city FROM users WHERE ${filter(
        { city: "Paris", active: true },
        "pg"
    )}`;
    return sql;
}, [
    { name: "Alice", age: 25, city: "Paris" },
    { name: "David", age: 40, city: "Paris" },
]);

// 2️⃣ Filter JSON path
await runTest("Filter: users from France (JSON field)", () => {
    const sql = `SELECT name FROM users WHERE ${filter(
        { "profile.country": "France" },
        "pg"
    )}`;
    return sql;
}, [
    { name: "Alice" },
    { name: "David" },
]);

// 3️⃣ Expression test
await runTest("Expression: computed age + 5", () => {
    const sql = `SELECT ${expression({ $add: ["$age", 5] }, "pg")} AS next_age FROM users WHERE name = 'Bob'`;
    return sql;
}, [{ next_age: 35 }]);

// 4️⃣ $group aggregate
await runTest("Aggregate: group by city avgAge", () => {
    const sql = aggregate([
        { $group: { _id: "$city", avgAge: { $avg: "$age" } } },
        { $sort: { avgAge: 1 } },
    ])("users", "pg");
    return sql;
}, [
    { _id: "Berlin", avgage: 28.5 },
    { _id: "London", avgage: 30 },
    { _id: "Paris", avgage: 32.5 },
]);

// 5️⃣ $match before $group
await runTest("Aggregate: active users count per city", () => {
    const sql = aggregate([
        { $match: { active: true } },
        { $group: { _id: "$city", total: { $sum: 1 } } },
        { $sort: { _id: 1 } },
    ])("users", "pg");
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
    ])("users", "pg");
    return sql;
}, [
    { _id: "London", avgage: 30 },
    { _id: "Paris", avgage: 32.5 },
]);

// 7️⃣ JSON field aggregation
await runTest("Aggregate: avg JSON score by country", () => {
    const sql = aggregate([
        { $group: { _id: "$profile.country", avgScore: { $avg: "$profile.score" } } },
        { $sort: { avgScore: 1 } },
    ])("users", "pg");
    return sql;
}, [
    { _id: "Germany", avgscore: 74 },
    { _id: "France", avgscore: 90 },
    { _id: "UK", avgscore: 90 },
]);

// 8️⃣ $count test
await runTest("Aggregate: count adult users", () => {
    const sql = aggregate([
        { $match: { age: { $gte: 25 } } },
        { $count: "totalAdults" },
    ])("users", "pg");
    return sql;
}, [{ totaladults: 4 }]);

// 9️⃣ $limit + $skip + $sort
await runTest("Aggregate: youngest users sorted", () => {
    const sql = aggregate([
        { $sort: { age: 1 } },
        { $skip: 1 },
        { $limit: 2 },
    ])("users", "pg");
    return sql;
}, [
    { id: 1, name: "Alice", age: 25, city: "Paris", active: true, profile: { country: "France", score: 85 } },
    { id: 2, name: "Bob", age: 30, city: "London", active: true, profile: { country: "UK", score: 90 } },
]);

// 🔟 Complex with JSON HAVING
await runTest("Aggregate: countries with avgScore >= 80", () => {
    const sql = aggregate([
        { $group: { _id: "$profile.country", avgScore: { $avg: "$profile.score" } } },
        { $match: { avgScore: { $gte: 80 } } },
    ])("users", "pg");
    return sql;
}, [
    { _id: "France", avgscore: 90 },
    { _id: "UK", avgscore: 90 },
]);

console.log("\n✅ All PostgreSQL aggregate and JSON tests completed.\n");
await client.end();
