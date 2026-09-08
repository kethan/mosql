import assert from 'assert';
import fs from 'fs';
import path from 'path';
import { createRequire } from 'module';
import { fileURLToPath, pathToFileURL } from 'url';

// Guards the public export surface of every entry point:
//   umosql           -> query builder (index.js / src/index.js)
//   umosql/schemaless-> createSchemalessAdapter / createSQLAdapter / createMemorySchemaless
//   umosql/memory    -> in-memory engine + adapter
//   umosql/client    -> createSchemalessClient (memory/sqlite/pg/mysql/mongodb/sql)
// The dist checks are skipped when the bundles have not been built yet.

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(import.meta.url);
const pass = (title) => console.log('PASS ' + title);
const assertAll = (mod, names, title) => {
    for (const name of names) assert.ok(mod[name] != null, `${title}: missing export "${name}"`);
    pass(`${title}: ${names.length} exports`);
};

// ---------------------------------------------------------------- source API
const schemaless = await import('../src/schemaless.js');
assertAll(schemaless, ['createSchemalessAdapter', 'createSQLAdapter', 'createMemorySchemaless'], 'schemaless exports');

const memory = await import('../src/memory.js');
assertAll(memory, [
    'collection', 'filter', 'expression', 'aggregate', 'project', 'FindQuery', 'extend', 'db',
    'Database', 'Collection', 'createMemoryDB', 'createMemorySchemaless',
    'filterOps', 'exprOps', 'updateOps', 'stageOps',
    'deepEquals', 'getPath', 'setPath', 'deletePath', 'clone',
], 'memory exports');
assert.equal(typeof memory.default, 'function', 'memory default export is the collection factory');

const client = await import('../src/client.js');
assertAll(client, [
    'createSchemalessClient', 'CLIENT_TYPES', 'loadDriver',
    'createSchemalessAdapter', 'createSQLAdapter', 'createMemorySchemaless', 'createMongoSchemaless',
], 'client exports');
assert.equal(typeof client.default, 'function', 'client default export is createSchemalessClient');
assert.deepEqual(client.CLIENT_TYPES, ['memory', 'sqlite', 'pg', 'mysql', 'mongodb', 'sql']);

const core = await import('../index.js');
assertAll(core, [
    'createQueryBuilder', 'filterOps', 'exprOps', 'updateOps', 'stageHandlers',
    'filter', 'expression', 'aggregate', 'insertMany', 'updateMany', 'deleteMany',
    'collection', 'FindQuery', 'extend', 'db', 'validate', 'escape', 'jsonPath',
], 'main entry exports (index.d.ts parity)');

const tiny = await import('../tiny/index.js');
assertAll(tiny, ['collection', 'aggregate', 'filter', 'expression', 'extend', 'db'], 'tiny exports (tiny/index.d.ts parity)');

// ------------------------------------------------- memory client (functional)
const memClient = await client.createSchemalessClient('memory');
assert.equal(memClient.type, 'memory');
assert.ok(memClient.adapter && memClient.raw, 'memory client exposes adapter + raw database');

const users = memClient.db('app', { idStrategy: 'mongo' }).collection('users');
const inserted = await users.insertOne({ name: 'Alice', age: 25, profile: { score: 85 } });
assert.ok(typeof inserted.insertedId === 'string' && inserted.insertedId.length === 24, 'mongo id strategy');
await users.insertMany([{ name: 'Bob', age: 30 }, { name: 'Charlie', age: 17 }]);

const adults = await (await users.find({ age: { $gte: 18 } })).sort({ age: -1 }).toArray();
assert.deepEqual(adults.map((d) => d.name), ['Bob', 'Alice'], 'find + sort through the client');
assert.equal(await users.countDocuments({}), 3);
assert.deepEqual((await users.distinct('name')).sort(), ['Alice', 'Bob', 'Charlie']);

const updated = await users.updateOne({ name: 'Alice' }, { $inc: { 'profile.score': 5 } });
assert.equal(updated.modifiedCount, 1);
assert.equal((await users.findOne({ name: 'Alice' })).profile.score, 90, 'JSON path update');

const grouped = await users.aggregate([{ $group: { _id: '$age', total: { $sum: 1 } } }, { $sort: { _id: 1 } }]);
assert.equal(grouped.length, 3, 'aggregate through the client');

// `client.db(name)` must not share documents across database names.
assert.equal(await memClient.db('other').collection('users').countDocuments({}), 0, 'memory db names are isolated');
assert.equal(await memClient.db('app').collection('users').countDocuments({}), 3, 'same db name reuses its store');

// custom id generator (explicit strategy, and inferred from idGenerator alone)
const customIds = memClient.db('app', { idStrategy: 'custom', idGenerator: () => 'fixed-1' }).collection('events');
assert.equal((await customIds.insertOne({ type: 'click' })).insertedId, 'fixed-1', 'custom idGenerator');
// NOTE: a new collection name is required - memory caches collections per db, so
// options are applied when the collection is first created.
const inferredIds = memClient.db('app', { idGenerator: () => 'fixed-2' }).collection('views');
assert.equal((await inferredIds.insertOne({ type: 'view' })).insertedId, 'fixed-2', 'idGenerator implies the custom strategy');

await memClient.close();
assert.equal(await memClient.db('app').collection('users').countDocuments({}), 0, 'close() clears memory stores');
pass('createSchemalessClient("memory") CRUD, ids, isolation, aggregate, close');

// --------------------------------------------------- sql executor backend
const statements = [];
const sqlClient = await client.createSchemalessClient('sql', {
    database: 'pg',
    executor: async (sql, params) => {
        statements.push(sql);
        return { rows: [], rowCount: 0 };
    },
});
assert.equal(sqlClient.raw, null, 'sql backend has no raw driver');
const events = sqlClient.db('analytics').collection('events');
await events.insertOne({ type: 'click' });
await (await events.find({ type: 'click' }, { type: 1 })).limit(5).toArray();
await events.updateMany({ type: 'click' }, { $set: { seen: true } });
await events.deleteMany({ type: 'click' });

const insertSQL = statements.find((s) => s.startsWith('INSERT'));
assert.match(insertSQL, /^INSERT INTO events \(type\) VALUES \('click'\) RETURNING _id$/, 'pg insert SQL');
assert.ok(statements.some((s) => s.startsWith('SELECT') && s.includes('LIMIT 5')), 'find SQL with limit');
assert.ok(statements.some((s) => s.startsWith('UPDATE') && s.includes('seen = TRUE')), 'update SQL');
assert.ok(statements.some((s) => s.startsWith('DELETE FROM events')), 'delete SQL');
pass('createSchemalessClient("sql", { executor }) generates dialect SQL');

// ------------------------------------------------------- schemaless adapter
const { adapter: memAdapter, database: memDatabase } = schemaless.createSchemalessAdapter(undefined, 'memory');
assert.equal(memAdapter.database, 'memory');
const notes = memAdapter.collection('notes');
await notes.insertOne({ title: 'hello', tags: ['a'] });
assert.equal(await notes.countDocuments({}), 1, 'createSchemalessAdapter memory default');
assert.ok(memDatabase.listCollections().includes('notes'));

const { adapter: standalone } = schemaless.createMemorySchemaless();
assert.equal(standalone.database, 'memory', 'createMemorySchemaless');

// ----------------------------------------------------------- error handling
await assert.rejects(() => client.createSchemalessClient('oracle'), /Unknown type: oracle/, 'unknown type rejected');
await assert.rejects(() => client.createSchemalessClient('sql', {}), /executor/, 'sql without executor rejected');
pass('createSchemalessClient validation');

// ------------------------------------------------------------ memory engine
const engine = memory.createMemoryDB();
const coll = engine.collection('people', [{ name: 'Ann', age: 41 }, { name: 'Bo', age: 22 }]);
assert.deepEqual(coll.find({ age: { $gt: 30 } }).toArray().map((d) => d.name), ['Ann']);
assert.equal(coll.deleteMany().deletedCount, 2, 'memory deleteMany() without a filter');
assert.equal(engine.filter({ age: { $gte: 20 } })({ age: 22 }), true);
assert.deepEqual(engine.aggregate([{ $group: { _id: null, n: { $sum: 1 } } }])([{ a: 1 }, { a: 2 }]), [{ _id: null, n: 2 }]);

// id options must survive both the `collection()` factory and the client
const factoryColl = memory.collection('tickets', [], { idStrategy: 'custom', idGenerator: () => 'g-1' });
assert.equal(factoryColl.insertOne({ title: 'x' }).insertedId, 'g-1', 'collection() forwards id options');
const mongoColl = memory.db('legacy').collection('logs', [], { idStrategy: 'mongo' });
assert.equal(String(mongoColl.insertOne({ level: 'info' }).insertedId).length, 24, 'mongo ids in memory engine');
pass('createMemoryDB engine (filter/aggregate/deleteMany default/id strategies)');

// ------------------------------------------------- bring your own driver
// umosql bundles no driver and depends on none: these backends run against
// hand-written fakes, proving the real packages are never required.
const fakeSql = [];

const fakeSqliteDriver = {
    default: function FakeDatabase(filename) {
        return {
            filename,
            prepare: (sql) => ({
                all: () => {
                    fakeSql.push(sql);
                    if (/COUNT\(\*\)/i.test(sql)) return [{ count: 1 }];
                    if (/FROM items/i.test(sql)) return [{ _id: 1, sku: 'a' }];
                    return [];
                },
                run: () => {
                    fakeSql.push(sql);
                    return { changes: 1, lastInsertRowid: 9 };
                },
            }),
            close: () => fakeSql.push('__closed__'),
        };
    },
};

const sqliteClient = await client.createSchemalessClient('sqlite', { driver: fakeSqliteDriver, filename: ':memory:' });
const items = sqliteClient.db('app').collection('items');
const sqliteInsert = await items.insertOne({ sku: 'a' });
assert.equal(sqliteInsert.insertedId, 9, 'sqlite lastInsertRowid through an injected driver');
assert.ok(fakeSql.some((s) => /^CREATE TABLE IF NOT EXISTS items/.test(s)), 'sqlite DDL auto-created');
assert.ok(fakeSql.some((s) => s.startsWith('INSERT INTO items')), 'sqlite insert executed');
assert.deepEqual(await (await items.find({ sku: 'a' })).toArray(), [{ _id: 1, sku: 'a' }], 'sqlite rows mapped');
assert.equal(await items.countDocuments({}), 1, 'sqlite count');
await sqliteClient.close();
assert.ok(fakeSql.includes('__closed__'), 'close() closes the injected sqlite handle');
pass('sqlite backend with an injected driver (better-sqlite3 never loaded)');

const pgLog = [];
const fakePgDriver = {
    Client: function FakePgClient(cfg) {
        this.config = cfg;
        this.connects = 0;
        this.connect = async () => { this.connects++; pgLog.push('__connect__'); };
        this.end = async () => { pgLog.push('__end__'); };
        this.query = async (sql, params) => {
            pgLog.push(sql);
            if (/COUNT\(\*\)/i.test(sql)) return { rows: [{ count: 2 }], rowCount: 1 };
            if (/RETURNING/i.test(sql)) return { rows: [{ _id: 7 }], rowCount: 1 };
            return { rows: [], rowCount: 0 };
        };
    },
};

const pgClient = await client.createSchemalessClient('pg', { driver: fakePgDriver, host: 'h', database: 'd' });
assert.equal(pgClient.raw.connects, 1, 'pg driver connected once when umosql creates it');
const pgUsers = pgClient.db('app').collection('users');
assert.equal((await pgUsers.insertOne({ name: 'Alice' })).insertedId, 7, 'pg RETURNING id');
assert.ok(pgLog.some((s) => s.startsWith('INSERT INTO users') && s.endsWith('RETURNING _id')), 'pg insert SQL');
assert.equal(await pgUsers.countDocuments({}), 2, 'pg count');
await pgClient.close();
assert.ok(pgLog.includes('__end__'), 'close() ends the pg connection');

// A caller-supplied instance must be adopted as-is: no driver import, no connect().
const supplied = { connects: 0, query: async () => ({ rows: [], rowCount: 0 }), end: async () => { supplied.connects = -1; } };
const adopted = await client.createSchemalessClient('pg', { client: supplied });
assert.equal(adopted.raw, supplied, 'supplied pg client adopted');
assert.equal(supplied.connects, 0, 'supplied pg client is not re-connected');
await adopted.close();
assert.equal(supplied.connects, -1, 'close() still ends a supplied client');
pass('pg backend with an injected driver and with a supplied client');

const myLog = [];
const fakeMysqlDriver = {
    createConnection: async (cfg) => ({
        config: cfg,
        execute: async (sql) => {
            myLog.push(sql);
            if (/COUNT\(\*\)/i.test(sql)) return [[{ count: 3 }], []];
            if (/^INSERT/i.test(sql)) return [{ affectedRows: 1, insertId: 11 }, []];
            return [[], []];
        },
        end: async () => myLog.push('__end__'),
    }),
};
const mysqlClient = await client.createSchemalessClient('mysql', { driver: fakeMysqlDriver, host: 'h', database: 'd' });
const mysqlUsers = mysqlClient.db('app').collection('users');
assert.equal((await mysqlUsers.insertOne({ name: 'Alice' })).insertedId, 11, 'mysql insertId');
assert.equal(await mysqlUsers.countDocuments({}), 3, 'mysql count through [rows, fields]');
await mysqlClient.close();
assert.ok(myLog.includes('__end__'), 'close() ends the mysql connection');
pass('mysql backend with an injected driver (mysql2 never loaded)');

// MongoDB through an injected driver module.
const fakeMongoDriver = {
    MongoClient: function FakeMongoClient(uri) {
        this.uri = uri;
        this.docs = [];
        this.db = () => ({
            collection: () => ({
                insertOne: async (doc) => { this.docs.push(doc); return { acknowledged: true, insertedId: doc._id || 1 }; },
                findOne: async (q) => this.docs.find((d) => d.name === q?.name) || null,
                find: () => ({ sort: () => this, skip: () => this, limit: () => this, toArray: async () => this.docs, count: async () => this.docs.length }),
                countDocuments: async () => this.docs.length,
                drop: async () => { this.docs = []; },
            }),
            listCollections: () => ({ toArray: async () => [] }),
        });
        this.connect = async () => { this.connected = true; };
        this.close = async () => { this.closed = true; };
    },
};
const mongoClient = await client.createSchemalessClient('mongodb', { driver: fakeMongoDriver, database: 'test' });
const mongoUsers = mongoClient.db('test').collection('users');
await mongoUsers.insertOne({ name: 'Alice' });
assert.equal((await mongoUsers.findOne({ name: 'Alice' })).name, 'Alice', 'mongodb roundtrip via injected driver');
assert.equal(await mongoUsers.countDocuments({}), 1);
await mongoClient.close();
assert.equal(mongoClient.raw.closed, true, 'close() closes the MongoClient');
pass('mongodb backend with an injected driver (mongodb never loaded)');

// A missing driver must fail with an actionable message, not a bare ESM error.
await assert.rejects(
    () => client.loadDriver('postgres', 'pg-not-installed-anywhere', () => import('pg-not-installed-anywhere')),
    /not installed[\s\S]*npm i pg-not-installed-anywhere[\s\S]*bring your own/,
    'missing driver explains how to install or inject it'
);
pass('missing driver error message');

const dist = path.join(root, 'dist');
const built = ['index', 'schemaless', 'memory', 'client'];
const hasBuild = built.every((name) => fs.existsSync(path.join(dist, `${name}.es.js`)) && fs.existsSync(path.join(dist, `${name}.cjs`)));

if (hasBuild) {
    for (const name of built) {
        const esm = await import(pathToFileURL(path.join(dist, `${name}.es.js`)).href);
        const cjs = require(path.join(dist, `${name}.cjs`));
        assert.deepEqual(Object.keys(esm).sort(), Object.keys(cjs).sort(), `dist/${name}: esm and cjs exports match`);
        pass(`dist/${name} loads as ESM and CommonJS`);
    }

    const esmClient = await import(pathToFileURL(path.join(dist, 'client.es.js')).href);
    const bundled = await esmClient.createSchemalessClient('memory');
    const items = bundled.db('app').collection('items');
    await items.insertOne({ sku: 'a' });
    assert.equal(await items.countDocuments({}), 1, 'bundled memory client works');
    await bundled.close();

    const cjsMemory = require(path.join(dist, 'memory.cjs'));
    assert.equal(cjsMemory.collection('x', [{ a: 1 }]).countDocuments({}), 1, 'bundled memory engine works');

    const esmSchemaless = await import(pathToFileURL(path.join(dist, 'schemaless.es.js')).href);
    assert.equal(typeof esmSchemaless.createSQLAdapter, 'function');
    assert.equal(esmSchemaless.createSchemalessAdapter(undefined, 'memory').adapter.database, 'memory');

    // Optional drivers must stay external instead of being inlined.
    const clientBundle = fs.readFileSync(path.join(dist, 'client.es.js'), 'utf8');
    for (const dep of ['better-sqlite3', 'pg', 'mysql2/promise', 'mongodb', 'dotenv']) {
        assert.ok(clientBundle.includes(`import('${dep}')`) || clientBundle.includes(`import("${dep}")`),
            `dist/client keeps "${dep}" as a lazy external import`);
    }
    pass('dist/client keeps drivers lazy/external');

    // ...and no driver implementation may ever be copied into a bundle.
    const markers = ['pg-pool', 'pg/lib', 'sqlstring', 'mysql2/lib', 'bson', 'saslprep', 'prebuild-install', 'dotenv/lib'];
    for (const file of fs.readdirSync(dist).filter((f) => /\.(js|cjs)$/.test(f))) {
        const code = fs.readFileSync(path.join(dist, file), 'utf8');
        for (const marker of markers) {
            assert.ok(!code.includes(marker), `dist/${file} must not contain driver code ("${marker}")`);
        }
    }
    pass('no driver code in any bundle');

    // Type declarations ship next to every entry point.
    for (const file of ['index.d.ts', 'schemaless.d.ts', 'memory.d.ts', 'client.d.ts',
        'lite/index.d.ts', 'tiny/index.d.ts']) {
        assert.ok(fs.existsSync(path.join(root, file)), `${file} exists`);
    }
    for (const file of ['dist/index.d.ts', 'dist/schemaless.d.ts', 'dist/memory.d.ts', 'dist/client.d.ts',
        'lite/dist/index.d.ts', 'tiny/dist/index.d.ts']) {
        assert.ok(fs.existsSync(path.join(root, file)), `${file} copied into the bundle folder`);
    }
    pass('type declarations present for every entry point');
} else {
    console.log('SKIP dist bundle checks (run `npm run build` first)');
}
