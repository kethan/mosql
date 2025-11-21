import dotenv from 'dotenv';
import { createSQLAdapter } from '../src/schemaless.js';
import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers } from '../index.js';
dotenv.config();

const qb = createQueryBuilder({ filterOps, exprOps, updateOps, stageHandlers });

const url = process.env.NEON_HTTP_URL || process.env.DATABASE_URL;
const key = process.env.NEON_API_KEY;

const execute = async (sql, params = []) => {
  const res = await fetch(url, { method: 'POST', headers: { Authorization: `Bearer ${key}`, 'Content-Type': 'application/json' }, body: JSON.stringify({ sql, params }) });
  const json = await res.json();
  if (json.rows) return { rows: json.rows };
  if (Array.isArray(json)) return json;
  return json;
};

(async () => {
  if (!url || !String(url).startsWith('http')) return;
  const adapter = createSQLAdapter({ database: 'pg', execute, queryBuilder: qb });
  await adapter.execute('DROP TABLE IF EXISTS users');
  const insertSQL = qb.insertMany('users', [ { name: 'Alice', age: 25, profile: { score: 85 } } ], 'pg');
  await adapter.execute(insertSQL);
  const findSQL = qb.collection('users', 'pg').find({ name: 'Alice' }).toSQL();
  const rows = await adapter.execute(findSQL);
  console.log(rows.rows || rows);
})();