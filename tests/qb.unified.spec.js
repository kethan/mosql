import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers } from '../index.js';
import { createSchemalessAdapter } from '../src/schemaless.js';
import { runTest, pgConfig, mysqlConfig, isConfigured, connectSkip } from './common.js';
import { loadEnv } from '../src/env.js';
import Database from 'better-sqlite3';
import pkg from 'pg';
import mysql from 'mysql2/promise';

await loadEnv();

const qb = createQueryBuilder({ filterOps, exprOps, updateOps, stageHandlers });

const dbs = [];
const label = 'qb.unified';

(async () => {
  const { adapter: sqlite } = createSchemalessAdapter(new Database(':memory:'), 'sqlite');
  dbs.push({ name: 'sqlite', adapter: sqlite });

  // A dialect joins `dbs` only when it is configured *and* reachable: the unit job
  // has no services at all, and these files also cover sqlite/memory, so a missing
  // database must not fail the whole run. Skipping is always announced.
  const pgCfg = pgConfig();
  if (isConfigured(pgCfg)) {
    try {
      const { Client } = pkg;
      const pgClient = new Client(pgCfg);
      await pgClient.connect();
      const pgInit = createSchemalessAdapter(pgClient, 'pg');
      dbs.push({ name: 'pg', adapter: pgInit.adapter, client: pgClient });
    } catch (e) {
      console.log(connectSkip(`${label} pg`, pgCfg, e));
    }
  }

  const myCfg = mysqlConfig();
  if (isConfigured(myCfg)) {
    try {
      const conn = await mysql.createConnection(myCfg);
      const myInit = createSchemalessAdapter(conn, 'mysql');
      dbs.push({ name: 'mysql', adapter: myInit.adapter, conn });
    } catch (e) {
      console.log(connectSkip(`${label} mysql`, myCfg, e));
    }
  }

  const setup = async (db) => {
  const a = db.adapter;
  if (db.name === 'pg') {
    await a.execute('DROP TABLE IF EXISTS orders');
    await a.execute('DROP TABLE IF EXISTS qb_users');
    await a.execute('CREATE TABLE qb_users (id SERIAL PRIMARY KEY, name TEXT, age INT, city TEXT, active BOOLEAN, profile JSONB, created_at TIMESTAMP, nullable TEXT)');
    await a.execute('CREATE TABLE orders (id SERIAL PRIMARY KEY, user_id INT REFERENCES users(id), amount NUMERIC, status TEXT)');
    const rows = [
      `INSERT INTO qb_users (name, age, city, active, profile, created_at, nullable) VALUES ('Alice',25,'Paris',TRUE,'{"country":"France","score":85}','2024-01-01 00:00:00',NULL)`,
      `INSERT INTO qb_users (name, age, city, active, profile, created_at, nullable) VALUES ('Bob',30,'London',TRUE,'{"country":"UK","score":90}','2024-06-01 00:00:00',NULL)`,
      `INSERT INTO qb_users (name, age, city, active, profile, created_at, nullable) VALUES ('Charlie',22,'Berlin',FALSE,'{"country":"Germany","score":60}','2023-12-31 00:00:00',NULL)`,
      `INSERT INTO qb_users (name, age, city, active, profile, created_at, nullable) VALUES ('David',40,'Paris',TRUE,'{"country":"France","score":95}','2024-02-02 00:00:00',NULL)`,
      `INSERT INTO qb_users (name, age, city, active, profile, created_at, nullable) VALUES ('Eve',35,'Berlin',TRUE,'{"country":"Germany","score":88}','2024-03-03 00:00:00',NULL)`,
    ];
    for (const sql of rows) await a.execute(sql);
    return;
  }
  if (db.name === 'mysql') {
    await a.execute('DROP TABLE IF EXISTS orders');
    await a.execute('DROP TABLE IF EXISTS qb_users');
    await a.execute('CREATE TABLE qb_users (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255), age INT, city VARCHAR(255), active TINYINT(1), profile JSON, created_at DATETIME, nullable TEXT)');
    await a.execute('CREATE TABLE orders (id INT AUTO_INCREMENT PRIMARY KEY, user_id INT, amount DECIMAL(10,2), status VARCHAR(255))');
    const rows = [
      `INSERT INTO qb_users (name, age, city, active, profile, created_at, nullable) VALUES ('Alice',25,'Paris',1,'{"country":"France","score":85}','2024-01-01 00:00:00',NULL)`,
      `INSERT INTO qb_users (name, age, city, active, profile, created_at, nullable) VALUES ('Bob',30,'London',1,'{"country":"UK","score":90}','2024-06-01 00:00:00',NULL)`,
      `INSERT INTO qb_users (name, age, city, active, profile, created_at, nullable) VALUES ('Charlie',22,'Berlin',0,'{"country":"Germany","score":60}','2023-12-31 00:00:00',NULL)`,
      `INSERT INTO qb_users (name, age, city, active, profile, created_at, nullable) VALUES ('David',40,'Paris',1,'{"country":"France","score":95}','2024-02-02 00:00:00',NULL)`,
      `INSERT INTO qb_users (name, age, city, active, profile, created_at, nullable) VALUES ('Eve',35,'Berlin',1,'{"country":"Germany","score":88}','2024-03-03 00:00:00',NULL)`,
    ];
    for (const sql of rows) await a.execute(sql);
    return;
  }
  // sqlite
  await a.execute('DROP TABLE IF EXISTS orders');
  await a.execute('DROP TABLE IF EXISTS qb_users');
  await a.execute("CREATE TABLE qb_users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, age INTEGER, city TEXT, active INTEGER, profile TEXT, created_at TEXT, nullable TEXT)");
  await a.execute('CREATE TABLE orders (id INTEGER PRIMARY KEY AUTOINCREMENT, user_id INTEGER, amount REAL, status TEXT)');
  const rows = [
    `INSERT INTO qb_users (name, age, city, active, profile, created_at, nullable) VALUES ('Alice',25,'Paris',1,'{"country":"France","score":85}',datetime('2024-01-01 00:00:00'),NULL)`,
    `INSERT INTO qb_users (name, age, city, active, profile, created_at, nullable) VALUES ('Bob',30,'London',1,'{"country":"UK","score":90}',datetime('2024-06-01 00:00:00'),NULL)`,
    `INSERT INTO qb_users (name, age, city, active, profile, created_at, nullable) VALUES ('Charlie',22,'Berlin',0,'{"country":"Germany","score":60}',datetime('2023-12-31 00:00:00'),NULL)`,
    `INSERT INTO qb_users (name, age, city, active, profile, created_at, nullable) VALUES ('David',40,'Paris',1,'{"country":"France","score":95}',datetime('2024-02-02 00:00:00'),NULL)`,
    `INSERT INTO qb_users (name, age, city, active, profile, created_at, nullable) VALUES ('Eve',35,'Berlin',1,'{"country":"Germany","score":88}',datetime('2024-03-03 00:00:00'),NULL)`,
  ];
  for (const sql of rows) await a.execute(sql);
};

for (const db of dbs) {
  await setup(db);

  const exec = async (sql) => {
    const res = await db.adapter.execute(sql);
    return (res.rows || res).map((r) => {
      const obj = {};
      for (const [k, v] of Object.entries(r)) {
        if (typeof v === 'string' && /^-?\d+(\.\d+)?$/.test(v)) obj[k] = parseFloat(v);
        else obj[k] = typeof v === 'number' ? Math.round(v * 1000) / 1000 : v;
      }
      return obj;
    });
  };

  await runTest(`QB ${db.name}: Filter active Paris`, async () => {
    const sql = `SELECT name, age, city FROM qb_users WHERE ${qb.filter({ city: 'Paris', active: true }, db.name)}`;
    return exec(sql);
  }, [
    { name: 'Alice', age: 25, city: 'Paris' },
    { name: 'David', age: 40, city: 'Paris' },
  ]);

  await runTest(`QB ${db.name}: Filter JSON country`, async () => {
    const sql = `SELECT name FROM qb_users WHERE ${qb.filter({ 'profile.country': 'France' }, db.name)}`;
    return exec(sql);
  }, [
    { name: 'Alice' },
    { name: 'David' },
  ]);

  await runTest(`QB ${db.name}: Expression age + 5`, async () => {
    const sql = `SELECT ${qb.expression({ $add: ['$age', 5] }, db.name)} AS next_age FROM qb_users WHERE name = 'Bob'`;
    return exec(sql);
  }, [
    { next_age: 35 },
  ]);

  await runTest(`QB ${db.name}: Aggregate avg age by city`, async () => {
    const sql = qb.aggregate([
      { $group: { _id: '$city', avgAge: { $avg: '$age' } } },
      { $sort: { avgAge: 1 } },
    ])('qb_users', db.name);
    return exec(sql);
  }, db.name === 'pg' ? [
    { _id: 'Berlin', avgage: 28.5 },
    { _id: 'London', avgage: 30 },
    { _id: 'Paris', avgage: 32.5 },
  ] : [
    { _id: 'Berlin', avgAge: 28.5 },
    { _id: 'London', avgAge: 30 },
    { _id: 'Paris', avgAge: 32.5 },
  ]);

  await runTest(`QB ${db.name}: Count adults`, async () => {
    const sql = qb.aggregate([
      { $match: { age: { $gte: 25 } } },
      { $count: 'totalAdults' },
    ])('qb_users', db.name);
    return exec(sql);
  }, db.name === 'pg' ? [
    { totaladults: 4 },
  ] : [
    { totalAdults: 4 },
  ]);

  // LIKE / ILIKE
  await runTest(`QB ${db.name}: string like`, async () => {
    const sql = `SELECT name FROM qb_users WHERE ${qb.filter({ city: { $like: 'Pa%' } }, db.name)}`;
    return exec(sql);
  }, [{ name: 'Alice' }, { name: 'David' }]);

  await runTest(`QB ${db.name}: string ilike`, async () => {
    const sql = `SELECT name FROM qb_users WHERE ${qb.filter({ city: { $ilike: 'pa%' } }, db.name)}`;
    return exec(sql);
  }, [{ name: 'Alice' }, { name: 'David' }]);

  // boolean filter
  await runTest(`QB ${db.name}: boolean active`, async () => {
    const sql = `SELECT name FROM qb_users WHERE ${qb.filter({ active: true }, db.name)}`;
    const rows = await exec(sql);
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name));
  }, [{ name: 'Alice' }, { name: 'Bob' }, { name: 'David' }, { name: 'Eve' }]);

  // between
  await runTest(`QB ${db.name}: age between`, async () => {
    const sql = `SELECT name FROM qb_users WHERE ${qb.filter({ age: { $between: [24, 26] } }, db.name)}`;
    return exec(sql);
  }, [{ name: 'Alice' }]);

  // date filter
  await runTest(`QB ${db.name}: date gte`, async () => {
    const sql = `SELECT name FROM qb_users WHERE ${qb.filter({ created_at: { $gte: '2024-01-01 00:00:00' } }, db.name)}`;
    const rows = await exec(sql);
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name));
  }, [{ name: 'Alice' }, { name: 'Bob' }, { name: 'David' }, { name: 'Eve' }]);

  // exists false (nullable missing)
  await runTest(`QB ${db.name}: exists false`, async () => {
    const sql = `SELECT name FROM qb_users WHERE ${qb.filter({ nullable: { $exists: false } }, db.name)}`;
    const rows = await exec(sql);
    return rows.slice(0, 1);
  }, [{ name: 'Alice' }]);

  // JSON $inc and verify
  const coll = db.adapter.collection('qb_users');
  await coll.updateOne({ name: 'Alice' }, { $inc: { 'profile.score': 5 } });
  await runTest(`QB ${db.name}: JSON inc`, async () => {
    const sql = `SELECT name FROM qb_users WHERE ${qb.filter({ 'profile.score': 90 }, db.name)}`;
    const rows = await exec(sql);
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name));
  }, [{ name: 'Alice' }, { name: 'Bob' }]);
}

for (const db of dbs) {
  try {
    if (db.client && typeof db.client.end === 'function') await db.client.end();
    if (db.conn && typeof db.conn.end === 'function') await db.conn.end();
  } catch { }
}
})();