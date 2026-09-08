export type MemoryFilter = Record<string, any>;
export type MemoryProjection = Record<string, any> | string | string[] | null;

export interface MemoryCollectionOptions {
  /** Id field name (default `_id`). */
  idColumn?: string;
  /** `auto` = incrementing number, `mongo` = 24-char hex, `custom` = `idGenerator`. */
  idStrategy?: 'auto' | 'mongo' | 'custom';
  idGenerator?: () => string | number;
}

/** Synchronous, chainable in-memory cursor. */
export declare class FindQuery<T = any> {
  constructor(data: T[], query?: MemoryFilter, projection?: MemoryProjection);
  sort(obj: Record<string, 1 | -1 | 'asc' | 'desc'>): FindQuery<T>;
  skip(n: number): FindQuery<T>;
  limit(n: number): FindQuery<T>;
  distinct(): FindQuery<T>;
  count(): FindQuery<T>;
  toArray(): T[];
  toString(): string;
}

export interface MemoryWriteResult {
  acknowledged: boolean;
  insertedId?: any;
  insertedIds?: any[];
  matchedCount?: number;
  modifiedCount?: number;
  deletedCount?: number;
  upsertedId?: any;
  upsertedCount?: number;
}

export declare class Collection<T = any> {
  constructor(name: string, data?: T[], options?: MemoryCollectionOptions);
  name: string;
  idColumn: string;
  idStrategy: 'auto' | 'mongo' | 'custom';
  find(query?: MemoryFilter, projection?: MemoryProjection, options?: { sort?: Record<string, any>; skip?: number; limit?: number }): FindQuery<T>;
  findOne(query?: MemoryFilter, projection?: MemoryProjection): T | null;
  findById(id: any): T | null;
  insertOne(doc: Record<string, any>): MemoryWriteResult;
  insertMany(docs: Record<string, any>[]): MemoryWriteResult;
  updateOne(query: MemoryFilter, update: Record<string, any>, options?: { upsert?: boolean }): MemoryWriteResult;
  updateMany(query: MemoryFilter, update: Record<string, any>, options?: { upsert?: boolean }): MemoryWriteResult;
  replaceOne(query: MemoryFilter, replacement: Record<string, any>, options?: { upsert?: boolean }): MemoryWriteResult;
  deleteOne(query: MemoryFilter): MemoryWriteResult;
  deleteMany(query?: MemoryFilter): MemoryWriteResult;
  countDocuments(query?: MemoryFilter): number;
  estimatedDocumentCount(): number;
  distinct(field: string, query?: MemoryFilter): any[];
  aggregate(pipeline: any[]): any[];
  drop(): { acknowledged: boolean };
  getAll(): T[];
  size(): number;
}

export declare class Database {
  constructor(name: string);
  name: string;
  collections: Map<string, Collection>;
  collection<T = any>(name: string, initData?: T[], options?: MemoryCollectionOptions): Collection<T>;
  dropCollection(name: string): boolean;
  listCollections(): string[];
  stats(name: string): { name: string; count: number; size: number; avgObjSize: number } | null;
  dropDatabase(): { acknowledged: boolean };
}

export interface MemoryExtendApi {
  filter(ops: Record<string, Function>): Record<string, Function>;
  expression(ops: Record<string, Function>): Record<string, Function>;
  update(ops: Record<string, Function>): Record<string, Function>;
  stage(ops: Record<string, Function>): Record<string, Function>;
}

export interface MemoryDBConfig {
  filterOps?: Record<string, Function>;
  exprOps?: Record<string, Function>;
  updateOps?: Record<string, Function>;
  stageOps?: Record<string, Function>;
}

export interface MemoryDB {
  filter(query: MemoryFilter): (doc: any) => boolean;
  expression(expr: any, ctx?: Record<string, any>): (doc: any) => any;
  aggregate(pipeline: any[]): (docs: any[]) => any[];
  project(doc: any, projection?: MemoryProjection): any;
  collection<T = any>(name: string, initData?: T[], options?: MemoryCollectionOptions): Collection<T>;
  FindQuery: typeof FindQuery;
  extend: MemoryExtendApi;
  db(name: string): Database;
  Database: typeof Database;
  Collection: typeof Collection;
}

/** Builds an isolated in-memory engine with an optional operator subset. */
export function createMemoryDB(config?: MemoryDBConfig): MemoryDB;

/** In-memory adapter exposing the same collection API as the SQL adapters (async). */
export function createMemorySchemaless(): { adapter: any; database: Database };

export const filterOps: Record<string, Function>;
export const exprOps: Record<string, Function>;
export const updateOps: Record<string, Function>;
export const stageOps: Record<string, Function>;

export const filter: MemoryDB['filter'];
export const expression: MemoryDB['expression'];
export const aggregate: MemoryDB['aggregate'];
export const project: MemoryDB['project'];
export const extend: MemoryDB['extend'];
export const collection: (name: string, initData?: any[], options?: MemoryCollectionOptions) => Collection;
export const db: (name: string) => Database;

export function deepEquals(a: any, b: any): boolean;
export function getPath(obj: any, path: string | string[]): any;
export function setPath(obj: any, path: string | string[], value: any): void;
export function deletePath(obj: any, path: string | string[]): void;
export function clone<T>(value: T): T;

declare const memoryDefault: typeof collection;
export default memoryDefault;
