// TEMPORARY diagnostic spec (remove after debugging). Prints every SQL the
// schemaless mysql adapter executes so CI logs show which statement MySQL
// rejects and why (1064 near 'longText VARCHAR(255)').
import { createSchemalessAdapter } from '../src/schemaless.js';
import { createMySQLConn } from './db-helpers.js';

const myCtx = await createMySQLConn();
if (!myCtx) {
  console.log('[diag] mysql unavailable, skipping');
  process.exit(0);
}
const { conn, stop } = myCtx;
const log = (...a) => console.log('[diag]', ...a);

try {
  const [[v]] = await conn.query('SELECT VERSION() AS v');
  log('mysql version:', v.v);

  const { adapter } = createSchemalessAdapter(conn, 'mysql');
  const orig = adapter.execute.bind(adapter);
  adapter.execute = async (sql, params = []) => {
    log('>>>', String(sql).slice(0, 180));
    try {
      const r = await orig(sql, params);
      log('    OK');
      return r;
    } catch (e) {
      log('    ERROR:', String(e?.message || e).slice(0, 220));
      throw e;
    }
  };

  log('--- replicate bun.unified first mysql test ---');
  await adapter.dropCollection('users').catch(() => { });
  const coll = adapter.collection('users');
  const docs = [
    { name: 'Alice', age: 25, city: 'Paris', active: true, profile: { country: 'FR', score: 85 }, intVal: 100, floatVal: 3.14159, shortText: 'Hi', longText: 'A'.repeat(600), unicodeText: '你好世界', emptyText: '', createdAt: new Date('2024-01-01T00:00:00Z'), jsonArr: [1, 2, 3], nullable: null },
    { name: 'Bob', age: 30, city: 'London', active: true, profile: { country: 'UK', score: 90 }, intVal: 200, floatVal: 2.5, shortText: 'Short', longText: 'B'.repeat(800), unicodeText: 'こんにちは', emptyText: '', createdAt: new Date('2024-06-01T00:00:00Z'), jsonArr: ['x', 'y'], nullable: null },
    { name: 'Charlie', age: 22, city: 'Berlin', active: false, profile: { country: 'DE', score: 60 }, intVal: 50, floatVal: 1.25, shortText: 'Test', longText: 'C'.repeat(300), unicodeText: '안녕하세요', emptyText: '', createdAt: new Date('2023-12-31T00:00:00Z'), jsonArr: [], nullable: null },
  ];
  try {
    await coll.insertMany(docs);
    log('insertMany OK');
    const rows = await (await coll.find({ 'profile.country': 'FR' })).toArray();
    log('find OK, rows:', rows.length);
  } catch (e) {
    log('FLOW FAILED:', String(e?.message || e).slice(0, 400));
  }

  log('--- raw probes (execute = prepared, query = text protocol) ---');
  const probes = [
    'CREATE TABLE IF NOT EXISTS diag_probe (_id INT AUTO_INCREMENT PRIMARY KEY)',
    'ALTER TABLE diag_probe ADD name VARCHAR(255)',
    'ALTER TABLE diag_probe ADD longText VARCHAR(255)',
    'ALTER TABLE diag_probe ADD COLUMN longText2 VARCHAR(255)',
    'ALTER TABLE diag_probe MODIFY COLUMN name VARCHAR(255)',
    'ALTER TABLE diag_probe MODIFY name VARCHAR(255)',
  ];
  for (const sql of probes) {
    try { await conn.execute(sql, []); log('EXECUTE OK:', sql); }
    catch (e) { log('EXECUTE FAIL:', sql, '=>', String(e?.message || e).slice(0, 180)); }
    try { await conn.query(sql); log('QUERY   OK:', sql); }
    catch (e) { log('QUERY   FAIL:', sql, '=>', String(e?.message || e).slice(0, 180)); }
  }
} finally {
  await conn.end().catch(() => { });
  await stop().catch?.(() => { });
}
