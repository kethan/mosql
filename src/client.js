// ============================================
// UNIFIED SCHEMALESS CLIENT
// ============================================
// One factory for every backend: memory, sqlite, pg, mysql, mongodb and any
// custom SQL executor (serverless drivers such as Neon / Turso / PlanetScale).
//
// NO driver is bundled and none is declared as a dependency. You install the
// driver you want, or hand us the instance/module you already have:
//   createSchemalessClient('pg', { client: myPgClient })   // driver never loaded
//   createSchemalessClient('pg', { driver: pgModule })     // we only construct
//   createSchemalessClient('pg', { host, ... })            // lazy import('pg')

import { loadEnv } from './env.js';
import { loadDriver, pick } from './drivers.js';
import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers } from './index.js';
import { createSchemalessAdapter, createSQLAdapter } from './schemaless.js';
import { createMemorySchemaless } from './adapter/memory/adapter.js';
import { createMongoSchemaless } from './adapter/mongodb/adapter.js';

export const CLIENT_TYPES = ['memory', 'sqlite', 'pg', 'mysql', 'mongodb', 'sql'];

const noop = async () => { };

export const createSchemalessClient = async (type = 'sqlite', cfg = {}) => {
    await loadEnv();

    let adapter;
    let raw = null;
    let close = noop;
    // SQL/Mongo backends have a single adapter; memory keeps one isolated
    // store per database name so `client.db('a')` and `client.db('b')` do not
    // share documents.
    let adapterFor = () => adapter;

    if (type === 'memory') {
        const stores = new Map();
        const defaultStore = cfg.database || 'unified';
        const store = (name = defaultStore) => {
            if (!stores.has(name)) stores.set(name, createMemorySchemaless());
            return stores.get(name);
        };

        const initial = store();
        adapter = initial.adapter;
        raw = initial.database;
        adapterFor = (name) => store(name).adapter;
        close = async () => {
            stores.forEach((s) => { try { s.database?.dropDatabase?.(); } catch { } });
            stores.clear();
        };
    } else if (type === 'sqlite') {
        if (cfg.client) {
            raw = cfg.client;
        } else {
            const Database = cfg.driver
                ? pick(cfg.driver, 'Database', 'default')
                : pick(await loadDriver('sqlite', 'better-sqlite3', () => import('better-sqlite3')), 'default', 'Database');
            if (typeof Database !== 'function') throw new Error("umosql: could not find a Database constructor in the 'better-sqlite3' driver (or pass { client })");
            raw = new Database(cfg.filename || ':memory:', cfg.driverOptions);
        }
        adapter = createSchemalessAdapter(raw, 'sqlite').adapter;
        close = async () => { try { raw?.close?.(); } catch { } };
    } else if (type === 'pg') {
        if (cfg.client) {
            // A caller supplied client is already connected - pg throws if you
            // connect twice, so only adopt it and never load the driver.
            raw = cfg.client;
        } else {
            const driver = cfg.driver || await loadDriver('pg', 'pg', () => import('pg'));
            const Client = pick(driver, 'Client');
            if (typeof Client !== 'function') throw new Error("umosql: could not find Client in the 'pg' driver (or pass { client })");
            const connectionString = cfg.connectionString || cfg.url;
            raw = new Client({
                host: cfg.host,
                port: cfg.port || 5432,
                user: cfg.user,
                password: cfg.password,
                database: cfg.database,
                ...(connectionString ? { connectionString } : {}),
                ...(cfg.ssl !== undefined ? { ssl: cfg.ssl } : {}),
                ...(cfg.driverOptions || {}),
            });
            await raw.connect();
        }
        adapter = createSchemalessAdapter(raw, 'pg').adapter;
        close = async () => { try { await raw?.end?.(); } catch { } };
    } else if (type === 'mysql') {
        raw = cfg.conn || cfg.client;
        if (!raw) {
            const driver = cfg.driver || await loadDriver('mysql', 'mysql2/promise', () => import('mysql2/promise'));
            const createConnection = pick(driver, 'createConnection');
            if (typeof createConnection !== 'function') throw new Error("umosql: could not find createConnection in the 'mysql2/promise' driver (or pass { conn })");
            const uri = cfg.uri || cfg.connectionString || cfg.url;
            raw = await createConnection({
                host: cfg.host,
                port: cfg.port,
                user: cfg.user,
                password: cfg.password,
                database: cfg.database,
                ...(uri ? { uri } : {}),
                ...(cfg.driverOptions || {}),
            });
        }
        adapter = createSchemalessAdapter(raw, 'mysql').adapter;
        close = async () => { try { await raw?.end?.(); } catch { } };
    } else if (type === 'mongodb') {
        // `driver` lets you inject your own MongoClient module.
        const init = await createMongoSchemaless(cfg);
        adapter = init.adapter;
        raw = init.client;
        close = async () => { try { await raw?.close?.(); } catch { } };
    } else if (type === 'sql') {
        if (typeof cfg.executor !== 'function') {
            throw new Error("createSchemalessClient('sql', { executor }) requires an executor(sql, params) function");
        }
        const qb = createQueryBuilder({ filterOps, exprOps, updateOps, stageHandlers, debug: cfg.debug });
        adapter = createSQLAdapter({ database: cfg.database || 'sqlite', execute: cfg.executor, queryBuilder: qb });
        raw = cfg.client || null;
        close = typeof cfg.close === 'function' ? cfg.close : noop;
    } else {
        throw new Error(`Unknown type: ${type}. Expected one of: ${CLIENT_TYPES.join(', ')}`);
    }

    const client = {
        type,
        raw,
        adapter,
        db(name, options = {}) {
            const idColumn = options.id || options.idColumn || '_id';
            const idGenerator = typeof options.idGenerator === 'function' ? options.idGenerator : null;
            // Supplying an idGenerator without a strategy means "custom" - under
            // `auto` every adapter would silently ignore it.
            const idStrategy = options.idStrategy || (idGenerator ? 'custom' : type === 'mongodb' ? 'mongo' : 'auto');
            const backend = adapterFor(name);

            return {
                name,
                collection: (col, colOptions = {}) => backend.collection(col, {
                    idColumn,
                    idStrategy,
                    idGenerator,
                    ...colOptions,
                }),
                adapter: backend,
            };
        },
        close,
    };

    return client;
};

export {
    loadDriver,
    createMemorySchemaless,
    createMongoSchemaless,
    createSchemalessAdapter,
    createSQLAdapter,
};

export default createSchemalessClient;
