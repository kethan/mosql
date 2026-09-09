import { loadEnv } from '../src/env.js';
import mysql from 'mysql2/promise';
import { createSchemalessAdapter } from '../src/schemaless.js';
import { runTest, mysqlConfig, isConfigured, skipMessage, connectSkip } from './common.js';
await loadEnv();

const config = mysqlConfig();
const label = 'schemaless.mysql';

const main = async () => {
  if (!isConfigured(config)) {
    console.log(skipMessage(label, 'mysql', config));
    return;
  }
  let conn;
  try { conn = await mysql.createConnection(config); }
  catch (e) {
    console.log(connectSkip(label, config, e));
    return;
  }
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

  await conn.end();
};

main().catch(e => { console.error('FAILED', e); process.exitCode = 1; });