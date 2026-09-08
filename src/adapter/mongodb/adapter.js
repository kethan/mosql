import { loadEnv } from '../../env.js';
import { loadDriver, pick } from '../../drivers.js';

export const createMongoSchemaless = async (config = {}) => {
  await loadEnv();
  // `mongodb` is never bundled: inject it via `config.driver`, or install it and
  // let this lazy import pick it up.
  const driver = config.driver || await loadDriver('mongodb', 'mongodb', () => import('mongodb'));
  const MongoClient = pick(driver, 'MongoClient');
  if (typeof MongoClient !== 'function') throw new Error("umosql: could not find MongoClient in the 'mongodb' driver (pass { driver })");
  const host = config.host || process.env.MONGO_HOST || 'localhost';
  const port = config.port || process.env.MONGO_PORT || 27017;
  const user = config.user || process.env.MONGO_USER;
  const password = config.password || process.env.MONGO_PASSWORD;
  const database = config.database || config.db || process.env.MONGO_DB || 'test';
  const strict = true;

  const auth = user && password ? `${encodeURIComponent(user)}:${encodeURIComponent(password)}@` : '';
  const uri = `mongodb://${auth}${host}:${port}`;
  const client = config.client || new MongoClient(uri, { serverSelectionTimeoutMS: 5000, ...(config.driverOptions || {}) });
  if (!config.client) await client.connect();
  const db = client.db(database);

  class MongoCollection {
    constructor(name, options = {}) { this.name = name; this.options = options || {}; }
    async insertOne(doc) { const r = await db.collection(this.name).insertOne(doc); return { acknowledged: r.acknowledged, insertedId: r.insertedId } }
    async insertMany(docs) { const r = await db.collection(this.name).insertMany(docs); return { acknowledged: r.acknowledged, insertedIds: Object.values(r.insertedIds) } }
    find(query = {}, projection = null, options = {}) {
      const cursor = db.collection(this.name).find(query, { projection });
      if (options.sort) cursor.sort(options.sort);
      if (options.skip != null) cursor.skip(options.skip);
      if (options.limit != null) cursor.limit(options.limit);
      const api = {
        sort: (obj) => { cursor.sort(obj); return api; },
        skip: (n) => { cursor.skip(n); return api; },
        limit: (n) => { cursor.limit(n); return api; },
        toArray: async () => await cursor.toArray(),
        count: async () => await cursor.count()
      };
      return api;
    }
    async findOne(query = {}, projection = null) { return await db.collection(this.name).findOne(query, { projection }); }
    async updateOne(filter, update, options = {}) { const r = await db.collection(this.name).updateOne(filter, update, options); return { acknowledged: r.acknowledged, matchedCount: r.matchedCount, modifiedCount: r.modifiedCount, upsertedId: r.upsertedId ?? null, upsertedCount: r.upsertedId ? 1 : 0 } }
    async updateMany(filter, update, options = {}) { const r = await db.collection(this.name).updateMany(filter, update, options); return { acknowledged: r.acknowledged, matchedCount: r.matchedCount, modifiedCount: r.modifiedCount } }
    async upsertOne(filter, update) { const r = await db.collection(this.name).updateOne(filter, update, { upsert: true }); return { acknowledged: r.acknowledged, upserted: !!r.upsertedId, upsertedId: r.upsertedId ?? null } }
    async deleteOne(filter) { const r = await db.collection(this.name).deleteOne(filter); return { acknowledged: r.acknowledged, deletedCount: r.deletedCount } }
    async deleteMany(filter = {}) { const r = await db.collection(this.name).deleteMany(filter); return { acknowledged: r.acknowledged, deletedCount: r.deletedCount } }
    async countDocuments(filter = {}) { return await db.collection(this.name).countDocuments(filter); }
    async estimatedDocumentCount() { return await db.collection(this.name).estimatedDocumentCount(); }
    async distinct(field, filter = {}) { return await db.collection(this.name).distinct(field, filter); }
    async aggregate(pipeline = []) { return await db.collection(this.name).aggregate(pipeline).toArray(); }
    async createIndex(field, options = {}) { const r = await db.collection(this.name).createIndex(field, options); return { acknowledged: !!r } }
  }

  const adapter = {
    database: 'mongodb',
    collection: (name, options = {}) => new MongoCollection(name, options),
    listCollections: async () => (await db.listCollections().toArray()).map(c => c.name),
    dropCollection: async (name) => { await db.collection(name).drop(); return { acknowledged: true }; },
    createIndex: async (tableName, field, options = {}) => new MongoCollection(tableName).createIndex(field, options),
  };

  return { adapter, client };
};

export default createMongoSchemaless;