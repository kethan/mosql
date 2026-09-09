import assert from 'assert';
let bunTest;
try { bunTest = await import('bun:test'); } catch { }

export const eq = (a, b, keys, ctor) => a === b || (
  a && b && (ctor = a.constructor) === b.constructor
    ? ctor === Array ? a.length === b.length && a.every((val, idx) => eq(val, b[idx]))
      : ctor === Object && (keys = Object.keys(a)).length === Object.keys(b).length && keys.every((k) => k in b && eq(a[k], b[k]))
    : (a !== a && b !== b)
);

export const normalizeRows = (rows) => rows.map((r) => {
  const obj = {};
  for (const [k, v] of Object.entries(r)) {
    if (typeof v === 'string' && /^-?\d+(\.\d+)?$/.test(v)) obj[k] = parseFloat(v);
    else obj[k] = typeof v === 'number' ? Math.round(v * 1000) / 1000 : v;
  }
  return obj;
});

export const runTest = async (title, exec, expected) => {
  if (bunTest?.test) {
    bunTest.test(title, async () => {
      const actual = normalizeRows(await exec());
      assert.deepStrictEqual(actual, expected);
    });
    return;
  }
  const actual = normalizeRows(await exec());
  assert.deepStrictEqual(actual, expected);
  console.log('PASS ' + title);
};

export const runStringTest = (title, exec, expected) => {
  if (bunTest?.test) {
    bunTest.test(title, async () => {
      const actual = exec();
      if (expected instanceof RegExp) assert.match(actual, expected);
      else assert.strictEqual(actual, expected);
    });
    return;
  }
  const actual = exec();
  if (expected instanceof RegExp) assert.match(actual, expected);
  else assert.strictEqual(actual, expected);
  console.log(`PASS ${title}`);
};

// The tail a single-row statement carries after its SET/FROM clause.
// `LIMIT 1` only exists on MySQL; SQLite narrows through `rowid` and PostgreSQL
// through `ctid`, because neither supports `UPDATE/DELETE ... LIMIT`.
export const oneRow = (db, table, where) => {
  const cond = where ? ` WHERE ${where}` : '';
  if (db === 'mysql') return `${cond} LIMIT 1`;
  const rid = db === 'pg' ? 'ctid' : 'rowid';
  const one = ` ${rid} IN (SELECT ${rid} FROM ${table}${cond} LIMIT 1)`;
  return cond ? `${cond} AND${one}` : ` WHERE${one}`;
};

// ---------------------------------------------------------------- DB run config
//
// The only place the database environment is read. The names are the ones CI and
// `.env.example` use - PG_/MYSQL_/MONGO_ prefixes - and the defaults (ports) live
// here instead of being repeated per file. libpq's PGHOST/PGUSER and the
// Planetscale-style MYSQLHOST were previously honoured by *some* specs only, so
// the same .env configured some files and left others skipping.
const toPort = (value, fallback) => (value == null || value === '' ? fallback : Number(value));

export const pgConfig = (env = process.env) => ({
  host: env.PG_HOST,
  port: toPort(env.PG_PORT, 5432),
  user: env.PG_USER,
  password: env.PG_PASSWORD,
  database: env.PG_DB,
});

export const mysqlConfig = (env = process.env) => ({
  host: env.MYSQL_HOST,
  port: toPort(env.MYSQL_PORT, 3306),
  user: env.MYSQL_USER,
  password: env.MYSQL_PASS,
  database: env.MYSQL_DB,
});

export const mongoConfig = (env = process.env) => ({
  host: env.MONGO_HOST,
  port: toPort(env.MONGO_PORT, 27017),
  user: env.MONGO_USER,
  password: env.MONGO_PASSWORD,
  database: env.MONGO_DB,
  authSource: env.MONGO_AUTH_SOURCE,
});

// A server counts as configured once host, user and database are set.
export const isConfigured = (cfg) => !!(cfg.host && cfg.user && cfg.database);

const REQUIRED_ENV = {
  pg: { host: 'PG_HOST', user: 'PG_USER', database: 'PG_DB' },
  mysql: { host: 'MYSQL_HOST', user: 'MYSQL_USER', database: 'MYSQL_DB' },
  mongo: { host: 'MONGO_HOST', user: 'MONGO_USER', database: 'MONGO_DB' },
};

// A skip has to say what it skipped: a live-database spec that quietly does
// nothing is indistinguishable from one that passed.
export const skipMessage = (label, kind, cfg) => {
  const map = REQUIRED_ENV[kind];
  const missing = Object.entries(map).filter(([key]) => !cfg[key]).map(([, name]) => name);
  return `SKIP ${label} - not configured, set ${missing.join(', ')} (see .env.example)`;
};

// Same shape for the other reason a live spec does not run: the variables are set,
// but nothing answered. Both are reported, so `npm test` output always says which
// database coverage was left out and why.
export const connectSkip = (label, cfg, error) =>
  `SKIP ${label} - cannot connect to ${cfg.host}:${cfg.port}/${cfg.database}: ${error?.message || error}`;

// ---------------------------------------------------------------- schema shape
//
// The columns a "NULL everywhere" document has to leave alone, which the schema's
// `required` list does not cover: SERIAL/BIGSERIAL (IDENTITY/AUTO_INCREMENT on
// other engines) are sequence-backed and therefore implicitly NOT NULL, PRIMARY
// KEY and NOT NULL say so in the declared type, and a column with a DEFAULT must
// stay omitted entirely or the default is never exercised. Writing an explicit NULL
// into any of them fails with 23502 (PostgreSQL) / 1048 (MySQL) instead of probing
// nullability, which is what makes "set every column to NULL" a schema-wide test
// rather than a list of the keys someone remembered.
const NOT_NULL_BY_TYPE = /SERIAL|IDENTITY|AUTO_INCREMENT|PRIMARY KEY|NOT NULL/i;

export const shapeOf = (def) => (typeof def === 'string' ? { type: def } : def || {});

export const nullableColumns = (schema) => Object.keys(schema || {}).filter((key) => {
  const def = shapeOf(schema[key]);
  return !def.required
    && def.default === undefined
    && !NOT_NULL_BY_TYPE.test(String(def.type || ''));
});
