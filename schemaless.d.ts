export type DBType = 'sqlite' | 'pg' | 'mysql';
export type SchemalessBackend = 'memory' | DBType;

/** Field definition accepted by `collection(name, { schema })`. */
export interface SchemaField {
  /** SQL type, e.g. `'TEXT'`, `'INTEGER'`, `'JSONB'`. Inferred when omitted. */
  type?: string;
  required?: boolean;
  unique?: boolean;
  default?: any | (() => any);
  /** Stripped from every document returned by the adapter. */
  hidden?: boolean;
}

export type Schema = Record<string, string | SchemaField>;

export interface CollectionOptions {
  schema?: Schema;
  /** Let `$set` add new columns on update (default `true`). */
  migrateOnUpdate?: boolean;
  /** Id column name (default `_id`). */
  idColumn?: string;
  /** `auto` = SQL identity, `mongo` = 24-char hex, `custom` = `idGenerator`. */
  idStrategy?: 'auto' | 'mongo' | 'custom';
  idGenerator?: () => string | number;
}

export interface FindOptions {
  select?: Record<string, any> | string | string[] | null;
  sort?: Record<string, 1 | -1 | 'asc' | 'desc'>;
  skip?: number;
  offset?: number;
  limit?: number;
  distinct?: boolean;
}

export interface FindManyOptions extends FindOptions {
  filter?: Record<string, any>;
  projection?: Record<string, any> | string | string[] | null;
  page?: number;
  pageIndex?: number;
  pageSize?: number;
  includeTotal?: boolean;
}

/** Chainable cursor. Note that `collection.find()` is async, so it must be awaited first. */
export interface Cursor<T = any> {
  sort(obj: Record<string, 1 | -1 | 'asc' | 'desc'>): Cursor<T>;
  skip(n: number): Cursor<T>;
  limit(n: number): Cursor<T>;
  distinct(): Cursor<T>;
  toArray(): Promise<T[]>;
  count(): Promise<number>;
}

export interface WriteResult {
  acknowledged: boolean;
  insertedId?: any;
  insertedIds?: any[];
  matchedCount?: number;
  modifiedCount?: number;
  deletedCount?: number;
  upserted?: boolean;
  upsertedId?: any;
}

export interface SchemalessCollection<T = any> {
  name: string;
  database: SchemalessBackend;
  initialize(): Promise<void>;
  insertOne(doc: Record<string, any>): Promise<WriteResult>;
  insertMany(docs: Record<string, any>[]): Promise<WriteResult>;
  find(query?: Record<string, any>, projection?: Record<string, any> | string | string[] | null, options?: FindOptions): Promise<Cursor<T>>;
  findOne(query?: Record<string, any>, projection?: Record<string, any> | string | string[] | null): Promise<T | null>;
  /** SQL-only pagination helper: `{ items, total?, page?, pageSize? }`. */
  findMany(options?: FindManyOptions): Promise<{ items: T[]; total?: number; page?: number; pageSize?: number }>;
  updateOne(query: Record<string, any>, update: Record<string, any>): Promise<WriteResult>;
  updateMany(query: Record<string, any>, update: Record<string, any>): Promise<WriteResult>;
  upsertOne(query: Record<string, any>, update: Record<string, any>, insertDoc?: Record<string, any>): Promise<WriteResult>;
  deleteOne(query: Record<string, any>): Promise<WriteResult>;
  deleteMany(query?: Record<string, any>): Promise<WriteResult>;
  countDocuments(query?: Record<string, any>): Promise<number>;
  estimatedDocumentCount(): Promise<number>;
  distinct(field: string, query?: Record<string, any>): Promise<any[]>;
  aggregate(pipeline: any[]): Promise<T[]>;
  createIndex(field: string, options?: { name?: string; unique?: boolean; type?: string }): Promise<{ acknowledged: boolean }>;
  /** pg/mysql only. */
  dropColumn(column: string): Promise<{ acknowledged: boolean }>;
}

export interface SchemalessAdapter {
  database: SchemalessBackend;
  execute(sql: string, params?: any[]): Promise<{ rows?: any[]; rowCount?: number; affectedRows?: number; insertId?: any; lastInsertRowid?: any; fields?: any }>;
  collection<T = any>(name: string, options?: CollectionOptions): SchemalessCollection<T>;
  tableExists(tableName: string): Promise<boolean>;
  getTableSchema(tableName: string): Promise<{ columns: Record<string, string> }>;
  listCollections(): Promise<string[]>;
  dropCollection(name: string): Promise<{ acknowledged: boolean }>;
  /** SQL only. */
  createTableWithSchema(tableName: string, schema?: Schema | Record<string, any>): Promise<{ acknowledged: boolean }>;
  addColumn(tableName: string, columnName: string, type: string, options?: { required?: boolean; unique?: boolean; default?: any }): Promise<{ acknowledged: boolean }>;
  renameColumn(tableName: string, oldName: string, newName: string): Promise<{ acknowledged: boolean }>;
  modifyColumn(tableName: string, columnName: string, newType: string, options?: { required?: boolean; unique?: boolean; default?: any }): Promise<{ acknowledged: boolean }>;
  createIndex(tableName: string, field: string, options?: { name?: string; unique?: boolean; type?: string }): Promise<{ acknowledged: boolean }>;
  dropIndex(tableName: string, indexName: string): Promise<{ acknowledged: boolean }>;
  dropColumn(tableName: string, columnName: string): Promise<{ acknowledged: boolean }>;
  buildInsert(tableName: string, documents: Record<string, any>[], options?: { returning?: string[] | '*' }): string;
  buildFind(tableName: string, query?: Record<string, any>, projection?: any): string;
  buildFindWithOptions(tableName: string, query: Record<string, any>, projection: any, options?: FindOptions): string;
  buildUpdateOne(tableName: string, query: Record<string, any>, update: Record<string, any>): string;
  buildUpdateMany(tableName: string, query: Record<string, any>, update: Record<string, any>): string;
  buildDeleteOne(tableName: string, query: Record<string, any>): string;
  buildDeleteMany(tableName: string, query: Record<string, any>): string;
  buildCount(tableName: string, query: Record<string, any>): string;
  buildAggregate(tableName: string, pipeline: any[]): string;
  buildDistinct(tableName: string, field: string, query?: Record<string, any>): string;
}

export interface SQLAdapterConfig {
  database?: DBType;
  /** Runs a statement and returns driver rows (`pg`-like, `mysql2`-like or a plain array). */
  execute: (sql: string, params?: any[]) => any | Promise<any>;
  /** `createQueryBuilder({ filterOps, exprOps, updateOps, stageHandlers })`. */
  queryBuilder: Record<string, any>;
  debug?: boolean;
}

export interface SchemalessAdapterResult {
  adapter: SchemalessAdapter;
  /** Raw driver handle for SQL backends. */
  client?: any;
  /** In-memory `Database` for the `memory` backend. */
  database?: any;
}

/**
 * Wraps a driver instance (`better-sqlite3`, `pg.Client`, `mysql2` connection)
 * in the unified schemaless adapter. `database` defaults to `memory`, in which
 * case `client` is ignored.
 */
export function createSchemalessAdapter(client?: any, database?: SchemalessBackend, options?: { debug?: boolean }): SchemalessAdapterResult;

/** Builds an adapter on top of any `execute(sql, params)` function (serverless drivers). */
export function createSQLAdapter(config: SQLAdapterConfig): SchemalessAdapter;

/** Standalone in-memory adapter, no driver required. */
export function createMemorySchemaless(): { adapter: SchemalessAdapter; database: any };

declare const schemalessDefault: typeof createSchemalessAdapter;
export default schemalessDefault;
