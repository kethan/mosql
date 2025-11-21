import tiny from '../tiny/index.js';
import lite from '../lite/index.js';
import { runStringTest } from './common.js';

for (const db of ['sqlite','pg','mysql']) {
  runStringTest(`tiny filter ${db} $eq`, () => tiny.filter({ age: { $eq: 25 } }, db), 'age = 25');
  runStringTest(`tiny aggregate ${db} $group`, () => tiny.aggregate([{ $group: { _id: '$city', total: { $sum: '$age' } } }])('users', db), /GROUP BY/);
  runStringTest(`lite filter ${db} $eq`, () => lite.filter({ age: { $eq: 25 } }, db), 'age = 25');
  runStringTest(`lite aggregate ${db} $group`, () => lite.aggregate([{ $group: { _id: '$city', total: { $sum: '$age' } } }])('users', db), /GROUP BY/);
}