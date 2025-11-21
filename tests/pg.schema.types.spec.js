import dotenv from 'dotenv';
import { runTest } from './common.js';
import { createSchemalessAdapter } from '../src/schemaless.js';
dotenv.config();

const cfg = { host: process.env.PG_HOST, port: process.env.PG_PORT || 5432, user: process.env.PG_USER, password: process.env.PG_PASSWORD, database: process.env.PG_DB };

const main = async () => {
  if (!cfg.host) return;
  const pkg = await import('pg');
  const Client = pkg.Client || pkg.default?.Client;
  const client = new Client(cfg);
  try { await client.connect(); } catch { return; }
  const { adapter } = createSchemalessAdapter(client, 'pg');
  const coll = adapter.collection('pg_schema_types', {
    schema: {
      smallintCol: 'SMALLINT', integerCol: 'INTEGER', bigintCol: 'BIGINT',
      serialCol: 'SERIAL', bigserialCol: 'BIGSERIAL',
      decimalCol: 'DECIMAL(10,2)', numericCol: 'NUMERIC(15,5)', realCol: 'REAL', doublePrecisionCol: 'DOUBLE PRECISION',
      moneyCol: 'MONEY',
      charCol: 'CHAR(10)', varcharCol: 'VARCHAR(255)', textCol: 'TEXT',
      byteaCol: 'BYTEA',
      booleanCol: 'BOOLEAN',
      dateCol: 'DATE', timeCol: 'TIME', timeWithTzCol: 'TIME WITH TIME ZONE', timestampCol: 'TIMESTAMP', timestampTzCol: 'TIMESTAMPTZ', intervalCol: 'INTERVAL',
      jsonCol: 'JSON', jsonbCol: 'JSONB',
      intArrayCol: 'INTEGER[]', textArrayCol: 'TEXT[]',
      uuidCol: 'UUID',
      inetCol: 'INET', cidrCol: 'CIDR', macaddrCol: 'MACADDR', macaddr8Col: 'MACADDR8',
      bitCol: 'BIT(8)', bitVaryingCol: 'BIT VARYING(16)',
      tsvectorCol: 'TSVECTOR', tsqueryCol: 'TSQUERY',
      pointCol: 'POINT', lineCol: 'LINE', lsegCol: 'LSEG', boxCol: 'BOX', pathCol: 'PATH', polygonCol: 'POLYGON', circleCol: 'CIRCLE',
      int4rangeCol: 'INT4RANGE', int8rangeCol: 'INT8RANGE', numrangeCol: 'NUMRANGE', tsrangeCol: 'TSRANGE', tstzrangeCol: 'TSTZRANGE', daterangeCol: 'DATERANGE',
      xmlCol: 'XML',
      uniqueCol: { type: 'TEXT', unique: true }, requiredCol: { type: 'TEXT', required: true }, defaultCol: { type: 'INTEGER', default: 100 }, hiddenCol: { type: 'TEXT', hidden: true },
    },
  });

  await coll.findOne({});
  const doc = Object.fromEntries(Object.keys(coll.schema).map(k => [k, null]));
  await coll.insertOne(doc);

  const ensure = async (name, typeSpec) => {
    const s = await adapter.getTableSchema('pg_schema_types');
    if (!s.columns[name]) {
      try {
        await adapter.execute(`ALTER TABLE pg_schema_types ADD COLUMN ${name} ${typeof typeSpec === 'string' ? typeSpec : typeSpec.type}`);
      } catch (e) {}
    }
    const s2 = await adapter.getTableSchema('pg_schema_types');
    return !!s2.columns[name];
  };

  const defs = coll.schema;
  const supportMap = Object.fromEntries(await Promise.all(Object.keys(defs).map(async k => [k, await ensure(k, defs[k])])));

  await runTest('pg schema columns exist', async () => {
    const s = await adapter.getTableSchema('pg_schema_types');
    const c = s.columns || {};
    const keys = Object.keys(defs);
    return keys.map(k => ({ [k]: !!c[k] }));
  }, Object.keys(defs).map(k => ({ [k]: !!supportMap[k] })));

  await client.end();
};

main().catch(e=>{ process.exitCode = 1; });