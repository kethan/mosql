import { createSchemalessClient } from '../src/client.js';

const client = await createSchemalessClient('sqlite', { filename: ':memory:' });
const users = client.db('app').collection('users');

await users.insertMany([
  { name: 'Alice', age: 25, city: 'Paris', active: true, profile: { country: 'FR', score: 85 } },
  { name: 'Bob', age: 30, city: 'London', active: true, profile: { country: 'UK', score: 90 } },
]);

const rows = await (await users.find({ city: 'Paris' })).toArray();
console.log(rows);

await users.updateOne({ name: 'Alice' }, { $set: { 'profile.score': 90 } });
console.log(await users.aggregate([{ $group: { _id: '$city', avgAge: { $avg: '$age' } } }]));

await client.close();
