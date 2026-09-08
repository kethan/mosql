// ============================================
// IN-MEMORY (MONGO-STYLE) ENGINE
// ============================================
// Public surface for `umosql/memory`: the standalone in-memory query engine
// plus the schemaless adapter built on top of it.

export {
    collection,
    filter,
    expression,
    aggregate,
    project,
    FindQuery,
    extend,
    db,
    Database,
    Collection,
    createMemoryDB,
    filterOps,
    exprOps,
    updateOps,
    stageOps,
    deepEquals,
    getPath,
    setPath,
    deletePath,
    clone,
} from './adapter/memory/memory.js';

export { createMemorySchemaless } from './adapter/memory/adapter.js';

export { default } from './adapter/memory/memory.js';
