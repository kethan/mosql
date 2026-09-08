import { createSchemalessClient } from '../src/client.js';
import { loadEnv } from '../src/env.js';

await loadEnv();

const cfg = { host: process.env.PG_HOST, port: process.env.PG_PORT || 5432, user: process.env.PG_USER, password: process.env.PG_PASSWORD, database: process.env.PG_DB };

const main = async () => {
  const client = await createSchemalessClient('pg', cfg);
  await client.adapter.dropCollection('users').catch(() => {});
  const coll = client.db('app').collection('users');
  console.log(await coll.insertOne({ name: 'Alice', age: 25 }));
  const rows = await (await coll.find({})).toArray();
  console.log(rows);
  await client.close();
};

main().catch(e => { console.error('ERR', e); process.exitCode = 1; });