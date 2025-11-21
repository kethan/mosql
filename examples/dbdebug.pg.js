import dotenv from 'dotenv';
import { createSchemalessAdapter } from '../src/schemaless.js';
import pkg from 'pg';
dotenv.config();

const cfg = { host: process.env.PG_HOST, port: process.env.PG_PORT || 5432, user: process.env.PG_USER, password: process.env.PG_PASSWORD, database: process.env.PG_DB };

const main = async () => {
  const { Client } = pkg;
  const client = new Client(cfg);
  await client.connect();
  const { adapter } = createSchemalessAdapter(client, 'pg');
  console.log('tables', await adapter.listCollections());
  const ex = await client.query("SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'pg_schema_types') as ex");
  console.log('exists', ex.rows[0].ex);
  const s = await adapter.getTableSchema('pg_schema_types');
  console.log(Object.keys(s.columns));
  try {
    await adapter.execute("ALTER TABLE pg_schema_types ADD COLUMN smallintCol SMALLINT");
    console.log('added smallintCol');
  } catch (e) { console.log('err smallintCol', e.message || e); }
  const s2 = await adapter.getTableSchema('pg_schema_types');
  console.log('after', Object.keys(s2.columns));
  await client.end();
};

main().catch(e => { console.error('ERR', e); process.exitCode = 1; });
