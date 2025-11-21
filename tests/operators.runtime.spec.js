import dotenv from 'dotenv';
dotenv.config();
import Database from 'better-sqlite3';
import mysql from 'mysql2/promise';
import pkg from 'pg';
import { createSchemalessAdapter } from '../src/schemaless.js';
import { createMongoSchemaless } from '../src/adapter/mongodb/adapter.js';
import { runTest } from './common.js';

const setups = [];

// SQLite
{
  const { adapter } = createSchemalessAdapter(new Database(':memory:'), 'sqlite');
  const users = adapter.collection('users');
  setups.push({ name: 'sqlite', adapter, users });
}

// Postgres
{
  const { Client } = pkg;
  const cfg = { host: process.env.PG_HOST || process.env.PGHOST, port: process.env.PG_PORT || 5432, user: process.env.PG_USER || process.env.PGUSER, password: process.env.PG_PASSWORD || process.env.PGPASSWORD, database: process.env.PG_DB || process.env.PGDATABASE };
  if (cfg.host && cfg.user && cfg.database) {
    const client = new Client(cfg);
    await client.connect();
    const { adapter } = createSchemalessAdapter(client, 'pg');
    setups.push({ name: 'pg', adapter, users: adapter.collection('users'), client });
  }
}

// MySQL
{
  const cfg = { host: process.env.MYSQL_HOST || process.env.MYSQLHOST, user: process.env.MYSQL_USER || process.env.MYSQLUSER, password: process.env.MYSQL_PASS || process.env.MYSQLPASSWORD, database: process.env.MYSQL_DB || process.env.MYSQLDATABASE, port: process.env.MYSQL_PORT || process.env.MYSQLPORT };
  if (cfg.host && cfg.user && cfg.database) {
    const conn = await mysql.createConnection(cfg);
    const { adapter } = createSchemalessAdapter(conn, 'mysql');
    setups.push({ name: 'mysql', adapter, users: adapter.collection('users'), conn });
  }
}

// Memory
{
  const { adapter } = createSchemalessAdapter();
  setups.push({ name: 'memory', adapter, users: adapter.collection('users') });
}

// MongoDB (env-guarded)
{
  try {
    const mongoInit = await createMongoSchemaless({
      host: process.env.MONGO_HOST,
      port: process.env.MONGO_PORT ? parseInt(process.env.MONGO_PORT) : undefined,
      user: process.env.MONGO_USER,
      password: process.env.MONGO_PASSWORD,
      database: process.env.MONGO_DB || 'test_database',
    });
    const adapter = mongoInit.adapter;
    setups.push({ name: 'mongodb', adapter, users: adapter.collection('users'), client: mongoInit.client });
  } catch { }
}

// Memory and MongoDB setups removed

for (const s of setups) {
  await s.adapter.dropCollection('users').catch(() => { });
  const u = s.users;
  await u.insertMany([
    { name: 'Alice', age: 25, city: 'Paris', active: true, profile: { score: 85, tags: ['x', 'y'] }, items: [1, 2, 3] },
    { name: 'Bob', age: 30, city: 'London', active: true, profile: { score: 90, tags: ['y'] }, items: ['a'] },
    { name: 'Charlie', age: 22, city: 'Berlin', active: false, profile: { score: 60, tags: [] }, items: [] },
    { name: 'David', age: 40, city: 'Paris', active: true, profile: { score: 95, tags: ['x'] }, items: [1] }
  ]);

  await runTest(`runtime/${s.name} $eq`, async () => {
    const rows = await (await u.find({ age: { $eq: 25 } })).toArray();
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name));
  }, [{ name: 'Alice' }]);

  await runTest(`runtime/${s.name} $regex`, async () => {
    const rows = await (await u.find({ name: { $regex: '^A' } })).toArray();
    return rows.map(r => ({ name: r.name }));
  }, [{ name: 'Alice' }]);

  await runTest(`runtime/${s.name} $ne`, async () => {
    const rows = await (await u.find({ age: { $ne: 25 } })).toArray();
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name)).slice(0, 2);
  }, [{ name: 'Bob' }, { name: 'Charlie' }]);

  await runTest(`runtime/${s.name} $gt/$lte`, async () => {
    const rows = await (await u.find({ $and: [{ age: { $gt: 22 } }, { age: { $lte: 30 } }] })).toArray();
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name));
  }, [{ name: 'Alice' }, { name: 'Bob' }]);

  await runTest(`runtime/${s.name} $in city`, async () => {
    const rows = await (await u.find({ city: { $in: ['Paris', 'Berlin'] } })).toArray();
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name));
  }, [{ name: 'Alice' }, { name: 'Charlie' }, { name: 'David' }]);

  await runTest(`runtime/${s.name} $nin city`, async () => {
    const rows = await (await u.find({ city: { $nin: ['London'] } })).toArray();
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name));
  }, [{ name: 'Alice' }, { name: 'Charlie' }, { name: 'David' }]);

  await runTest(`runtime/${s.name} $like`, async () => {
    const rows = await (await u.find({ name: { $like: 'A%' } })).toArray();
    return rows.map(r => ({ name: r.name }));
  }, [{ name: 'Alice' }]);

  await runTest(`runtime/${s.name} $ilike`, async () => {
    const rows = await (await u.find({ name: { $ilike: 'a%' } })).toArray();
    return rows.map(r => ({ name: r.name }));
  }, [{ name: 'Alice' }]);

  await runTest(`runtime/${s.name} $exists`, async () => {
    await u.updateOne({ name: 'Alice' }, { $set: { alias: 'ally' } });
    const rows = await (await u.find({ alias: { $exists: true } })).toArray();
    return rows.map(r => ({ name: r.name })).slice(0, 1);
  }, [{ name: 'Alice' }]);

  await runTest(`runtime/${s.name} $between`, async () => {
    const rows = await (await u.find({ age: { $between: [20, 30] } })).toArray();
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name));
  }, [{ name: 'Alice' }, { name: 'Bob' }, { name: 'Charlie' }]);

  await runTest(`runtime/${s.name} $mod`, async () => {
    const rows = await (await u.find({ age: { $mod: [5, 0] } })).toArray();
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name));
  }, [{ name: 'Alice' }, { name: 'Bob' }, { name: 'David' }]);

  await runTest(`runtime/${s.name} JSON path filter`, async () => {
    const rows = await (await u.find({ 'profile.score': { $gte: 85 } })).toArray();
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name));
  }, [{ name: 'Alice' }, { name: 'Bob' }, { name: 'David' }]);

  await runTest(`runtime/${s.name} $expr arithmetic`, async () => {
    const rows = await (await u.find({ $expr: { $gt: [{ $add: ['$age', 5] }, 30] } })).toArray();
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name));
  }, [{ name: 'Bob' }, { name: 'David' }]);

  await runTest(`runtime/${s.name} $size`, async () => {
    const rows = await (await u.find({ $expr: { $gt: [{ $size: ['$items'] }, 1] } })).toArray();
    return rows.map(r => ({ name: r.name }));
  }, [{ name: 'Alice' }]);

  await runTest(`runtime/${s.name} update $inc`, async () => {
    await u.updateOne({ name: 'Alice' }, { $inc: { 'profile.score': 5 } });
    const rows = await (await u.find({ name: 'Alice' })).toArray();
    return rows.map(r => ({ score: r.profile?.score ?? r.score ?? (r.profile && JSON.parse(r.profile).score) })).slice(0, 1);
  }, [{ score: 90 }]);

  await runTest(`runtime/${s.name} update $mul`, async () => {
    await u.updateOne({ name: 'Alice' }, { $mul: { 'profile.score': 2 } });
    const rows = await (await u.find({ name: 'Alice' })).toArray();
    return rows.map(r => ({ score: r.profile?.score ?? (r.profile && JSON.parse(r.profile).score) })).slice(0, 1);
  }, [{ score: 180 }]);

  await runTest(`runtime/${s.name} update $min/$max`, async () => {
    await u.updateOne({ name: 'Bob' }, { $min: { age: 29 } });
    await u.updateOne({ name: 'Bob' }, { $max: { age: 31 } });
    const row = await u.findOne({ name: 'Bob' });
    return [{ age: row.age }];
  }, [{ age: 31 }]);

  await runTest(`runtime/${s.name} update $unset`, async () => {
    await u.updateOne({ name: 'Charlie' }, { $unset: { city: true } });
    const row = await u.findOne({ name: 'Charlie' });
    return [{ isNull: row?.city == null }];
  }, [{ isNull: true }]);

  await runTest(`runtime/${s.name} currentDate`, async () => {
    await u.updateOne({ name: 'David' }, { $currentDate: { createdAt: { $type: 'timestamp' } } });
    const row = await u.findOne({ name: 'David' });
    return [{ ok: !!row.createdAt }];
  }, [{ ok: true }]);

  await runTest(`runtime/${s.name} rename`, async () => {
    await u.updateOne({ name: 'David' }, { $rename: { city: 'town' } });
    const row = await u.findOne({ name: 'David' });
    return [{ isCityNull: row?.city == null, hasTown: row?.town != null }];
  }, [{ isCityNull: true, hasTown: true }]);

  await runTest(`runtime/${s.name} aggregate $project/$addFields`, async () => {
    const rows = await u.aggregate([{ $project: { name: 1, nextAge: { $add: ['$age', 1] } } }, { $addFields: { isAdult: { $gte: ['$age', 18] } } }]).toArray();
    return rows.map(r => ({ name: r.name, nextAge: r.nextAge, isAdult: r.isAdult })).sort((a, b) => a.name.localeCompare(b.name)).slice(0, 2);
  }, [{ name: 'Alice', nextAge: 26, isAdult: true }, { name: 'Bob', nextAge: 31, isAdult: true }]);

  await runTest(`runtime/${s.name} aggregate $group avg age by city`, async () => {
    const rows = await u.aggregate([{ $group: { _id: '$city', avgAge: { $avg: '$age' } } }, { $sort: { avgAge: -1 } }]).toArray();
    return rows.map(r => ({ id: r._id, avgAge: Math.round((r.avgAge || 0) * 100) / 100 }));
  }, [{ id: 'Paris', avgAge: 32.5 }, { id: 'London', avgAge: 30 }, { id: 'Berlin', avgAge: 22 }]);

  await runTest(`runtime/${s.name} aggregate $limit/$skip`, async () => {
    const rows = await u.aggregate([{ $project: { name: 1, age: 1 } }, { $sort: { age: -1 } }, { $skip: 1 }, { $limit: 1 }]).toArray();
    return rows.map(r => ({ name: r.name }));
  }, [{ name: 'Alice' }]);

  await runTest(`runtime/${s.name} aggregate $count`, async () => {
    const rows = await u.aggregate([{ $match: { active: true } }, { $count: 'count' }]).toArray();
    return rows.map(r => ({ count: r.count }));
  }, [{ count: 3 }]);

  await runTest(`runtime/${s.name} aggregate $sortByCount`, async () => {
    const rows = await u.aggregate([{ $sortByCount: '$city' }]).toArray();
    return rows.map(r => ({ id: r._id, count: r.count })).slice(0, 1);
  }, [{ id: 'Paris', count: 2 }]);

  await runTest(`runtime/${s.name} aggregate $bucket`, async () => {
    const rows = await u.aggregate([{ $bucket: { groupBy: '$age', boundaries: [0, 25, 50], default: 'other', output: { count: { $count: 1 } } } }]).toArray();
    return rows.map(r => ({ id: r._id, count: r.count })).sort((a, b) => String(a.id).localeCompare(String(b.id)));
  }, [{ id: 0, count: 2 }, { id: 25, count: 2 }]);

  await runTest(`runtime/${s.name} expr arithmetic set`, async () => {
    const rows = await u.aggregate([
      { $match: { name: 'Alice' } },
      {
        $project: {
          d: { $subtract: ['$age', 5] },
          half: { $divide: ['$age', 2] },
          abs: { $abs: { $subtract: ['$age', 30] } },
          ceil: { $ceil: { $divide: ['$age', 2] } },
          floor: { $floor: { $divide: ['$age', 2] } },
          round: { $round: [{ $divide: ['$age', 2] }, 0] }
        }
      }
    ]).toArray();
    return rows.map(r => ({ d: r.d, half: r.half, abs: r.abs, ceil: r.ceil, floor: r.floor, round: r.round }));
  }, [{ d: 20, half: 12.5, abs: 5, ceil: 13, floor: 12, round: 13 }]);

  await runTest(`runtime/${s.name} expr $pow/$sqrt`, async () => {
    const rows = await u.aggregate([
      { $match: { name: 'Alice' } },
      { $project: { power: { $pow: ['$age', 2] }, sq: { $sqrt: '$age' } } }
    ]).toArray();
    return rows.map(r => ({ power: r.power, sq: r.sq }));
  }, [{ power: 625, sq: 5 }]);

  await u.updateOne({ name: 'Bob' }, { $set: { alias: '  hi  ', val: '42' } });
  await runTest(`runtime/${s.name} expr string ops`, async () => {
    const rows = await u.aggregate([
      { $match: { name: 'Bob' } },
      {
        $project: {
          up: { $upper: '$name' },
          low: { $lower: '$name' },
          sub: { $substr: ['$name', 1, 3] },
          t: { $trim: '$alias' },
          lt: { $ltrim: '$alias' },
          rt: { $rtrim: '$alias' },
          len: { $strLen: '$name' },
          rep: { $replace: ['$name', 'o', '0'] }
        }
      }
    ]).toArray();
    return rows.map(r => ({ up: r.up, low: r.low, sub: r.sub, t: r.t, lt: r.lt, rt: r.rt, len: r.len, rep: r.rep }));
  }, [{ up: 'BOB', low: 'bob', sub: 'Bob', t: 'hi', lt: 'hi  ', rt: '  hi', len: 3, rep: 'B0b' }]);

  await u.updateOne({ name: 'Charlie' }, { $set: { createdAt: '2024-01-01' } });
  await runTest(`runtime/${s.name} expr date parts`, async () => {
    const rows = await u.aggregate([
      { $match: { name: 'Charlie' } },
      {
        $project: {
          y: { $year: '$createdAt' },
          m: { $month: '$createdAt' },
          d: { $dayOfMonth: '$createdAt' },
          dw: { $dayOfWeek: '$createdAt' },
          h: { $hour: { $toDate: '$createdAt' } },
          mi: { $minute: { $toDate: '$createdAt' } },
          s2: { $second: { $toDate: '$createdAt' } },
          w: { $week: '$createdAt' }
        }
      }
    ]).toArray();
    return rows.map(r => ({ y: r.y, m: r.m, d: r.d, dw: r.dw, h: r.h, mi: r.mi, s2: r.s2, w: r.w }));
  }, [{ y: 2024, m: 1, d: 1, dw: 2, h: 0, mi: 0, s2: 0, w: 1 }]);

  await runTest(`runtime/${s.name} expr casts`, async () => {
    const rows = await u.aggregate([
      { $match: { name: 'Charlie' } },
      {
        $project: {
          ystr: { $substr: [{ $toString: '$createdAt' }, 1, 4] },
          intval: { $toInt: '$age' },
          dbl: { $toDouble: '$age' }
        }
      }
    ]).toArray();
    return rows.map(r => ({ ystr: r.ystr, intval: r.intval, dbl: r.dbl }));
  }, [{ ystr: 2024, intval: 22, dbl: 22 }]);

  await runTest(`runtime/${s.name} expr $switch/$ifNull`, async () => {
    const rows = await u.aggregate([
      { $match: { name: 'David' } },
      {
        $project: {
          code: { $switch: { branches: [{ case: { $eq: ['$town', 'Paris'] }, then: 'FR' }], default: 'OTHER' } },
          nick: { $ifNull: ['$alias', 'none'] }
        }
      }
    ]).toArray();
    return rows.map(r => ({ code: r.code, nick: r.nick }));
  }, [{ code: 'FR', nick: 'none' }]);

  await runTest(`runtime/${s.name} filter $nor and $not`, async () => {
    const rows = await (await u.find({
      $nor: [{ city: 'Paris' }, { age: { $gte: 35 } }],
      age: { $not: { $lt: 23 } }
    })).toArray();
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name));
  }, [{ name: 'Bob' }]);

  await runTest(`runtime/${s.name} expr $cmp`, async () => {
    const rows = await u.aggregate([
      { $match: { name: 'Charlie' } },
      { $project: { cmp: { $cmp: ['$age', 25] } } }
    ]).toArray();
    return rows.map(r => ({ cmp: r.cmp }));
  }, [{ cmp: -1 }]);

  await runTest(`runtime/${s.name} expr casts $toBool/$literal`, async () => {
    const rows = await u.aggregate([
      { $match: { name: 'Bob' } },
      { $project: { b: { $toBool: '$age' }, lit: { $literal: ['X'] } } }
    ]).toArray();
    return rows.map(r => ({ b: !!(r.b), lit: r.lit }));
  }, [{ b: true, lit: 'X' }]);

  if (s.name === 'memory') {
    await u.insertOne({ name: 'M1', tags: ['x'] });
    await u.updateOne({ name: 'M1' }, { $push: { tags: 'y' } });
    await runTest(`runtime/${s.name} update $push array`, async () => {
      const rows = await (await u.find({ name: 'M1' })).toArray();
      return rows.map(r => ({ tags: r.tags }));
    }, [{ tags: ['x', 'y'] }]);

    await u.updateOne({ name: 'M1' }, { $addToSet: { tags: 'y' } });
    await runTest(`runtime/${s.name} update $addToSet dedupe`, async () => {
      const rows = await (await u.find({ name: 'M1' })).toArray();
      return rows.map(r => ({ tags: r.tags }));
    }, [{ tags: ['x', 'y'] }]);

    await u.updateOne({ name: 'M1' }, { $pull: { tags: 'x' } });
    await runTest(`runtime/${s.name} update $pull remove`, async () => {
      const rows = await (await u.find({ name: 'M1' })).toArray();
      return rows.map(r => ({ tags: r.tags }));
    }, [{ tags: ['y'] }]);

    await u.insertMany([{ name: 'UW1', items: [1, 2] }, { name: 'UW2', items: [3] }]);
    await runTest(`runtime/${s.name} stage $unwind memory`, async () => {
      const rows = await u.aggregate([{ $match: { name: { $in: ['UW1', 'UW2'] } } }, { $unwind: '$items' }]);
      const out = rows.map(r => ({ name: r.name, item: r.items }));
      out.sort((a, b) => (a.name + a.item).localeCompare(b.name + b.item));
      return out;
    }, [{ name: 'UW1', item: 1 }, { name: 'UW1', item: 2 }, { name: 'UW2', item: 3 }]);
  }

  await runTest(`runtime/${s.name} update $set json path`, async () => {
    await u.updateOne({ name: 'Alice' }, { $set: { 'profile.tags': ['z'] } });
    const rows = await (await u.find({ name: 'Alice' })).toArray();
    return rows.map(r => ({ tags: (r.profile?.tags) || (r.profile && JSON.parse(r.profile).tags) }));
  }, [{ tags: ['z'] }]);
}

for (const s of setups) {
  try {
    if (s.client && typeof s.client.end === 'function') await s.client.end();
    if (s.conn && typeof s.conn.end === 'function') await s.conn.end();
  } catch { }
}