import { collection } from '../src/memory.js';

const users = collection('users', [
  { name: 'Alice', age: 25, city: 'Paris', active: true, profile: { country: 'FR', score: 85 } },
  { name: 'Bob', age: 30, city: 'London', active: true, profile: { country: 'UK', score: 90 } },
  { name: 'Charlie', age: 22, city: 'Berlin', active: false, profile: { country: 'DE', score: 60 } },
]);

console.log(users.find({ city: 'Paris', active: true }).toArray());
console.log(users.aggregate([{ $group: { _id: '$city', avgAge: { $avg: '$age' } } }, { $sort: { avgAge: 1 } }]).map(x => ({ _id: x._id, avgAge: x.avgAge })));
users.updateOne({ name: 'Alice' }, { $inc: { 'profile.score': 5 } });
console.log(users.find({ 'profile.score': 90 }).toArray().map(x => ({ name: x.name })));