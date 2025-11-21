import lite from '../lite/index.js';

console.log(lite.filter({ 'profile.country': 'FR' }, 'mysql'));
console.log(lite.aggregate([{ $sortByCount: '$city' }])('users', 'sqlite'));
