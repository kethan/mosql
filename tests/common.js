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
