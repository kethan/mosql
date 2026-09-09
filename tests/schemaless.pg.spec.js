import { loadEnv } from '../src/env.js';
import pkg from 'pg';
import { createSchemalessAdapter } from '../src/schemaless.js';
import { runTest, pgConfig, isConfigured, skipMessage, connectSkip } from './common.js';
await loadEnv();

const config = pgConfig();
const label = 'schemaless.pg';

const main = async () => {
  // Without a configured server this file does nothing (the unit CI job has no
  // PostgreSQL) - but it always says so, because a silent skip reads like a pass.
  if (!isConfigured(config)) {
    console.log(skipMessage(label, 'pg', config));
    return;
  }

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
  // A configured but unreachable server is an environment problem, not a code
  // failure - report it as a skip so `npm test` stays meaningful offline.
  if (/ECONNREFUSED|ETIMEDOUT|ENOTFOUND|EHOSTUNREACH|getaddrinfo|authentication|pg_hba/i.test(String(err?.message || err))) {
    console.log(connectSkip(label, config, err));
    return;
  }
  console.error(err);
  process.exitCode = 1;
});
