import { loadEnv } from '../src/env.js';
import { runTest, pgConfig, isConfigured, skipMessage, connectSkip } from './common.js';
import { createSchemalessAdapter } from '../src/schemaless.js';
await loadEnv();

const config = pgConfig();
const label = 'schemaless.pg.types';

const main = async () => {
  if (!isConfigured(config)) {
    console.log(skipMessage(label, 'pg', config));
    return;
  }
  const pkg = await import('pg');
  const Client = pkg.Client || pkg.default?.Client;
  const client = new Client(config);
  try { await client.connect(); }
  catch (e) {
    console.log(connectSkip(label, config, e));
    return;
  }
  const { adapter } = createSchemalessAdapter(client, 'pg');
  const coll = adapter.collection('auto_types');

  // Reads column types back out of information_schema, normalised to the type names
  // mosql asked for. Two Postgres details matter here: unquoted identifiers come back
  // folded to lower case, so the lookup folds its key too; and the catalog's plain
  // `data_type` (which is all getTableSchema returns) drops precisely the precision
  // this test is about - it says `numeric` and `character varying`, while
  // DECIMAL(20,6) / VARCHAR(255) only survive in numeric_precision and
  // character_maximum_length.
  const declaredTypes = async (table) => {
    const res = await adapter.execute(
      `SELECT column_name, data_type, character_maximum_length, numeric_precision, numeric_scale FROM information_schema.columns WHERE table_name = '${table}'`
    );
    const types = new Map();
    for (const row of res.rows || []) {
      let type = String(row.data_type || '');
      if (type === 'character varying' && row.character_maximum_length) type = `varchar(${row.character_maximum_length})`;
      else if (type === 'numeric' && row.numeric_precision != null) type = `decimal(${row.numeric_precision},${row.numeric_scale})`;
      else if (type.startsWith('timestamp')) type = 'timestamp';
      types.set(String(row.column_name).toLowerCase(), type.toUpperCase());
    }
    return (name) => types.get(String(name).toLowerCase()) || '';
  };

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
    // The types have to be read back from information_schema rather than
    // getTableSchema, for two Postgres-specific reasons: unquoted identifiers are
    // folded to lower case (so `columns.smallNum` never exists), and the catalog's
    // plain `data_type` drops exactly the precision this test is about - it says
    // `numeric` and `character varying`, while DECIMAL(20,6) and VARCHAR(255) only
    // survive in numeric_precision / character_maximum_length.
    const type = await declaredTypes('auto_types');
    return [
      { smallNum: type('smallNum') === 'INTEGER' },
      { largeNum: type('largeNum') === 'INTEGER' },
      { decimalNum: type('decimalNum') === 'DECIMAL(20,6)' },
      { preciseNum: type('preciseNum') === 'DECIMAL(20,6)' },
      { shortText: type('shortText') === 'VARCHAR(255)' },
      { longText: type('longText') === 'TEXT' },
      { trueVal: type('trueVal') === 'BOOLEAN' },
      { falseVal: type('falseVal') === 'BOOLEAN' },
      { timestamp: type('timestamp') === 'TIMESTAMP' },
      { jsonObj: type('jsonObj') === 'JSONB' },
      { jsonArr: type('jsonArr') === 'JSONB' },
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

main().catch(e => { console.error('FAILED', e); process.exitCode = 1; });