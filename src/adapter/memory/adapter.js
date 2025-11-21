import { createMemoryDB, filterOps as mFilterOps, exprOps as mExprOps, updateOps as mUpdateOps, stageOps as mStageOps } from './memory.js';

export const createMemorySchemaless = () => {
  const mem = createMemoryDB({ filterOps: mFilterOps, exprOps: mExprOps, updateOps: mUpdateOps, stageOps: mStageOps });
  const database = mem.db('unified');

  const adapter = {
    database: 'memory',
    collection: (name, config = {}) => database.collection(name, undefined, config),
    listCollections: async () => database.listCollections(),
    dropCollection: async (name) => { database.dropCollection(name); return { acknowledged: true }; },
    createIndex: async () => ({ acknowledged: true }),
    dropIndex: async () => ({ acknowledged: true }),
  };

  return { adapter, database };
};

export default createMemorySchemaless;