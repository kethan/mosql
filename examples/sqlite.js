import { createSQLiteSchemaless } from '../src/adapter/sqlite/adapter.js';

const { adapter } = createSQLiteSchemaless(':memory:');
const users = adapter.collection('users');

(async () => {
  await users.insertMany([
    { name: 'Alice', age: 25, city: 'Paris', active: true, profile: { country: 'FR', score: 85 } },
    { name: 'Bob', age: 30, city: 'London', active: true, profile: { country: 'UK', score: 90 } },
  ]);
  const rows = await users.find({ city: 'Paris' });
  console.log(await rows.toArray());
  await users.updateOne({ name: 'Alice' }, { $set: { 'profile.score': 90 } });
  const agg = await users.aggregate([{ $group: { _id: '$city', avgAge: { $avg: '$age' } } }]);
  console.log(agg);
})();