import Database from "better-sqlite3";
import { collection } from '../src/index.js';

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


const users = collection('users', 'sqlite')

db.prepare(users.insertMany([
    { name: 'Alice', age: 25, city: 'Paris', active: true, profile: JSON.stringify({ country: 'France', score: 85 }) },
    { name: 'Bob', age: 30, city: 'London', active: true, profile: JSON.stringify({ country: 'UK', score: 90 }) },
    { name: 'Charlie', age: 22, city: 'Berlin', active: false, profile: JSON.stringify({ country: 'Germany', score: 60 }) },
    { name: 'David', age: 40, city: 'Paris', active: true, profile: JSON.stringify({ country: 'France', score: 95 }) },
    { name: 'Eve', age: 35, city: 'Berlin', active: true, profile: JSON.stringify({ country: 'Germany', score: 88 }) },
])).run();


const orders = collection('orders', 'sqlite');

db.prepare(orders.insertMany([
    { user_id: 1, amount: 120.5, status: 'paid' },
    { user_id: 1, amount: 80.0, status: 'pending' },
    { user_id: 2, amount: 200.0, status: 'paid' },
    { user_id: 3, amount: 150.0, status: 'cancelled' },
    { user_id: 4, amount: 300.0, status: 'paid' },
    { user_id: 5, amount: 180.0, status: 'paid' },
])).run();

// ============================================
// find Operations
// ============================================


console.log(
    db.prepare(users.find({ age: { $gte: 35 } }).limit(1).toSQL()).all()
);


console.log(
    db.prepare(users.aggregate([

        { $group: { _id: "$profile.country", avgScore: { $avg: "$profile.score" } } },
        { $sort: { avgScore: 1 } },
        // { $match: { active: true } },
        // { $group: { _id: '$city', avgAge: { $avg: '$age' } } },
        // { $sort: { avgAge: -1 } }
    ])).all()

)
// console.log();




// "SELECT DISTINCT city FROM users WHERE age >= 18"