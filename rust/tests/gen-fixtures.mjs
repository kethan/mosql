// Generates tests/fixtures.json by running a corpus through the real JavaScript
// implementation. `cargo test` replays the same corpus against the Rust port and
// asserts byte-for-byte equality.
import { writeFileSync } from 'node:fs';
import * as full from '../../src/index.js';
import lite from '../../lite/index.js';
import tiny from '../../tiny/index.js';

const cases = [];
const add = (name, variant, db, kind, table, args, chain) => {
    cases.push({ name, variant, db, kind, table, args: args ?? [], chain: chain ?? [] });
};

const filterQueries = {
    eq: { age: 30 },
    ne: { age: { $ne: 30 } },
    gt: { age: { $gt: 30 } },
    gte: { age: { $gte: 30 } },
    lt: { age: { $lt: 30 } },
    lte: { age: { $lte: 30 } },
    in: { age: { $in: [1, 2, 3] } },
    'in-empty': { age: { $in: [] } },
    nin: { age: { $nin: [1, 2] } },
    'nin-empty': { age: { $nin: [] } },
    like: { name: { $like: 'A%' } },
    nlike: { name: { $nlike: 'A%' } },
    ilike: { name: { $ilike: 'a%' } },
    nilike: { name: { $nilike: 'a%' } },
    regex: { name: { $regex: '^a' } },
    'regex-full': { name: { $regex: '^abc$' } },
    'regex-tail': { name: { $regex: 'abc$' } },
    exists: { age: { $exists: true } },
    'not-exists': { age: { $exists: false } },
    between: { age: { $between: [10, 20] } },
    mod: { age: { $mod: [2, 0] } },
    multi: { age: { $gte: 18, $lt: 65 } },
    'not-op': { age: { $not: { $gt: 30 } } },
    and: { $and: [{ a: 1 }, { b: 2 }] },
    or: { $or: [{ a: 1 }, { b: 2 }] },
    nor: { $nor: [{ a: 1 }, { b: 2 }] },
    'not-top': { $not: { a: 1 } },
    expr: { $expr: { $gt: ['$a', '$b'] } },
    'json-string': { 'profile.name': 'x' },
    'json-number': { 'profile.score': { $gt: 5 } },
    'json-boolean': { 'profile.active': true },
    'json-nested': { 'a.b.c': { $gte: 1 } },
    'json-numeric-eq': { 'a.b': 7 },
    bool: { active: true },
    null: { deleted_at: null },
    string: { status: 'active' },
};

const dbs = ['sqlite', 'pg', 'mysql'];

for (const db of dbs) {
    for (const [name, q] of Object.entries(filterQueries)) {
        add(`filter/${db}/${name}`, 'full', db, 'filter', 'users', [q]);
    }

    // find + chain
    add(`find/${db}/plain`, 'full', db, 'find', 'users', [{ age: { $gte: 18 } }], []);
    add(`find/${db}/project-include`, 'full', db, 'find', 'users', [{ age: 1 }, { name: 1, age: 1 }]);
    add(`find/${db}/project-computed`, 'full', db, 'find', 'users', [{}, { upper: { $upper: '$name' }, total: { $add: ['$a', '$b'] } }]);
    add(`find/${db}/project-array`, 'full', db, 'find', 'users', [{}, ['name', 'profile.score']]);
    add(`find/${db}/project-string`, 'full', db, 'find', 'users', [{}, 'name, age']);
    add(`find/${db}/sort`, 'full', db, 'find', 'users', [{ age: 1 }], [{ m: 'sort', a: [{ age: -1, name: 1 }] }]);
    add(`find/${db}/distinct-sort`, 'full', db, 'find', 'users', [{ age: 1 }], [{ m: 'distinct' }, { m: 'sort', a: [{ age: 1 }] }]);
    add(`find/${db}/limit-skip`, 'full', db, 'find', 'users', [{ age: 1 }], [{ m: 'sort', a: [{ age: 1 }] }, { m: 'limit', a: [10] }, { m: 'skip', a: [20] }]);
    add(`find/${db}/count`, 'full', db, 'find', 'users', [{ age: 1 }], [{ m: 'count' }]);
    add(`find/${db}/json-sort`, 'full', db, 'find', 'users', [{ age: 1 }], [{ m: 'sort', a: [{ 'profile.score': -1 }] }]);
    add(`count/${db}/plain`, 'full', db, 'countDocuments', 'users', [{ active: true }]);
    add(`distinct/${db}/plain`, 'full', db, 'distinct', 'users', ['profile.score', { active: true }]);
}

const pipelines = {
    match: [{ $match: { age: { $gte: 18 } } }],
    project: [{ $project: { name: 1, age: 1, _id: 0 } }],
    'project-computed': [{ $project: { name: 1, adult: { $gte: ['$age', 18] } } }],
    'project-exclude': [{ $project: { secret: 0 } }],
    group: [{ $group: { _id: '$status', total: { $sum: 1 }, avgAge: { $avg: '$age' }, minAge: { $min: '$age' }, maxAge: { $max: '$age' } } }],
    'group-null': [{ $group: { _id: null, n: { $sum: 1 } } }],
    'group-compound': [{ $group: { _id: { status: '$status', city: '$city' }, n: { $sum: 1 } } }],
    'group-push': [{ $group: { _id: '$status', names: { $push: '$name' } } }],
    'add-fields': [{ $addFields: { score: { $multiply: ['$age', 2] } } }],
    set: [{ $set: { score: { $add: ['$age', 1] } } }],
    sort: [{ $sort: { age: -1, name: 1 } }],
    limit: [{ $limit: 5 }],
    skip: [{ $skip: 10 }],
    'sort-limit-skip': [{ $sort: { age: -1 } }, { $skip: 10 }, { $limit: 5 }],
    count: [{ $count: 'total' }],
    sample: [{ $sample: { size: 10 } }],
    'sort-by-count': [{ $sortByCount: '$status' }],
    bucket: [{ $bucket: { groupBy: '$age', boundaries: [0, 18, 65, 200], default: 'other', output: { n: { $sum: 1 } } } }],
    'match-after-group': [{ $group: { _id: '$status', n: { $sum: 1 } } }, { $match: { n: { $gte: 2 } } }],
    'sort-after-project': [{ $project: { name: 1, adult: { $gte: ['$age', 18] } } }, { $sort: { adult: -1 } }],
    'full-pipeline': [
        { $match: { age: { $gte: 18 } } },
        { $addFields: { senior: { $gte: ['$age', 65] } } },
        { $group: { _id: '$status', n: { $sum: 1 }, avg: { $avg: '$age' } } },
        { $match: { n: { $gte: 1 } } },
        { $sort: { n: -1 } },
        { $skip: 1 },
        { $limit: 10 },
    ],
};

for (const db of dbs) {
    for (const [name, pipeline] of Object.entries(pipelines)) {
        add(`aggregate/${db}/${name}`, 'full', db, 'aggregate', 'users', [pipeline]);
    }
}

const updates = {
    set: { $set: { name: 'x', age: 1 } },
    'set-dotted': { $set: { 'profile.name': 'x', 'profile.age': 1 } },
    'set-dotted-multi': { $set: { 'profile.name': 'x', 'profile.age': 1, 'profile.active': true } },
    'set-object': { $set: { profile: { a: 1, b: [1, 2] } } },
    'set-null': { $set: { 'profile.name': null } },
    inc: { $inc: { age: 1, 'profile.score': 5 } },
    mul: { $mul: { age: 2, 'profile.score': 3 } },
    min: { $min: { age: 10, 'profile.score': 5 } },
    max: { $max: { age: 90, 'profile.score': 5 } },
    unset: { $unset: { age: '', 'profile.name': '' } },
    'current-date': { $currentDate: { updated_at: true, 'profile.touched': true } },
    rename: { $rename: { name: 'full_name' } },
    mixed: { $set: { name: 'x' }, $inc: { age: 1 }, $unset: { tmp: '' } },
    'field-assign': { name: 'x', 'profile.name': 'y' },
};

for (const db of dbs) {
    for (const [name, update] of Object.entries(updates)) {
        add(`update/${db}/${name}`, 'full', db, 'updateMany', 'users', [{ active: true }, update]);
    }
}

const inserts = {
    simple: [{ name: 'a', age: 1 }, { name: 'b', age: 2 }],
    missing: [{ name: 'a', age: 1 }, { name: 'b' }, { age: 3 }],
    nested: [{ profile: { a: 1, b: [1, 2] }, tags: ['x', 'y'] }],
    booleans: [{ a: true, b: false }],
};

for (const db of dbs) {
    for (const [name, docs] of Object.entries(inserts)) {
        add(`insert/${db}/${name}`, 'full', db, 'insertMany', 'users', [docs]);
    }
    add(`delete/${db}/filtered`, 'full', db, 'deleteMany', 'users', [{ active: false }]);
    add(`delete/${db}/all`, 'full', db, 'deleteMany', 'users', [{}, { allowDeleteAll: true }]);
}

// errors — message text is part of the contract
add('error/filter-unknown-op', 'full', 'sqlite', 'filter', 'users', [{ age: { $nope: 1 } }]);
add('error/filter-unknown-top-op', 'full', 'sqlite', 'filter', 'users', [{ $nope: 1 }]);
add('error/expr-unknown-op', 'full', 'sqlite', 'expression', 'users', [{ $nope: 1 }]);
add('error/agg-unknown-stage', 'full', 'sqlite', 'aggregate', 'users', [[{ $nope: {} }]]);
add('error/invalid-column', 'full', 'sqlite', 'filter', 'users', [{ 'a b': 1 }]);
add('error/invalid-table', 'full', 'sqlite', 'find', 'a b', [{}]);
add('error/limit-negative', 'full', 'sqlite', 'find', 'users', [{ age: 1 }], [{ m: 'limit', a: [-1] }]);
add('error/delete-all', 'full', 'sqlite', 'deleteMany', 'users', [{}]);
add('error/insert-empty', 'full', 'sqlite', 'insertMany', 'users', [[]]);
add('error/insert-no-fields', 'full', 'sqlite', 'insertMany', 'users', [[{}]]);
add('error/cond-arity', 'full', 'sqlite', 'expression', 'users', [{ $cond: [1] }]);
add('error/in-arity', 'full', 'sqlite', 'expression', 'users', [{ $in: [1, 2] }]);
add('error/rename-json', 'full', 'sqlite', 'updateMany', 'users', [{}, { $rename: { 'a.b': 'c' } }]);

// lite / tiny restrictions
add('lite/filter-ok', 'lite', 'sqlite', 'filter', 'users', [{ age: { $gte: 1 } }]);
add('lite/expr-ok', 'lite', 'sqlite', 'expression', 'users', [{ $add: ['$a', 1] }]);
add('lite/expr-blocked', 'lite', 'sqlite', 'expression', 'users', [{ $switch: { branches: [] } }]);
add('lite/stage-blocked', 'lite', 'sqlite', 'aggregate', 'users', [[{ $addFields: { a: 1 } }]]);
add('tiny/filter-ok', 'tiny', 'sqlite', 'filter', 'users', [{ age: { $gte: 1 } }]);
add('tiny/filter-blocked', 'tiny', 'sqlite', 'filter', 'users', [{ age: { $regex: 'a' } }]);
add('tiny/update-blocked', 'tiny', 'sqlite', 'updateMany', 'users', [{}, { $inc: { age: 1 } }]);
add('tiny/expr-blocked', 'tiny', 'sqlite', 'expression', 'users', [{ $abs: ['$a'] }]);
add('tiny/stage-blocked', 'tiny', 'sqlite', 'aggregate', 'users', [[{ $sample: { size: 1 } }]]);

const variants = { full, lite, tiny };
const out = [];

for (const c of cases) {
    const api = variants[c.variant];
    const db = c.db;
    const [table, ...rest] = [c.table, ...c.args];
    let result;
    try {
        let sql;
        switch (c.kind) {
            case 'find': {
                let q = api.collection(table, db).find(rest[0], rest[1]);
                for (const step of c.chain) {
                    if (step.m === 'sort') q = q.sort(step.a[0]);
                    else if (step.m === 'limit') q = q.limit(step.a[0]);
                    else if (step.m === 'skip') q = q.skip(step.a[0]);
                    else if (step.m === 'distinct') q = q.distinct();
                    else if (step.m === 'count') q = q.count();
                }
                sql = q.toSQL();
                break;
            }
            case 'filter':
                sql = api.filter(rest[0], db);
                break;
            case 'expression':
                sql = api.expression(rest[0], db);
                break;
            case 'aggregate':
                sql = api.aggregate(rest[0])(table, db);
                break;
            case 'insertMany':
                sql = api.insertMany(table, rest[0], db, rest[1] || {});
                break;
            case 'updateMany':
                sql = api.updateMany(table, rest[0], rest[1], db, rest[2] || {});
                break;
            case 'deleteMany':
                sql = api.deleteMany(table, rest[0], db, rest[1] || {});
                break;
            case 'countDocuments':
                sql = api.collection(table, db).countDocuments(rest[0]);
                break;
            case 'distinct':
                sql = api.collection(table, db).distinct(rest[0], rest[1]);
                break;
            default:
                throw new Error(`unknown kind ${c.kind}`);
        }
        result = { ...c, sql };
    } catch (e) {
        result = { ...c, error: String(e?.message ?? e) };
    }
    out.push(result);
}

writeFileSync(new URL('./fixtures.json', import.meta.url), JSON.stringify(out, null, 0));
console.log(`wrote ${out.length} fixtures`);
