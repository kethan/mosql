import { test } from 'bun:test';
import { runTest, pgConfig, mysqlConfig, isConfigured } from './common.js';
import { createSQLiteSchemaless } from '../src/adapter/sqlite/adapter.js';
import { createMySQLSchemaless } from '../src/adapter/mysql/adapter.js';
import { createPostgresSchemaless } from '../src/adapter/pg/adapter.js';
import { collection as memCollection } from '../src/adapter/memory/memory.js';
// NOTE: this file is not part of `npm test` - run-all.js skips `*.bun.*` specs, and
// the createSQLite/MySQL/PostgresSchemaless factories it imports were removed when
// the adapters were folded into src/schemaless.js. Kept as the bun:test entry point
// if that harness comes back; the env names below are shared with the other specs.
const configs = {
  mysql: mysqlConfig(),
  pg: pgConfig(),
};

const createCtx = async (db) => {
  if (db === 'sqlite') {
    const { adapter } = createSQLiteSchemaless(':memory:');
    const coll = adapter.collection('users');
    return { db, coll, close: async () => {} };
  }
  if (db === 'mysql') {
    const cfg = configs.mysql; if (!isConfigured(cfg)) return null;
    const { adapter, conn } = await createMySQLSchemaless(cfg);
    const coll = adapter.collection('users');
    return { db, coll, close: async () => { await conn.end(); } };
  }
  if (db === 'pg') {
    const cfg = configs.pg; if (!isConfigured(cfg)) return null;
    const { adapter, client } = await createPostgresSchemaless(cfg);
    const coll = adapter.collection('users');
    return { db, coll, close: async () => { await client.end(); } };
  }
  if (db === 'memory') {
    const coll = memCollection('users', []);
    return { db, coll, close: async () => {} };
  }
  return null;
};

const insertAll = async (ctx) => {
  const docs = [
    { name: 'Alice', age: 25, city: 'Paris', active: true, profile: { country: 'FR', score: 85 }, intVal: 100, floatVal: 3.14159, shortText: 'Hi', longText: 'A'.repeat(600), unicodeText: '你好世界', emptyText: '', createdAt: new Date('2024-01-01T00:00:00Z'), jsonArr: [1,2,3], nullable: null },
    { name: 'Bob', age: 30, city: 'London', active: true, profile: { country: 'UK', score: 90 }, intVal: 200, floatVal: 2.5, shortText: 'Short', longText: 'B'.repeat(800), unicodeText: 'こんにちは', emptyText: '', createdAt: new Date('2024-06-01T00:00:00Z'), jsonArr: ['x','y'], nullable: null },
    { name: 'Charlie', age: 22, city: 'Berlin', active: false, profile: { country: 'DE', score: 60 }, intVal: 50, floatVal: 1.25, shortText: 'Test', longText: 'C'.repeat(300), unicodeText: '안녕하세요', emptyText: '', createdAt: new Date('2023-12-31T00:00:00Z'), jsonArr: [], nullable: null },
  ];
  if (ctx.db === 'memory') ctx.coll.insertMany(docs); else await ctx.coll.insertMany(docs);
};

const execWithCtx = (db, fn) => async () => {
  const ctx = await createCtx(db); if (!ctx) return [];
  await insertAll(ctx);
  const out = await fn(ctx);
  await ctx.close();
  return out;
};

for (const db of ['sqlite','mysql','pg','memory']) {
  await runTest(`Unified ${db} JSON find`, execWithCtx(db, async (ctx) => {
    if (ctx.db === 'memory') return ctx.coll.find({ 'profile.country': 'FR' }).toArray().map(x => ({ name: x.name }));
    const rows = await ctx.coll.find({ 'profile.country': 'FR' });
    const arr = await rows.toArray();
    return arr.map(x => ({ name: x.name }));
  }), [ { name: 'Alice' } ]);

  await runTest(`Unified ${db} number range`, execWithCtx(db, async (ctx) => {
    if (ctx.db === 'memory') return ctx.coll.find({ intVal: { $gte: 100, $lte: 200 } }).toArray().map(x => ({ name: x.name })).sort((a,b)=>a.name.localeCompare(b.name));
    const rows = await ctx.coll.find({ intVal: { $gte: 100, $lte: 200 } });
    const arr = await rows.toArray();
    return arr.map(x => ({ name: x.name })).sort((a,b)=>a.name.localeCompare(b.name));
  }), [ { name: 'Alice' }, { name: 'Bob' } ]);

  await runTest(`Unified ${db} float compare`, execWithCtx(db, async (ctx) => {
    if (ctx.db === 'memory') return ctx.coll.find({ floatVal: { $gt: 2.0 } }).toArray().map(x => ({ name: x.name })).sort((a,b)=>a.name.localeCompare(b.name));
    const rows = await ctx.coll.find({ floatVal: { $gt: 2.0 } });
    const arr = await rows.toArray();
    return arr.map(x => ({ name: x.name })).sort((a,b)=>a.name.localeCompare(b.name));
  }), [ { name: 'Alice' }, { name: 'Bob' } ]);

  await runTest(`Unified ${db} string like`, execWithCtx(db, async (ctx) => {
    if (ctx.db === 'memory') return ctx.coll.find({ shortText: { $like: 'Sh%' } }).toArray().map(x => ({ name: x.name }));
    const rows = await ctx.coll.find({ shortText: { $like: 'Sh%' } });
    const arr = await rows.toArray();
    return arr.map(x => ({ name: x.name }));
  }), [ { name: 'Bob' } ]);

  await runTest(`Unified ${db} unicode like`, execWithCtx(db, async (ctx) => {
    const pattern = '%世%';
    if (ctx.db === 'memory') return ctx.coll.find({ unicodeText: { $like: pattern } }).toArray().map(x => ({ name: x.name })).slice(0,1);
    const rows = await ctx.coll.find({ unicodeText: { $like: pattern } });
    const arr = await rows.toArray();
    return arr.map(x => ({ name: x.name })).slice(0,1);
  }), [ { name: 'Alice' } ]);

  await runTest(`Unified ${db} boolean`, execWithCtx(db, async (ctx) => {
    if (ctx.db === 'memory') return ctx.coll.find({ active: true }).toArray().map(x => ({ name: x.name })).sort((a,b)=>a.name.localeCompare(b.name));
    const rows = await ctx.coll.find({ active: true });
    const arr = await rows.toArray();
    return arr.map(x => ({ name: x.name })).sort((a,b)=>a.name.localeCompare(b.name));
  }), [ { name: 'Alice' }, { name: 'Bob' } ]);

  await runTest(`Unified ${db} date`, execWithCtx(db, async (ctx) => {
    if (ctx.db === 'memory') return ctx.coll.find({ createdAt: { $gte: new Date('2024-01-01T00:00:00Z') } }).toArray().map(x => ({ name: x.name })).sort((a,b)=>a.name.localeCompare(b.name));
    const rows = await ctx.coll.find({ createdAt: { $gte: new Date('2024-01-01T00:00:00Z') } });
    const arr = await rows.toArray();
    return arr.map(x => ({ name: x.name })).sort((a,b)=>a.name.localeCompare(b.name));
  }), [ { name: 'Alice' }, { name: 'Bob' } ]);

  await runTest(`Unified ${db} null exists`, execWithCtx(db, async (ctx) => {
    if (ctx.db === 'memory') return ctx.coll.find({ nullable: { $exists: false } }).toArray().map(x => ({ name: x.name })).slice(0,1);
    const rows = await ctx.coll.find({ nullable: { $exists: false } });
    const arr = await rows.toArray();
    return arr.map(x => ({ name: x.name })).slice(0,1);
  }), [ { name: 'Alice' } ]);

  await runTest(`Unified ${db} JSON inc`, execWithCtx(db, async (ctx) => {
    if (ctx.db === 'memory') ctx.coll.updateOne({ name: 'Alice' }, { $inc: { 'profile.score': 5 } });
    else await ctx.coll.updateOne({ name: 'Alice' }, { $inc: { 'profile.score': 5 } });
    if (ctx.db === 'memory') return ctx.coll.find({ 'profile.score': 90 }).toArray().map(x => ({ name: x.name })).sort((a,b)=>a.name.localeCompare(b.name));
    const rows = await ctx.coll.find({ 'profile.score': 90 });
    const arr = await rows.toArray();
    return arr.map(x => ({ name: x.name })).sort((a,b)=>a.name.localeCompare(b.name));
  }), [ { name: 'Alice' }, { name: 'Bob' } ]);

  await runTest(`Unified ${db} aggregate avg age`, execWithCtx(db, async (ctx) => {
    if (ctx.db === 'memory') return ctx.coll.aggregate([ { $group: { _id: null, avgAge: { $avg: '$age' } } } ]).map(x => ({ avgAge: Math.round(x.avgAge * 10)/10 }));
    const rows = await ctx.coll.aggregate([ { $group: { _id: null, avgAge: { $avg: '$age' } } } ]);
    return rows.map(x => ({ avgAge: Math.round(x.avgAge * 10)/10 }));
  }), [ { avgAge: 25.7 } ]);

  await runTest(`Unified ${db} pipeline`, execWithCtx(db, async (ctx) => {
    if (ctx.db === 'memory') return ctx.coll.aggregate([
      { $match: { active: true } },
      { $project: { name: 1, next: { $add: ['$age', 1] } } },
      { $addFields: { isAdult: { $gte: ['$next', 18] } } },
      { $sort: { next: -1 } },
      { $skip: 1 },
      { $limit: 2 },
    ]).map(x => ({ name: x.name, isAdult: x.isAdult }));
    const rows = await ctx.coll.aggregate([
      { $match: { active: true } },
      { $project: { name: 1, next: { $add: ['$age', 1] } } },
      { $addFields: { isAdult: { $gte: ['$next', 18] } } },
      { $sort: { next: -1 } },
      { $skip: 1 },
      { $limit: 2 },
    ]);
    return rows.map(x => ({ name: x.name, isAdult: x.isAdult }));
  }), [ { name: 'Bob', isAdult: true } ]);

  await runTest(`Unified ${db} distinct name`, execWithCtx(db, async (ctx) => {
    if (ctx.db === 'memory') {
      const names = Array.from(new Set(ctx.coll.find({}).toArray().map(x => x.name))).sort();
      return names.map(n => ({ name: n }));
    }
    const rows = await ctx.coll.distinct('name');
    const names = rows.map(x => x.name || x.NAME || x.table_name || x.TABLE_NAME).sort();
    return names.map(n => ({ name: n }));
  }), [ { name: 'Alice' }, { name: 'Bob' }, { name: 'Charlie' } ]);

  await runTest(`Unified ${db} drop column behavior`, execWithCtx(db, async (ctx) => {
    let ok = true;
    try {
      if (ctx.db === 'memory') ctx.coll.updateMany({}, { $unset: { age: 1 } });
      else await ctx.coll.dropColumn('age');
    } catch (e) { ok = false; }
    return [ { ok } ];
  }), [ { ok: (db === 'sqlite' ? false : true) } ]);
}