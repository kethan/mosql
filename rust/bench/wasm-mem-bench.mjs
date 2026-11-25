// Same workload as src/bin/mem-bench.rs, run through the wasm bridge.
// Documents live inside wasm; only the query and the result cross the boundary.
import { loadMosql } from '../wasm/mosql.js';

const mosql = await loadMosql(new URL('../wasm/mosql.wasm', import.meta.url));
const mem = mosql.openMemory();

const doc = (i) => ({
    _id: i,
    name: `user${i}`,
    age: 18 + (i % 60),
    active: i % 3 === 0,
    tags: ['t1', 't2'],
    profile: { score: (i * 7) % 100, city: i % 2 === 0 ? 'London' : 'Paris' },
});

const N = 10000;
for (let i = 0; i < N; i += 500) {
    mem.insert(Array.from({ length: 500 }, (_, k) => doc(i + k)));
}

const query = { age: { $gte: 40 }, $or: [{ active: true }, { 'profile.score': { $gt: 80 } }] };
const pipeline = [
    { $match: { active: true } },
    { $group: { _id: '$profile.city', n: { $sum: 1 }, avg: { $avg: '$age' } } },
    { $sort: { n: -1 } },
];

const ROUNDS = Number(process.env.ROUNDS || 3);

// Best-of-N: the sandbox shares CPU with other work, so a single reading is
// unreliable. The minimum is the cleanest estimate of the real cost.
function bench(label, iters, fn) {
    if (global.gc) global.gc();
    for (let i = 0; i < 3; i++) fn();
    let best = Infinity;
    for (let r = 0; r < ROUNDS; r++) {
        const t = process.hrtime.bigint();
        for (let i = 0; i < iters; i++) fn();
        const ns = Number(process.hrtime.bigint() - t) / iters;
        if (ns < best) best = ns;
    }
    console.log(`${label.padEnd(12)} ${best.toFixed(1).padStart(10)} ns/op ${(1e9 / best).toFixed(0).padStart(12)} ops/s   (${(best / 1e6).toFixed(2)} ms/op)`);
}

bench('find', 200, () => {
    const out = mem.find(query);
    if (!out.length) throw new Error('empty');
});
bench('find+sort', 200, () => {
    const out = mem.find(query, { sort: { age: -1 }, limit: 10 });
    if (!out.length) throw new Error('empty');
});
bench('aggregate', 100, () => {
    const out = mem.aggregate(pipeline);
    if (!out.length) throw new Error('empty');
});
const sink = mosql.openMemory();
bench('insert1', 2000, () => sink.insert([doc(999999)]));

console.log(`\ncollection size: ${mem.find({}, { count: true })[0].count}`);
