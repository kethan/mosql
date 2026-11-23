/**
 * umosql — native Rust bridge (Node-API, zero dependencies).
 *
 *   import { loadMosql } from './mosql.js';
 *   const mosql = loadMosql();
 *   mosql.collection('users', 'pg').find({ age: { $gte: 18 } }).toSQL();
 *
 *   // stateful in-memory engine — documents never leave Rust:
 *   const mem = mosql.openMemory();
 *   mem.insert([{ name: 'Ada', age: 36 }]);
 *   mem.find({ age: { $gte: 18 } }, { sort: { age: -1 }, limit: 10 });
 *
 * This mirrors `../wasm/mosql.js` exactly, so the same code runs against either
 * backend. Unlike the wasm bridge there is no JSON.stringify/parse and no copy
 * at the boundary: JS values are converted to Rust values in place.
 */

import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));

export function loadMosql() {
  return new Mosql(require(join(here, 'mosql.node')));
}

export default loadMosql;

class Find {
  constructor(mosql, table, db, query, projection) {
    this.mosql = mosql;
    this.table = table;
    this.db = db;
    this.query = query ?? {};
    this.projection = projection;
    this._sort = undefined;
    this._limit = undefined;
    this._skip = undefined;
    this._distinct = false;
    this._count = false;
  }
  select(projection) { this.projection = projection; return this; }
  sort(obj) { this._sort = obj; return this; }
  limit(n) { this._limit = n; return this; }
  skip(n) { this._skip = n; return this; }
  distinct() { this._distinct = true; return this; }
  count() { this._count = true; return this; }
  toSQL() {
    return this.mosql.build({
      op: 'find',
      table: this.table,
      db: this.db,
      query: this.query,
      projection: this.projection,
      sort: this._sort,
      limit: this._limit,
      skip: this._skip,
      distinct: this._distinct,
      count: this._count,
    });
  }
  toString() { return this.toSQL(); }
}

class MemoryCollection {
  constructor(mosql, handle) {
    this.mosql = mosql;
    this.handle = handle;
  }
  insert(docs) { return this.mosql.native.memoryInsert(this.handle, docs); }
  find(query, options = {}) { return this.mosql.native.memoryFind(this.handle, query, options); }
  aggregate(pipeline) { return this.mosql.native.memoryAggregate(this.handle, pipeline); }
  close() { this.mosql.native.memoryClose(this.handle); }
}

class Mosql {
  constructor(native) {
    this.native = native;
  }

  build(req) { return this.native.build(req); }

  openMemory() {
    return new MemoryCollection(this, this.native.memoryOpen());
  }

  collection(table, db = 'sqlite') {
    const m = this;
    const find = (query = {}, projection) => new Find(m, table, db, query, projection);
    return {
      find,
      findOne: (query = {}, projection) => find(query, projection).limit(1),
      insertOne: (doc, opts = {}) => m.build({ op: 'insertOne', table, db, doc, ...opts }),
      insertMany: (docs, opts = {}) => m.build({ op: 'insertMany', table, db, docs, ...opts }),
      updateOne: (query, update, opts = {}) => m.build({ op: 'updateOne', table, db, query, update, ...opts }),
      updateMany: (query, update, opts = {}) => m.build({ op: 'updateMany', table, db, query, update, ...opts }),
      deleteOne: (query, opts = {}) => m.build({ op: 'deleteOne', table, db, query, ...opts }),
      deleteMany: (query = {}, opts = {}) => m.build({ op: 'deleteMany', table, db, query, ...opts }),
      countDocuments: (query = {}) => m.build({ op: 'countDocuments', table, db, query }),
      distinct: (field, query = {}) => m.build({ op: 'distinct', table, db, field, query }),
      aggregate: (pipeline) => m.build({ op: 'aggregate', table, db, pipeline }),
    };
  }

  filter(query, db = 'sqlite') { return this.build({ op: 'filter', query, db }); }
  expression(expr, db = 'sqlite') { return this.build({ op: 'expression', expr, db }); }
  aggregate(pipeline, table, db = 'sqlite') { return this.build({ op: 'aggregate', table, db, pipeline }); }
}
