import dotenv from 'dotenv';
import { runTest, SkipError } from './common.js';
import { createSchemalessAdapter } from '../src/schemaless.js';
import { collection as memCollection } from '../src/adapter/memory/memory.js';
import { createPGClient, createMySQLConn } from './db-helpers.js';
import Database from 'better-sqlite3';
dotenv.config();

const createCtx = async (db) => {
  if (db === 'sqlite') {
    const { adapter } = createSchemalessAdapter(new Database(':memory:'), 'sqlite');
    const coll = adapter.collection('users');
    return { db, coll, close: async () => {} };
  }
  if (db === 'mysql') {
    // Real server when MYSQL_HOST is set; otherwise embedded mysqld (no Docker).
    const myCtx = await createMySQLConn();
    if (!myCtx) return null;
    const { conn, stop } = myCtx;
    const { adapter } = createSchemalessAdapter(conn, 'mysql');
    await adapter.dropCollection('users').catch(() => { });
    const coll = adapter.collection('users');
    return {
      db,
      coll,
      close: async () => {
        await conn.end();
        if (typeof stop === 'function') await stop().catch?.(() => { });
      },
    };
  }
  if (db === 'pg') {
    // Real server when PG_HOST is set; otherwise embedded PGlite (WASM Postgres).
    const pgCtx = await createPGClient();
    if (!pgCtx) return null;
    const { client } = pgCtx;
    let ok = true; try { await client.connect(); } catch { ok = false; }
    if (!ok) return null;
    const { adapter } = createSchemalessAdapter(client, 'pg');
    await adapter.dropCollection('users').catch(() => { });
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
  const ctx = await createCtx(db);
  if (!ctx) throw new SkipError(`${db} backend unavailable`);
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
    // Postgres lower-cases the output alias (avgage); other backends keep avgAge.
    return rows.map(x => { const v = x.avgAge ?? x.avgage; return { avgAge: Math.round((v ?? NaN) * 10) / 10 }; });
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
    // Postgres lower-cases the output alias (isadult); others keep isAdult.
    return rows.map(x => ({ name: x.name, isAdult: x.isAdult ?? x.isadult }));
    // active docs sort by next: Bob(31) > Alice(26); skip 1 -> Alice, limit 2 -> [Alice].
  }), [ { name: 'Alice', isAdult: (db === 'sqlite' || db === 'mysql') ? 1 : true } ]);

  await runTest(`Unified ${db} distinct name`, execWithCtx(db, async (ctx) => {
    if (ctx.db === 'memory') {
      const names = Array.from(new Set(ctx.coll.find({}).toArray().map(x => x.name))).sort();
      return names.map(n => ({ name: n }));
    }
    // SQL adapters return the raw distinct values.
    const rows = await ctx.coll.distinct('name');
    const names = rows.map(x => (typeof x === 'object' ? (x.name || x.NAME || x.table_name || x.TABLE_NAME) : x)).sort();
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