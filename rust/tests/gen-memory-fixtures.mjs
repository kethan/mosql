// Generates tests/memory-fixtures.json from the JS memory engine.
import { writeFileSync } from 'node:fs';
import {
    filter, expression, project, aggregate, updateOps,
    deepEquals, getPath, setPath,
} from '../../src/adapter/memory/memory.js';
import { createMemoryDB } from '../../src/adapter/memory/memory.js';

const docs = [
    { _id: 1, name: 'Ada', age: 36, active: true, tags: ['x', 'y'], profile: { score: 95, city: 'London' }, joined: '2024-01-02T03:04:05Z' },
    { _id: 2, name: 'Bob', age: 17, active: false, tags: ['y'], profile: { score: 55, city: 'Paris' }, joined: '2023-06-07T08:09:10Z' },
    { _id: 3, name: 'Cyd', age: 45, active: true, tags: [], profile: { score: 75, city: 'London' }, joined: '2022-12-31T23:59:59Z' },
    { _id: 4, name: 'Dee', age: 28, active: null, profile: { score: 85, city: 'Berlin' }, joined: '2021-03-04T05:06:07Z' },
];

const cases = [];
const add = (name, kind, payload) => cases.push({ name, kind, ...payload });

// ---- filter -------------------------------------------------------------
const filters = {
    eq: { name: 'Ada' },
    ne: { name: { $ne: 'Ada' } },
    gt: { age: { $gt: 30 } },
    gte: { age: { $gte: 36 } },
    lt: { age: { $lt: 30 } },
    lte: { age: { $lte: 28 } },
    'in-num': { age: { $in: [17, 45] } },
    'in-str': { name: { $in: ['Ada', 'Bob'] } },
    nin: { name: { $nin: ['Ada'] } },
    and: { $and: [{ age: { $gte: 18 } }, { active: true }] },
    or: { $or: [{ age: { $lt: 18 } }, { age: { $gt: 40 } }] },
    nor: { $nor: [{ age: { $lt: 18 } }] },
    not: { age: { $not: { $gt: 30 } } },
    'not-top': { $not: { age: { $gt: 30 } } },
    mod: { age: { $mod: [2, 0] } },
    between: { age: { $between: [20, 40] } },
    regex: { name: { $regex: '^A' } },
    'regex-mid': { name: { $regex: 'o' } },
    like: { name: { $like: 'A%' } },
    ilike: { name: { $ilike: 'a%' } },
    nlike: { name: { $nlike: 'A%' } },
    'exists-true': { age: { $exists: true } },
    'exists-false': { missing: { $exists: false } },
    type: { age: { $type: 'number' } },
    'type-array': { tags: { $type: 'array' } },
    'all': { tags: { $all: ['x', 'y'] } },
    size: { tags: { $size: 2 } },
    elemMatch: { tags: { $elemMatch: { $eq: 'x' } } },
    'json-path': { 'profile.score': { $gt: 80 } },
    'json-path-eq': { 'profile.city': 'London' },
    expr: { $expr: { $gt: ['$age', 30] } },
    'expr-eq': { $expr: { $eq: ['$profile.score', 95] } },
    'expr-and': { $expr: { $and: [{ $gte: ['$age', 18] }, { $eq: ['$active', true] }] } },
    'nested-obj': { profile: { score: 95, city: 'London' } },
    bool: { active: true },
    null: { active: null },
    'in-field': { name: { $in: ['Ada', 'Zed'] } },
};

for (const [name, query] of Object.entries(filters)) {
    add(`filter/${name}`, 'filter', { docs, query, result: docs.filter(filter(query)) });
}

// ---- expression ---------------------------------------------------------
const doc = docs[0];
const exprs = {
    add: { $add: ['$age', 1, 2] },
    subtract: { $subtract: ['$age', 6] },
    multiply: { $multiply: ['$age', 2, 1] },
    divide: { $divide: [10, 4] },
    'divide-zero': { $divide: [10, 0] },
    mod: { $mod: [10, 3] },
    abs: { $abs: -5 },
    ceil: { $ceil: 1.2 },
    floor: { $floor: 1.8 },
    round: { $round: [1.2345, 2] },
    'round-0': { $round: [1.5] },
    pow: { $pow: [2, 3] },
    sqrt: { $sqrt: 16 },
    concat: { $concat: ['$name', '!', 1] },
    'concat-null': { $concat: ['$missing', 'x'] },
    upper: { $upper: '$name' },
    lower: { $lower: '$name' },
    substr: { $substr: ['$name', 0, 2] },
    'substr-neg': { $substr: ['$name', -1, 2] },
    strLen: { $strLen: '$name' },
    trim: { $trim: '  x  ' },
    ltrim: { $ltrim: '  x  ' },
    rtrim: { $rtrim: '  x  ' },
    replace: { $replace: ['$name', 'a', 'A'] },
    split: { $split: ['$name', 'd'] },
    min: { $min: [1, 2, 3] },
    max: { $max: [1, 2, 3] },
    avg: { $avg: [1, 2, 3] },
    sum: { $sum: [1, 2, 3] },
    'sum-1': { $sum: 1 },
    eq: { $eq: ['$age', 36] },
    ne: { $ne: ['$age', 36] },
    gt: { $gt: ['$age', 30] },
    gte: { $gte: ['$age', 36] },
    lt: { $lt: ['$age', 30] },
    lte: { $lte: ['$age', 36] },
    cmp: { $cmp: ['$age', 30] },
    in: { $in: ['x', '$tags'] },
    nin: { $nin: ['q', '$tags'] },
    size: { $size: '$tags' },
    'arrayElemAt': { $arrayElemAt: ['$tags', 0] },
    'arrayElemAt-neg': { $arrayElemAt: ['$tags', -1] },
    slice: { $slice: ['$tags', 1] },
    'slice-3': { $slice: ['$tags', 0, 1] },
    map: { $map: ['$tags', 't', { $upper: '$$t' }] },
    filter: { $filter: ['$tags', 't', { $eq: ['$t', 'x'] }] },
    reduce: { $reduce: [[1, 2, 3], 0, { $add: ['$$value', '$$this'] }] },
    and: { $and: [true, 1] },
    or: { $or: [false, 0] },
    not: { $not: [true] },
    cond: { $cond: [{ $gt: ['$age', 18] }, 'adult', 'minor'] },
    ifNull: { $ifNull: ['$missing', 'd'] },
    switch: { $switch: { branches: [{ case: { $gt: ['$age', 40] }, then: 'old' }], default: 'young' } },
    type: { $type: '$age' },
    'year': { $year: '$joined' },
    'month': { $month: '$joined' },
    'dayOfMonth': { $dayOfMonth: '$joined' },
    'hour': { $hour: '$joined' },
    'minute': { $minute: '$joined' },
    'second': { $second: '$joined' },
    'millisecond': { $millisecond: '$joined' },
    'dayOfWeek': { $dayOfWeek: '$joined' },
    'week': { $week: '$joined' },
    toString: { $toString: 1 },
    toInt: { $toInt: '42' },
    toDouble: { $toDouble: '3.5' },
    toBool: { $toBool: 0 },
    toDate: { $toDate: '$joined' },
    literal: { $literal: 5 },
    'field': '$name',
    'field-path': '$profile.city',
    'literal-str': 'hello',
    'literal-num': 7,
    'null': null,
    'string-dot': 'a.b',
};

for (const [name, expr] of Object.entries(exprs)) {
    const out = expression(expr)(doc);
    add(`expr/${name}`, 'expr', { doc, expr, result: out === undefined ? { $undefined: true } : out });
}

// ---- update -------------------------------------------------------------
const updates = {
    set: { $set: { name: 'Ada L', 'profile.score': 100 } },
    unset: { $unset: { age: '', 'profile.city': '' } },
    inc: { $inc: { age: 1, 'profile.score': 5 } },
    mul: { $mul: { age: 2 } },
    min: { $min: { age: 10, 'profile.score': 50 } },
    max: { $max: { age: 90, 'profile.score': 99 } },
    push: { $push: { tags: 'z' } },
    'push-each': { $push: { tags: { $each: ['z', 'w'] } } },
    'push-sort': { $push: { tags: { $each: ['a', 'c'], $sort: 1 } } },
    'push-slice': { $push: { tags: { $each: ['z'], $slice: 2 } } },
    pull: { $pull: { tags: 'x' } },
    pullAll: { $pullAll: { tags: ['x'] } },
    addToSet: { $addToSet: { tags: 'x' } },
    'addToSet-each': { $addToSet: { tags: { $each: ['x', 'z'] } } },
    'pop-last': { $pop: { tags: 1 } },
    'pop-first': { $pop: { tags: -1 } },
    rename: { $rename: { name: 'full_name' } },
    'set-on-insert': { $setOnInsert: { created: true } },
    'field-assign': { name: 'Ada2', 'profile.score': 99 },
};

for (const [name, update] of Object.entries(updates)) {
    const copy = JSON.parse(JSON.stringify(docs[0]));
    for (const [op, fields] of Object.entries(update)) {
        if (op.startsWith('$')) updateOps[op](fields, copy, false, filter);
        else setPath(copy, op, fields);
    }
    add(`update/${name}`, 'update', { doc: docs[0], update, result: copy });
}

// ---- aggregate ----------------------------------------------------------
const pipelines = {
    match: [{ $match: { active: true } }],
    project: [{ $project: { name: 1, age: 1, _id: 0 } }],
    'project-computed': [{ $project: { name: 1, adult: { $gte: ['$age', 18] } } }],
    'project-exclude': [{ $project: { profile: 0, tags: 0 } }],
    'add-fields': [{ $addFields: { senior: { $gte: ['$age', 40] }, 'profile.flag': 'y' } }],
    set: [{ $set: { decade: { $divide: ['$age', 10] } } }],
    unset: [{ $unset: ['profile', 'tags'] }],
    group: [{ $group: { _id: '$active', n: { $sum: 1 }, avgAge: { $avg: '$age' }, minAge: { $min: '$age' }, maxAge: { $max: '$age' }, names: { $push: '$name' }, first: { $first: '$name' }, last: { $last: '$name' }, cities: { $addToSet: '$profile.city' } } }],
    'group-null': [{ $group: { _id: null, total: { $sum: '$age' } } }],
    sort: [{ $sort: { age: -1, name: 1 } }],
    'sort-nulls': [{ $sort: { active: 1 } }],
    limit: [{ $limit: 2 }],
    skip: [{ $skip: 2 }],
    count: [{ $count: 'total' }],
    unwind: [{ $unwind: '$tags' }],
    'unwind-preserve': [{ $unwind: { path: '$tags', preserveNullAndEmptyArrays: true } }],
    'unwind-index': [{ $unwind: { path: '$tags', includeArrayIndex: 'i' } }],
    bucket: [{ $bucket: { groupBy: '$age', boundaries: [0, 18, 40, 100], default: 'other', output: { n: { $sum: 1 }, avg: { $avg: '$age' } } } }],
    'sort-by-count': [{ $sortByCount: '$profile.city' }],
    'sort-by-count-expr': [{ $sortByCount: { $gte: ['$age', 18] } }],
    pipeline: [
        { $match: { age: { $gte: 18 } } },
        { $addFields: { senior: { $gte: ['$age', 40] } } },
        { $group: { _id: '$active', n: { $sum: 1 }, avg: { $avg: '$age' } } },
        { $sort: { n: -1 } },
        { $skip: 0 },
        { $limit: 5 },
    ],
};

for (const [name, pipeline] of Object.entries(pipelines)) {
    add(`aggregate/${name}`, 'aggregate', { docs, pipeline, result: aggregate(pipeline)(docs) });
}

// ---- collection ---------------------------------------------------------
const db = createMemoryDB();
const coll = db.collection('users', []);

const steps = [];
const step = (op, args, options) => steps.push({ op, args, options });

step('insertOne', [{ name: 'Ada', age: 36, active: true, tags: ['x', 'y'], profile: { score: 95, city: 'London' } }]);
step('insertMany', [[
    { name: 'Bob', age: 17, active: false, tags: ['y'], profile: { score: 55, city: 'Paris' } },
    { name: 'Cyd', age: 45, active: true, tags: [], profile: { score: 75, city: 'London' } },
]]);
step('findOne', [{ name: 'Ada' }]);
step('findById', [1]);
step('find', [{}, { sort: { age: -1 } }]);
step('find', [{}, { sort: { age: 1 }, skip: 1, limit: 1 }]);
step('countDocuments', [{ active: true }]);
step('estimatedDocumentCount', []);
step('distinct', ['profile.city', {}]);
step('aggregate', [[{ $group: { _id: '$active', n: { $sum: 1 } } }]]);
step('updateOne', [{ name: 'Ada' }, { $set: { age: 37 }, $inc: { 'profile.score': 5 } }, false]);
step('updateMany', [{ active: true }, { $set: { verified: true } }]);
step('updateOne', [{ name: 'Zed' }, { $set: { age: 1 } }, { upsert: true }]);
step('replaceOne', [{ name: 'Zed' }, { name: 'Zed', age: 2 }, { upsert: false }]);
step('deleteOne', [{ name: 'Zed' }]);
step('deleteMany', [{ active: false }]);
step('size', []);
step('getAll', []);

const results = steps.map((st) => {
    const run = {
        insertOne: () => coll.insertOne(...st.args),
        insertMany: () => coll.insertMany(...st.args),
        findOne: () => coll.findOne(...st.args),
        findById: () => coll.findById(...st.args),
        find: () => {
            const [q, opts] = st.args;
            let fq = coll.find(q, null, opts);
            return fq.toArray().map(d => d.name);
        },
        countDocuments: () => coll.countDocuments(...st.args),
        estimatedDocumentCount: () => coll.estimatedDocumentCount(),
        distinct: () => coll.distinct(...st.args),
        aggregate: () => coll.aggregate(...st.args),
        updateOne: () => coll.updateOne(...st.args),
        updateMany: () => coll.updateMany(...st.args),
        replaceOne: () => coll.replaceOne(...st.args),
        deleteOne: () => coll.deleteOne(...st.args),
        deleteMany: () => coll.deleteMany(...st.args),
        size: () => coll.size(),
        getAll: () => coll.getAll().map(d => d.name),
    }[st.op];
    try { return { op: st.op, args: st.args, result: JSON.parse(JSON.stringify(run())) }; }
    catch (e) { return { op: st.op, args: st.args, error: String(e.message) }; }
});

add('collection/script', 'collection', { results });

writeFileSync(new URL('./memory-fixtures.json', import.meta.url), JSON.stringify(cases));
console.log(`wrote ${cases.length} memory fixtures`);
