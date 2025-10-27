
import { collection, db } from './index.js';

const users = collection('users', 'pg');

// ============================================
// FIND Operations
// ============================================


[
    users.find({ age: { $gte: 25 } }).toSQL(),
    // "SELECT * FROM users WHERE age >= 25"

    users.find({ city: 'Paris' }, { name: 1, age: 1 }).toSQL(),
    // "SELECT name, age FROM users WHERE city = 'Paris'"

    users.find({ active: true }, ['name', 'email']).toSQL(),
    // "SELECT name, email FROM users WHERE active = TRUE"  

    users.find({ age: { $gte: 18 } })
        .select({ name: 1, age: 1, city: 1 })
        .sort({ age: -1, name: 1 })
        .skip(10)
        .limit(5)
        .toSQL(),
    // "SELECT name, age, city FROM users WHERE age >= 18 
    // ORDER BY age DESC, name ASC LIMIT 5 OFFSET 10"

    users.find({ 'profile.country': 'France' })
        .select(['name', 'profile.score'])
        .sort({ 'profile.score': -1 })
        .toSQL(),
    // "SELECT name, profile->>'score' AS profile_score FROM users WHERE profile->>'country' = 'France' ORDER BY profile->>'score' DESC"    
    users.find({ age: { $gte: 18 } })
        .select(['city'])
        .distinct()
        .toSQL(),
    // "SELECT DISTINCT city FROM users WHERE age >= 18"
    users.findOne({ email: 'a@a.com' }).toSQL(),
    // "SELECT * FROM users WHERE email = '


    users.find({ active: true }).count().toSQL(),
    // "SELECT COUNT(*) AS count FROM users WHERE active = TRUE"

    users.countDocuments({ city: 'Paris' }),
    // "SELECT COUNT(*) AS count FROM users WHERE city = 'Paris'"

    // ============================================
    // INSERT Operations
    // ============================================

    users.insertOne({
        name: 'Alice',
        age: 25,
        email: 'a@a.com',
    }),
    // "INSERT INTO users (name, age, email) 
    // VALUES ('Alice', 25, '   


    users.insertOne({
        name: 'Bob',
        age: 30
    }, { returning: ['id', 'name'] }),
    // "INSERT INTO users (name, age) VALUES ('Bob', 30) 
    // RETURNING id, name"
    users.insertMany([
        { name: 'Charlie', age: 22 },
        { name: 'David', age: 28 },
        { name: 'Eve', age: 35 }
    ]),
    // "INSERT INTO users (name, age) VALUES 
    // ('Charlie', 22), ('David', 28), ('Eve', 35)"
    users.insertOne({
        name: 'Frank',
        profile: { country: 'USA', score: 85 }
    }, { returning: '*' }),
    // "INSERT INTO users (name, profile) VALUES 
    // ('Frank', '{"country":"USA","score":85}'::jsonb) RETURNING *"    
    // ============================================
    // UPDATE Operations
    // ============================================
    users.updateOne(
        { name: 'Alice' },
        { $set: { age: 26, city: 'London' } }
    ),
    // "UPDATE users SET age = 26, city = 'London' 
    // WHERE name = 'Alice' LIMIT 1"
    users.updateMany(
        { age: { $lt: 18 } },
        { $set: { status: 'minor' } }
    ),
    // "UPDATE users SET status = 'minor' WHERE age < 18"
    users.updateMany(
        { active: true },
        {
            $inc: { loginCount: 1 },
            $set: { lastSeen: new Date() },
            $currentDate: { updatedAt: true }
        }
    ),
    // "UPDATE users SET loginCount = loginCount + 1,
    // lastSeen = '2024-06-01 12:00:00',
    // updatedAt = CURRENT_TIMESTAMP WHERE active = TRUE"
    users.updateOne(
        { id: 1 },
        { $set: { 'profile.score': 95 } }
    ),
    // "UPDATE users SET profile = jsonb_set(profile, '{score}', '95'::jsonb) 
    // WHERE id = 1"
    users.updateMany(
        { city: 'Paris' },
        { $inc: { points: 10 } },
        { returning: ['id', 'points'] }
    ),
    // "UPDATE users SET points = points + 10 
    // WHERE city = 'Paris' RETURNING id, points"
    // ============================================
    // DELETE Operations
    // ============================================
    users.deleteOne({ email: 'a@a.com' }),
    // "DELETE FROM users WHERE email = '
    users.deleteMany({ active: false }),
    // "DELETE FROM users WHERE active = FALSE"
    users.deleteMany({}, { allowDeleteAll: true }),
    // "DELETE FROM users"
    users.deleteMany(
        { age: { $lt: 13 } },
        { returning: ['id', 'name'] }
    ),
    // "DELETE FROM users WHERE age < 13 RETURNING id, name"
    // ============================================
    // AGGREGATE Operations
    // ============================================
    users.aggregate([
        { $match: { active: true } },
        {
            $group: {
                _id: '$city',
                avgAge: { $avg: '$age' },
                count: { $sum: 1 }
            }
        },
        { $sort: { avgAge: -1 } },
        { $limit: 10 }
    ]),
    // "SELECT city AS _id, AVG(age) AS avgAge, SUM(1) AS count 
    // FROM (SELECT * FROM users WHERE active = TRUE) AS t1
    // GROUP BY city ORDER BY avgAge DESC LIMIT 10",
    // ============================================
    // DISTINCT Operations
    // ============================================
    users.distinct('city', { active: true }),
    // "SELECT DISTINCT city FROM users WHERE active = TRUE"

].map((query) => console.log(query));
// ============================================
// Alternative API (db function)
// ============================================ 
const Products = db('products', 'mysql');

Products.find({ price: { $gte: 100 } })
    .sort({ price: 1 })
    .limit(20)
    .toSQL();

// "SELECT * FROM products WHERE price >= 100
// ORDER BY price ASC LIMIT 20"