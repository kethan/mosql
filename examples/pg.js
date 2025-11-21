import dotenv from 'dotenv';
import { createPostgresSchemaless } from '../src/adapter/pg/adapter.js';
dotenv.config();

(async () => {
  const cfg = { host: process.env.PG_HOST, port: process.env.PG_PORT || 5432, user: process.env.PG_USER, password: process.env.PG_PASSWORD, database: process.env.PG_DB };
  const { adapter, client } = await createPostgresSchemaless(await (await import('pg')).then(m => new m.Client(cfg)).then(async c => { await c.connect(); return c; }));
  const users = adapter.collection('users');
  await adapter.dropCollection('users').catch(()=>{});
  await users.insertOne({ name: 'Alice', age: 25, profile: { score: 85 } });
  await users.updateOne({ name: 'Alice' }, { $inc: { 'profile.score': 5 } });
  const res = await client.query("SELECT profile->>'score' AS score FROM users WHERE name = 'Alice'");
  console.log('score', res.rows);
  console.log('estimatedDocumentCount', await users.estimatedDocumentCount());
  await client.end();
})();