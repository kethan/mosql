import { createSchemalessAdapter } from '../src/schemaless.js';
import { runTest } from './common.js';
import { createPGClient } from './db-helpers.js';

const main = async () => {
  let client;
  try {
    ({ client } = await createPGClient());
    await client.connect();
  } catch (e) {
    console.error('[schemaless.pg] database unavailable, skipping:', e?.message || e);
    return;
  }

  try {
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
  console.error('[schemaless.pg] FAILED');
  console.error(err);
  process.exitCode = 1;
});
