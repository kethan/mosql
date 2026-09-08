import { collection as memCollection } from '../src/adapter/memory/memory.js';
import { runTest } from './common.js';

const users = memCollection('users', [
  { name: 'Alice', age: 25, city: 'Paris', active: true, profile: { country: 'France', score: 85 } },
  { name: 'Bob', age: 30, city: 'London', active: true, profile: { country: 'UK', score: 90 } },
  { name: 'Charlie', age: 22, city: 'Berlin', active: false, profile: { country: 'Germany', score: 60 } },
  { name: 'David', age: 40, city: 'Paris', active: true, profile: { country: 'France', score: 95 } },
  { name: 'Eve', age: 35, city: 'Berlin', active: true, profile: { country: 'Germany', score: 88 } },
]);

const main = async () => {
  await runTest('Filter active Paris', async () => {
    const res = users.find({ city: 'Paris', active: true }).toArray();
    return res;
  }, [
    { name: 'Alice', age: 25, city: 'Paris', active: true, profile: { country: 'France', score: 85 } },
    { name: 'David', age: 40, city: 'Paris', active: true, profile: { country: 'France', score: 95 } },
  ]);

  await runTest('Filter JSON country', async () => {
    const res = users.find({ 'profile.country': 'France' }).toArray();
    return res.map(x => ({ name: x.name }));
  }, [
    { name: 'Alice' },
    { name: 'David' },
  ]);

  await runTest('Aggregate avg age by city', async () => {
    const res = users.aggregate([
      { $group: { _id: '$city', avgAge: { $avg: '$age' } } },
      { $sort: { avgAge: 1 } },
    ]);
    return res.map(x => ({ _id: x._id, avgAge: x.avgAge }));
  }, [
    { _id: 'Berlin', avgAge: 28.5 },
    { _id: 'London', avgAge: 30 },
    { _id: 'Paris', avgAge: 32.5 },
  ]);

  await runTest('Count adults', async () => {
    const res = users.aggregate([
      { $match: { age: { $gte: 25 } } },
      { $count: 'totalAdults' },
    ]);
    return res;
  }, [
    { totalAdults: 4 },
  ]);

  await runTest('Update $inc age', async () => {
    users.updateOne({ name: 'Alice' }, { $inc: { age: 1 } });
    const r = users.find({ name: 'Alice' }).limit(1).toArray();
    return r.map(x => ({ age: x.age }));
  }, [
    { age: 26 },
  ]);

  await runTest('Update nested $set', async () => {
    users.updateOne({ name: 'Alice' }, { $set: { 'profile.score': 100 } });
    const r = users.find({ name: 'Alice' }).limit(1).toArray();
    return r.map(x => ({ score: x.profile.score }));
  }, [
    { score: 100 },
  ]);

  await runTest('Project and addFields', async () => {
    const r = users.aggregate([
      { $project: { name: 1, age: 1 } },
      { $addFields: { isAdult: { $gte: ['$age', 18] } } },
      { $match: { isAdult: true } },
      { $sort: { age: 1 } },
      { $limit: 2 },
    ]);
    return r.map(x => ({ name: x.name, isAdult: x.isAdult }));
  }, [
    { name: 'Charlie', isAdult: true },
    { name: 'Alice', isAdult: true },
  ]);

  users.updateMany({}, { $set: { tags: [] } });
  users.updateOne({ name: 'Alice' }, { $set: { tags: ['a', 'b'] } });
  users.updateOne({ name: 'Bob' }, { $set: { tags: ['b'] } });

  await runTest('Unwind array', async () => {
    const r = users.aggregate([
      { $unwind: { path: '$tags', preserveNullAndEmptyArrays: false } },
      { $project: { name: 1, tags: 1 } },
      { $sort: { name: 1 } },
    ]);
    return r.filter(x => x.name === 'Alice').map(x => ({ tag: x.tags }));
  }, [
    { tag: 'a' },
    { tag: 'b' },
  ]);
};

main().catch(e => { console.error('FAILED', e); process.exitCode = 1; });