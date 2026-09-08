import { createSchemalessClient } from '../src/client.js';
import { loadEnv } from '../src/env.js';

await loadEnv();

const cfg = {
  host: process.env.PG_HOST,
  port: process.env.PG_PORT || 5432,
  user: process.env.PG_USER,
  password: process.env.PG_PASSWORD,
  database: process.env.PG_DB,
};

// Bring your own driver instance if you prefer umosql not to resolve `pg` at all:
//
//   import { Client } from 'pg';
//   const pgClient = new Client({ connectionString: process.env.DATABASE_URL });
//   await pgClient.connect();
//   const client = await createSchemalessClient('pg', { client: pgClient });
//
// A supplied client is adopted as-is (never re-connected) and `client.close()` still ends it.

(async () => {
  const client = await createSchemalessClient('pg', cfg);
  const users = client.db('app').collection('users');

  await client.adapter.dropCollection('users').catch(() => { });
  await users.insertOne({ name: 'Alice', age: 25, profile: { score: 85 } });
  await users.updateOne({ name: 'Alice' }, { $inc: { 'profile.score': 5 } });

  // `client.raw` is the underlying pg.Client - use it for plain SQL.
  const res = await client.raw.query("SELECT profile->>'score' AS score FROM users WHERE name = 'Alice'");
  console.log('score', res.rows);
  console.log('estimatedDocumentCount', await users.estimatedDocumentCount());

  await client.close();
})();
