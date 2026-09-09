import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers } from '../index.js';
import { escape, jsonPath } from '../src/index.js';
import { runStringTest, oneRow } from './common.js';

const qb = createQueryBuilder({ filterOps, exprOps, updateOps, stageHandlers });

const dbs = ['sqlite', 'pg', 'mysql'];

// The projection `profile.score >= 80` per dialect, shared by the JSON cases.
const jsonCmp = (db) => db === 'sqlite'
  ? "json_extract(profile, '$.score') >= 80"
  : db === 'pg'
    ? "((profile::jsonb #>> '{score}'))::numeric >= 80"
    : "CAST(JSON_UNQUOTE(JSON_EXTRACT(profile, '$.score')) AS DECIMAL(20,6)) >= 80";

const filterCases = [
  { title: '$eq', make: (db) => qb.filter({ age: { $eq: 25 } }, db), expect: (db) => 'age = 25' },
  { title: '$ne', make: (db) => qb.filter({ age: { $ne: 25 } }, db), expect: (db) => 'age != 25' },
  { title: '$gt', make: (db) => qb.filter({ age: { $gt: 25 } }, db), expect: (db) => 'age > 25' },
  { title: '$gte', make: (db) => qb.filter({ age: { $gte: 25 } }, db), expect: (db) => 'age >= 25' },
  { title: '$lt', make: (db) => qb.filter({ age: { $lt: 25 } }, db), expect: (db) => 'age < 25' },
  { title: '$lte', make: (db) => qb.filter({ age: { $lte: 25 } }, db), expect: (db) => 'age <= 25' },
  { title: '$in', make: (db) => qb.filter({ age: { $in: [25, 30] } }, db), expect: (db) => 'age IN (25, 30)' },
  { title: '$in empty', make: (db) => qb.filter({ age: { $in: [] } }, db), expect: (db) => 'age = 1 AND 1 = 0' },
  { title: '$nin', make: (db) => qb.filter({ age: { $nin: [25, 30] } }, db), expect: (db) => 'age NOT IN (25, 30)' },
  { title: '$nin empty', make: (db) => qb.filter({ age: { $nin: [] } }, db), expect: (db) => 'age = 1 OR 1 = 1' },
  { title: '$like', make: (db) => qb.filter({ name: { $like: '%li%' } }, db), expect: (db) => "name LIKE '%li%'" },
  { title: '$ilike', make: (db) => qb.filter({ name: { $ilike: '%x%' } }, db), expect: (db) => db==='pg' ? "name ILIKE '%x%'" : "LOWER(name) LIKE LOWER('%x%')" },
  { title: '$nlike', make: (db) => qb.filter({ name: { $nlike: '%x%' } }, db), expect: (db) => "name NOT LIKE '%x%'" },
  { title: '$nilike', make: (db) => qb.filter({ name: { $nilike: '%x%' } }, db), expect: (db) => db==='pg' ? "name NOT ILIKE '%x%'" : "LOWER(name) NOT LIKE LOWER('%x%')" },
  { title: '$regex', make: (db) => qb.filter({ name: { $regex: '^A' } }, db), expect: (db) => db==='pg' ? "name ~ '^A'" : (db==='sqlite' ? "name LIKE 'A%'" : "name REGEXP '^A'") },
  { title: '$exists true', make: (db) => qb.filter({ age: { $exists: true } }, db), expect: (db) => 'age IS NOT NULL' },
  { title: '$exists false', make: (db) => qb.filter({ age: { $exists: false } }, db), expect: (db) => 'age IS NULL' },
  { title: '$between', make: (db) => qb.filter({ age: { $between: [20,40] } }, db), expect: (db) => 'age BETWEEN 20 AND 40' },
  { title: '$mod', make: (db) => qb.filter({ age: { $mod: [10,0] } }, db), expect: (db) => 'age % 10 = 0' },
  { title: '$and', make: (db) => qb.filter({ $and: [{ age: { $gte: 25 } }, { age: { $lte: 40 } }] }, db), expect: (db) => "(age >= 25 AND age <= 40)" },
  { title: '$or', make: (db) => qb.filter({ $or: [{ age: { $lt: 20 } }, { age: { $gte: 30 } }] }, db), expect: (db) => "(age < 20 OR age >= 30)" },
  { title: '$not', make: (db) => qb.filter({ $not: { age: { $gte: 25 } } }, db), expect: (db) => "NOT (age >= 25)" },
  { title: '$nor', make: (db) => qb.filter({ $nor: [{ age: { $lt: 20 } }, { age: { $gt: 30 } }] }, db), expect: (db) => "NOT (age < 20 OR age > 30)" },
  { title: 'JSON path', make: (db) => qb.filter({ 'profile.score': { $gte: 80 } }, db), expect: (db) => jsonCmp(db) },
  { title: '$not on a field', make: (db) => qb.filter({ age: { $not: { $lt: 23 } } }, db), expect: (db) => 'NOT (age < 23)' },
  { title: '$not on a JSON path', make: (db) => qb.filter({ 'profile.score': { $not: { $gte: 80 } } }, db), expect: (db) => `NOT (${jsonCmp(db)})` },
  { title: '$mod rejects injection', make: (db) => { try { return qb.filter({ age: { $mod: ["5 = 0 OR 1 = 1 --", 0] } }, db); } catch (e) { return e.message; } }, expect: (db) => '$mod requires finite numbers' },
  { title: '$mod rejects non numbers', make: (db) => { try { return qb.filter({ age: { $mod: ['2', '0'] } }, db); } catch (e) { return e.message; } }, expect: (db) => '$mod requires finite numbers' },
  { title: '$mod rejects a zero divisor', make: (db) => { try { return qb.filter({ age: { $mod: [0, 0] } }, db); } catch (e) { return e.message; } }, expect: (db) => '$mod divisor cannot be zero' },
  { title: '$mod rejects NaN', make: (db) => { try { return qb.filter({ age: { $mod: [NaN, 0] } }, db); } catch (e) { return e.message; } }, expect: (db) => '$mod requires finite numbers' },
  { title: '$mod accepts bigints', make: (db) => qb.filter({ age: { $mod: [10n, 0n] } }, db), expect: (db) => 'age % 10 = 0' },
  { title: '$expr arithmetic', make: (db) => qb.filter({ $expr: { $gt: [{ $add: ['$age', 5] }, 30] } }, db), expect: (db) => "((age + 5) > 30)" },
];

const exprCases = [
  { title: '$add', make: (db) => qb.expression({ $add: ['$age', 5] }, db), expect: (db) => '(age + 5)' },
  { title: '$subtract', make: (db) => qb.expression({ $subtract: ['$age', 5] }, db), expect: (db) => '(age - 5)' },
  { title: '$multiply', make: (db) => qb.expression({ $multiply: ['$age', 2] }, db), expect: (db) => '(age * 2)' },
  { title: '$divide', make: (db) => qb.expression({ $divide: ['$age', 2] }, db), expect: (db) => db === 'sqlite' ? '(CAST(age AS REAL) / NULLIF(2, 0))' : db === 'pg' ? '(CAST(age AS numeric) / NULLIF(2, 0))' : '(age / NULLIF(2, 0))' },
  { title: '$mod', make: (db) => qb.expression({ $mod: ['$age', 2] }, db), expect: (db) => '(age % 2)' },
  { title: '$abs', make: (db) => qb.expression({ $abs: ['$age'] }, db), expect: (db) => 'ABS(age)' },
  { title: '$ceil', make: (db) => qb.expression({ $ceil: ['$age'] }, db), expect: (db) => 'CEIL(age)' },
  { title: '$floor', make: (db) => qb.expression({ $floor: ['$age'] }, db), expect: (db) => 'FLOOR(age)' },
  { title: '$round', make: (db) => qb.expression({ $round: ['$age', 1] }, db), expect: (db) => /^ROUND\(age(?:,\s*1)?\)$/ },
  { title: '$pow', make: (db) => qb.expression({ $pow: ['$age', 2] }, db), expect: (db) => 'POWER(age, 2)' },
  { title: '$sqrt', make: (db) => qb.expression({ $sqrt: ['$age'] }, db), expect: (db) => 'SQRT(age)' },
  { title: '$concat', make: (db) => qb.expression({ $concat: ['$name', '!', '$city'] }, db), expect: (db) => db==='sqlite' ? "(name || '!' || city)" : `CONCAT(name, '!', city)` },
  { title: '$upper', make: (db) => qb.expression({ $upper: ['$name'] }, db), expect: (db) => 'UPPER(name)' },
  { title: '$lower', make: (db) => qb.expression({ $lower: ['$name'] }, db), expect: (db) => 'LOWER(name)' },
  { title: '$substr', make: (db) => qb.expression({ $substr: ['$name', 1, 2] }, db), expect: (db) => 'SUBSTRING(name, 2, 2)' },
  { title: '$substr zero based', make: (db) => qb.expression({ $substr: ['$name', 0, 3] }, db), expect: (db) => 'SUBSTRING(name, 1, 3)' },
  { title: '$substr computed start', make: (db) => qb.expression({ $substr: ['$name', { $add: [1, 1] }, 2] }, db), expect: (db) => 'SUBSTRING(name, ((1 + 1)) + 1, 2)' },
  { title: '$strLen', make: (db) => qb.expression({ $strLen: ['$name'] }, db), expect: (db) => 'LENGTH(name)' },
  { title: '$replace', make: (db) => qb.expression({ $replace: ['$name', 'a', 'x'] }, db), expect: (db) => "REPLACE(name, 'a', 'x')" },
  { title: '$sum', make: (db) => qb.expression({ $sum: ['$age'] }, db), expect: (db) => 'SUM(age)' },
  { title: '$avg', make: (db) => qb.expression({ $avg: ['$age'] }, db), expect: (db) => db==='mysql' ? 'CAST(AVG(age) AS DOUBLE)' : 'AVG(age)' },
  { title: '$stdDevPop', make: (db) => qb.expression({ $stdDevPop: ['$age'] }, db), expect: (db) => 'STDDEV_POP(age)' },
  { title: '$stdDevSamp', make: (db) => qb.expression({ $stdDevSamp: ['$age'] }, db), expect: (db) => 'STDDEV_SAMP(age)' },
  { title: '$min', make: (db) => qb.expression({ $min: ['$age'] }, db), expect: (db) => 'MIN(age)' },
  { title: '$max', make: (db) => qb.expression({ $max: ['$age'] }, db), expect: (db) => 'MAX(age)' },
  { title: '$count', make: (db) => qb.expression({ $count: [] }, db), expect: (db) => 'COUNT(*)' },
  { title: '$eq', make: (db) => qb.expression({ $eq: ['$age', 25] }, db), expect: (db) => '(age = 25)' },
  { title: '$ne', make: (db) => qb.expression({ $ne: ['$age', 25] }, db), expect: (db) => '(age <> 25)' },
  { title: '$gt', make: (db) => qb.expression({ $gt: ['$age', 25] }, db), expect: (db) => '(age > 25)' },
  { title: '$gte', make: (db) => qb.expression({ $gte: ['$age', 25] }, db), expect: (db) => '(age >= 25)' },
  { title: '$lt', make: (db) => qb.expression({ $lt: ['$age', 25] }, db), expect: (db) => '(age < 25)' },
  { title: '$lte', make: (db) => qb.expression({ $lte: ['$age', 25] }, db), expect: (db) => '(age <= 25)' },
  { title: '$cmp', make: (db) => qb.expression({ $cmp: ['$age', 25] }, db), expect: (db) => 'CASE WHEN age < 25 THEN -1 WHEN age > 25 THEN 1 ELSE 0 END' },
  { title: '$in', make: (db) => qb.expression({ $in: ['$status', ['a','b']] }, db), expect: (db) => "(status IN ('a', 'b'))" },
  { title: '$nin', make: (db) => qb.expression({ $nin: ['$status', ['a','b']] }, db), expect: (db) => "(status NOT IN ('a', 'b'))" },
  { title: '$size', make: (db) => qb.expression({ $size: ['$items'] }, db), expect: (db) => db==='pg' ? 'jsonb_array_length(items::jsonb)' : db==='mysql' ? 'JSON_LENGTH(items)' : 'json_array_length(items)' },
  { title: '$and', make: (db) => qb.expression({ $and: [ true, { $gt: ['$age', 20] } ] }, db), expect: (db) => `(${db==='pg' ? 'TRUE' : '1'} AND (age > 20))` },
  { title: '$or', make: (db) => qb.expression({ $or: [ { $gt: ['$age', 20] }, { $lt: ['$age', 50] } ] }, db), expect: (db) => '((age > 20) OR (age < 50))' },
  { title: '$not', make: (db) => qb.expression({ $not: [ { $gt: ['$age', 20] } ] }, db), expect: (db) => '(NOT (age > 20))' },
  { title: '$cond', make: (db) => qb.expression({ $cond: [ { $gt: ['$age', 20] }, 'A', 'B' ] }, db), expect: (db) => "CASE WHEN (age > 20) THEN 'A' ELSE 'B' END" },
  { title: '$ifNull', make: (db) => qb.expression({ $ifNull: ['$x', 0] }, db), expect: (db) => 'COALESCE(x, 0)' },
  { title: '$switch', make: (db) => qb.expression({ $switch: [{ branches: [ { case: { $gt: ['$age', 30] }, then: 'A' } ], default: 'B' }] }, db), expect: (db) => "CASE WHEN (age > 30) THEN 'A' ELSE 'B' END" },
  { title: '$exists expr', make: (db) => qb.expression({ $exists: ['$age', true] }, db), expect: (db) => '(age IS NOT NULL)' },
  { title: '$year', make: (db) => qb.expression({ $year: ['$createdAt'] }, db), expect: (db) => db==='pg' ? 'EXTRACT(YEAR FROM createdAt)' : db==='mysql' ? 'YEAR(createdAt)' : "CAST(strftime('%Y', createdAt) AS INTEGER)" },
  { title: '$month', make: (db) => qb.expression({ $month: ['$createdAt'] }, db), expect: (db) => db==='pg' ? 'EXTRACT(MONTH FROM createdAt)' : db==='mysql' ? 'MONTH(createdAt)' : "CAST(strftime('%m', createdAt) AS INTEGER)" },
  { title: '$dayOfMonth', make: (db) => qb.expression({ $dayOfMonth: ['$createdAt'] }, db), expect: (db) => db==='pg' ? 'EXTRACT(DAY FROM createdAt)' : db==='mysql' ? 'DAY(createdAt)' : "CAST(strftime('%d', createdAt) AS INTEGER)" },
  { title: '$dayOfWeek', make: (db) => qb.expression({ $dayOfWeek: ['$createdAt'] }, db), expect: (db) => db==='pg' ? 'EXTRACT(DOW FROM createdAt) + 1' : db==='mysql' ? 'DAYOFWEEK(createdAt)' : "CAST(strftime('%w', createdAt) AS INTEGER) + 1" },
  { title: '$hour', make: (db) => qb.expression({ $hour: ['$createdAt'] }, db), expect: (db) => db==='pg' ? 'EXTRACT(HOUR FROM createdAt)' : db==='mysql' ? 'HOUR(createdAt)' : "CAST(strftime('%H', createdAt) AS INTEGER)" },
  { title: '$minute', make: (db) => qb.expression({ $minute: ['$createdAt'] }, db), expect: (db) => db==='pg' ? 'EXTRACT(MINUTE FROM createdAt)' : db==='mysql' ? 'MINUTE(createdAt)' : "CAST(strftime('%M', createdAt) AS INTEGER)" },
  { title: '$second', make: (db) => qb.expression({ $second: ['$createdAt'] }, db), expect: (db) => db==='pg' ? 'EXTRACT(SECOND FROM createdAt)' : db==='mysql' ? 'SECOND(createdAt)' : "CAST(strftime('%S', createdAt) AS INTEGER)" },
  { title: '$week', make: (db) => qb.expression({ $week: ['$createdAt'] }, db), expect: (db) => db==='pg' ? 'EXTRACT(WEEK FROM createdAt)' : db==='mysql' ? 'WEEK(createdAt)' : "CAST(strftime('%W', createdAt) AS INTEGER)" },
  { title: '$toString', make: (db) => qb.expression({ $toString: ['$age'] }, db), expect: (db) => 'CAST(age AS TEXT)' },
  { title: '$toInt', make: (db) => qb.expression({ $toInt: ['$age'] }, db), expect: (db) => 'CAST(age AS INTEGER)' },
  { title: '$toDouble', make: (db) => qb.expression({ $toDouble: ['$age'] }, db), expect: (db) => db==='pg' ? 'CAST(age AS DOUBLE PRECISION)' : db==='mysql' ? 'CAST(age AS DECIMAL(20,6))' : 'CAST(age AS REAL)' },
  { title: '$toBool', make: (db) => qb.expression({ $toBool: ['$age'] }, db), expect: (db) => 'CAST(age AS BOOLEAN)' },
  { title: '$toDate', make: (db) => qb.expression({ $toDate: '$age' }, db), expect: (db) => db === 'sqlite' ? 'datetime(age)' : 'CAST(age AS TIMESTAMP)' },
  { title: '$literal', make: (db) => qb.expression({ $literal: ['$x'] }, db), expect: (db) => "'$x'" },
  { title: '$trim', make: (db) => qb.expression({ $trim: ['$name'] }, db), expect: (db) => 'TRIM(name)' },
  { title: '$ltrim', make: (db) => qb.expression({ $ltrim: ['$name'] }, db), expect: (db) => 'LTRIM(name)' },
  { title: '$rtrim', make: (db) => qb.expression({ $rtrim: ['$name'] }, db), expect: (db) => 'RTRIM(name)' },
];

const stageCases = [
  { title: '$project', make: (db) => qb.aggregate([ { $project: { name: 1, nextAge: { $add: ['$age', 1] } } } ])('users', db), expect: (db) => `SELECT name AS name, (${jsonPath('age', db, 'text')} + ${escape(1, db)}) AS nextAge FROM (SELECT * FROM users) AS t1` },
  { title: '$addFields', make: (db) => qb.aggregate([ { $addFields: { isAdult: { $gte: ['$age', 18] } } } ])('users', db), expect: (db) => `SELECT *, (${jsonPath('age', db, 'text')} >= ${escape(18, db)}) AS isAdult FROM (SELECT * FROM users) AS t1` },
  { title: '$set', make: (db) => qb.aggregate([ { $set: { flag: true } } ])('users', db), expect: (db) => `SELECT *, ${escape(true, db)} AS flag FROM (SELECT * FROM users) AS t1` },
  { title: '$group', make: (db) => qb.aggregate([ { $group: { _id: '$city', avgAge: { $avg: '$age' } } } ])('users', db), expect: (db) => `SELECT city AS _id, ${db==='mysql' ? 'CAST(AVG(age) AS DOUBLE)' : 'AVG(age)'} AS avgAge FROM (SELECT * FROM users) AS t1 GROUP BY city` },
  { title: '$sort', make: (db) => qb.aggregate([ { $group: { _id: '$city', avgAge: { $avg: '$age' } } }, { $sort: { avgAge: 1 } } ])('users', db), expect: (db) => new RegExp(`^SELECT city AS _id, ${db==='mysql' ? 'CAST\\(AVG\\(age\\) AS DOUBLE\\)' : 'AVG\\(age\\)'} AS avgAge FROM \\(SELECT \\* FROM users\\) AS t1 GROUP BY city ORDER BY (avgAge|AVG\\(age\\)|CAST\\(AVG\\(age\\) AS DOUBLE\\)) ASC$`) },
  { title: '$limit', make: (db) => qb.aggregate([ { $limit: 5 } ])('users', db), expect: (db) => 'SELECT * FROM users LIMIT 5' },
  { title: '$skip', make: (db) => qb.aggregate([ { $skip: 2 } ])('users', db), expect: (db) => 'SELECT * FROM users OFFSET 2' },
  { title: '$count', make: (db) => qb.aggregate([ { $count: 'total' } ])('users', db), expect: (db) => 'SELECT COUNT(*) AS total FROM (SELECT * FROM users) AS t1' },
  { title: '$sample', make: (db) => qb.aggregate([ { $sample: { size: 3 } } ])('users', db), expect: (db) => `SELECT * FROM (SELECT * FROM users) AS t1 ORDER BY ${db==='mysql' ? 'RAND()' : 'RANDOM()'} LIMIT 3` },
  { title: '$sortByCount', make: (db) => qb.aggregate([ { $sortByCount: '$city' } ])('users', db), expect: (db) => 'SELECT city AS _id, COUNT(*) AS count FROM (SELECT * FROM users) AS t1 GROUP BY city ORDER BY count DESC' },
  { title: '$bucket', make: (db) => qb.aggregate([ { $bucket: { groupBy: '$age', boundaries: [0,30,40], default: 'other' } } ])('users', db), expect: (db) => `SELECT CASE WHEN age >= 0 AND age < 30 THEN 0 WHEN age >= 30 AND age < 40 THEN 30 ELSE 'other' END AS _id, COUNT(*) AS count FROM (SELECT * FROM users) AS t1 GROUP BY CASE WHEN age >= 0 AND age < 30 THEN 0 WHEN age >= 30 AND age < 40 THEN 30 ELSE 'other' END` },
];

for (const db of dbs) {
  for (const c of filterCases) runStringTest(`Filter ${db} ${c.title}`, () => c.make(db), c.expect(db));
  for (const c of exprCases) runStringTest(`Expr ${db} ${c.title}`, () => c.make(db), c.expect(db));
  for (const c of stageCases) runStringTest(`Stage ${db} ${c.title}`, () => c.make(db), c.expect(db));
}

// Update operator SQL cases
// updateOne must touch a single row on every dialect (see `oneRow`); updateMany
// and the unbounded cases must stay free of any narrowing.
const U = (db, set, where) => `UPDATE users SET ${set}${oneRow(db, 'users', where)}`;

const updateCases = [
  { title: '$set scalar', make: (db) => qb.collection('users', db).updateOne({ name: 'Alice' }, { $set: { age: 26 } }), expect: (db) => U(db, "age = 26", "name = 'Alice'") },
  { title: '$set JSON', make: (db) => qb.collection('users', db).updateOne({ name: 'Alice' }, { $set: { 'profile.score': 100 } }), expect: (db) => U(db, db === 'pg' ? "profile = jsonb_set(COALESCE(profile::jsonb, '{}'::jsonb), '{score}', to_jsonb(100), true)" : db === 'mysql' ? "profile = JSON_SET(COALESCE(profile, '{}'), '$.score', 100)" : "profile = json_set(COALESCE(profile, '{}'), '$.score', 100)", "name = 'Alice'") },
  { title: '$inc JSON', make: (db) => qb.collection('users', db).updateOne({ name: 'Alice' }, { $inc: { 'profile.score': 1 } }), expect: (db) => U(db, db === 'pg' ? "profile = jsonb_set(profile::jsonb, '{score}', to_jsonb(COALESCE((profile::jsonb #>> '{score}')::numeric, 0) + 1), true)" : db === 'mysql' ? "profile = JSON_SET(profile, '$.score', COALESCE(JSON_EXTRACT(profile, '$.score'), 0) + 1)" : "profile = json_set(profile, '$.score', COALESCE(json_extract(profile, '$.score'), 0) + 1)", "name = 'Alice'") },
  { title: '$mul JSON', make: (db) => qb.collection('users', db).updateOne({ name: 'Alice' }, { $mul: { 'profile.score': 2 } }), expect: (db) => U(db, db === 'pg' ? "profile = jsonb_set(profile::jsonb, '{score}', to_jsonb(COALESCE((profile::jsonb #>> '{score}')::numeric, 1) * 2), true)" : db === 'mysql' ? "profile = JSON_SET(profile, '$.score', COALESCE(JSON_EXTRACT(profile, '$.score'), 1) * 2)" : "profile = json_set(profile, '$.score', COALESCE(json_extract(profile, '$.score'), 1) * 2)", "name = 'Alice'") },
  { title: '$min scalar', make: (db) => qb.collection('users', db).updateOne({ name: 'Alice' }, { $min: { age: 26 } }), expect: (db) => U(db, `age = ${db === 'sqlite' ? 'MIN' : 'LEAST'}(age, 26)`, "name = 'Alice'") },
  { title: '$max scalar', make: (db) => qb.collection('users', db).updateOne({ name: 'Alice' }, { $max: { age: 26 } }), expect: (db) => U(db, `age = ${db === 'sqlite' ? 'MAX' : 'GREATEST'}(age, 26)`, "name = 'Alice'") },
  { title: '$unset JSON', make: (db) => qb.collection('users', db).updateOne({ name: 'Alice' }, { $unset: { 'profile.score': 0 } }), expect: (db) => U(db, db === 'pg' ? "profile = profile::jsonb #- '{score}'" : db === 'mysql' ? "profile = JSON_REMOVE(profile, '$.score')" : "profile = json_remove(profile, '$.score')", "name = 'Alice'") },
  { title: '$currentDate', make: (db) => qb.collection('users', db).updateOne({ name: 'Alice' }, { $currentDate: { updatedAt: true } }), expect: (db) => U(db, `updatedAt = ${db === 'pg' ? 'CURRENT_TIMESTAMP' : db === 'mysql' ? 'NOW()' : "datetime('now')"}`, "name = 'Alice'") },
  { title: '$rename', make: (db) => qb.collection('users', db).updateOne({ name: 'Alice' }, { $rename: { oldField: 'newField' } }), expect: (db) => U(db, "newField = oldField, oldField = NULL", "name = 'Alice'") },
  { title: 'updateMany is not narrowed', make: (db) => qb.collection('users', db).updateMany({ name: 'Alice' }, { $set: { age: 26 } }), expect: (db) => `UPDATE users SET age = 26 WHERE name = 'Alice'` },
  { title: 'updateOne without a filter', make: (db) => qb.collection('users', db).updateOne({}, { $set: { age: 26 } }), expect: (db) => `UPDATE users SET age = 26${oneRow(db, 'users', '')}` },
  { title: 'deleteOne without a filter', make: (db) => qb.collection('users', db).deleteOne({}), expect: (db) => `DELETE FROM users${oneRow(db, 'users', '')}` },
  { title: 'deleteMany keeps the guard rail', make: (db) => { try { return qb.collection('users', db).deleteMany({}); } catch (e) { return e.message; } }, expect: () => 'deleteMany requires a filter or allowDeleteAll option' },
];

for (const db of dbs) {
  for (const c of updateCases) runStringTest(`Update ${db} ${c.title}`, () => c.make(db), c.expect(db));
}