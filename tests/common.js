import assert from 'assert';
let bunTest;
try { bunTest = await import('bun:test'); } catch { }

// Thrown by a test body when a backend (pg/mysql/mongodb) is unavailable,
// so specs can be written once for every backend and degrade gracefully.
export class SkipError extends Error {
  constructor(reason) { super(reason || 'backend unavailable'); this.name = 'SkipError'; }
}

let skipCount = 0;
export const getSkipCount = () => skipCount;
export const resetSkipCount = () => { skipCount = 0; };

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
      let actual;
      try {
        actual = normalizeRows(await exec());
      } catch (e) {
        if (e instanceof SkipError) { console.log(`SKIP ${title} (${e.message})`); return; }
        throw e;
      }
      assert.deepStrictEqual(actual, expected);
    });
    return;
  }
  let actual;
  try {
    actual = normalizeRows(await exec());
  } catch (e) {
    if (e instanceof SkipError) { skipCount++; console.log(`SKIP ${title} (${e.message})`); return; }
    throw e;
  }
  assert.deepStrictEqual(actual, expected);
  console.log('PASS ' + title);
};

export const runStringTest = (title, exec, expected) => {
  if (bunTest?.test) {
    bunTest.test(title, async () => {
      let actual;
      try {
        actual = exec();
      } catch (e) {
        if (e instanceof SkipError) { console.log(`SKIP ${title} (${e.message})`); return; }
        throw e;
      }
      if (expected instanceof RegExp) assert.match(actual, expected);
      else assert.strictEqual(actual, expected);
    });
    return;
  }
  let actual;
  try {
    actual = exec();
  } catch (e) {
    if (e instanceof SkipError) { skipCount++; console.log(`SKIP ${title} (${e.message})`); return; }
    throw e;
  }
  if (expected instanceof RegExp) assert.match(actual, expected);
  else assert.strictEqual(actual, expected);
  console.log(`PASS ${title}`);
};