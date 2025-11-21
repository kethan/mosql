import dotenv from 'dotenv';
import { createMySQLSchemaless } from '../src/adapter/mysql/adapter.js';
dotenv.config();

(async () => {
  const cfg = { host: process.env.MYSQL_HOST, user: process.env.MYSQL_USER, password: process.env.MYSQL_PASS, database: process.env.MYSQL_DB };
  const mysql = await import('mysql2/promise');
  const conn = await mysql.createConnection(cfg);
  const { adapter } = await createMySQLSchemaless(conn);
  const users = adapter.collection('users');
  await conn.query('DROP TABLE IF EXISTS users');
  await users.insertOne({ name: 'Alice', age: 25, profile: { score: 85 } });
  await users.updateOne({ name: 'Alice' }, { $set: { 'profile.score': 90 } });
  const [rows] = await conn.query("SELECT JSON_EXTRACT(profile, '$.score') AS score FROM users WHERE name = 'Alice'");
  console.log('score', rows);
  console.log('estimatedDocumentCount', await users.estimatedDocumentCount());
  await conn.end();
})();