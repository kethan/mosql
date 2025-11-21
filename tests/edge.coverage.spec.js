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