// ============================================
// SCHEMA-LESS SQL ADAPTER
// ============================================

import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers } from './index.js';
import { createMemorySchemaless } from './adapter/memory/adapter.js';

export { createMemorySchemaless };

/* Copied from v2/src/new.js (renamed) */
/* Contents identical to ensure API continuity */

// ============================================
// TYPE INFERENCE
// ============================================

const isObject = (v) => typeof v === 'object' && v !== null && !Array.isArray(v);

const inferSQLType = (value, database) => {
    const types = {
        pg: { string: (v) => v.length > 255 ? 'TEXT' : 'VARCHAR(255)', number: (v) => Number.isInteger(v) ? 'INTEGER' : 'DECIMAL(20,6)', boolean: 'BOOLEAN', date: 'TIMESTAMP', json: 'JSONB' },
        mysql: { string: 'VARCHAR(255)', number: (v) => Number.isInteger(v) ? 'INT' : 'DECIMAL(20,6)', boolean: 'TINYINT(1)', date: 'DATETIME', json: 'JSON' },
        sqlite: { string: 'TEXT', number: (v) => Number.isInteger(v) ? 'INTEGER' : 'REAL', boolean: 'INTEGER', date: 'TEXT', json: 'TEXT' },
    };
    const t = types[database];
    if (value instanceof Date) return t.date;
    if (Array.isArray(value)) return t.json;
    if (typeof value === 'object' && value !== null) return t.json;
    const type = typeof value;
    if (type === 'string') return typeof t.string === 'function' ? t.string(value) : t.string;
    if (type === 'number') return typeof t.number === 'function' ? t.number(value) : t.number;
    if (type === 'boolean') return t.boolean;
    return typeof t.string === 'function' ? t.string('') : t.string;
};

const widenType = (current, next) => {
    const rank = { INTEGER: 1, INT: 1, TINYINT: 1, BIGINT: 2, DECIMAL: 3, REAL: 3, VARCHAR: 4, TEXT: 5 };
    const c = current.replace(/\(.*\)/, '').toUpperCase();
    const n = next.replace(/\(.*\)/, '').toUpperCase();
    return (rank[c] || 0) > (rank[n] || 0) ? current : next;
};

const escapeValue = (value, database) => {
    if (value === null) return 'NULL';
    if (typeof value === 'number') return String(value);
    if (typeof value === 'boolean') return database === 'pg' ? (value ? 'TRUE' : 'FALSE') : (value ? '1' : '0');
    if (value instanceof Date) {
        const pad = (n) => String(n).padStart(2, '0');
        const yyyy = value.getUTCFullYear();
        const mm = pad(value.getUTCMonth() + 1);
        const dd = pad(value.getUTCDate());
        const hh = pad(value.getUTCHours());
        const mi = pad(value.getUTCMinutes());
        const ss = pad(value.getUTCSeconds());
        const ts = `${yyyy}-${mm}-${dd} ${hh}:${mi}:${ss}`;
        return `'${ts}'`;
    }
    if (typeof value === 'object') return `'${JSON.stringify(value).replace(/'/g, "''")}'`;
    return `'${String(value).replace(/'/g, "''")}'`;
};

const normalizeResult = (result, database) => {
    if (database === 'mysql' && Array.isArray(result) && result.length === 2) {
        const payload = result[0];
        if (payload && typeof payload === 'object' && !Array.isArray(payload)) {
            return { rows: Array.isArray(payload) ? payload : [], fields: result[1], affectedRows: payload.affectedRows, insertId: payload.insertId };
        }
        return { rows: payload, fields: result[1] };
    }
    if (result && result.rows) {
        return { rows: result.rows, rowCount: result.rowCount, affectedRows: result.rowCount };
    }
    if (Array.isArray(result)) { return { rows: result }; }
    return result;
};

class SQLCollection {
    constructor(name, adapter, config = {}) {
        this.name = name;
        this.adapter = adapter;
        this.database = adapter.database;
        this.schema = config.schema || {};
        this.tableSchema = null;
        this.isInitialized = false;
        this.options = { migrateOnUpdate: config.migrateOnUpdate !== undefined ? !!config.migrateOnUpdate : true, idStrategy: config.idStrategy || 'auto', idColumn: config.idColumn || '_id', idGenerator: typeof config.idGenerator === 'function' ? config.idGenerator : null };
    }
    async initialize() { if (this.isInitialized) return; const exists = await this.adapter.tableExists(this.name); if (!exists && this.schema && Object.keys(this.schema).length) { await this.createTable({}); } this.tableSchema = await this.adapter.getTableSchema(this.name) || { columns: {} }; this.isInitialized = true; }
    async createTable(document) {
        const columns = []; const keys = Array.from(new Set([ ...Object.keys(this.schema || {}), ...Object.keys(document || {}) ]));
        keys.forEach((key) => { if (key === '_id') return; const schemaDef = this.schema[key]; const value = document[key]; const type = (typeof schemaDef === 'string' ? schemaDef : schemaDef?.type) || inferSQLType(value, this.database); const nullable = schemaDef?.required ? ' NOT NULL' : ''; const unique = schemaDef?.unique ? ' UNIQUE' : ''; const defaultValue = schemaDef?.default !== undefined ? ` DEFAULT ${typeof schemaDef.default === 'function' ? escapeValue(schemaDef.default(), this.database) : escapeValue(schemaDef.default, this.database)}` : ''; columns.push(`${key} ${type}${nullable}${unique}${defaultValue}`); });
        const idCol = this.options.idColumn; let idColumn; if (this.options.idStrategy === 'auto') { idColumn = this.database === 'pg' ? `${idCol} SERIAL PRIMARY KEY` : this.database === 'mysql' ? `${idCol} INT AUTO_INCREMENT PRIMARY KEY` : `${idCol} INTEGER PRIMARY KEY AUTOINCREMENT`; } else { const idType = this.database === 'pg' ? 'TEXT' : this.database === 'mysql' ? 'VARCHAR(24)' : 'TEXT'; idColumn = `${idCol} ${idType} PRIMARY KEY`; }
        const timestamps = this.database === 'pg' ? 'created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP, updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP' : this.database === 'mysql' ? 'created_at DATETIME DEFAULT CURRENT_TIMESTAMP, updated_at DATETIME DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP' : "created_at TEXT DEFAULT (datetime('now')), updated_at TEXT DEFAULT (datetime('now'))";
        const baseColumns = [idColumn, timestamps].join(', ');
        await this.adapter.execute(`CREATE TABLE IF NOT EXISTS ${this.name} (${baseColumns})`);
        for (const col of columns) { const addKw = this.database === 'mysql' ? 'ADD' : 'ADD COLUMN'; try { await this.adapter.execute(`ALTER TABLE ${this.name} ${addKw} ${col}`); } catch (e) {} }
    }
    async migrate(document) {
        const migrations = []; const currentSchema = await this.adapter.getTableSchema(this.name);
        Object.entries(document).forEach(([key, value]) => { if (key === '_id') return; const schemaDef = this.schema[key]; if (!currentSchema.columns[key]) { const type = (typeof schemaDef === 'string' ? schemaDef : schemaDef?.type) || inferSQLType(value, this.database); const nullable = schemaDef?.required ? ' NOT NULL' : ''; const unique = schemaDef?.unique ? ' UNIQUE' : ''; const defaultValue = schemaDef?.default !== undefined ? ` DEFAULT ${typeof schemaDef.default === 'function' ? escapeValue(schemaDef.default(), this.database) : escapeValue(schemaDef.default, this.database)}` : ''; const addKw = this.database === 'mysql' ? 'ADD' : 'ADD COLUMN'; migrations.push(`ALTER TABLE ${this.name} ${addKw} ${key} ${type}${nullable}${unique}${defaultValue}`); this.tableSchema.columns[key] = type; } else { const currentType = currentSchema.columns[key]; const newType = (typeof schemaDef === 'string' ? schemaDef : schemaDef?.type) || inferSQLType(value, this.database); const widened = widenType(currentType, newType); if (widened !== currentType && this.database !== 'sqlite') { const alterSQL = this.database === 'pg' ? `ALTER TABLE ${this.name} ALTER COLUMN ${key} TYPE ${widened}` : `ALTER TABLE ${this.name} MODIFY COLUMN ${key} ${widened}`; migrations.push(alterSQL); this.tableSchema.columns[key] = widened; } } });
        for (const sql of migrations) { try { await this.adapter.execute(sql); } catch (e) { const msg = String(e?.message || e).toLowerCase(); if (!(msg.includes('already exists') || msg.includes('duplicate') || msg.includes('exists'))) throw e; } } this.tableSchema = currentSchema;
    }
    // A missing column and a missing table must not be confused. SQLSTATE and errno
    // are exact, so they settle it whenever the driver reports them; the message
    // patterns only back up the drivers that do not, and they have to decide the
    // column case first: PostgreSQL words a missing column as
    // `column "alias" of relation "users" does not exist`, which contains the same
    // "does not exist" that identifies a missing table. Reading that as "the table is
    // gone" runs a no-op CREATE TABLE IF NOT EXISTS and then re-runs the very
    // statement that just failed, so migrateOnUpdate never gets to add the column -
    // which is why updating a field the table did not have yet failed on pg only.
    classifyError(e) { const code = e?.code; const errno = e?.errno; const msg = String(e?.message || '').toLowerCase(); const missingColumn = code === '42703' || errno === 1054 || msg.includes('no such column') || msg.includes('unknown column') || (msg.includes('does not exist') && msg.includes('column')); const missingTable = !missingColumn && (code === '42P01' || errno === 1146 || msg.includes('no such table') || msg.includes('unknown table') || (msg.includes('does not exist') && !msg.includes('column'))); return { missingTable, missingColumn }; }
    // Order matters: migrate the column before considering a CREATE TABLE, because
    // only the migration can fix `column ... does not exist`, and the retry has to
    // be a statement that can actually succeed.
    async execWithDDLRetry(sql, opts = {}) { try { return await this.adapter.execute(sql); } catch (e) { const { missingTable, missingColumn } = this.classifyError(e); if (missingColumn && opts.allowColumnMigrate) { await this.migrate(opts.docForMigrate || {}); return await this.adapter.execute(sql); } if (missingTable) { await this.createTable(opts.docForCreate || {}); return await this.adapter.execute(sql); } throw e; } }
    // Fields an update statement writes to. `migrateOnUpdate` only fires after the
    // database reports a missing column, and it can only add columns it knows
    // about - so the target of `$rename` (and `$inc`/`$mul`/`$min`/`$max` on a
    // field that does not exist yet) has to be part of that document, otherwise
    // the retry re-runs the very same failing statement.
    updateTargets(update) {
        const targets = {};
        if (!isObject(update)) return targets;
        Object.assign(targets, update.$set || {});
        if (isObject(update.$currentDate)) for (const k of Object.keys(update.$currentDate)) targets[k] = new Date();
        // Arithmetic operators may hit a field the table does not have yet, and
        // the target of `$rename` is by definition a column that does not exist
        // until this statement runs. Only unknown columns are described here: an
        // existing one must keep its own type.
        const missing = (k) => k && !k.includes('.') && !this.tableSchema?.columns?.[k];
        for (const op of ['$inc', '$mul', '$min', '$max']) {
            if (!isObject(update[op])) continue;
            for (const [k, v] of Object.entries(update[op])) {
                if (missing(k)) targets[k] = typeof v === 'number' ? v : this.sampleFor(k);
            }
        }
        if (isObject(update.$rename)) {
            for (const [from, to] of Object.entries(update.$rename)) {
                if (missing(to)) targets[to] = this.sampleFor(from);
            }
        }
        for (const [key, val] of Object.entries(update)) {
            if (key && !key.startsWith('$')) targets[key] = val;
        }
        for (const k of Object.keys(targets)) if (k.includes('.')) delete targets[k];
        return targets;
    }

    // Representative JS value for an existing column, used to pick the type of a
    // column that has to be created for it.
    sampleFor(column) {
        const type = String(this.tableSchema?.columns?.[column] || '');
        if (/INT|NUMERIC|DECIMAL|REAL|DOUBLE|FLOAT|MONEY|SERIAL/.test(type)) return 0;
        if (/BOOL/.test(type)) return false;
        if (/TIMESTAMP|DATE|TIME|YEAR/.test(type)) return new Date();
        if (/JSON/.test(type)) return {};
        return '';
    }
    applyDefaults(document) { const result = { ...document }; Object.entries(this.schema).forEach(([key, def]) => { if (result[key] === undefined && def.default !== undefined) { result[key] = typeof def.default === 'function' ? def.default() : def.default; } }); return result; }
    filterHidden(document) { if (!document) return document; const result = { ...document }; Object.entries(this.schema).forEach(([key, def]) => { if (def.hidden) delete result[key]; }); return result; }
    generateId() { if (this.options.idGenerator) return this.options.idGenerator(); if (this.options.idStrategy === 'mongo') { const ts = Math.floor(Date.now() / 1000).toString(16).padStart(8, '0'); const rand = Array.from({ length: 16 }, () => Math.floor(Math.random() * 16).toString(16)).join(''); return (ts + rand).slice(0, 24); } return null; }
    async insertOne(document) { await this.initialize(); const doc = this.applyDefaults(document); const idCol = this.options.idColumn; const nonAuto = this.options.idStrategy !== 'auto'; if (nonAuto) { if (!doc[idCol]) doc[idCol] = this.generateId(); }
        if (Object.keys(this.tableSchema.columns).length === 0) { await this.createTable(doc); this.tableSchema = await this.adapter.getTableSchema(this.name); }
        await this.migrate(doc);
        const insert = { ...doc };
        const sql = this.adapter.buildInsert(this.name, [insert], this.database === 'pg' ? { returning: [idCol] } : {});
        const result = await this.execWithDDLRetry(sql, { docForCreate: doc, docForMigrate: doc, allowColumnMigrate: true });
        const insertedId = nonAuto ? insert[idCol] : (result.insertId || result.rows?.[0]?.[idCol] || result.lastInsertRowid);
        return { insertedId, acknowledged: true };
    }
    async insertMany(documents) { const ids = []; for (const doc of documents) { const result = await this.insertOne(doc); ids.push(result.insertedId); } return { insertedIds: ids, acknowledged: true }; }
    async find(query = {}, projection = null, options = {}) { const state = { query, projection: projection ?? options?.select ?? null, sort: options?.sort || options?.order, skip: options?.skip ?? options?.offset, limit: options?.limit, distinct: options?.distinct, }; const self = this; const cursor = { sort(obj) { state.sort = obj; return this; }, skip(n) { state.skip = n; return this; }, limit(n) { state.limit = n; return this; }, distinct() { state.distinct = true; return this; }, async toArray() { await self.initialize(); const hasOpts = !!(state.sort || state.distinct || state.limit != null || state.skip != null); const sql = hasOpts ? self.adapter.buildFindWithOptions(self.name, state.query, state.projection, { sort: state.sort, limit: state.limit, skip: state.skip, distinct: state.distinct }) : self.adapter.buildFind(self.name, state.query, state.projection); const result = await self.execWithDDLRetry(sql, { docForCreate: {}, allowColumnMigrate: false }); const rows = (result.rows || result).map(row => self.filterHidden(row)); return rows; }, async count() { await self.initialize(); const sql = self.adapter.buildCount(self.name, state.query); const res = await self.execWithDDLRetry(sql, { docForCreate: {}, allowColumnMigrate: false }); const row = res.rows?.[0] || res[0]; let n = parseInt(row?.count || 0) || 0; if (state.skip != null) n = Math.max(0, n - state.skip); if (state.limit != null) n = Math.min(n, state.limit); return n; } }; return cursor; }
    async findOne(query = {}, projection = null) { await this.initialize(); const sql = this.adapter.buildFind(this.name, query, projection); const result = await this.execWithDDLRetry(sql, { docForCreate: {}, allowColumnMigrate: false }); const rows = result.rows || result; return rows[0] ? this.filterHidden(rows[0]) : null; }
    async updateOne(query, update) { await this.initialize(); const preCountSQL = this.adapter.buildCount(this.name, query); const preCountRes = await this.execWithDDLRetry(preCountSQL, { docForCreate: {}, allowColumnMigrate: false }); const preRow = preCountRes.rows?.[0] || preCountRes[0]; const matchedCount = Math.min(1, parseInt(preRow?.count || 0)); const sql = this.adapter.buildUpdateOne(this.name, query, update); const result = await this.execWithDDLRetry(sql, { docForCreate: {}, docForMigrate: this.updateTargets(update), allowColumnMigrate: this.options.migrateOnUpdate }); return { matchedCount, modifiedCount: result.affectedRows || result.rowCount || result.changes || 0, acknowledged: true }; }
    async updateMany(query, update) { await this.initialize(); const preCountSQL = this.adapter.buildCount(this.name, query); const preCountRes = await this.execWithDDLRetry(preCountSQL, { docForCreate: {}, allowColumnMigrate: false }); const preRow = preCountRes.rows?.[0] || preCountRes[0]; const matchedCount = parseInt(preRow?.count || 0); const sql = this.adapter.buildUpdateMany(this.name, query, update); const result = await this.execWithDDLRetry(sql, { docForCreate: {}, docForMigrate: this.updateTargets(update), allowColumnMigrate: this.options.migrateOnUpdate }); return { matchedCount, modifiedCount: result.affectedRows || result.rowCount || result.changes || 0, acknowledged: true }; }
    async upsertOne(query, update, insertDoc = {}) { await this.initialize(); const res = await this.updateOne(query, update); if ((res.modifiedCount || 0) === 0) { const eqs = {}; for (const [k, v] of Object.entries(query || {})) { if (v && typeof v === 'object' && '$eq' in v) eqs[k] = v.$eq; else if (v === null || typeof v !== 'object') eqs[k] = v; } const base = Object.assign({}, eqs, insertDoc, update?.$set || {}); const r = await this.insertOne(base); return { upserted: true, insertedId: r.insertedId, acknowledged: true }; } return { upserted: false, acknowledged: true }; }
    async deleteOne(query) { await this.initialize(); const sql = this.adapter.buildDeleteOne(this.name, query); const result = await this.execWithDDLRetry(sql, { docForCreate: {}, allowColumnMigrate: false }); return { deletedCount: result.affectedRows || result.rowCount || result.changes || 0, acknowledged: true }; }
    async deleteMany(query = {}) { await this.initialize(); const sql = this.adapter.buildDeleteMany(this.name, query); const result = await this.execWithDDLRetry(sql, { docForCreate: {}, allowColumnMigrate: false }); return { deletedCount: result.affectedRows || result.rowCount || result.changes || 0, acknowledged: true }; }
    async countDocuments(query = {}) { await this.initialize(); const sql = this.adapter.buildCount(this.name, query); const result = await this.execWithDDLRetry(sql, { docForCreate: {}, allowColumnMigrate: false }); const row = result.rows?.[0] || result[0]; return parseInt(row?.count || 0); }
    async estimatedDocumentCount() { return this.countDocuments({}); }
    async distinct(field, query = {}) { await this.initialize(); const sql = this.adapter.buildDistinct(this.name, field, query); const result = await this.execWithDDLRetry(sql, { docForCreate: {}, allowColumnMigrate: false }); const rows = result.rows || result; return rows.map(r => Object.values(r)[0]); }
    async findMany(opts = {}) { await this.initialize(); const { filter = {}, projection = null, includeTotal } = opts; const sort = opts.sort || opts.order; const limit = opts.limit; const offset = opts.offset ?? opts.skip; const page = opts.page ?? opts.pageIndex; const pageSize = opts.pageSize ?? limit; const distinct = opts.distinct; const select = opts.select ?? projection; const finalLimit = pageSize != null ? pageSize : limit; const finalSkip = page != null && pageSize != null ? Math.max(0, (page - 1) * pageSize) : (offset != null ? offset : undefined); const sql = this.adapter.buildFindWithOptions(this.name, filter, select, { sort, limit: finalLimit, skip: finalSkip, distinct }); const result = await this.execWithDDLRetry(sql, { docForCreate: {}, allowColumnMigrate: false }); const items = (result.rows || result).map(row => this.filterHidden(row)); let total = undefined; if (includeTotal) { const csql = this.adapter.buildCount(this.name, filter); const cres = await this.execWithDDLRetry(csql, { docForCreate: {}, allowColumnMigrate: false }); const row = cres.rows?.[0] || cres[0]; total = parseInt(row?.count || 0); } return { items, total, page: page != null ? page : undefined, pageSize: pageSize != null ? pageSize : finalLimit }; }
    async aggregate(pipeline) { await this.initialize(); const sql = this.adapter.buildAggregate(this.name, pipeline); const result = await this.adapter.execute(sql); return (result.rows || result).map(row => this.filterHidden(row)); }
    async createIndex(field, options = {}) { const name = options.name || `idx_${this.name}_${field.replace(/\./g, '_')}`; const unique = options.unique ? 'UNIQUE' : ''; const type = this.database === 'pg' && options.type ? `USING ${options.type.toUpperCase()}` : ''; if (this.database === 'mysql') { const existing = await this.adapter.execute(`SHOW INDEX FROM ${this.name} WHERE Key_name = '${name}'`); const rows = existing.rows || existing; if (!rows || rows.length === 0) { await this.adapter.execute(`CREATE ${unique} INDEX ${name} ON ${this.name} (${field})`.trim()); } } else { await this.adapter.execute(`CREATE ${unique} INDEX IF NOT EXISTS ${name} ON ${this.name} ${type} (${field})`.trim()); } return { acknowledged: true }; }
    async dropColumn(column) { if (this.database === 'sqlite') throw new Error('SQLite does not support DROP COLUMN'); await this.adapter.execute(`ALTER TABLE ${this.name} DROP COLUMN ${column}`); if (this.tableSchema && this.tableSchema.columns) { delete this.tableSchema.columns[column]; } return { acknowledged: true }; }
}

export const createSQLAdapter = (config = {}) => {
    const { database = 'sqlite', execute, queryBuilder } = config;
    if (!execute) throw new Error('execute function is required');
    if (!queryBuilder) throw new Error('queryBuilder is required (pass createQueryBuilder({...}))');
    const adapter = {
        database,
        async execute(sql, params = []) { const result = await execute(sql, params); return normalizeResult(result, database); },
        async tableExists(tableName) { try { const sql = database === 'pg' ? `SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'public' AND table_name = '${tableName}')` : database === 'mysql' ? `SELECT COUNT(*) as count FROM information_schema.tables WHERE table_name = '${tableName}' AND table_schema = DATABASE()` : `SELECT name FROM sqlite_master WHERE type='table' AND name='${tableName}'`; const result = await this.execute(sql); const row = result.rows?.[0] || result[0]; return !!(row?.exists || row?.count || row?.name); } catch (e) { return false; } },
        async getTableSchema(tableName) { const sql = database === 'pg' ? `SELECT column_name, data_type FROM information_schema.columns WHERE table_name = '${tableName}'` : database === 'mysql' ? `SELECT COLUMN_NAME, DATA_TYPE FROM information_schema.columns WHERE TABLE_NAME = '${tableName}' AND TABLE_SCHEMA = DATABASE()` : `PRAGMA table_info(${tableName})`; const result = await this.execute(sql); const rows = result.rows || result; const schema = { columns: {} }; rows.forEach(row => { const columnName = row.column_name || row.COLUMN_NAME || row.name; const columnType = row.data_type || row.DATA_TYPE || row.type; if (!columnName || columnName === '_id' || columnName.startsWith('created_') || columnName.startsWith('updated_')) return; schema.columns[columnName] = String(columnType || '').toUpperCase(); }); return schema; },
        buildInsert(tableName, documents, options = {}) { return queryBuilder.insertMany(tableName, documents, database, options); },
        buildFind(tableName, query, projection) { return queryBuilder.collection(tableName, database).find(query, projection).toSQL(); },
        buildFindWithOptions(tableName, query, projection, options = {}) { let q = queryBuilder.collection(tableName, database).find(query, projection); if (options.sort) q = q.sort(options.sort); if (options.limit != null) q = q.limit(options.limit); if (options.skip != null) q = q.skip(options.skip); if (options.distinct) q = q.distinct(); return q.toSQL(); },
        buildUpdateOne(tableName, query, update) { return queryBuilder.collection(tableName, database).updateOne(query, update); },
        buildUpdateMany(tableName, query, update) { return queryBuilder.collection(tableName, database).updateMany(query, update); },
        buildDeleteOne(tableName, query) { return queryBuilder.collection(tableName, database).deleteOne(query); },
        buildDeleteMany(tableName, query) { return queryBuilder.collection(tableName, database).deleteMany(query, { allowDeleteAll: true }); },
        buildCount(tableName, query) { return queryBuilder.collection(tableName, database).countDocuments(query); },
        buildAggregate(tableName, pipeline) { return queryBuilder.aggregate(pipeline)(tableName, database); },
        buildDistinct(tableName, field, query) { return queryBuilder.collection(tableName, database).distinct(field, query); },
        collection(name, config) { return new SQLCollection(name, this, config); },
        async dropCollection(name) { const sql = this.database === 'pg' ? `DROP TABLE IF EXISTS ${name} CASCADE` : `DROP TABLE IF EXISTS ${name}`; await this.execute(sql); return { acknowledged: true }; },
        async listCollections() { const sql = database === 'pg' ? `SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'` : database === 'mysql' ? `SELECT TABLE_NAME FROM information_schema.tables WHERE TABLE_SCHEMA = DATABASE()` : `SELECT name FROM sqlite_master WHERE type='table'`; const result = await this.execute(sql); const rows = result.rows || result; return rows.map(row => row.table_name || row.TABLE_NAME || row.name); },
        async createTableWithSchema(tableName, schema = {}) { let mapped = schema; if (schema && schema.properties) { const props = schema.properties; const req = Array.isArray(schema.required) ? new Set(schema.required) : new Set(); const toType = (p) => { if (p.type === 'string') { if (p.format === 'date-time') return database === 'pg' ? 'TIMESTAMP' : database === 'mysql' ? 'DATETIME' : 'TEXT'; if (p.maxLength && database === 'pg') return `VARCHAR(${p.maxLength})`; if (p.maxLength && database === 'mysql') return `VARCHAR(${p.maxLength})`; return database === 'pg' ? 'TEXT' : database === 'mysql' ? 'VARCHAR(255)' : 'TEXT'; } if (p.type === 'integer') return database === 'pg' ? 'INTEGER' : database === 'mysql' ? 'INT' : 'INTEGER'; if (p.type === 'number') return database === 'pg' ? 'DECIMAL(20,6)' : database === 'mysql' ? 'DECIMAL(20,6)' : 'REAL'; if (p.type === 'boolean') return database === 'pg' ? 'BOOLEAN' : database === 'mysql' ? 'TINYINT(1)' : 'INTEGER'; if (p.type === 'array' || p.type === 'object') return database === 'pg' ? 'JSONB' : database === 'mysql' ? 'JSON' : 'TEXT'; return database === 'pg' ? 'TEXT' : database === 'mysql' ? 'VARCHAR(255)' : 'TEXT'; }; const m = {}; for (const [k, p] of Object.entries(props)) { m[k] = { type: toType(p), required: req.has(k), default: p.default }; } mapped = m; } const coll = new SQLCollection(tableName, this, { schema: mapped }); await coll.createTable({}); return { acknowledged: true }; },
        async addColumn(tableName, columnName, type, options = {}) { const nullable = options.required ? ' NOT NULL' : ''; const unique = options.unique ? ' UNIQUE' : ''; const def = options.default !== undefined ? ` DEFAULT ${escapeValue(typeof options.default === 'function' ? options.default() : options.default, database)}` : ''; const addKw = database === 'mysql' ? 'ADD' : 'ADD COLUMN'; const sql = `ALTER TABLE ${tableName} ${addKw} ${columnName} ${type}${nullable}${unique}${def}`; await this.execute(sql); return { acknowledged: true }; },
        async renameColumn(tableName, oldName, newName) { const sql = `ALTER TABLE ${tableName} RENAME COLUMN ${oldName} TO ${newName}`; await this.execute(sql); return { acknowledged: true }; },
        async modifyColumn(tableName, columnName, newType, options = {}) { if (database === 'sqlite') throw new Error('SQLite does not support MODIFY COLUMN'); const nullable = options.required ? ' NOT NULL' : ''; const unique = options.unique ? ' UNIQUE' : ''; const def = options.default !== undefined ? ` DEFAULT ${escapeValue(typeof options.default === 'function' ? options.default() : options.default, database)}` : ''; const sql = database === 'pg' ? `ALTER TABLE ${tableName} ALTER COLUMN ${columnName} TYPE ${newType}` : `ALTER TABLE ${tableName} MODIFY COLUMN ${columnName} ${newType}${nullable}${unique}${def}`; await this.execute(sql); return { acknowledged: true }; },
        async dropIndex(tableName, indexName) { const sql = database === 'pg' ? `DROP INDEX IF EXISTS ${indexName}` : database === 'mysql' ? `DROP INDEX ${indexName} ON ${tableName}` : `DROP INDEX IF EXISTS ${indexName}`; await this.execute(sql); return { acknowledged: true }; },
        async dropColumn(tableName, columnName) { return this.collection(tableName).dropColumn(columnName); },
        async createIndex(tableName, field, options = {}) { return this.collection(tableName).createIndex(field, options); },
    };
    return adapter;
};

export const createSchemalessAdapter = (client, database = 'memory', { debug = false } = {}) => {
    const qb = createQueryBuilder({ filterOps, exprOps, updateOps, stageHandlers, debug });
    let execute;
    if (database === 'memory') {
        return createMemorySchemaless();
    } else if (database === 'sqlite') {
        execute = async (sql, params = []) => {
            const up = sql.trim().toUpperCase();
            if (up.startsWith('SELECT') || up.startsWith('PRAGMA')) return client.prepare(sql).all(...params);
            const info = client.prepare(sql).run(...params);
            return { changes: info.changes, lastInsertRowid: info.lastInsertRowid };
        };
    } else if (database === 'pg') {
        execute = async (sql, params = []) => {
            const res = await client.query(sql, params);
            return { rows: res.rows, rowCount: res.rowCount };
        };
    } else if (database === 'mysql') {
        execute = async (sql, params = []) => {
            return await client.execute(sql, params);
        };
    } else {
        throw new Error('Unsupported database');
    }
    return { adapter: createSQLAdapter({ database, execute, queryBuilder: qb, debug }), client };
};
