import { createSchemalessClient } from '../src/client.js';
import { loadEnv } from '../src/env.js';

// Optional: read PG_*/MYSQL_*/MONGO_* from a local .env file.
await loadEnv();

// One API, every backend. Run without any database:
//   node examples/client.js
// Optional live backends are exercised when their env vars are present
// (PG_HOST, MYSQL_HOST, MONGO_HOST) - see docker-compose.yml.

const demo = async (label, users) => {
  await users.insertOne({ name: 'Alice', age: 25, profile: { score: 85 } });
  await users.insertOne({ name: 'Bob', age: 30, profile: { score: 60 } });
  await users.updateOne({ name: 'Alice' }, { $inc: { 'profile.score': 5 } });

  const rows = await (await users.find({ age: { $gte: 18 } })).sort({ age: -1 }).toArray();
  // SQLite stores the JSON column as TEXT, so a nested field has to be parsed
  // there; memory/pg/mysql hand back a value the driver already decoded.
  const score = (r) => {
    const p = typeof r.profile === 'string' ? JSON.parse(r.profile) : r.profile;
    return p?.score;
  };
  console.log(`[${label}] find  ->`, rows.map((r) => ({ name: r.name, age: r.age, score: score(r) })));
  console.log(`[${label}] count ->`, await users.countDocuments({}));
  console.log(`[${label}] agg   ->`, await users.aggregate([
    { $group: { _id: null, avgAge: { $avg: '$age' } } },
  ]));
};

// 1. Memory - zero dependencies, isolated per database name.
const memory = await createSchemalessClient('memory');
await demo('memory', memory.db('app', { idStrategy: 'mongo' }).collection('users'));
await memory.close();

// 2. Any serverless/HTTP SQL driver through a single `executor`.
const statements = [];
const virtual = await createSchemalessClient('sql', {
  database: 'pg',
  executor: async (sql) => {
    statements.push(sql);
    return { rows: [], rowCount: 0 };
  },
});
await virtual.db('analytics').collection('events').insertOne({ type: 'click' });
console.log('[sql] generated ->', statements.filter((s) => s.startsWith('INSERT')));
await virtual.close();

// 3. SQLite (needs better-sqlite3).
try {
  const sqlite = await createSchemalessClient('sqlite', { filename: ':memory:' });
  await demo('sqlite', sqlite.db('app').collection('users'));
  await sqlite.close();
} catch (e) {
  console.log('[sqlite] skipped:', e.message.split('\n')[0]);
}

// 4. PostgreSQL / MySQL / MongoDB when configured.
if (process.env.PG_HOST) {
  const pg = await createSchemalessClient('pg', {
    host: process.env.PG_HOST,
    port: process.env.PG_PORT || 5432,
    user: process.env.PG_USER,
    password: process.env.PG_PASSWORD,
    database: process.env.PG_DB,
  });
  await pg.adapter.dropCollection('users').catch(() => { });
  await demo('pg', pg.db('app').collection('users'));
  await pg.close();
}

if (process.env.MYSQL_HOST) {
  const mysql = await createSchemalessClient('mysql', {
    host: process.env.MYSQL_HOST,
    user: process.env.MYSQL_USER,
    password: process.env.MYSQL_PASS,
    database: process.env.MYSQL_DB,
  });
  await mysql.adapter.dropCollection('users').catch(() => { });
  await demo('mysql', mysql.db('app').collection('users'));
  await mysql.close();
}

if (process.env.MONGO_HOST) {
  const mongo = await createSchemalessClient('mongodb', {
    host: process.env.MONGO_HOST,
    port: process.env.MONGO_PORT ? parseInt(process.env.MONGO_PORT) : undefined,
    user: process.env.MONGO_USER,
    password: process.env.MONGO_PASSWORD,
    database: process.env.MONGO_DB || 'test_database',
  });
  await mongo.adapter.dropCollection('users').catch(() => { });
  await demo('mongodb', mongo.db('app').collection('users'));
  await mongo.close();
}
