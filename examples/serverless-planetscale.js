import { createSQLAdapter } from '../src/schemaless.js';
import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers } from '../index.js';
import { loadEnv } from '../src/env.js';

await loadEnv();

const qb = createQueryBuilder({ filterOps, exprOps, updateOps, stageHandlers });

const url = process.env.PSCALE_DATA_API_URL;
const token = process.env.PSCALE_TOKEN;

const execute = async (sql, params = []) => {
  const res = await fetch(url, { method: 'POST', headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' }, body: JSON.stringify({ sql, params }) });
  const json = await res.json();
  if (json.rows) return { rows: json.rows };
  if (Array.isArray(json)) return json;
  return json;
};

(async () => {
  const adapter = createSQLAdapter({ database: 'mysql', execute, queryBuilder: qb });
  await adapter.execute('DROP TABLE IF EXISTS users');
  const insertSQL = qb.insertMany('users', [ { name: 'Alice', age: 25, profile: { score: 85 } } ], 'mysql');
  await adapter.execute(insertSQL);
  const updSQL = qb.collection('users', 'mysql').updateOne({ name: 'Alice' }, { $set: { 'profile.score': 90 } });
  await adapter.execute(updSQL);
  const findSQL = qb.collection('users', 'mysql').find({ name: 'Alice' }).toSQL();
  const rows = await adapter.execute(findSQL);
  console.log(rows.rows || rows);
})();