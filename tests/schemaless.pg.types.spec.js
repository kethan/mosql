import { runTest } from './common.js';
import { createSchemalessAdapter } from '../src/schemaless.js';
import { createPGClient } from './db-helpers.js';

const main = async () => {
  let client;
  try {
    ({ client } = await createPGClient());
    await client.connect();
  } catch (e) {
    console.error('[schemaless.pg.types] database unavailable, skipping:', e?.message || e);
    return;
  }

  try {
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
      });      const schema = await adapter.getTableSchema('auto_types');
      const c = schema.columns || {};
      // getTableSchema returns raw information_schema data_type values (SQL-standard names).
      // Postgres folds unquoted identifiers to lowercase, so look columns up case-insensitively.
      const typeOf = (name) => c[name] || c[name.toLowerCase()] || c[Object.keys(c).find((k) => k.toLowerCase() === name.toLowerCase()) || ''];
      return [
        { smallNum: typeOf('smallNum') === 'INTEGER' },
        { largeNum: typeOf('largeNum') === 'INTEGER' },
        { decimalNum: typeOf('decimalNum') === 'NUMERIC' },
        { preciseNum: typeOf('preciseNum') === 'NUMERIC' },
        { shortText: typeOf('shortText') === 'CHARACTER VARYING' },
        { longText: typeOf('longText') === 'TEXT' },
        { trueVal: typeOf('trueVal') === 'BOOLEAN' },
        { falseVal: typeOf('falseVal') === 'BOOLEAN' },
        { timestamp: typeOf('timestamp') === 'TIMESTAMP WITHOUT TIME ZONE' },
        { jsonObj: typeOf('jsonObj') === 'JSONB' },
        { jsonArr: typeOf('jsonArr') === 'JSONB' },
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
  } finally {
    await client.end().catch(() => {});
  }
};

main().catch(e => {
  console.error('[schemaless.pg.types] FAILED');
  console.error(e);
  process.exitCode = 1;
});
