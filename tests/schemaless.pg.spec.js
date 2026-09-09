import dotenv from 'dotenv';
import pkg from 'pg';
import { createSchemalessAdapter } from '../src/schemaless.js';
import { runTest } from './common.js';
dotenv.config();

const config = { host: process.env.PG_HOST, port: process.env.PG_PORT || 5432, user: process.env.PG_USER, password: process.env.PG_PASSWORD, database: process.env.PG_DB };

const main = async () => {
  if (!config.host) return; // skip when no Postgres is configured (matches mysql spec behavior)

  const { Client } = pkg;
  const client = new Client(config);

  try {
    await client.connect();

    const { adapter } = createSchemalessAdapter(client, 'pg');
    const users = adapter.collection('users');

    await runTest(
      'Insert and update JSON',
      async () => {
        await client.query('DROP TABLE IF EXISTS users');
        await client.query(
          'CREATE TABLE users (_id SERIAL PRIMARY KEY, name TEXT, profile JSONB)'
        );

        await users.insertOne({
          name: 'Alice',
          age: 25,
          profile: { score: 85 }
        });

        await users.updateOne(
          { name: 'Alice' },
          { $inc: { 'profile.score': 5 } }
        );

        const res = await client.query(
          "SELECT profile->>'score' AS score FROM users WHERE name = 'Alice'"
        );

        return res.rows.map(r => ({
          score: parseInt(r.score)
        }));
      },
      [{ score: 90 }]
    );
  } finally {
    await client.end().catch(() => {});
  }
};

main().catch(err => {
  console.error(err);
  process.exitCode = 1;
});
