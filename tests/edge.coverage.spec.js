import Database from 'better-sqlite3';
import { createSchemalessAdapter } from '../src/schemaless.js';
import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers } from '../src/index.js';
import { runTest } from './common.js';

const { adapter } = createSchemalessAdapter(new Database(':memory:'), 'sqlite');
const qb = createQueryBuilder({ filterOps, exprOps, updateOps, stageHandlers });

const coll = adapter.collection('edge_cov');

await runTest('edge/sqlite createTableWithSchema', async () => {
  await adapter.createTableWithSchema('edge_schema_table', {
    properties: { name: { type: 'string' }, count: { type: 'integer' }, created: { type: 'string', format: 'date-time' } },
    required: ['name']
  });
  const s = await adapter.getTableSchema('edge_schema_table');
  return [{ hasName: !!s.columns.name, hasCount: !!s.columns.count }];
}, [{ hasName: true, hasCount: true }]);

await runTest('edge/sqlite execWithDDLRetry creates table on find', async () => {
  const rows = await (await adapter.collection('edge_new_table').find({})).toArray();
  const list = await adapter.listCollections();
  return [{ ok: Array.isArray(rows), has: list.includes('edge_new_table') }];
}, [{ ok: true, has: true }]);

await runTest('edge/sqlite update $rename JSON error', async () => {
  let threw = false;
  try {
    qb.collection('users', 'sqlite').updateOne({}, { $rename: { 'profile.k': 'new' } });
  } catch { threw = true; }
  return [{ threw }];
}, [{ threw: true }]);

await runTest('edge/sqlite dropColumn not supported', async () => {
  let threw = false;
  try { await coll.dropColumn('nope'); } catch { threw = true; }
  return [{ threw }];
}, [{ threw: true }]);

await runTest('edge/sqlite modifyColumn not supported', async () => {
  let threw = false;
  try { await adapter.modifyColumn('edge_schema_table', 'name', 'TEXT'); } catch { threw = true; }
  return [{ threw }];
}, [{ threw: true }]);
// What each driver's message means for the DDL retry. PostgreSQL words a missing
// column with "does not exist" as well (`column "x" of relation "y" does not exist`),
// and reading that as a missing table makes the retry re-run the failing statement
// after a no-op CREATE TABLE IF NOT EXISTS instead of adding the column - which is
// how updating an unknown field came to fail on pg only.
await runTest('edge/DDL retry separates a missing column from a missing table', async () => {
  const of = (e) => { const { missingTable, missingColumn } = coll.classifyError(e); return { missingTable, missingColumn }; };
  return [
    of({ message: 'column "alias" of relation "users" does not exist' }),
    of({ message: 'relation "users" does not exist' }),
    of({ code: '42703' }),
    of({ code: '42P01' }),
    of({ message: "Unknown column 'alias' in 'field list'", errno: 1054 }),
    of({ message: 'no such table: users' }),
  ];
}, [
  { missingTable: false, missingColumn: true },
  { missingTable: true, missingColumn: false },
  { missingTable: false, missingColumn: true },
  { missingTable: true, missingColumn: false },
  { missingTable: false, missingColumn: true },
  { missingTable: true, missingColumn: false },
]);
