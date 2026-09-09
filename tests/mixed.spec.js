import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers, jsonPath, escape } from '../index.js';
import { runStringTest } from './common.js';

const { expression, filter, extend, aggregate } = createQueryBuilder({
    filterOps,
    exprOps,
    updateOps,
    stageHandlers,
});
const eq = (a, b, keys, ctor) => a === b || (
    a && b && (ctor = a.constructor) === b.constructor
        ? ctor === Array ? a.length === b.length && a.every((val, idx) => eq(val, b[idx]))
            : ctor === Object && (keys = ctor.keys(a)).length === ctor.keys(b).length && keys.every((k) => k in b && eq(a[k], b[k]))
        : (a !== a && b !== b)
);

const describe = (title, call) => {
    console.log(title);
    call();
}

const runTests = (testCases) => {
    testCases.forEach(testCase => {
        runStringTest(testCase.title, testCase.input, testCase.expected);
    });
}

describe('Filter Test', () => {
    runTests([
        // Test equality operator
        {
            title: '$eq',
            input: () => (filter({ age: 25 })),
            expected: 'age = 25'
        },
        {
            title: '$eq',
            input: () => (filter({ age: { $eq: 25 } })),
            expected: 'age = 25'
        },

        // Test inequality operator
        {
            title: '$ne',
            input: () => (filter({ age: { $ne: 25 } })),
            expected: 'age != 25'
        },

        // Test greater than operator
        {
            title: '$gt',
            input: () => (filter({ age: { $gt: 25 } })),
            expected: 'age > 25'
        },

        // Test greater than or equal to operator
        {
            title: '$gte',
            input: () => (filter({ age: { $gte: 25 } })),
            expected: 'age >= 25'
        },

        // Test less than operator
        {
            title: '$lt',
            input: () => (filter({ age: { $lt: 25 } })),
            expected: 'age < 25'
        },

        // Test less than or equal to operator
        {
            title: '$lte',
            input: () => (filter({ age: { $lte: 25 } })),
            expected: 'age <= 25'
        },

        // Test 'IN' operator
        {
            title: '$in',
            input: () => (filter({ age: { $in: [25, 30, 35] } })),
            expected: 'age IN (25, 30, 35)'
        },

        // Test 'NOT IN' operator
        {
            title: '$nin',
            input: () => (filter({ age: { $nin: [25, 30, 35] } })),
            expected: 'age NOT IN (25, 30, 35)'
        },

        // Test logical AND operator
        {
            title: '$and',
            input: () => (filter({ $and: [{ age: { $gte: 25 } }, { age: { $lte: 30 } }] })),
            expected: '(age >= 25 AND age <= 30)'
        },

        // Test logical OR operator
        {
            title: '$or',
            input: () => (filter({ $or: [{ age: { $lt: 20 } }, { age: { $gte: 30 } }] })),
            expected: '(age < 20 OR age >= 30)'
        },

        // Test logical NOT operator
        {
            title: '$not',
            input: () => (filter({ $not: { age: { $gte: 25 } } })),
            expected: 'NOT (age >= 25)'
        },

        // Test existence operator (exists true)
        {
            title: '$exists (true)',
            input: () => (filter({ age: { $exists: true } })),
            expected: 'age IS NOT NULL'
        },

        // Test existence operator (exists false)
        {
            title: '$exists (false)',
            input: () => (filter({ age: { $exists: false } })),
            expected: 'age IS NULL'
        },

        {
            title: '$expr',
            input: () => (filter({ $expr: { $eq: ['$age', 25] } })),
            expected: '(age = 25)'
        },

        // Test size operator
        // {
        //     title: '$size',
        //     input: () => (filter({ list: { $size: 3 } })),
        //     expected: 'json_array_length(list) = 3'
        // },

        // Test like operator
        {
            title: '$like',
            input: () => (filter({ name: { $like: '%John%' } })),
            expected: "name LIKE '%John%'"
        },

        // Test ilike operator
        {
            title: '$ilike',
            input: () => (filter({ name: { $ilike: '%john%' } })),
            expected: "LOWER(name) LIKE LOWER('%john%')"
        },

        // Test not like operator
        {
            title: '$nlike',
            input: () => (filter({ name: { $nlike: '%John%' } })),
            expected: "name NOT LIKE '%John%'"
        },

        // Test not ilike operator
        {
            title: '$nilike',
            input: () => (filter({ name: { $nilike: '%john%' } })),
            expected: "LOWER(name) NOT LIKE LOWER('%john%')"
        },
        {
            title: 'nested complex sqlite and mysql operators',
            input: () => filter({
                $and: [
                    { 'profile.age': { $gte: 25 } },
                    {
                        $or: [
                            { 'profile.name': { $like: '%John%' } },
                            { 'profile.age': { $gt: 30 } }
                        ]
                    }
                ]
            }
            ),
            expected: "(json_extract(profile, '$.age') >= 25 AND (json_extract(profile, '$.name') LIKE '%John%' OR json_extract(profile, '$.age') > 30))"
        },
        {
            title: 'nested complex pg',
            input: () => filter({
                $and: [
                    { 'profile.age': { $gte: 25 } },
                    {
                        $or: [
                            { 'profile.name': { $ilike: '%john%' } },
                            { 'profile.age': { $gt: 30 } }
                        ]
                    }
                ]
            }, "pg"),
            expected: />= 25.*ILIKE '%john%'.*> 30/
        }

        // Test elemMatch operator (SQLite)
        // {
        //     title: '$elemMatch (SQLite)',
        //     input: () => (filter({ items: { $elemMatch: { price: { $gt: 20 } } } })),
        //     expected: 'EXISTS (SELECT 1 FROM json_each(items, '$') WHERE (type = 'integer' AND value > 20))'
        // },

        // // Test elemMatch operator (PostgreSQL)
        // {
        //     title: '$elemMatch (PostgreSQL)',
        //     input: () => (filter({ items: { $elemMatch: { price: { $gt: 20 } } } }, 'pg')),
        //     expected: '(items::json #> '{price}') > 20'
        // },

        // Test all operator (SQLite)
        // {
        //     title: '$all (SQLite)',
        //     input: () => (filter({ items: { $all: [{ price: { $gte: 10 } }, { price: { $lte: 50 } }] } })),
        //     expected: '(price >= 10 AND price <= 50)'
        // },       
    ]);
});

describe('Expression Test', () => {
    runTests([
        {
            title: '$add',
            input: () => expression({ $add: [1, 2, 3] }),
            expected: '(1 + 2 + 3)'
        },
        {
            title: '$subtract',
            input: () => expression({ $subtract: [10, 5] }),
            expected: '(10 - 5)'
        },
        {
            title: '$multiply',
            input: () => expression({ $multiply: [2, 3] }),
            expected: '(2 * 3)'
        },
        {
            title: '$divide',
            input: () => expression({ $divide: [10, 2] }),
            expected: '(10 * 1.0 / NULLIF(2, 0))'
        },
        {
            title: '$concat',
            input: () => expression({ $concat: ['Hello', ' ', 'World'] }),
            expected: "('Hello' || ' ' || 'World')"
        },
        {
            title: '$min',
            input: () => expression({ $min: [10, 20, 30] }),
            expected: 'LEAST(10, 20, 30)'
        },
        {
            title: '$max',
            input: () => expression({ $max: [10, 20, 30] }),
            expected: 'GREATEST(10, 20, 30)'
        },
        {
            title: '$avg',
            input: () => expression({ $avg: [10, 20, 30] }),
            expected: 'AVG(10)'
        },
        {
            title: '$sum',
            input: () => expression({ $sum: [10, 20, 30] }),
            expected: 'SUM(10)'
        },
        {
            title: '$cond 1',
            input: () => expression({ $cond: [{ $gt: [10, 5] }, 'yes', 'no'] }),
            expected: "CASE WHEN (10 > 5) THEN 'yes' ELSE 'no' END"
        },
        {
            title: '$cond 2',
            input: () => expression({ $cond: [{ $and: [{ $gt: ['$x', 1] }, { $lt: ['$x', 10] }] }, "discount", "no discount"] }),
            expected: "CASE WHEN ((x > 1) AND (x < 10)) THEN 'discount' ELSE 'no discount' END"
        },
        {
            title: '$switch 1',
            input: () => expression({ $switch: [{ branches: [{ case: { $eq: [1, 1] }, then: 'equal' }], default: 'not equal' }] }),
            expected: "CASE WHEN (1 = 1) THEN 'equal' ELSE 'not equal' END"
        },
        {
            title: '$min with field',
            input: () => expression({ $min: ['$age', 18] }),
            expected: "LEAST(age, 18)"
        },
        {
            title: '$concat with fields',
            input: () => expression({ $concat: ["$firstName", " ", "hello", "$lastName"] }),
            expected: `(firstName || ' ' || 'hello' || lastName)`
        },
        {
            title: '$add with field',
            input: () => expression({ $add: [1, 2, "$some"] }),
            expected: "(1 + 2 + some)"
        },
        {
            title: '$subtract with field',
            input: () => expression({ $multiply: [365, "$age", 90] }),
            expected: "(365 * age * 90)"
        },
        {
            title: '$and',
            input: () => expression({ $and: [{ $multiply: ["$age", 365] }, { $concat: ["$firstName", " ", "$lastName"] }] }),
            expected: "((age * 365) AND (firstName || ' ' || lastName))"
        },
        {
            title: 'mixed with field',
            input: () => expression({ $add: [{ $multiply: ["$a", { $subtract: ["$b", "$c"] }] }, "$d"] }),
            expected: "((a * (b - c)) + d)"
        },
        {
            title: 'mixed 1',
            input: () => expression({ $add: [{ $multiply: [5, { $subtract: [10, 2] }] }, 8] }),
            expected: "((5 * (10 - 2)) + 8)"
        },
        {
            title: 'mixed 2',
            input: () => expression({ $concat: ["$field1", "*", "$field2", "=", { $multiply: ["$field1", "$field2"] }] }),
            expected: "(field1 || '*' || field2 || '=' || (field1 * field2))"
        },
        {
            title: '$exists 1',
            input: () => expression({ $exists: ["$age", false] }),
            expected: "(age IS NULL)"
        },
        {
            title: '$exists 2',
            input: () => expression({ $exists: ["$age", true] }),
            expected: "(age IS NOT NULL)"
        },
        // $eq, $ne, $lt, $gt, $gte, $lte, $in, $nin, $and, $or, $not
        {
            title: '$eq',
            input: () => expression({ $eq: [1, 2] }),
            expected: "(1 = 2)"
        },
        {
            title: '$eq with field',
            input: () => expression({ $eq: ["$age", 18] }),
            expected: "(age = 18)"
        },
        {
            title: '$eq with nested field sqlite and mysql',
            input: () => expression({ $eq: ["$address.city", "New York"] }),
            expected: "(json_extract(address, '$.city') = 'New York')"
        },
        {
            title: '$eq with nested field pg',
            input: () => expression({ $eq: ["$address.city", "New York"] }, "pg"),
            expected: "((address::jsonb #>> '{city}') = 'New York')"
        },
        {
            title: '$eq with nested field and operator sqlite, mysql',
            input: () => expression({ $eq: ["$address.city", { $eq: ["$address.state", "NY"] }] }),
            expected: "(json_extract(address, '$.city') = (json_extract(address, '$.state') = 'NY'))"
        },
        {
            title: '$eq with nested field and operator pg',
            input: () => expression({ $eq: ["$address.city", { $eq: ["$address.state", "NY"] }] }, "pg"),
            expected: "((address::jsonb #>> '{city}') = ((address::jsonb #>> '{state}') = 'NY'))"
        },
        {
            title: '$ne',
            input: () => expression({ $ne: [1, 2] }),
            expected: "(1 <> 2)"
        },
        {
            title: '$ne with field',
            input: () => expression({ $ne: ["$age", 18] }),
            expected: "(age <> 18)"
        },
        {
            title: '$lt',
            input: () => expression({ $lt: [1, 2] }),
            expected: "(1 < 2)"
        },
        {
            title: '$lt with field',
            input: () => expression({ $lt: ["$age", 18] }),
            expected: "(age < 18)"
        },
        {
            title: '$lte',
            input: () => expression({ $lte: [1, 2] }),
            expected: "(1 <= 2)"
        },
        {
            title: '$lte with field',
            input: () => expression({ $lte: ["$age", 18] }),
            expected: "(age <= 18)"
        },
        {
            title: '$in',
            input: () => expression({ $in: [1, [2, 3]] }),
            expected: "(1 IN (2, 3))"
        },
        {
            title: '$in nested field sqlite, mysql',
            input: () => expression({ $in: ["$address.city", ["New York", "Los Angeles"]] }),
            expected: "(json_extract(address, '$.city') IN ('New York', 'Los Angeles'))"
        },
        {
            title: '$in nested field',
            input: () => expression({ $in: ["$address.city", ["New York", "Los Angeles"]] }, "pg"),
            expected: "((address::jsonb #>> '{city}') IN ('New York', 'Los Angeles'))"
        },
        {
            title: '$in nested field and operator sqlite, mysql',
            input: () => expression({ $in: ["$address.city", [{ $eq: ["$address.state", "NY"] }]] }),
            expected: "(json_extract(address, '$.city') IN ((json_extract(address, '$.state') = 'NY')))"
        },
        {
            title: '$in nested field and operator pg',
            input: () => expression({ $in: ["$address.city", [{ $eq: ["$address.state", "NY"] }]] }, "pg"),
            expected: "((address::jsonb #>> '{city}') IN (((address::jsonb #>> '{state}') = 'NY')))"
        },
        {
            title: '$in with field',
            input: () => expression({ $in: ["$age", [18, 20, 22]] }),
            expected: "(age IN (18, 20, 22))"
        },
        {
            title: '$nin',
            input: () => expression({ $nin: [1, [2, 3]] }),
            expected: "(1 NOT IN (2, 3))"
        },
        {
            title: '$nin with field',
            input: () => expression({ $nin: ["$age", [18, 20, 22]] }),
            expected: "(age NOT IN (18, 20, 22))"
        },
        {
            title: '$and',
            input: () => expression({ $and: [{ $eq: [1, 2] }, { $lt: [3, 4] }] }),
            expected: "((1 = 2) AND (3 < 4))"
        },
        {
            title: '$and with field',
            input: () => expression({ $and: [{ $eq: ["$age", 18] }, { $lt: ["$age", 20] }] }),
            expected: "((age = 18) AND (age < 20))"
        },
        {
            title: '$or',
            input: () => expression({ $or: [{ $eq: [1, 2] }, { $lt: [3, 4] }] }),
            expected: "((1 = 2) OR (3 < 4))"
        },
        {
            title: '$or with field',
            input: () => expression({ $or: [{ $eq: ["$age", 18] }, { $lt: ["$age", 20] }] }),
            expected: "((age = 18) OR (age < 20))"
        },
        {
            title: '$not',
            input: () => expression({ $not: { $eq: [1, 2] } }),
            expected: "(NOT (1 = 2))"
        },
        {
            title: '$not with field',
            input: () => expression({ $not: { $eq: ["$age", 18] } }),
            expected: "(NOT (age = 18))"
        },
        {
            title: '$and with nested field sqlite, mysql',
            input: () => expression({ $and: [{ $eq: ["$address.city", "New York"] }, { $eq: ["$address.state", "NY"] }] }, "pg"),
            expected: "(((address::jsonb #>> '{city}') = 'New York') AND ((address::jsonb #>> '{state}') = 'NY'))"
        },
        {
            title: '$and with nested field pg',
            input: () => expression({ $and: [{ $eq: ["$address.city", "New York"] }, { $eq: ["$address.state", "NY"] }] }, "pg"),
            expected: "(((address::jsonb #>> '{city}') = 'New York') AND ((address::jsonb #>> '{state}') = 'NY'))"
        },
        {
            title: 'nested complex expression sqlite and mysql',
            input: () => expression({ $and: [{ $eq: ["$address.city", "New York"] }, { $eq: ["$address.state", "NY"] }] }),
            expected: "((json_extract(address, '$.city') = 'New York') AND (json_extract(address, '$.state') = 'NY'))"
        },
        {
            title: 'nested complex expression pg',
            input: () => expression({ $and: [{ $eq: ["$address.city", "New York"] }, { $eq: ["$address.state", "NY"] }] }, "pg"),
            expected: "(((address::jsonb #>> '{city}') = 'New York') AND ((address::jsonb #>> '{state}') = 'NY'))"
        }
    ]);
});

describe('Pipleline Test', () => {
    runTests([
        {
            title: '$project',
            input: () => aggregate(
                [
                    { $project: { name: 1, age: 1 } },
                ],
            )("users"),
            expected: "SELECT name AS name, age AS age FROM (SELECT * FROM users) AS t1"
        },
        {
            title: '$project nested field sqlite and mysql',
            input: () => aggregate(
                [
                    { $project: { street: '$address.street', city: '$address.city' } },
                ],
            )("users"),
            expected: "SELECT json_extract(address, '$.street') AS street, json_extract(address, '$.city') AS city FROM (SELECT * FROM users) AS t1"
        },
        {
            title: '$project nested field pg',
            input: () => aggregate(
                [
                    { $project: { street: '$address.street', city: '$address.city' } },
                ],
            )("users", "pg"),
            expected: "SELECT (address::jsonb #>> '{street}') AS street, (address::jsonb #>> '{city}') AS city FROM (SELECT * FROM users) AS t1"
        },
        {
            title: '$match',
            input: () => aggregate(
                [
                    { $match: { age: { $gt: 18 } } },
                ],
            )("users"),
            expected: "SELECT * FROM users WHERE age > 18",
        },
        {
            title: '$match nested field sqlite and mysql',
            input: () => aggregate(
                [
                    { $match: { 'profile.age': { $gt: 18 } } },
                ],
            )("users"),
            expected: "SELECT * FROM users WHERE json_extract(profile, '$.age') > 18",
        },
        {
            title: '$match nested field pg',
            input: () => aggregate(
                [
                    { $match: { 'profile.age': { $gt: 18 } } },
                ],
            )("users", "pg"),
            expected: "SELECT * FROM users WHERE ((profile::jsonb #>> '{age}'))::numeric > 18",
        },
        {
            title: '$group',
            input: () => aggregate(
                [
                    { $group: { _id: '$age' } },
                ],
            )("users"),
            expected: "SELECT age AS _id FROM (SELECT * FROM users) AS t1 GROUP BY age",
        },
        {
            title: '$group nested fields sqlite and mysql',
            input: () => aggregate(
                [
                    { $group: { _id: '$profile.age' } },
                ],
            )("users"),
            expected: "SELECT json_extract(profile, '$.age') AS _id FROM (SELECT * FROM users) AS t1 GROUP BY json_extract(profile, '$.age')",
        },
        {
            title: '$group nested field pg',
            input: () => aggregate(
                [
                    { $group: { _id: '$profile.age' } },
                ],
            )("users", "pg"),
            expected: "SELECT (profile::jsonb #>> '{age}') AS _id FROM (SELECT * FROM users) AS t1 GROUP BY (profile::jsonb #>> '{age}')"
        },
        {
            title: '$sort',
            input: () => aggregate(
                [
                    { $sort: { age: -1 } },
                ],
            )("users"),
            expected: "SELECT * FROM users ORDER BY age DESC",
        },
        {
            title: '$sort nested fields sqlite and mysql',
            input: () => aggregate(
                [
                    { $sort: { 'profile.age': -1 } },
                ],
            )("users"),
            expected: "SELECT * FROM users ORDER BY json_extract(profile, '$.age') DESC",
        },
        {
            title: '$sort nested field pg',
            input: () => aggregate(
                [
                    { $sort: { 'profile.age': -1 } },
                ],
            )("users", "pg"),
            expected: "SELECT * FROM users ORDER BY (profile::jsonb #>> '{age}') DESC"
        },
        {
            title: '$limit',
            input: () => aggregate(
                [
                    { $limit: 5 },
                ],
            )("users"),
            expected: "SELECT * FROM users LIMIT 5",
        },
        {
            title: '$skip',
            input: () => aggregate(
                [
                    { $limit: 5 },
                    { $skip: 2 },
                ],
            )("users"),
            expected: "SELECT * FROM users LIMIT 5 OFFSET 2",
        },
        {
            title: '$count',
            input: () => aggregate(
                [
                    { $count: "totalUsers" },
                ],
            )("users"),
            expected: "SELECT COUNT(*) AS totalUsers FROM (SELECT * FROM users) AS t1",
        }
    ])
});

describe('JSONPath Test', () => {
    runTests([
        {
            title: 'Empty path',
            input: () => jsonPath(''),
            expected: "",
        },
        {
            title: 'Simple path with default database type',
            input: () => jsonPath('user.name', 'sqlite'),
            expected: "json_extract(user, '$.name')",
        },
        {
            title: 'Path with nested objects and default database type',
            input: () => jsonPath('user.address.city', 'sqlite'),
            expected: "json_extract(user, '$.address.city')",
        },
        {
            title: 'Path with arrays and default database type',
            input: () => jsonPath('user.orders.0.item'),
            expected: "json_extract(user, '$.orders[0].item')",
        },
        {
            title: 'Path with arrays and multiple indexes and default database type',
            input: () => jsonPath('user.orders.0.items.1.name', 'sqlite'),
            expected: "json_extract(user, '$.orders[0].items[1].name')",
        },
        {
            title: 'Path with no dot notation and default database type',
            input: () => jsonPath('user'),
            expected: "user",
        },
        {
            title: 'PostgreSQL database type with simple path',
            input: () => jsonPath('user.name', 'pg'),
            expected: "(user::jsonb #>> '{name}')",
        },
        {
            title: 'PostgreSQL database type with nested objects',
            input: () => jsonPath('user.address.city', 'pg'),
            expected: "(user::jsonb #>> '{address,city}')",
        },
        {
            title: 'PostgreSQL database type with arrays',
            input: () => jsonPath('user.orders.0.item', 'pg'),
            expected: "(user::jsonb #>> '{orders,0,item}')",
        },
        {
            title: 'PostgreSQL database type with multiple array indexes',
            input: () => jsonPath('user.orders.0.items.1.name', 'pg'),
            expected: "(user::jsonb #>> '{orders,0,items,1,name}')",
        },
        {
            title: 'PostgreSQL database type with only array indexes',
            input: () => jsonPath('user.orders.0.items.1', 'pg'),
            expected: "(user::jsonb #>> '{orders,0,items,1}')",
        },
        // {
        //     input: () => jsonPath('user..address..city'),
        //     expected: "json_extract(user, '$.address.city')",
        //     title: 'Path with multiple consecutive dots should be normalized'
        // },
        {
            title: 'Path with special characters',
            input: () => jsonPath('user.special.chars.0.data'),
            expected: "json_extract(user, '$.special.chars[0].data')",
        },
        {
            title: 'PostgreSQL database type with nested objects',
            input: () => jsonPath('user.address.city', 'pg'),
            expected: "(user::jsonb #>> '{address,city}')",
        },
        {
            title: 'Path with nested objects',
            input: () => jsonPath('user.address.city'),
            expected: "json_extract(user, '$.address.city')",
        },
        {
            title: 'Path with arrays',
            input: () => jsonPath('user.orders.0.item'),
            expected: "json_extract(user, '$.orders[0].item')",
        },
        {
            title: 'Path with arrays and multiple indexes',
            input: () => jsonPath('user.orders.0.items.1.name'),
            expected: "json_extract(user, '$.orders[0].items[1].name')",
        }
    ]);
});

describe('Edge Case Tests', () => {
    runTests([
        // ✅ 1. String escaping with single quote
        {
            title: 'string escaping in filter',
            input: () => filter({ name: { $eq: "O'Reilly" } }),
            expected: "name = 'O''Reilly'",
        },
        {
            title: 'string escaping in expression',
            input: () => expression({ $eq: ["$author", "O'Reilly"] }),
            expected: "(author = 'O''Reilly')",
        },

        // ✅ 2. $ilike Postgres handling
        {
            title: '$ilike postgres variant',
            input: () => filter({ title: { $ilike: '%hello%' } }, 'pg'),
            expected: "title ILIKE '%hello%'",
        },
        {
            title: '$ilike sqlite fallback',
            input: () => filter({ title: { $ilike: '%hello%' } }, 'sqlite'),
            expected: "LOWER(title) LIKE LOWER('%hello%')",
        },

        // ✅ 3. Alias sanitization in $project
        {
            title: '$project alias sanitization',
            input: () =>
                aggregate([{ $project: { 'Total Amount($)': { $sum: '$amount' } } }])(
                    'orders'
                ),
            expected:
                "SELECT SUM(amount) AS Total_Amount___ FROM (SELECT * FROM orders) AS t1",
        },

        // ✅ 4. $group with _id: null (global aggregation)
        {
            title: '$group global aggregation no GROUP BY',
            input: () =>
                aggregate([{ $group: { _id: null, total: { $sum: '$amount' } } }])(
                    'orders'
                ),
            expected: "SELECT SUM(amount) AS total FROM (SELECT * FROM orders) AS t1",
        },

        // ✅ 5. multiple $match after $group (HAVING)
        {
            title: 'multiple $match after $group (HAVING logic)',
            input: () =>
                aggregate([
                    { $group: { _id: '$country', total: { $sum: '$amount' } } },
                    { $match: { total: { $gt: 100 } } },
                    { $match: { total: { $lt: 500 } } },
                ])('orders'),
            expected:
                "SELECT country AS _id, SUM(amount) AS total FROM (SELECT * FROM orders) AS t1 GROUP BY country HAVING SUM(amount) > 100 AND SUM(amount) < 500",
        },

        // ✅ 6. $switch logic
        {
            title: '$switch multiple branches',
            input: () =>
                expression({
                    $switch: [
                        {
                            branches: [
                                { case: { $eq: ['$score', 100] }, then: 'perfect' },
                                { case: { $gt: ['$score', 50] }, then: 'pass' },
                            ],
                            default: 'fail',
                        },
                    ],
                }),
            expected:
                "CASE WHEN (score = 100) THEN 'perfect' WHEN (score > 50) THEN 'pass' ELSE 'fail' END",
        },

        // ✅ 7. Custom operator (added dynamically)
        {
            title: 'add custom $between filter',
            input: () => {
                extend.filter({
                    // Check if field modulo value equals remainder
                    // Between operator
                    $between: (value, db, field) => {
                        if (!Array.isArray(value) || value.length !== 2) {
                            throw new Error('$between requires [min, max]');
                        }
                        return `BETWEEN ${escape(value[0], db)} AND ${escape(value[1], db)}`;
                    },
                });
                return filter({ age: { $between: [18, 30] } });
            },
            expected: 'age BETWEEN 18 AND 30',
        },

        {
            title: 'add custom $round expression',
            input: () => {
                extend.expression({
                    // Round operator
                    $round: (args, ctx) => `ROUND(${ctx.expr(args[0])})`,
                });
                return expression({ $round: ['$salary'] });
            },
            expected: "ROUND(salary)",
        },

        // ✅ 8. Escaped string inside complex expression
        {
            title: 'escaped string inside nested expression',
            input: () =>
                expression({
                    $concat: ["$author", " says: ", "O'Reilly"],
                }),
            expected: "(author || ' says: ' || 'O''Reilly')",
        },

        // ✅ 9. Complex HAVING with nested JSON
        {
            title: 'complex $group + $having with JSON path',
            input: () =>
                aggregate([
                    { $group: { _id: '$profile.country', avgAge: { $avg: '$profile.age' } } },
                    { $match: { avgAge: { $gte: 30 } } },
                ])('users', 'sqlite'),
            expected:
                "SELECT json_extract(profile, '$.country') AS _id, AVG(json_extract(profile, '$.age')) AS avgAge FROM (SELECT * FROM users) AS t1 GROUP BY json_extract(profile, '$.country') HAVING AVG(json_extract(profile, '$.age')) >= 30",
        },

        // ✅ 10. Field name with space or special characters
        {
            title: 'field alias sanitization with spaces',
            input: () =>
                aggregate([{ $project: { 'User Name': '$profile.name' } }])('users'),
            expected:
                "SELECT json_extract(profile, '$.name') AS User_Name FROM (SELECT * FROM users) AS t1",
        },
    ]);
});

describe('Aggregate Extended Tests', () => {
    runTests([
        // === 1. Simple project + match ===
        {
            title: '$project + $match basic',
            input: () => aggregate([
                { $project: { name: 1, age: 1 } },
                { $match: { age: { $gt: 18 } } }
            ])('users'),
            expected:
                "SELECT name AS name, age AS age FROM (SELECT * FROM users) AS t1 WHERE age > 18",
        },
        {
            title: '$project + $match basic pg',
            input: () => aggregate([
                { $project: { name: 1, age: 1 } },
                { $match: { age: { $gt: 18 } } }
            ])('users', 'pg'),
            expected:
                "SELECT name AS name, age AS age FROM (SELECT * FROM users) AS t1 WHERE age > 18",
        },

        // === 2. $project nested JSON field ===
        {
            title: '$project JSON fields (sqlite)',
            input: () => aggregate([
                { $project: { country: '$profile.country', city: '$profile.address.city' } },
            ])('users', 'sqlite'),
            expected:
                "SELECT json_extract(profile, '$.country') AS country, json_extract(profile, '$.address.city') AS city FROM (SELECT * FROM users) AS t1",
        },

        {
            title: '$project JSON fields (pg)',
            input: () => aggregate([
                { $project: { country: '$profile.country', city: '$profile.address.city' } },
            ])('users', 'pg'),
            expected:
                "SELECT (profile::jsonb #>> '{country}') AS country, (profile::jsonb #>> '{address,city}') AS city FROM (SELECT * FROM users) AS t1",
        },

        // === 3. $group only ===
        {
            title: '$group by simple field',
            input: () => aggregate([
                { $group: { _id: '$country' } },
            ])('users'),
            expected:
                "SELECT country AS _id FROM (SELECT * FROM users) AS t1 GROUP BY country",
        },

        {
            title: '$group by JSON path (sqlite)',
            input: () => aggregate([
                { $group: { _id: '$profile.country' } },
            ])('users', 'sqlite'),
            expected:
                "SELECT json_extract(profile, '$.country') AS _id FROM (SELECT * FROM users) AS t1 GROUP BY json_extract(profile, '$.country')",
        },

        {
            title: '$group by JSON path (pg)',
            input: () => aggregate([
                { $group: { _id: '$profile.country' } },
            ])('users', 'pg'),
            expected:
                "SELECT (profile::jsonb #>> '{country}') AS _id FROM (SELECT * FROM users) AS t1 GROUP BY (profile::jsonb #>> '{country}')",
        },

        // === 4. $group with aggregate fields ===
        {
            title: '$group with $sum and $avg',
            input: () => aggregate([
                { $group: { _id: '$country', total: { $sum: '$amount' }, avg: { $avg: '$amount' } } },
            ])('orders'),
            expected:
                "SELECT country AS _id, SUM(amount) AS total, AVG(amount) AS avg FROM (SELECT * FROM orders) AS t1 GROUP BY country",
        },

        // === 5. $group by null (global aggregation) ===
        {
            title: '$group global aggregation',
            input: () => aggregate([
                { $group: { _id: null, total: { $sum: '$amount' } } },
            ])('orders'),
            expected:
                "SELECT SUM(amount) AS total FROM (SELECT * FROM orders) AS t1",
        },

        // === 6. $group + $match (HAVING) ===
        {
            title: '$group + $match (HAVING)',
            input: () => aggregate([
                { $group: { _id: '$country', total: { $sum: '$amount' } } },
                { $match: { total: { $gt: 100 } } },
            ])('orders'),
            expected:
                "SELECT country AS _id, SUM(amount) AS total FROM (SELECT * FROM orders) AS t1 GROUP BY country HAVING SUM(amount) > 100",
        },

        {
            title: '$group + multiple $match (HAVING chain)',
            input: () => aggregate([
                { $group: { _id: '$country', total: { $sum: '$amount' } } },
                { $match: { total: { $gt: 100 } } },
                { $match: { total: { $lt: 500 } } },
            ])('orders'),
            expected:
                "SELECT country AS _id, SUM(amount) AS total FROM (SELECT * FROM orders) AS t1 GROUP BY country HAVING SUM(amount) > 100 AND SUM(amount) < 500",
        },

        {
            title: '$group + $match JSON field (sqlite)',
            input: () => aggregate([
                { $group: { _id: '$profile.country', avgAge: { $avg: '$profile.age' } } },
                { $match: { avgAge: { $gte: 30 } } },
            ])('users', 'sqlite'),
            expected:
                "SELECT json_extract(profile, '$.country') AS _id, AVG(json_extract(profile, '$.age')) AS avgAge FROM (SELECT * FROM users) AS t1 GROUP BY json_extract(profile, '$.country') HAVING AVG(json_extract(profile, '$.age')) >= 30",
        },

        // === 7. $sort + $limit + $skip ===
        {
            title: '$sort + $limit + $skip (sqlite)',
            input: () => aggregate([
                { $sort: { 'profile.age': -1 } },
                { $limit: 5 },
                { $skip: 2 },
            ])('users', 'sqlite'),
            expected:
                "SELECT * FROM users ORDER BY json_extract(profile, '$.age') DESC LIMIT 5 OFFSET 2",
        },

        {
            title: '$sort + $limit + $skip (pg)',
            input: () => aggregate([
                { $sort: { 'profile.age': 1 } },
                { $limit: 10 },
                { $skip: 5 },
            ])('users', 'pg'),
            expected:
                "SELECT * FROM users ORDER BY (profile::jsonb #>> '{age}') ASC LIMIT 10 OFFSET 5",
        },

        // === 8. $count ===
        {
            title: '$count after match',
            input: () => aggregate([
                { $match: { age: { $gte: 18 } } },
                { $count: 'totalAdults' },
            ])('users'),
            expected:
                "SELECT COUNT(*) AS totalAdults FROM (SELECT * FROM users WHERE age >= 18) AS t1",
        },

        // === 9. $project + expression ===
        {
            title: '$project with expression',
            input: () => aggregate([
                {
                    $project: {
                        name: 1,
                        yearly: { $multiply: ['$salary', 12] },
                    },
                },
            ])('employees'),
            expected:
                "SELECT name AS name, (salary * 12) AS yearly FROM (SELECT * FROM employees) AS t1",
        },

        // === 10. Complex pipeline (mix of all) ===
        {
            title: 'complex nested pipeline',
            input: () => aggregate([
                { $match: { 'profile.active': true } },
                {
                    $group: {
                        _id: '$profile.country',
                        total: { $sum: '$salary' },
                        avg: { $avg: '$salary' },
                    },
                },
                { $match: { avg: { $gt: 5000 } } },
                { $sort: { total: -1 } },
                { $limit: 10 },
            ])('employees', 'sqlite'),
            expected:
                "SELECT json_extract(profile, '$.country') AS _id, SUM(salary) AS total, AVG(salary) AS avg FROM (SELECT * FROM employees WHERE json_extract(profile, '$.active') = 1) AS t1 GROUP BY json_extract(profile, '$.country') HAVING AVG(salary) > 5000 ORDER BY SUM(salary) DESC LIMIT 10",
        },
    ]);
});
