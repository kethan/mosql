import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers } from '../index.js';
import { runStringTest } from './common.js';

const qb = createQueryBuilder({ filterOps, exprOps, updateOps, stageHandlers });

const DBs = ['sqlite','pg','mysql'];

for (const db of DBs) {
  // FILTERS
  runStringTest(`ops/${db} filter eq`, () => qb.collection('users', db).find({ age: 25 }).toSQL(), `SELECT * FROM users WHERE age = 25`);
  runStringTest(`ops/${db} filter gt/gte`, () => qb.collection('users', db).find({ score: { $gt: 10, $gte: 5 } }).toSQL(), `SELECT * FROM users WHERE (score > 10 AND score >= 5)`);
  runStringTest(`ops/${db} filter lt/lte`, () => qb.collection('users', db).find({ score: { $lt: 10, $lte: 5 } }).toSQL(), `SELECT * FROM users WHERE (score < 10 AND score <= 5)`);
  runStringTest(`ops/${db} filter in`, () => qb.collection('users', db).find({ city: { $in: ['Paris','London'] } }).toSQL(), `SELECT * FROM users WHERE city IN ('Paris', 'London')`);
  runStringTest(`ops/${db} filter nin`, () => qb.collection('users', db).find({ city: { $nin: ['Paris','London'] } }).toSQL(), `SELECT * FROM users WHERE city NOT IN ('Paris', 'London')`);
  runStringTest(`ops/${db} filter exists true`, () => qb.collection('users', db).find({ nickname: { $exists: true } }).toSQL(), `SELECT * FROM users WHERE nickname IS NOT NULL`);
  runStringTest(`ops/${db} filter exists false`, () => qb.collection('users', db).find({ nickname: { $exists: false } }).toSQL(), `SELECT * FROM users WHERE nickname IS NULL`);
  runStringTest(`ops/${db} filter like`, () => qb.collection('users', db).find({ name: { $like: 'A%' } }).toSQL(), `SELECT * FROM users WHERE name LIKE 'A%'`);
  runStringTest(`ops/${db} filter between`, () => qb.collection('users', db).find({ age: { $between: [18, 30] } }).toSQL(), `SELECT * FROM users WHERE age BETWEEN 18 AND 30`);
  runStringTest(`ops/${db} filter json path`, () => qb.collection('users', db).find({ 'profile.country': 'France' }).toSQL(), /SELECT \* FROM users WHERE .*country.*= 'France'/);

  // EXPRESSIONS
  runStringTest(`ops/${db} expr add`, () => qb.collection('users', db).find({}, { next: { $add: ['$age', 5] } }).toSQL(), `SELECT (age + 5) AS next FROM users`);
  runStringTest(`ops/${db} expr concat upper`, () => qb.collection('users', db).find({}, { U: { $upper: { $concat: ['$city', ', ', '$name'] } } }).toSQL(), db === 'sqlite' ? `SELECT UPPER((city || ', ' || name)) AS U FROM users` : `SELECT UPPER(CONCAT(city, ', ', name)) AS U FROM users`);
  runStringTest(`ops/${db} expr cond`, () => qb.collection('users', db).find({}, { flag: { $cond: [ { $gte: ['$age', 18] }, 'adult', 'minor' ] } }).toSQL(), `SELECT CASE WHEN (age >= 18) THEN 'adult' ELSE 'minor' END AS flag FROM users`);
  runStringTest(`ops/${db} expr aggregates`, () => qb.aggregate([ { $group: { _id: '$city', total: { $sum: '$age' }, avgAge: { $avg: '$age' }, maxAge: { $max: '$age' } } }, { $sort: { total: -1 } } ])('users', db), db === 'mysql' ? `SELECT city AS _id, SUM(age) AS total, CAST(AVG(age) AS DOUBLE) AS avgAge, MAX(age) AS maxAge FROM (SELECT * FROM users) AS t1 GROUP BY city ORDER BY total DESC` : `SELECT city AS _id, SUM(age) AS total, AVG(age) AS avgAge, MAX(age) AS maxAge FROM (SELECT * FROM users) AS t1 GROUP BY city ORDER BY total DESC`
  );

  // STAGES
  runStringTest(`ops/${db} stage match+project`, () => qb.aggregate([ { $match: { active: true } }, { $project: { name: 1, next: { $add: ['$age', 1] } } } ])('users', db), `SELECT name AS name, (age + 1) AS next FROM (SELECT * FROM users WHERE active = ${db === 'pg' ? 'TRUE' : '1'}) AS t1`);
  runStringTest(`ops/${db} stage sort/limit/skip`, () => qb.aggregate([ { $sort: { age: -1 } }, { $limit: 10 }, { $skip: 20 } ])('users', db), `SELECT * FROM users ORDER BY age DESC LIMIT 10 OFFSET 20`);
  runStringTest(`ops/${db} stage count`, () => qb.aggregate([ { $match: { age: { $gte: 25 } } }, { $count: 'total' } ])('users', db), `SELECT COUNT(*) AS total FROM (SELECT * FROM users WHERE age >= 25) AS t1`);
  runStringTest(`ops/${db} stage sortByCount`, () => qb.aggregate([ { $sortByCount: '$city' } ])('users', db), `SELECT city AS _id, COUNT(*) AS count FROM (SELECT * FROM users) AS t1 GROUP BY city ORDER BY count DESC`);
  runStringTest(`ops/${db} stage bucket`, () => qb.aggregate([ { $bucket: { groupBy: '$age', boundaries: [0,18,30,100], default: 'other', output: { cnt: { $sum: 1 } } } } ])('users', db), `SELECT CASE WHEN age >= 0 AND age < 18 THEN 0 WHEN age >= 18 AND age < 30 THEN 18 WHEN age >= 30 AND age < 100 THEN 30 ELSE 'other' END AS _id, COUNT(*) AS count, COUNT(*) AS cnt FROM (SELECT * FROM users) AS t1 GROUP BY CASE WHEN age >= 0 AND age < 18 THEN 0 WHEN age >= 18 AND age < 30 THEN 18 WHEN age >= 30 AND age < 100 THEN 30 ELSE 'other' END`);
  runStringTest(`ops/${db} stage addFields`, () => qb.aggregate([ { $addFields: { cityUpper: { $upper: '$city' }, next: { $add: ['$age', 1] } } } ])('users', db), `SELECT *, UPPER(${db === 'sqlite' ? 'city' : 'city'}) AS cityUpper, (age + 1) AS next FROM (SELECT * FROM users) AS t1`);
  runStringTest(`ops/${db} stage set`, () => qb.aggregate([ { $set: { cityUpper: { $upper: '$city' }, next: { $add: ['$age', 1] } } } ])('users', db), `SELECT *, UPPER(${db === 'sqlite' ? 'city' : 'city'}) AS cityUpper, (age + 1) AS next FROM (SELECT * FROM users) AS t1`);

  // UPDATES
  runStringTest(`ops/${db} updateOne set`, () => qb.collection('users', db).updateOne({ name: 'Alice' }, { $set: { age: 26, 'profile.country': 'FR' } }), db === 'pg'
    ? `UPDATE users SET age = 26, profile = jsonb_set(COALESCE(profile::jsonb, '{}'::jsonb), '{country}', to_jsonb('FR'), true) WHERE name = 'Alice'`
    : db === 'mysql'
      ? `UPDATE users SET age = 26, profile = JSON_SET(COALESCE(profile, '{}'), '$.country', 'FR') WHERE name = 'Alice' LIMIT 1`
      : `UPDATE users SET age = 26, profile = json_set(COALESCE(profile, '{}'), '$.country', 'FR') WHERE name = 'Alice' LIMIT 1`
  );

  runStringTest(`ops/${db} updateMany inc/mul`, () => qb.collection('users', db).updateMany({ active: true }, { $inc: { age: 1 }, $mul: { score: 2 } }), db === 'pg'
    ? `UPDATE users SET age = age + 1, score = score * 2 WHERE active = TRUE`
    : db === 'mysql'
      ? `UPDATE users SET age = age + 1, score = score * 2 WHERE active = 1`
      : `UPDATE users SET age = age + 1, score = score * 2 WHERE active = 1`
  );

  runStringTest(`ops/${db} update unset`, () => qb.collection('users', db).updateMany({ name: 'Bob' }, { $unset: { nickname: 1 } }), db === 'pg'
    ? `UPDATE users SET nickname = NULL WHERE name = 'Bob'`
    : `UPDATE users SET nickname = NULL WHERE name = 'Bob'`
  );

  runStringTest(`ops/${db} update min`, () => qb.collection('users', db).updateMany({ active: true }, { $min: { age: 50 } }), db === 'sqlite'
    ? `UPDATE users SET age = MIN(age, 50) WHERE active = 1`
    : `UPDATE users SET age = LEAST(age, 50) WHERE active = ${db === 'pg' ? 'TRUE' : '1'}`
  );

  runStringTest(`ops/${db} update max`, () => qb.collection('users', db).updateMany({ active: true }, { $max: { age: 50 } }), db === 'sqlite'
    ? `UPDATE users SET age = MAX(age, 50) WHERE active = 1`
    : `UPDATE users SET age = GREATEST(age, 50) WHERE active = ${db === 'pg' ? 'TRUE' : '1'}`
  );

  runStringTest(`ops/${db} update currentDate`, () => qb.collection('users', db).updateMany({ name: 'Alice' }, { $currentDate: { lastModified: true } }), db === 'pg'
    ? `UPDATE users SET lastModified = CURRENT_TIMESTAMP WHERE name = 'Alice'`
    : db === 'mysql'
      ? `UPDATE users SET lastModified = NOW() WHERE name = 'Alice'`
      : `UPDATE users SET lastModified = datetime('now') WHERE name = 'Alice'`
  );

  runStringTest(`ops/${db} update rename`, () => qb.collection('users', db).updateMany({}, { $rename: { nickname: 'alias' } }), `UPDATE users SET alias = nickname, nickname = NULL`);
}