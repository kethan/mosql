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
};

main().catch(e => { process.exitCode = 1; });