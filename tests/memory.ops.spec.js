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
  // Memory-only operators that previously had no test coverage at all.
  // ---------------------------------------------------------------
  const extras = collection('extras', [
    { _id: 1, name: 'A', age: 25, tags: ['x', 'y'], nums: [3, 1, 2], created: '2024-01-01T00:00:00.123Z' },
    { _id: 2, name: 'B', age: 31, tags: ['y'], nums: [5], created: '2024-06-01T00:00:00.456Z' },
  ]);

  await runTest('mem filter $where', async () =>
    extras.find({ $where: function () { return this.age > 30; } }).toArray().map(x => ({ name: x.name })),
  [ { name: 'B' } ]);

  await runTest('mem filter $type', async () =>
    extras.find({ tags: { $type: 'array' }, name: { $type: 'string' } }).toArray().map(x => ({ name: x.name })),
  [ { name: 'A' }, { name: 'B' } ]);

  await runTest('mem expr $type', async () =>
    extras.aggregate([ { $match: { name: 'A' } }, { $project: { t: { $type: '$tags' }, n: { $type: '$nums' } } } ]).map(x => ({ t: x.t, n: x.n })),
  [ { t: 'array', n: 'array' } ]);

  await runTest('mem expr $split', async () =>
    extras.aggregate([ { $match: { name: 'A' } }, { $project: { s: { $split: ['a-b-c', '-'] } } } ]).map(x => ({ s: x.s })),
  [ { s: ['a', 'b', 'c'] } ]);

  await runTest('mem expr $slice', async () =>
    extras.aggregate([ { $match: { name: 'A' } }, { $project: { a: { $slice: ['$nums', 2] }, b: { $slice: ['$nums', -2] }, c: { $slice: ['$nums', 1, 2] } } } ]).map(x => ({ a: x.a, b: x.b, c: x.c })),
  [ { a: [3, 1], b: [1, 2], c: [1, 2] } ]);

  await runTest('mem expr $arrayElemAt', async () =>
    extras.aggregate([ { $match: { name: 'A' } }, { $project: { a: { $arrayElemAt: ['$nums', 0] }, b: { $arrayElemAt: ['$nums', -1] } } } ]).map(x => ({ a: x.a, b: x.b })),
  [ { a: 3, b: 2 } ]);

  await runTest('mem expr $map', async () =>
    extras.aggregate([ { $match: { name: 'A' } }, { $project: { up: { $map: ['$tags', 't', { $upper: '$$t' }] } } } ]).map(x => ({ up: x.up })),
  [ { up: ['X', 'Y'] } ]);

  await runTest('mem expr $filter', async () =>
    extras.aggregate([ { $match: { name: 'A' } }, { $project: { f: { $filter: ['$tags', 't', { $ne: ['$$t', 'x'] }] } } } ]).map(x => ({ f: x.f })),
  [ { f: ['y'] } ]);

  await runTest('mem expr $reduce', async () =>
    extras.aggregate([ { $match: { name: 'A' } }, { $project: { r: { $reduce: ['$nums', 0, { $add: ['$$value', '$$this'] }] } } } ]).map(x => ({ r: x.r })),
  [ { r: 6 } ]);

  await runTest('mem expr $millisecond', async () =>
    extras.aggregate([ { $match: { name: 'A' } }, { $project: { ms: { $millisecond: '$created' } } } ]).map(x => ({ ms: x.ms })),
  [ { ms: 123 } ]);

  await runTest('mem update $pullAll', async () => {
    extras.updateOne({ name: 'A' }, { $pullAll: { tags: ['x'] } });
    return extras.find({ name: 'A' }).toArray().map(x => ({ tags: x.tags }));
  }, [ { tags: ['y'] } ]);

  await runTest('mem update $pop', async () => {
    extras.updateOne({ name: 'A' }, { $pop: { tags: 1 } });
    return extras.find({ name: 'A' }).toArray().map(x => ({ tags: x.tags }));
  }, [ { tags: [] } ]);

  await runTest('mem update $setOnInsert upsert', async () => {
    const res = extras.updateOne({ name: 'New' }, { $setOnInsert: { age: 9 } }, { upsert: true });
    const doc = extras.findOne({ name: 'New' });
    return [ { upserted: res.upsertedCount, age: doc.age } ];
  }, [ { upserted: 1, age: 9 } ]);

  await runTest('mem update $setOnInsert existing doc noop', async () => {
    extras.updateOne({ name: 'New' }, { $setOnInsert: { age: 99 } });
    const doc = extras.findOne({ name: 'New' });
    return [ { age: doc.age } ];
  }, [ { age: 9 } ]);

  await runTest('mem group $push/$addToSet/$first/$last', async () =>
    users.aggregate([
      { $group: {
        _id: '$city',
        names: { $push: '$name' },
        oneCity: { $addToSet: '$city' },
        firstAge: { $first: '$age' },
        lastAge: { $last: '$age' },
      } },
      { $sort: { _id: 1 } },
    ]).map(x => ({ _id: x._id, names: x.names, oneCity: x.oneCity, firstAge: x.firstAge, lastAge: x.lastAge })),
  [
    { _id: 'Berlin', names: ['Charlie', 'Eve'], oneCity: ['Berlin'], firstAge: 22, lastAge: 35 },
    { _id: 'London', names: ['Bob'], oneCity: ['London'], firstAge: 30, lastAge: 30 },
    { _id: 'Paris', names: ['Alice', 'David'], oneCity: ['Paris'], firstAge: 25, lastAge: 40 },
  ]);
};

main().catch(e => { process.exitCode = 1; });