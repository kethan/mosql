import dotenv from 'dotenv';
import { createPostgresSchemaless } from '../src/adapter/pg/adapter.js';
import pkg from 'pg';
dotenv.config();

const cfg = { host: process.env.PG_HOST, port: process.env.PG_PORT || 5432, user: process.env.PG_USER, password: process.env.PG_PASSWORD, database: process.env.PG_DB };

const main = async () => {
  const { Client } = pkg;
  const client = new Client(cfg);
  await client.connect();
  const { adapter } = await createPostgresSchemaless(client);
  await adapter.dropCollection('users');
  const coll = adapter.collection('users');
  await coll.insertOne({ name: 'Alice', age: 25 });
  const rows = await coll.find({}).toArray();
  console.log(rows);
  await client.end();
};

main().catch(e => { console.error('ERR', e); process.exitCode = 1; });