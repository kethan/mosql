// Deterministic regression tests for the MySQL DDL-migration paths of the
// schemaless SQL adapter, driven by a mock mysql2 connection instead of a
// real server (no MySQL reachable from this sandbox). The mock faithfully
// reproduces the two MySQL behaviors these tests guard against:
//   1. information_schema reports VARCHAR columns without their length
//      (DATA_TYPE is bare 'varchar'), and MySQL rejects a lengthless
//      VARCHAR in DDL with a syntax error (errno 1064). $rename pre-adds the
//      target column with the type copied from the source column, which used
//      to emit `ALTER TABLE ... ADD town VARCHAR` and fail silently.
//   2. Updating an unknown column fails with errno 1054, which triggers the
//      adapter's DDL-retry hook.

import assert from 'assert';
import { createSQLAdapter } from '../src/schemaless.js';
import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers } from '../src/index.js';
import { runTest } from './common.js';

const mysqlError = (errno, message) => {
  const e = new Error(message);
  e.errno = errno;
  return e;
};

// Minimal SQL engine for exactly the statements the adapter emits for the
// exercised flows: information_schema lookups, CREATE/ALTER/INSERT/UPDATE,
// COUNT and SELECT * with a single `name = '...'` predicate.
const makeMockMySQL = () => {
  const tables = new Map(); // tableName -> { columns: Map<name,type>, rows: [], nextId }
  const ddl = []; // every DDL statement, for assertions
  const parseColumns = (inside) => inside.split(',').map((part) => {
    const m = part.trim().match(/^`?([a-zA-Z_][\w]*)`?\s+([\w]+)(\([^)]*\))?/);
    return m ? { name: m[1], type: (m[2] + (m[3] || '')).toUpperCase() } : null;
  }).filter(Boolean);

  const execute = async (sql) => {
    sql = String(sql).trim();
    let m;

    m = sql.match(/^SELECT COUNT\(\*\) as count FROM information_schema\.tables WHERE table_name = '(\w+)'/i);
    if (m) return [[{ count: tables.has(m[1]) ? 1 : 0 }], []];

    m = sql.match(/^SELECT COLUMN_NAME, DATA_TYPE FROM information_schema\.columns WHERE TABLE_NAME = '(\w+)'/i);
    if (m) {
      const tbl = tables.get(m[1]);
      const rows = tbl
        ? [...tbl.columns.entries()].map(([COLUMN_NAME, type]) => ({ COLUMN_NAME, DATA_TYPE: type.replace(/\(.*\)/, '').toLowerCase() }))
        : [];
      return [rows, []];
    }

    m = sql.match(/^CREATE TABLE IF NOT EXISTS `?(\w+)`? \((.*)\)$/is);
    if (m) {
      const tbl = tables.get(m[1]) || { columns: new Map(), rows: [], nextId: 1 };
      for (const col of parseColumns(m[2])) tbl.columns.set(col.name, col.type);
      tables.set(m[1], tbl);
      ddl.push(sql);
      return { affectedRows: 0 };
    }

    m = sql.match(/^ALTER TABLE `?(\w+)`? ADD(?: COLUMN)? `?(\w+)`?\s+(.+)$/i);
    if (m) {
      const type = m[3].trim();
      // MySQL rejects a lengthless VARCHAR/CHAR in DDL — reproduce that so
      // the regression test fails if the adapter ever emits one again.
      if (/^(varchar|char)$/i.test(type)) throw mysqlError(1064, `You have an error in your SQL syntax near '${type}'`);
      // MySQL's grammar for `ALTER TABLE ... ADD col` breaks when the column
      // name matches a type keyword case-insensitively, e.g.
      // `ADD longText VARCHAR(255)` is a 1064 on real MySQL 8.0 because
      // `longText` lexes as LONGTEXT (even after `ADD COLUMN`). Backtick-
      // quoted identifiers are fine. Mirror that so this stays guarded.
      const withoutColumn = !/\bADD\s+COLUMN\b/i.test(sql);
      const quoted = new RegExp('`' + m[2] + '`', 'i').test(sql);
      if (!quoted && /^(longtext|mediumtext|tinytext|text|longblob|mediumblob|tinyblob|blob|date|datetime|timestamp|time|year|json)$/i.test(m[2])) {
        throw mysqlError(1064, `You have an error in your SQL syntax near '${m[2]} ${type}'`);
      }
      const tbl = tables.get(m[1]) || { columns: new Map(), rows: [], nextId: 1 };
      tbl.columns.set(m[2], type.toUpperCase());
      tables.set(m[1], tbl);
      ddl.push(sql);
      return { affectedRows: 0 };
    }

    m = sql.match(/^ALTER TABLE `?(\w+)`? MODIFY COLUMN `?(\w+)`?\s+(.+)$/i);
    if (m) {
      const tbl = tables.get(m[1]) || { columns: new Map(), rows: [], nextId: 1 };
      tbl.columns.set(m[2], m[3].trim().toUpperCase());
      tables.set(m[1], tbl);
      ddl.push(sql);
      return { affectedRows: 0 };
    }

    m = sql.match(/^DROP TABLE IF EXISTS `?(\w+)`?/i);
    if (m) { tables.delete(m[1]); return { affectedRows: 0 }; }

    m = sql.match(/^INSERT INTO `?(\w+)`? \(([^)]*)\) VALUES \((.*)\)$/is);
    if (m) {
      const tbl = tables.get(m[1]) || { columns: new Map(), rows: [], nextId: 1 };
      tables.set(m[1], tbl);
      const cols = m[2].split(',').map((c) => c.trim().replace(/`/g, ''));
      const vals = [...m[3].matchAll(/'(?:''|[^'])*'|-?\d+(?:\.\d+)?|NULL/gi)].map((v) => {
        const raw = v[0];
        if (raw.toUpperCase() === 'NULL') return null;
        if (raw.startsWith("'")) return raw.slice(1, -1).replace(/''/g, "'");
        return Number(raw);
      });
      const row = { _id: tbl.nextId++ };
      cols.forEach((c, i) => { row[c] = vals[i]; });
      tbl.rows.push(row);
      return { affectedRows: 1, insertId: row._id };
    }

    m = sql.match(/^UPDATE `?(\w+)`? SET (.+?)(?: WHERE (.+?))?(?: LIMIT \d+)?$/is);
    if (m) {
      const tbl = tables.get(m[1]);
      const sets = m[2].split(',').map((clause) => {
        const eq = clause.indexOf('=');
        return { col: clause.slice(0, eq).trim().replace(/`/g, ''), expr: clause.slice(eq + 1).trim() };
      });
      for (const { col } of sets) {
        if (!tbl || !tbl.columns.has(col)) throw mysqlError(1054, `Unknown column '${col}' in 'field list'`);
      }
      const wm = m[3]?.trim().match(/^`?(\w+)`? = '([^']*)'$/);
      let affected = 0;
      for (const row of tbl.rows) {
        if (wm && row[wm[1]] !== wm[2]) continue;
        for (const { col, expr } of sets) {
          if (/^NULL$/i.test(expr)) row[col] = null;
          else if (/^`?[a-zA-Z_][\w]*`?$/.test(expr)) row[col] = row[expr.replace(/`/g, '')];
          else row[col] = expr.startsWith("'") ? expr.slice(1, -1).replace(/''/g, "'") : Number(expr);
        }
        affected++;
      }
      return { affectedRows: affected };
    }

    m = sql.match(/^SELECT COUNT\(\*\) AS count FROM `?(\w+)`?(?: WHERE (.+?))?$/is);
    if (m) {
      const tbl = tables.get(m[1]);
      const wm = m[2]?.trim().match(/^`?(\w+)`? = '([^']*)'$/);
      const n = tbl ? (wm ? tbl.rows.filter((r) => r[wm[1]] === wm[2]).length : tbl.rows.length) : 0;
      return [[{ count: n }], []];
    }

    m = sql.match(/^SELECT \* FROM `?(\w+)`?(?: WHERE (.+?))?$/is);
    if (m) {
      const tbl = tables.get(m[1]);
      const wm = m[2]?.trim().match(/^`?(\w+)`? = '([^']*)'$/);
      const rows = tbl ? (wm ? tbl.rows.filter((r) => r[wm[1]] === wm[2]) : tbl.rows) : [];
      const names = [...(tbl?.columns.keys() || [])];
      return [rows.map((r) => { const out = {}; for (const n of names) out[n] = r[n] ?? null; return out; }), []];
    }

    throw new Error(`mock mysql: unsupported SQL: ${sql.slice(0, 80)}`);
  };

  return { execute, ddl };
};

const setup = () => {
  const mock = makeMockMySQL();
  const adapter = createSQLAdapter({
    database: 'mysql',
    execute: mock.execute,
    queryBuilder: createQueryBuilder({ filterOps, exprOps, updateOps, stageHandlers }),
  });
  return { adapter, mock, users: adapter.collection('users') };
};

const seed = [
  { name: 'Alice', age: 25, city: 'Paris' },
  { name: 'Bob', age: 30, city: 'London' },
  { name: 'Charlie', age: 22, city: 'Berlin' },
  { name: 'David', age: 40, city: 'Paris' },
];

await runTest('mysql mock $rename pre-adds target column with a valid type', async () => {
  const { users, mock } = setup();
  await users.insertMany(seed);
  await users.updateOne({ name: 'David' }, { $rename: { city: 'town' } });
  const row = await users.findOne({ name: 'David' });
  const addTown = mock.ddl.find((d) => /ADD(?: COLUMN)? `?town`?/i.test(d));
  assert.ok(addTown, 'expected an ALTER TABLE ... ADD town statement');
  assert.match(addTown, /VARCHAR\(\d+\)/i, 'town must be added with a length (bare VARCHAR is invalid DDL in MySQL)');
  return [{ isCityNull: row?.city == null, hasTown: row?.town != null, town: row?.town }];
}, [{ isCityNull: true, hasTown: true, town: 'Paris' }]);

await runTest('mysql mock plain inserts do not trigger spurious MODIFY COLUMN', async () => {
  const { users, mock } = setup();
  await users.insertMany(seed);
  await users.insertMany([{ name: 'Erin', age: 28, city: 'Rome' }]);
  const modifies = mock.ddl.filter((d) => /MODIFY COLUMN/i.test(d));
  return [{ modifies: modifies.length }];
}, [{ modifies: 0 }]);

await runTest('mysql mock $set migrates a missing column on update', async () => {
  const { users } = setup();
  await users.insertMany(seed);
  await users.updateOne({ name: 'Alice' }, { $set: { alias: 'ally' } });
  const row = await users.findOne({ name: 'Alice' });
  return [{ alias: row?.alias }];
}, [{ alias: 'ally' }]);

await runTest('mysql mock inferred columns are added with ADD COLUMN (type-named column like longText)', async () => {
  const { users, mock } = setup();
  // On real MySQL 8.0, `ALTER TABLE users ADD longText VARCHAR(255)` and even
  // `ADD COLUMN longText VARCHAR(255)` are syntax errors (longText lexes as
  // the LONGTEXT type keyword); the adapter must emit `ADD COLUMN` with a
  // backtick-quoted identifier so inferred columns survive.
  await users.insertOne({ name: 'Alice', longText: 'A'.repeat(600) });
  const row = await users.findOne({ name: 'Alice' });
  const adds = mock.ddl.filter((d) => /ADD/i.test(d) && /longText/i.test(d));
  assert.ok(adds.length > 0, 'expected an ALTER TABLE statement adding longText');
  assert.ok(adds.every((d) => /\bADD\s+COLUMN\b/i.test(d) && /`longText`/i.test(d)), `longText must be added as ADD COLUMN with a quoted identifier, got: ${adds.join(' | ')}`);
  return [{ longText: row?.longText?.length }];
}, [{ longText: 600 }]);
