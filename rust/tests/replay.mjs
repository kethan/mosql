/**
 * Replays the JavaScript-generated fixture corpus through a mosql bridge.
 *
 * Used by both `../wasm/test.mjs` and `../node/test.mjs`, so the wasm build and
 * the native addon are held to exactly the same bar.
 */

export function loadFixtures(url) {
  return JSON.parse(readFileSync(url));
}

import { readFileSync } from 'node:fs';

/** Replay `tests/fixtures.json` (the SQL builder corpus). Returns failures. */
export function replayBuilder(mosql, fixtures) {
  const failures = [];

  for (const c of fixtures) {
    const req = { op: c.kind, table: c.table, db: c.db, variant: c.variant };
    const a = c.args;

    switch (c.kind) {
      case 'find':
      case 'findOne':
        req.query = a[0] ?? {};
        if (a.length > 1) req.projection = a[1];
        for (const s of c.chain ?? []) {
          if (s.m === 'sort') req.sort = s.a[0];
          else if (s.m === 'limit') req.limit = s.a[0];
          else if (s.m === 'skip') req.skip = s.a[0];
          else if (s.m === 'distinct') req.distinct = true;
          else if (s.m === 'count') req.count = true;
        }
        break;
      case 'filter': req.query = a[0]; break;
      case 'expression': req.expr = a[0]; break;
      case 'aggregate': req.pipeline = a[0]; break;
      case 'insertMany': req.docs = a[0]; break;
      case 'updateMany': req.query = a[0]; req.update = a[1]; break;
      case 'deleteMany':
        req.query = a[0];
        req.allowDeleteAll = !!(a[1] && a[1].allowDeleteAll);
        break;
      case 'countDocuments': req.query = a[0]; break;
      case 'distinct': req.field = a[0]; req.query = a[1]; break;
      default: throw new Error('unknown kind ' + c.kind);
    }

    let sql;
    let error;
    try {
      sql = mosql.build(req);
    } catch (e) {
      error = String(e.message);
    }

    if (c.error !== undefined) {
      if (error !== c.error) {
        failures.push(`${c.name}\n  js:   ${c.error}\n  got:  ${error}`);
      }
    } else if (sql !== c.sql) {
      failures.push(`${c.name}\n  js:   ${c.sql}\n  got:  ${sql}`);
    }
  }

  return failures;
}

/** Replay the filter and aggregate cases of `tests/memory-fixtures.json`. */
export function replayMemory(mosql, fixtures) {
  const failures = [];

  for (const c of fixtures) {
    if (c.kind !== 'filter' && c.kind !== 'aggregate') continue;

    const coll = mosql.openMemory();
    coll.insert(c.docs);

    const actual = c.kind === 'filter'
      ? coll.find(c.query).map((d) => d._id)
      : coll.aggregate(c.pipeline);
    const expected = c.kind === 'filter' ? c.result.map((d) => d._id) : c.result;
    coll.close();

    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
      failures.push(
        `${c.kind}/${c.name}\n  js:   ${JSON.stringify(expected)}\n  got:  ${JSON.stringify(actual)}`,
      );
    }
  }

  return failures;
}

/** A handful of behavioural checks that the fixture corpus does not cover. */
export function replayBehaviour(mosql) {
  const failures = [];
  const check = (label, actual, expected) => {
    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
      failures.push(`${label}\n  expected: ${JSON.stringify(expected)}\n  actual:   ${JSON.stringify(actual)}`);
    }
  };

  const mem = mosql.openMemory();
  mem.insert([
    { _id: 1, name: 'Ada', age: 36, active: true, tags: ['x', 'y'], profile: { score: 95, city: 'London' } },
    { _id: 2, name: 'Bob', age: 17, active: false, tags: ['y'], profile: { score: 55, city: 'Paris' } },
    { _id: 3, name: 'Cyd', age: 45, active: true, tags: [], profile: { score: 75, city: 'London' } },
    { _id: 4, name: 'Dee', age: 28, active: null, tags: ['z'], profile: { score: 85, city: 'Berlin' } },
  ]);

  check('find/sort', mem.find({}, { sort: { age: -1 }, limit: 2 }).map((d) => d.name), ['Cyd', 'Ada']);
  check('find/count', mem.find({ active: true }, { count: true }), [{ count: 2 }]);
  check('find/projection', mem.find({ name: 'Ada' }, { projection: { name: 1, _id: 0 } }), [{ name: 'Ada' }]);
  check(
    'aggregate/group',
    mem.aggregate([{ $group: { _id: '$profile.city', n: { $sum: 1 } } }, { $sort: { _id: 1 } }]),
    [
      { _id: 'Berlin', n: 1 },
      { _id: 'London', n: 2 },
      { _id: 'Paris', n: 1 },
    ],
  );
  mem.close();

  return failures;
}
