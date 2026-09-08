import dotenv from 'dotenv';
import mysql from 'mysql2/promise';
import { createSchemalessAdapter } from '../src/schemaless.js';
import { runTest } from './common.js';
dotenv.config();

const config = { host: process.env.MYSQL_HOST, user: process.env.MYSQL_USER, password: process.env.MYSQL_PASS, database: process.env.MYSQL_DB };

const main = async () => {
  if (!config.host) return;
  let conn;
  try { conn = await mysql.createConnection(config); } catch { return; }
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