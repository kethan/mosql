// Same workload as src/bin/mem-bench.rs, run through the JavaScript engine.
import { createMemoryDB } from '../../src/adapter/memory/memory.js';

const doc = (i) => ({
    _id: i,
    name: `user${i}`,
    age: 18 + (i % 60),
    active: i % 3 === 0,
    tags: ['t1', 't2'],
    profile: { score: (i * 7) % 100, city: i % 2 === 0 ? 'London' : 'Paris' },
});

const N = 10000;
const db = createMemoryDB();
const coll = db.collection('users', Array.from({ length: N }, (_, i) => doc(i)));

const query = { age: { $gte: 40 }, $or: [{ active: true }, { 'profile.score': { $gt: 80 } }] };
const pipeline = [
    { $match: { active: true } },
    { $group: { _id: '$profile.city', n: { $sum: 1 }, avg: { $avg: '$age' } } },
    { $sort: { n: -1 } },
];
const update = { $set: { verified: true }, $inc: { 'profile.score': 1 } };

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
    const out = coll.find(query).toArray();
    if (!out.length) throw new Error('empty');
});
bench('find+sort', 200, () => {
    const out = coll.find(query).sort({ age: -1 }).limit(10).toArray();
    if (!out.length) throw new Error('empty');
});
bench('aggregate', 100, () => {
    const out = coll.aggregate(pipeline);
    if (!out.length) throw new Error('empty');
});
bench('updateMany', 100, () => {
    const r = coll.updateMany(query, update);
    if (!r.matchedCount) throw new Error('no match');
});
const sink = db.collection('inserts', []);
bench('insert1', 2000, () => sink.insertOne(doc(999999)));

console.log(`\ncollection size: ${coll.size()}`);

// The check mode prints what the engine actually computed, so the Rust
// and the Go baselines can be compared against it. A timing comparison
// only means something if the three are doing the same work.
if (process.env.CHECK) {
    const matched = coll.find(query).toArray().length;
    const groups = coll.aggregate(pipeline);
    const first = groups[0] || {};
    const updated = coll.updateMany(query, update).matchedCount;
    console.log(
        `CHECK find=${matched} groups=${groups.length} city=${first._id} n=${first.n} avg=${first.avg} updated=${updated}`
    );
}
