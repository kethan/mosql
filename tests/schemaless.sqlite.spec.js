import { createSchemalessAdapter } from '../src/schemaless.js';
import Database from 'better-sqlite3';
import { runTest } from './common.js';

const { adapter, client } = createSchemalessAdapter(new Database(':memory:'), 'sqlite');
const users = adapter.collection('users');

const main = async () => {
  await runTest('InsertOne creates table and returns id', async () => {
    const r = await users.insertOne({ name: 'Alice', age: 25, active: true, profile: { country: 'FR', score: 85 } });
    return [{ ok: !!r.insertedId }];
  }, [ { ok: true } ]);

  await runTest('Find by JSON path', async () => {
    const rows = await users.find({ 'profile.country': 'FR' });
    const arr = await rows.toArray();
    return arr.map(r => ({ name: r.name }));
  }, [ { name: 'Alice' } ]);

  await runTest('Update nested JSON $inc', async () => {
    await users.updateOne({ name: 'Alice' }, { $inc: { 'profile.score': 5 } });
    const sql = `SELECT json_extract(profile, '$.score') AS score FROM users WHERE name = 'Alice'`;
    return client.prepare(sql).all();
  }, [ { score: 90 } ]);

  await runTest('Aggregate avg age', async () => {
    await users.insertMany([ { name: 'Bob', age: 30 }, { name: 'Charlie', age: 22 } ]);
    const rows = await users.aggregate([ { $group: { _id: null, avgAge: { $avg: '$age' } } } ]);
    return rows.map(x => ({ avgAge: Math.round(x.avgAge * 10) / 10 }));
  }, [ { avgAge: 25.7 } ]);

  // distinct() resolves to the list of values (Mongo-style), not row objects.
  await runTest('Distinct names', async () => {
    const values = await users.distinct('name');
    return values.map((v) => ({ name: typeof v === 'object' && v !== null ? (v.name ?? Object.values(v)[0]) : v }))
      .sort((a, b) => String(a.name).localeCompare(String(b.name)));
  }, [ { name: 'Alice' }, { name: 'Bob' }, { name: 'Charlie' } ]);

  await runTest('Find cursor count uses COUNT(*)', async () => {
    const cursor = await users.find({ age: { $gte: 18 } });
    return [{ count: await cursor.count() }];
  }, [ { count: 3 } ]);

  let dropErr = null;
  try { await users.dropColumn('age'); } catch (e) { dropErr = true; }
  await runTest('SQLite dropColumn throws', async () => [{ ok: !!dropErr }], [ { ok: true } ]);
};

main().catch(e => { console.error('FAILED', e); process.exitCode = 1; });