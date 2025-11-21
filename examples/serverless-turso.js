import dotenv from 'dotenv';
import { createSQLAdapter } from '../src/schemaless.js';
import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers } from '../index.js';
dotenv.config();

const qb = createQueryBuilder({ filterOps, exprOps, updateOps, stageHandlers });

const url = process.env.TURSO_HTTP_URL || process.env.DATABASE_URL;
const token = process.env.TURSO_TOKEN || process.env.DATABASE_AUTH_TOKEN;

const execute = async (sql, params = []) => {
  const res = await fetch(url, { method: 'POST', headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' }, body: JSON.stringify({ sql, params }) });
  const json = await res.json();
  if (json.rows) return { rows: json.rows };
  if (Array.isArray(json)) return json;
  return json;
};

(async () => {
  const adapter = createSQLAdapter({ database: 'sqlite', execute, queryBuilder: qb });
  await adapter.execute('DROP TABLE IF EXISTS users');
  const insertSQL = qb.insertMany('users', [ { name: 'Alice', age: 25, profile: { score: 85 } } ], 'sqlite');
  await adapter.execute(insertSQL);
  const findSQL = qb.collection('users', 'sqlite').find({ name: 'Alice' }).toSQL();
  const rows = await adapter.execute(findSQL);
  console.log(rows.rows || rows);
})();