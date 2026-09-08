import type { SchemalessAdapter, SchemalessCollection, SchemalessBackend, CollectionOptions, Schema } from './schemaless.js';

export type ClientType = 'memory' | 'sqlite' | 'pg' | 'mysql' | 'mongodb' | 'sql';

/** Supported `createSchemalessClient` types. */
export const CLIENT_TYPES: ClientType[];

export interface ClientConfig {
  /** Reuse an existing driver instance instead of creating one. */
  client?: any;
  /** Alias of `client` for MySQL (`mysql2` connection). */
  conn?: any;

  /** sqlite: file to open (default `:memory:`). */
  filename?: string;

  /** pg / mysql / mongodb connection settings. */
  host?: string;
  port?: number;
  user?: string;
  password?: string;
  database?: string;
  db?: string;

  /** pg: full connection string (Neon, Supabase, ...). */
  connectionString?: string;
  url?: string;
  ssl?: any;

  /** mysql: full connection URI. */
  uri?: string;

  /**
   * Bring your own driver module (e.g. `await import('pg')`, or a stub in tests)
   * so umosql never resolves the package itself. Nothing is bundled.
   */
  driver?: any;
  /** Extra options forwarded to the driver constructor/connection factory. */
  driverOptions?: Record<string, any>;

  /** `sql`: executor for serverless drivers, `(sql, params) => rows`. */
  executor?: (sql: string, params?: any[]) => any | Promise<any>;
  /** `sql`: optional teardown used by `client.close()`. */
  close?: () => any | Promise<any>;

  /** Log every generated statement. */
  debug?: boolean;

  /** Default id handling for `client.db(name)` collections. */
  id?: string;
  idColumn?: string;
  idStrategy?: 'auto' | 'mongo' | 'custom';
  idGenerator?: () => string | number;

  /** mongodb / schemaless passthrough. */
  schema?: Schema;
  migrateOnUpdate?: boolean;
  [key: string]: any;
}

export interface DatabaseHandle {
  /** Name passed to `client.db(name)`. */
  name?: string;
  collection<T = any>(name: string, options?: CollectionOptions): SchemalessCollection<T>;
  adapter: SchemalessAdapter;
}

export interface SchemalessClient {
  type: ClientType;
  /** Raw driver instance (`null` for the `sql` executor backend). */
  raw: any;
  adapter: SchemalessAdapter;
  /** Scoped collection factory; `options` sets id handling for every collection. */
  db(name?: string, options?: CollectionOptions & { id?: string }): DatabaseHandle;
  /** Releases the underlying driver/connection. Safe to call more than once. */
  close(): Promise<void>;
}

/**
 * One factory for every backend. Driver packages (`better-sqlite3`, `pg`,
 * `mysql2`, `mongodb`) are imported lazily, so only the backend you request
 * needs to be installed.
 *
 * ```js
 * const client = await createSchemalessClient('memory');
 * const users = client.db('app').collection('users');
 * await users.insertOne({ name: 'Alice' });
 * ```
 */
export function createSchemalessClient(type?: ClientType, config?: ClientConfig): Promise<SchemalessClient>;

/**
 * Lazily resolves a driver package and turns "not installed" into an actionable
 * error. Exposed so you can load drivers the same way in custom backends.
 */
export declare function loadDriver<T = any>(backend: string, specifier: string, load: () => Promise<T>): Promise<T>;

export { createSchemalessAdapter, createSQLAdapter, createMemorySchemaless } from './schemaless.js';
export type { SchemalessAdapter, SchemalessCollection, SchemalessBackend, CollectionOptions, Schema };

/** Optional MongoDB backend (requires the `mongodb` package). */
export declare function createMongoSchemaless(config?: ClientConfig): Promise<{ adapter: SchemalessAdapter; client: any }>;

declare const clientDefault: typeof createSchemalessClient;
export default clientDefault;
