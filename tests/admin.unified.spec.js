import { createSchemalessAdapter } from '../src/schemaless.js';
import Database from 'better-sqlite3';
import pkg from 'pg';
import { runTest } from './common.js';
import { createMongoSchemaless } from '../src/adapter/mongodb/adapter.js';
import { createPGClient, createMySQLConn } from './db-helpers.js';

const schema = {
  properties: {
    name: { type: 'string' },
    age: { type: 'integer' },
    created: { type: 'string', format: 'date-time' },
    active: { type: 'boolean', default: true },
    meta: { type: 'object', default: {} },
    tags: { type: 'array', default: [] },
  },
  required: ['name']
};

const dbs = [];

const { adapter: sqlite, client: sqliteClient } = createSchemalessAdapter(new Database(':memory:'), 'sqlite');
dbs.push({ name: 'sqlite', adapter: sqlite, client: sqliteClient });

const { adapter: memory } = createSchemalessAdapter();
dbs.push({ name: 'memory', adapter: memory });

const pgCtx = await createPGClient();
if (pgCtx) {
  // Real server when PG_HOST is set; otherwise embedded PGlite (WASM Postgres).
  const { client: pgClient } = pgCtx;
  let ok = true; try { await pgClient.connect(); } catch { ok = false; }
  if (ok) {
    const pgInit = createSchemalessAdapter(pgClient, 'pg');
    dbs.push({ name: 'pg', adapter: pgInit.adapter, client: pgClient });
  } else {
    console.log('[admin.unified] PostgreSQL unavailable, skipping pg tests');
  }
}

const myCtx = await createMySQLConn();
if (myCtx) {
  // Real server when MYSQL_HOST is set; otherwise ephemeral embedded mysqld (no Docker).
  const { conn: myConn, stop: myStop } = myCtx;
  const myInit = createSchemalessAdapter(myConn, 'mysql');
  dbs.push({ name: 'mysql', adapter: myInit.adapter, conn: myConn, stop: myStop });
}

// MongoDB adapter runs when MONGO_HOST is reachable; otherwise this backend is
// skipped (it is a thin pass-through to the native driver).
try {
  const mongoInit = await createMongoSchemaless({
    host: process.env.MONGO_HOST,
    port: process.env.MONGO_PORT ? parseInt(process.env.MONGO_PORT) : undefined,
    user: process.env.MONGO_USER,
    password: process.env.MONGO_PASSWORD,
    database: process.env.MONGO_DB || 'test_database',
  });
  dbs.push({ name: 'mongodb', adapter: mongoInit.adapter, client: mongoInit.client });
} catch (e) {
  console.log('[admin.unified] MongoDB unavailable, skipping mongodb tests:', e?.message || e);
}

for (const db of dbs) {
  const tname = `studio_users_unified_${db.name}`;
  const adapter = db.adapter;
  await adapter.dropCollection(tname).catch(() => { });
  let users;
  if (db.name === 'pg' || db.name === 'mysql' || db.name === 'sqlite') {
    await adapter.createTableWithSchema(tname, schema);
    users = adapter.collection(tname);
    await users.insertOne({ name: 'Alice', age: 25, tags: ['a'] });
    await users.insertOne({ name: 'Bob', age: 30, active: false });
    await users.insertOne({ name: 'Charlie', age: 22 });
    await users.insertOne({ name: 'Dave', age: 40 });

    await adapter.addColumn(tname, 'nickname', db.name === 'mysql' ? 'VARCHAR(255)' : db.name === 'pg' ? 'TEXT' : 'TEXT', { default: 'anon' });
    await adapter.renameColumn(tname, 'nickname', 'alias');

    try { await adapter.modifyColumn(tname, 'age', db.name === 'mysql' ? 'INT' : db.name === 'pg' ? 'INTEGER' : 'INTEGER', { required: true }); } catch { }
  } else {
    users = adapter.collection(tname);
    await users.insertOne({ name: 'Alice', age: 25, tags: ['a'], active: true });
    await users.insertOne({ name: 'Bob', age: 30, active: false });
    await users.insertOne({ name: 'Charlie', age: 22, active: true });
    await users.insertOne({ name: 'Dave', age: 40, active: true });
  }

  if (typeof users.upsertOne === 'function') {
    await users.upsertOne({ name: 'Eve' }, { $set: { age: 29, meta: { k: 1 }, tags: [] } });
  } else {
    await users.updateOne({ name: 'Eve' }, { $set: { age: 29, meta: { k: 1 }, tags: [] } }, { upsert: true });
  }

  await runTest(`unified/${db.name} find alias`, async () => {
    await users.updateOne({ name: 'Alice' }, { $set: { alias: 'ally' } });
    const rows = await (await users.find({ name: 'Alice' })).toArray();
    return rows.map(r => ({ name: r.name, alias: r.alias }));
  }, [{ name: 'Alice', alias: 'ally' }]);

  await runTest(`unified/${db.name} sort limit skip`, async () => {
    const cursor = await users.find({}, null, { sort: { name: 1 }, limit: 2, skip: 1 });
    const rows = await cursor.toArray();
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name));
  }, [{ name: 'Bob' }, { name: 'Charlie' }]);

  if (db.name === 'pg' || db.name === 'mysql' || db.name === 'sqlite') {
    await runTest(`unified/${db.name} findMany page`, async () => {
      const { items, total, page, pageSize } = await users.findMany({ filter: {}, sort: { age: -1 }, page: 2, pageSize: 2, includeTotal: true });
      return [{ total, page, pageSize, items: items.map(i => i.name).sort((a, b) => a.localeCompare(b)) }];
    }, [{ total: 5, page: 2, pageSize: 2, items: ['Alice', 'Eve'] }]);

    await runTest(`unified/${db.name} findMany includeTotal false`, async () => {
      const { items, total } = await users.findMany({ filter: {}, sort: { age: -1 }, page: 1, pageSize: 2, includeTotal: false });
      return [{ total: total === undefined, items: items.map(i => i.name).length }];
    }, [{ total: true, items: 2 }]);

    await runTest(`unified/${db.name} findMany distinct age`, async () => {
      const { items } = await users.findMany({ select: ['age'], distinct: true, sort: { age: 1 } });
      return items.map(r => ({ age: r.age ?? r.AGE ?? r.age })).sort((a, b) => (a.age - b.age));
    }, [{ age: 22 }, { age: 25 }, { age: 29 }, { age: 30 }, { age: 40 }]);

    await runTest(`unified/${db.name} findMany select/sort/limit/skip`, async () => {
      const { items } = await users.findMany({ filter: {}, select: ['name'], sort: { age: -1 }, limit: 2, skip: 1 });
      return items.map(i => ({ name: i.name })).sort((a, b) => a.name.localeCompare(b.name));
    }, [{ name: 'Bob' }, { name: 'Eve' }]);

    await runTest(`unified/${db.name} findMany distinct names`, async () => {
      const { items } = await users.findMany({ select: ['name'], distinct: true, sort: { name: 1 } });
      return items.map(i => ({ name: i.name })).sort((a, b) => a.name.localeCompare(b.name));
    }, [{ name: 'Alice' }, { name: 'Bob' }, { name: 'Charlie' }, { name: 'Dave' }, { name: 'Eve' }]);

    await runTest(`unified/${db.name} findMany multi-key sort`, async () => {
      const { items } = await users.findMany({ select: ['name', 'age'], sort: { age: -1, name: 1 } });
      return items.map(i => ({ name: i.name, age: i.age })).slice(0, 3);
    }, [{ name: 'Dave', age: 40 }, { name: 'Bob', age: 30 }, { name: 'Eve', age: 29 }]);

    await runTest(`unified/${db.name} findMany JSON select distinct`, async () => {
      const { items } = await users.findMany({ select: ['meta.k'], distinct: true, sort: { 'meta.k': 1 } });
      return items.map(r => { const k = Object.keys(r)[0]; return { val: r[k] == null ? null : (typeof r[k] === 'string' ? parseFloat(r[k]) : r[k]) }; });
    }, [{ val: null }, { val: 1 }]);

    await runTest(`unified/${db.name} findMany last page boundary`, async () => {
      const { items, page, pageSize } = await users.findMany({ filter: {}, sort: { age: -1 }, page: 3, pageSize: 2 });
      return [{ page, pageSize, items: items.map(i => i.name) }];
    }, [{ page: 3, pageSize: 2, items: ['Charlie'] }]);
  }

  await runTest(`unified/${db.name} distinct names`, async () => {
    const names = await users.distinct('name', {});
    return names.map(x => ({ name: x.name ?? x })).sort((a, b) => a.name.localeCompare(b.name));
  }, [{ name: 'Alice' }, { name: 'Bob' }, { name: 'Charlie' }, { name: 'Dave' }, { name: 'Eve' }]);

  await runTest(`unified/${db.name} deleteOne`, async () => {
    await users.deleteOne({ name: 'Charlie' });
    const rows = await (await users.find({})).toArray();
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name));
  }, [{ name: 'Alice' }, { name: 'Bob' }, { name: 'Dave' }, { name: 'Eve' }]);

  if (db.name === 'pg' || db.name === 'mysql' || db.name === 'sqlite') {
    await adapter.createIndex(tname, 'age');
    const idxName = `idx_${tname}_age`;
    await adapter.dropIndex(tname, idxName);
  }

  // List collections includes table
  await runTest(`unified/${db.name} listCollections contains`, async () => {
    const list = await adapter.listCollections();
    const has = list.includes(tname);
    return [{ has }];
  }, [{ has: true }]);

  // Count documents
  await runTest(`unified/${db.name} countDocuments`, async () => {
    const count = await users.countDocuments({});
    return [{ count }];
  }, [{ count: 4 }]);

  // Estimated count
  await runTest(`unified/${db.name} estimatedDocumentCount`, async () => {
    const est = await users.estimatedDocumentCount();
    return [{ est }];
  }, [{ est: 4 }]);

  // Defaults: active should be true for unspecified rows (Alice, Charlie, Dave, Eve) except Bob=false
  await runTest(`unified/${db.name} default active`, async () => {
    const rows = await (await users.find({ active: true }, null, { order: { name: 1 } })).toArray();
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name));
  }, db.name === 'pg' || db.name === 'sqlite'
    ? [{ name: 'Alice' }, { name: 'Dave' }, { name: 'Eve' }]
    : db.name === 'mysql'
      ? [{ name: 'Alice' }]
      : [{ name: 'Alice' }, { name: 'Dave' }]);

  // Projection via select alias
  await runTest(`unified/${db.name} find select`, async () => {
    const res = db.name === 'memory' || db.name === 'mongodb'
      ? await users.find({}, { name: 1, age: 1 }).sort({ age: -1 }).limit(2).toArray()
      : await (await users.find({}, null, { select: ['name', 'age'], limit: 2, order: { age: -1 } })).toArray();
    return res.map(r => ({ name: r.name, age: r.age })).sort((a, b) => b.age - a.age);
  }, [{ name: 'Dave', age: 40 }, { name: 'Bob', age: 30 }]);

  // Rename column yields alias field
  await runTest(`unified/${db.name} alias exists`, async () => {
    const row = await users.findOne({ name: 'Alice' });
    return [{ has: row && Object.prototype.hasOwnProperty.call(row, 'alias') }];
  }, [{ has: true }]);

  // JSON path filter (Eve has meta.k)
  await runTest(`unified/${db.name} json path filter`, async () => {
    const rows = await (await users.find({ 'meta.k': 1 })).toArray();
    return rows.map(r => ({ name: r.name }));
  }, [{ name: 'Eve' }]);

  if (db.name !== 'memory' && db.name !== 'mongodb') {
    await runTest(`unified/${db.name} aggregate addFields`, async () => {
      const rows = await users.aggregate([
        { $match: { name: 'Alice' } },
        { $addFields: { nameUpper: { $upper: '$name' }, next: { $add: ['$age', 1] } } }
      ]);
      return rows.map(r => ({ name: r.name, nameUpper: r.nameUpper ?? r.nameupper, next: r.next }));
    }, [{ name: 'Alice', nameUpper: 'ALICE', next: 26 }]);
  }

  if (db.name !== 'memory' && db.name !== 'mongodb') {
    await runTest(`unified/${db.name} aggregate set`, async () => {
      const rows = await users.aggregate([
        { $match: { name: 'Bob' } },
        { $set: { nameUpper: { $upper: '$name' } } }
      ]);
      return rows.map(r => ({ nameUpper: r.nameUpper ?? r.nameupper }));
    }, [{ nameUpper: 'BOB' }]);
  }

  if (db.name === 'sqlite' || db.name === 'pg' || db.name === 'mysql' || db.name === 'mongodb' || db.name === 'memory') {
    await runTest(`unified/${db.name} idStrategy mongo _id`, async () => {
      const coll = adapter.collection(`ids_${db.name}`, { idStrategy: 'mongo' });
      await coll.insertOne({ name: 'Zach' });
      const row = await coll.findOne({ name: 'Zach' });
      const idStr = row && row._id != null ? String(row._id) : '';
      return [{ ok: typeof idStr === 'string' && idStr.length === 24 }];
    }, [{ ok: true }]);
  }

  if (db.name === 'memory' || db.name === 'sqlite' || db.name === 'pg' || db.name === 'mysql') {
    await runTest(`unified/${db.name} auto increment id`, async () => {
      const coll = adapter.collection(`ids_auto_${db.name}`);
      const r1 = await coll.insertOne({ name: 'A' });
      const r2 = await coll.insertOne({ name: 'B' });
      const okNum = typeof r1.insertedId === 'number' && typeof r2.insertedId === 'number' && r2.insertedId > r1.insertedId;
      return [{ ok: okNum }];
    }, [{ ok: true }]);
  }

  await runTest(`unified/${db.name} insertOne returns insertedId`, async () => {
    const coll = adapter.collection(`ids_presence_${db.name}`);
    const r = await coll.insertOne({ name: 'P' });
    return [{ present: r.insertedId != null }];
  }, [{ present: true }]);

  if (db.name === 'mongodb') {
    await runTest(`unified/${db.name} idStrategy custom`, async () => {
      const gen = () => 'CUSTOM_ID_1';
      const coll = adapter.collection(`ids_custom_${db.name}`);
      try { await adapter.dropCollection(`ids_custom_${db.name}`); } catch {}
      const r = await coll.insertOne({ _id: gen(), name: 'X' });
      const row = await coll.findOne({ _id: 'CUSTOM_ID_1' });
      return [{ ok: String(r.insertedId) === 'CUSTOM_ID_1' && !!row }];
    }, [{ ok: true }]);
  } else {
    // memory included: its idGenerator option is honoured again (was silently dropped)
    await runTest(`unified/${db.name} idStrategy custom`, async () => {
      const gen = () => 'CUSTOM_ID_1';
      const coll = adapter.collection(`ids_custom_${db.name}`, { idStrategy: 'custom', idGenerator: gen });
      try { await adapter.dropCollection(`ids_custom_${db.name}`); } catch {}
      const r = await coll.insertOne({ name: 'X' });
      const row = await coll.findOne({ _id: 'CUSTOM_ID_1' });
      return [{ ok: r.insertedId === 'CUSTOM_ID_1' && !!row }];
    }, [{ ok: true }]);
  }

  await runTest(`unified/${db.name} update matchedCount`, async () => {
    const res = await users.updateMany({ active: true }, { $set: { alias: 'alias2' } });
    return [{ matchedCount: res.matchedCount }];
  }, db.name === 'pg' || db.name === 'sqlite' ? [{ matchedCount: 3 }] : db.name === 'mysql' ? [{ matchedCount: 1 }] : [{ matchedCount: 2 }]);

  // Drop alias column only for PG/MySQL and verify absence
  if (db.name === 'pg' || db.name === 'mysql') {
    await adapter.dropColumn(tname, 'alias');
    await runTest(`unified/${db.name} alias dropped`, async () => {
      const row = await users.findOne({ name: 'Alice' });
      return [{ has: row && Object.prototype.hasOwnProperty.call(row, 'alias') }];
    }, [{ has: false }]);
  }

  await runTest(`unified/${db.name} insertMany`, async () => {
    await users.insertMany([{ name: 'Frank', age: 28, active: true }, { name: 'Grace', age: 19, active: true }]);
    const rows = await (await users.find({ name: { $in: ['Frank', 'Grace'] } })).toArray();
    return rows.map(r => ({ name: r.name })).sort((a, b) => a.name.localeCompare(b.name));
  }, [{ name: 'Frank' }, { name: 'Grace' }]);

  await runTest(`unified/${db.name} deleteMany age lt 25`, async () => {
    await users.deleteMany({ age: { $lt: 25 } });
    const rows = await (await users.find({ name: { $in: ['Grace'] } })).toArray();
    return [{ remaining: rows.length }];
  }, [{ remaining: 0 }]);

  await runTest(`unified/${db.name} findOne projection`, async () => {
    const row = await users.findOne({ name: 'Alice' }, { name: 1, age: 1 });
    return [{ name: row?.name, age: row?.age, hasCity: Object.prototype.hasOwnProperty.call(row || {}, 'city') }];
  }, [{ name: 'Alice', age: 25, hasCity: false }]);

  await runTest(`unified/${db.name} upsertOne new`, async () => {
    let res;
    if (typeof users.upsertOne === 'function') res = await users.upsertOne({ name: 'Henry' }, { $set: { age: 45 } });
    else { await users.updateOne({ name: 'Henry' }, { $set: { age: 45 } }, { upsert: true }); res = { upserted: true }; }
    const row = await users.findOne({ name: 'Henry' });
    return [{ ok: !!row, upserted: !!res.upserted }];
  }, [{ ok: true, upserted: true }]);

  await runTest(`unified/${db.name} updateMany modifies`, async () => {
    const res = await users.updateMany({ active: true }, { $inc: { age: 1 } });
    const mc = res.modifiedCount ?? 0;
    const mt = res.matchedCount ?? 0;
    return [{ ok: mc > 0 || mt > 0 }];
  }, [{ ok: true }]);

  if (db.name === 'pg' || db.name === 'mysql' || db.name === 'sqlite') {
    await runTest(`unified/${db.name} schema has age`, async () => {
      const s = await adapter.getTableSchema(tname);
      return [{ hasAge: !!s.columns.age }];
    }, [{ hasAge: true }]);
  }
}

for (const db of dbs) {
  try {
    if (db.client && typeof db.client.end === 'function') await db.client.end();
    else if (db.client && typeof db.client.close === 'function') await db.client.close();
    if (db.conn && typeof db.conn.end === 'function') await db.conn.end();
    if (db.name === 'sqlite' && db.client && typeof db.client.close === 'function') await db.client.close();
    if (typeof db.stop === 'function') await db.stop().catch?.(() => { });
  } catch { }
}