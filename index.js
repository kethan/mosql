export {
    createQueryBuilder,
    filterOps,
    exprOps,
    updateOps,
    stageHandlers,
    filter,
    expression,
    aggregate,
    insertMany,
    updateMany,
    deleteMany,
    collection,
    FindQuery,
    extend,
    db,
    validate,
    escape,
    jsonPath,
} from './src/index.js';

export { default } from './src/index.js';

// Adapters live in their own entry points so the core bundle stays small:
//   umosql/schemaless -> createSchemalessAdapter, createSQLAdapter
//   umosql/memory     -> in-memory engine + createMemorySchemaless
//   umosql/client     -> createSchemalessClient (memory/sqlite/pg/mysql/mongodb/sql)
