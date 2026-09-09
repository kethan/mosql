import { runTest } from './common.js';
import { createSchemalessAdapter } from '../src/schemaless.js';
import { createMySQLConn } from './db-helpers.js';

const main = async () => {
  const myCtx = await createMySQLConn();
  if (!myCtx) {
    console.log('[schemaless.mysql] database unavailable, skipping');
    return;
  }
  const { conn, stop } = myCtx;

  try {
    const { adapter } = createSchemalessAdapter(conn, 'mysql');
    const users = adapter.collection('users');

    await runTest('Insert and update JSON', async () => {
      await conn.query('DROP TABLE IF EXISTS users');
      await conn.query('CREATE TABLE users (_id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255), profile JSON)');
      await users.insertOne({ name: 'Alice', age: 25, profile: { score: 85 } });
      await users.updateOne({ name: 'Alice' }, { $set: { 'profile.score': 90 } });
      const [rows] = await conn.query("SELECT JSON_EXTRACT(profile, '$.score') AS score FROM users WHERE name = 'Alice'");
      return rows;
    }, [ { score: 90 } ]);
  } finally {
    await conn.end().catch(() => {});
    await stop().catch?.(() => {});
  }
};

main().catch(e => {
  console.error('[schemaless.mysql] FAILED');
  console.error(e);
  process.exitCode = 1;
});
