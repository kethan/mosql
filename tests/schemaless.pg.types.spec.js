import dotenv from 'dotenv';
import { runTest } from './common.js';
import { createSchemalessAdapter } from '../src/schemaless.js';
dotenv.config();

const config = { host: process.env.PG_HOST, port: process.env.PG_PORT || 5432, user: process.env.PG_USER, password: process.env.PG_PASSWORD, database: process.env.PG_DB };

const main = async () => {
  if (!config.host) return;
  const pkg = await import('pg');
  const Client = pkg.Client || pkg.default?.Client;
  const client = new Client(config);
  await client.connect();
  const { adapter } = createSchemalessAdapter(client, 'pg');
  const coll = adapter.collection('auto_types');

  await runTest('pg schemaless insert auto types', async () => {
    await coll.insertOne({
      smallNum: 100,
      largeNum: 2147483647,
      decimalNum: 99.99,
      preciseNum: 3.141592653589793,
      shortText: 'Short',
      longText: 'A'.repeat(500),
      trueVal: true,
      falseVal: false,
      timestamp: new Date(),
      jsonObj: { name: 'Alice', age: 30 },
      jsonArr: [1, 2, 3],
      nullField: null,
    });
    const schema = await adapter.getTableSchema('auto_types');
    const c = schema.columns || {};
    return [
      { smallNum: c.smallNum === 'INTEGER' },
      { largeNum: c.largeNum === 'INTEGER' },
      { decimalNum: c.decimalNum?.toUpperCase() === 'DECIMAL(20,6)' },
      { preciseNum: c.preciseNum?.toUpperCase() === 'DECIMAL(20,6)' },
      { shortText: c.shortText?.toUpperCase() === 'VARCHAR(255)' },
      { longText: c.longText?.toUpperCase() === 'TEXT' },
      { trueVal: c.trueVal?.toUpperCase() === 'BOOLEAN' },
      { falseVal: c.falseVal?.toUpperCase() === 'BOOLEAN' },
      { timestamp: c.timestamp?.toUpperCase() === 'TIMESTAMP' },
      { jsonObj: c.jsonObj?.toUpperCase() === 'JSONB' },
      { jsonArr: c.jsonArr?.toUpperCase() === 'JSONB' },
    ];
  }, [
    { smallNum: true },
    { largeNum: true },
    { decimalNum: true },
    { preciseNum: true },
    { shortText: true },
    { longText: true },
    { trueVal: true },
    { falseVal: true },
    { timestamp: true },
    { jsonObj: true },
    { jsonArr: true },
  ]);

  await runTest('pg schemaless queries', async () => {
    const checks = [];
    const q1 = await coll.find({ smallNum: { $gte: 50 } });
    checks.push({ smallNumOk: (await q1.toArray()).length >= 1 });
    const q2 = await coll.find({ decimalNum: { $between: [50, 100] } });
    checks.push({ decimalNumOk: (await q2.toArray()).length >= 1 });
    const q3 = await coll.find({ shortText: { $ilike: '%short%' } });
    checks.push({ shortTextOk: (await q3.toArray()).length >= 1 });
    const q4 = await coll.find({ trueVal: true });
    checks.push({ boolOk: (await q4.toArray()).length >= 1 });
    const q5 = await coll.find({ timestamp: { $gte: new Date('2024-01-01') } });
    checks.push({ tsOk: (await q5.toArray()).length >= 1 });
    const q6 = await coll.find({ 'jsonObj.name': 'Alice' });
    checks.push({ jsonOk: (await q6.toArray()).length >= 1 });
    const q7 = await coll.find({ nullField: { $exists: false } });
    checks.push({ nullOk: (await q7.toArray()).length >= 0 });
    return checks;
  }, [
    { smallNumOk: true },
    { decimalNumOk: true },
    { shortTextOk: true },
    { boolOk: true },
    { tsOk: true },
    { jsonOk: true },
    { nullOk: true },
  ]);

  await client.end();
};

main().catch(e => { process.exitCode = 1; });