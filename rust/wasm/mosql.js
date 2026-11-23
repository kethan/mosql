/**
 * umosql — Rust/WASM bridge (zero dependencies).
 *
 *   import { loadMosql } from './mosql.js';
 *   const mosql = await loadMosql('./mosql.wasm');
 *   mosql.collection('users', 'pg').find({ age: { $gte: 18 } }).toSQL();
 *
 *   // stateful in-memory engine — documents stay inside wasm:
 *   const mem = mosql.openMemory();
 *   mem.insert([{ name: 'Ada', age: 36 }]);
 *   mem.find({ age: { $gte: 18 } }, { sort: { age: -1 }, limit: 10 });
 *
 * The ABI is a handful of functions and no imports, so the same module loads in
 * Node, Deno, browsers and edge runtimes.
 */

export async function loadMosql(source) {
  let bytes;
  if (source instanceof Uint8Array) {
    bytes = source;
  } else if (typeof source === 'string' || source instanceof URL) {
    if (typeof process !== 'undefined' && process.versions?.node) {
      const { readFile } = await import('node:fs/promises');
      bytes = await readFile(source);
    } else {
      const res = await fetch(source);
      if (!res.ok) throw new Error(`mosql: failed to load ${source}: ${res.status}`);
      bytes = new Uint8Array(await res.arrayBuffer());
    }
  } else {
    throw new Error('mosql: loadMosql(url | URL | Uint8Array)');
  }

  const { instance } = await WebAssembly.instantiate(bytes, {});
  return new Mosql(instance.exports);
}

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
  insert(docs) { return this.mosql._call('mosql_mem_insert', this.handle, { docs }).result; }
  find(query, options = {}) { return this.mosql._call('mosql_mem_find', this.handle, { query, ...options }).result; }
  aggregate(pipeline) { return this.mosql._call('mosql_mem_aggregate', this.handle, { pipeline }).result; }
  close() { this.mosql.exports.mosql_mem_close(this.handle); }
}

class Mosql {
  constructor(exports) {
    this.exports = exports;
    this.encoder = new TextEncoder();
    this.decoder = new TextDecoder();
    // While no collection is open, every call can release its allocations,
    // which keeps stateless SQL building on the fast bump path.
    this._persistent = false;
  }

  _call(fn, handle, req) {
    const bytes = this.encoder.encode(JSON.stringify(req));
    const ptr = this.exports.mosql_alloc(bytes.length);
    if (ptr === 0) throw new Error('mosql: out of memory');
    new Uint8Array(this.exports.memory.buffer, ptr, bytes.length).set(bytes);

    const out = handle === undefined
      ? this.exports[fn](ptr, bytes.length)
      : this.exports[fn](handle, ptr, bytes.length);
    this.exports.mosql_free(ptr, bytes.length);

    const view = new DataView(this.exports.memory.buffer);
    const len = view.getUint32(out, true);
    const text = this.decoder.decode(new Uint8Array(this.exports.memory.buffer, out + 4, len));
    this.exports.mosql_free(out, 4 + len);

    const res = JSON.parse(text);
    if (!res.ok) throw new Error(res.error);
    return res;
  }

  build(req) {
    const res = this._call('mosql_build', undefined, req);
    if (!this._persistent) this.exports.mosql_reset();
    return res.sql !== undefined ? res.sql : res.result;
  }

  openMemory() {
    this._persistent = true;
    return new MemoryCollection(this, this.exports.mosql_mem_open());
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

export default loadMosql;
