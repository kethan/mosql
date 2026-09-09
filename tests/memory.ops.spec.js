import { collection, filter, expression, aggregate } from '../src/memory.js';
import { runTest } from './common.js';

const users = collection('users', [
  { _id: 1, name: 'Alice', age: 25, city: 'Paris', active: true, tags: ['a','b'], profile: { country: 'France', score: 85 } },
  { _id: 2, name: 'Bob', age: 30, city: 'London', active: true, tags: ['b'], profile: { country: 'UK', score: 90 } },
  { _id: 3, name: 'Charlie', age: 22, city: 'Berlin', active: false, tags: [], profile: { country: 'Germany', score: 60 } },
  { _id: 4, name: 'David', age: 40, city: 'Paris', active: true, tags: [], profile: { country: 'France', score: 95 } },
  { _id: 5, name: 'Eve', age: 35, city: 'Berlin', active: true, tags: [], profile: { country: 'Germany', score: 88 } },
]);

const main = async () => {
  await runTest('mem $eq', async () => users.find({ age: { $eq: 25 } }).toArray(), [ { _id: 1, name: 'Alice', age: 25, city: 'Paris', active: true, tags: ['a','b'], profile: { country: 'France', score: 85 } } ]);
  await runTest('mem $ne', async () => users.find({ age: { $ne: 25 } }).limit(1).toArray().map(x => ({ age: x.age })), [ { age: 30 } ]);
  await runTest('mem $gt', async () => users.find({ age: { $gt: 30 } }).toArray().map(x => ({ name: x.name })), [ { name: 'David' }, { name: 'Eve' } ]);
  await runTest('mem $gte', async () => users.find({ age: { $gte: 40 } }).toArray().map(x => ({ name: x.name })), [ { name: 'David' } ]);
  await runTest('mem $lt', async () => users.find({ age: { $lt: 25 } }).toArray().map(x => ({ name: x.name })), [ { name: 'Charlie' } ]);
  await runTest('mem $lte', async () => users.find({ age: { $lte: 25 } }).toArray().map(x => ({ name: x.name })), [ { name: 'Alice' }, { name: 'Charlie' } ]);
  await runTest('mem $in', async () => users.find({ age: { $in: [25,35] } }).toArray().map(x => ({ age: x.age })), [ { age: 25 }, { age: 35 } ]);
  await runTest('mem $nin', async () => users.find({ age: { $nin: [25,35] } }).toArray().map(x => ({ age: x.age })), [ { age: 30 }, { age: 22 }, { age: 40 } ]);
  await runTest('mem $and', async () => users.find({ $and: [ { age: { $gte: 25 } }, { city: 'Paris' } ] }).toArray().map(x => ({ name: x.name })), [ { name: 'Alice' }, { name: 'David' } ]);
  await runTest('mem $or', async () => users.find({ $or: [ { city: 'Paris' }, { city: 'Berlin' } ] }).toArray().map(x => ({ city: x.city })), [ { city: 'Paris' }, { city: 'Berlin' }, { city: 'Paris' }, { city: 'Berlin' } ]);
  await runTest('mem $not', async () => users.find({ $not: { active: true } }).toArray().map(x => ({ name: x.name })), [ { name: 'Charlie' } ]);
  await runTest('mem $nor', async () => users.find({ $nor: [ { active: true }, { city: 'Paris' } ] }).toArray().map(x => ({ name: x.name })), [ { name: 'Charlie' } ]);
  await runTest('mem $regex', async () => users.find({ name: { $regex: '^A' } }).toArray().map(x => ({ name: x.name })), [ { name: 'Alice' } ]);
  await runTest('mem $exists', async () => users.find({ middle: { $exists: false } }).limit(1).toArray().map(x => ({ ok: true })), [ { ok: true } ]);
  await runTest('mem $between', async () => users.find({ age: { $between: [30, 40] } }).toArray().map(x => ({ age: x.age })).sort((a,b)=>a.age-b.age), [ { age: 30 }, { age: 35 }, { age: 40 } ]);
  await runTest('mem $mod', async () => users.find({ age: { $mod: [5, 0] } }).toArray().map(x => ({ age: x.age })).sort((a,b)=>a.age-b.age), [ { age: 25 }, { age: 30 }, { age: 35 }, { age: 40 } ]);
  await runTest('mem $elemMatch', async () => users.find({ tags: { $elemMatch: { $eq: 'a' } } }).toArray().map(x => ({ name: x.name })), [ { name: 'Alice' } ]);
  await runTest('mem $all', async () => users.find({ tags: { $all: ['b'] } }).toArray().map(x => ({ name: x.name })), [ { name: 'Alice' }, { name: 'Bob' } ]);
  await runTest('mem $size', async () => users.find({ tags: { $size: 1 } }).toArray().map(x => ({ name: x.name })), [ { name: 'Bob' } ]);
  await runTest('mem $expr', async () => users.find({ $expr: { $gt: [{ $add: ['$age', 5] }, 30] } }).toArray().map(x => ({ name: x.name })), [ { name: 'Bob' }, { name: 'David' }, { name: 'Eve' } ]);
  await runTest('mem $like', async () => users.find({ name: { $like: '%li%' } }).toArray().map(x => ({ name: x.name })), [ { name: 'Alice' }, { name: 'Charlie' } ]);

  await runTest('mem expr $sum', async () => users.aggregate([ { $group: { _id: null, total: { $sum: '$age' } } } ]).map(x => ({ total: x.total })), [ { total: 152 } ]);
  await runTest('mem stage $bucket', async () => users.aggregate([ { $bucket: { groupBy: '$age', boundaries: [0, 30, 40, 100] } } ]).map(x => ({ _id: x._id, count: x.count })).sort((a,b)=>{
    const av = typeof a._id === 'number' ? a._id : Number.MAX_SAFE_INTEGER;
    const bv = typeof b._id === 'number' ? b._id : Number.MAX_SAFE_INTEGER;
    return av - bv;
  }), [ { _id: 0, count: 2 }, { _id: 30, count: 2 }, { _id: 40, count: 1 } ]);
  await runTest('mem stage $unwind', async () => users.aggregate([ { $unwind: '$tags' }, { $match: { name: 'Alice' } } ]).map(x => ({ tag: x.tags })), [ { tag: 'a' }, { tag: 'b' } ]);

  // ---------------------------------------------------------------
  // Parity with the SQL adapters: the same pipeline on a JSON/text date, a
  // missing field or a three-way comparison has to give the same answer here
  // that `tests/operators.runtime.spec.js` asserts against SQLite.
  // ---------------------------------------------------------------
  const logins = collection('logins', [
    { _id: 1, at: '2024-01-01' },                              // ISO text, what the SQL backends store
    { _id: 2, at: new Date('2024-01-08T00:00:00Z') },          // a real Date
    { _id: 3 },                                                // no date at all
  ]);

  // `$project` keeps `_id` in memory, so only the asserted fields are mapped out.
  const pick = (...keys) => (r) => Object.fromEntries(keys.map((k) => [k, r[k] ?? null]));

  await runTest('mem date parts read ISO text', async () => logins.aggregate([
    { $match: { _id: 1 } },
    { $project: { y: { $year: '$at' }, m: { $month: '$at' }, d: { $dayOfMonth: '$at' }, dw: { $dayOfWeek: '$at' }, w: { $week: '$at' } } },
  ]).map(pick('y', 'm', 'd', 'dw', 'w')), [{ y: 2024, m: 1, d: 1, dw: 2, w: 1 }]);

  await runTest('mem date parts read Date values', async () => logins.aggregate([
    { $match: { _id: 2 } },
    { $project: { y: { $year: '$at' }, w: { $week: '$at' } } },
  ]).map(pick('y', 'w')), [{ y: 2024, w: 2 }]);

  await runTest('mem date parts of a missing field are null', async () => logins.aggregate([
    { $match: { _id: 3 } },
    { $project: { y: { $year: '$at' }, w: { $week: '$at' }, t: { $toDate: '$at' } } },
  ]).map(pick('y', 'w', 't')), [{ y: null, w: null, t: null }]);

  // Every document lacks `tag`, so all three fall into the NULL group - the key
  // used to be `undefined` here, which the SQL backends report as NULL.
  await runTest('mem $group over a missing field keys on null', async () => logins.aggregate([
    { $group: { _id: '$tag', n: { $sum: 1 } } },
  ]).map(x => ({ _id: x._id, n: x.n })), [{ _id: null, n: 3 }]);

  await runTest('mem $sortByCount tolerates missing fields', async () => users.aggregate([
    { $sortByCount: '$middle' },
  ]).map(x => ({ _id: x._id, count: x.count })), [{ _id: null, count: 5 }]);

  const nums = collection('nums', [{ v: 1 }, { v: 5 }, { v: 9 }, { v: null }]);
  await runTest('mem expr $cmp', async () => nums.aggregate([
    { $project: { c: { $cmp: ['$v', 5] } } },
    { $sort: { c: 1 } },
  ]).map(x => ({ c: x.c })), [{ c: -1 }, { c: -1 }, { c: 0 }, { c: 1 }]);


  const tagged = collection('tagged', [
    { _id: 1, tags: ['x', 'y', 'z'] },
    { _id: 2, tags: ['x', 'y'] },
  ]);
  await runTest('mem $pull removes the matching value', async () => {
    tagged.updateOne({ _id: 1 }, { $pull: { tags: 'y' } });
    return tagged.find({ _id: 1 }).toArray().map(x => ({ tags: x.tags }));
  }, [{ tags: ['x', 'z'] }]);

  await runTest('mem $pull with a condition', async () => {
    tagged.updateOne({ _id: 2 }, { $pull: { tags: { $ne: 'y' } } });
    return tagged.find({ _id: 2 }).toArray().map(x => ({ tags: x.tags }));
  }, [{ tags: ['y'] }]);
};

main().catch(e => { console.error('FAILED', e); process.exitCode = 1; });