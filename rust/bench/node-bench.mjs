// Same workload as src/bin/bench.rs, run through the JavaScript implementation.
import { collection } from '../../src/index.js';

const users = collection('users', 'pg');
const query = {
    age: { $gte: 18 },
    status: 'active',
    $or: [{ role: 'admin' }, { 'profile.score': { $gt: 90 } }],
};
const update = { $set: { status: 'done' } };
const pipeline = [
    { $match: query },
    { $group: { _id: '$status', total: { $sum: 1 } } },
    { $sort: { total: -1 } },
];
const docs = Array.from({ length: 50 }, (_, i) => ({ name: `u${i}`, age: i }));

const N = Number(process.env.N || 20000);

const ROUNDS = Number(process.env.ROUNDS || 3);

// Best-of-N: the sandbox shares CPU with other work, so a single reading is
// unreliable. The minimum is the cleanest estimate of the real cost.
function bench(label, fn) {
    if (global.gc) global.gc();
    for (let i = 0; i < 1000; i++) fn();
    let best = Infinity;
    for (let r = 0; r < ROUNDS; r++) {
        const t = process.hrtime.bigint();
        for (let i = 0; i < N; i++) fn();
        const ns = Number(process.hrtime.bigint() - t) / N;
        if (ns < best) best = ns;
    }
    console.log(`${label.padEnd(10)} ${best.toFixed(1).padStart(8)} ns/op  ${(1e9 / best).toFixed(0).padStart(10)} ops/s`);
}

bench('find', () => users.find(query).toSQL());
bench('update', () => users.updateMany(query, update));
bench('aggregate', () => users.aggregate(pipeline));
bench('insert50', () => users.insertMany(docs));
