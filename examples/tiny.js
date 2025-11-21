import tiny from '../tiny/index.js';

console.log(tiny.filter({ age: { $eq: 25 } }, 'sqlite'));
console.log(tiny.aggregate([{ $group: { _id: '$city', total: { $sum: '$age' } } }])('users', 'pg'));