import lite from '../lite/index.js';

console.log(lite.filter({ 'profile.country': 'FR' }, 'mysql'));
// lite ships the basic stages only ($match/$project/$group/$sort/$limit/$skip/$count)
console.log(lite.aggregate([{ $group: { _id: '$city', total: { $sum: 1 } } }, { $sort: { total: -1 } }])('users', 'sqlite'));
