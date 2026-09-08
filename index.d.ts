export type DBType = 'sqlite' | 'pg' | 'mysql';

export interface FindQuery {
  select(projection?: Record<string, any> | string | string[]): FindQuery;
  sort(obj: Record<string, 1 | -1 | 'asc' | 'desc'>): FindQuery;
  skip(n: number): FindQuery;
  limit(n: number): FindQuery;
  distinct(): FindQuery;
  count(): FindQuery;
  toSQL(): string;
  toString(): string;
}

export interface CollectionApi {
  find(query?: Record<string, any>, projection?: Record<string, any> | string | string[]): FindQuery;
  findOne(query?: Record<string, any>, projection?: Record<string, any> | string | string[]): FindQuery;
  insertOne(doc: Record<string, any>, opts?: CrudOptions): string;
  insertMany(docs: Record<string, any>[], opts?: CrudOptions): string;
  /** Rewrites at most one row on every dialect. */
  updateOne(query: Record<string, any>, update: Record<string, any>, opts?: CrudOptions): string;
  updateMany(query: Record<string, any>, update: Record<string, any>, opts?: CrudOptions): string;
  /** Deletes at most one row on every dialect. */
  deleteOne(query: Record<string, any>, opts?: CrudOptions): string;
  deleteMany(query?: Record<string, any>, opts?: CrudOptions): string;
  countDocuments(query?: Record<string, any>): string;
  distinct(field: string, query?: Record<string, any>): string;
  aggregate(pipeline: any[]): (table: string, db?: DBType) => string;
}

export interface ExtendApi {
  filter(ops: Record<string, Function>): Record<string, Function>;
  expression(ops: Record<string, Function>): Record<string, Function>;
  update(ops: Record<string, Function>): Record<string, Function>;
  stage(ops: Record<string, Function>): Record<string, Function>;
}

export interface QueryBuilderApi {
  filter(q: Record<string, any>, db?: DBType): string;
  expression(x: any, db?: DBType): string;
  aggregate(pipeline: any[]): (table: string, db?: DBType) => string;
  insertMany(table: string, docs: Record<string, any>[], db?: DBType, opts?: CrudOptions): string;
  updateMany(table: string, query: Record<string, any>, update: Record<string, any>, db?: DBType, opts?: CrudOptions): string;
  deleteMany(table: string, query: Record<string, any>, db?: DBType, opts?: CrudOptions): string;
  collection(name: string, db?: DBType): CollectionApi;
  FindQuery: new (table: string, query: Record<string, any>, projection?: any, db?: DBType) => FindQuery;
  extend: ExtendApi;
  db(collectionName: string, database?: DBType): CollectionApi;
}

export interface QueryBuilderConfig {
  filterOps?: Record<string, Function>;
  exprOps?: Record<string, Function>;
  updateOps?: Record<string, Function>;
  stageHandlers?: Record<string, Function>;
  debug?: boolean;
}

export interface ValidateApi {
  col(name: string, db?: DBType): string;
  alias(str: any): string;
  arr(value: any, op: string): any[];
  int(value: any, op: string): number;
  /** Numbers used as bare SQL literals (`$mod`); rejects anything not finite. */
  num(value: any, op: string): number | bigint;
}

/** Options shared by the CRUD builders. */
export interface CrudOptions {
  returning?: string[] | '*';
  allowDeleteAll?: boolean;
  /**
   * Restrict the statement to a single row. `updateOne`/`deleteOne` set it;
   * MySQL uses `LIMIT 1`, SQLite/PostgreSQL a `rowid`/`ctid` sub-select, since
   * neither dialect supports LIMIT on DML.
   */
  limitOne?: boolean;
}

/** Operator tables, usable as-is or as a base for a custom build. */
export const filterOps: Record<string, (value: any, db?: DBType, field?: string) => string>;
export const exprOps: Record<string, (args: any[], ctx?: any) => string>;
export const updateOps: Record<string, (fields: any, db?: DBType) => string | string[]>;
export const stageHandlers: Record<string, (spec: any, state: any, db?: DBType, helpers?: any) => void>;

/** Builds an isolated query builder with a custom operator subset. */
export function createQueryBuilder(config?: QueryBuilderConfig): QueryBuilderApi;

export const validate: ValidateApi;
export function escape(value: any, db?: DBType): string;
export function jsonPath(path: string, db?: DBType, castType?: 'numeric' | 'int' | 'boolean'): string;

export function filter(q: Record<string, any>, db?: DBType): string;
export function expression(x: any, db?: DBType): string;
export function aggregate(pipeline: any[]): (table: string, db?: DBType) => string;
export function insertMany(table: string, docs: Record<string, any>[], db?: DBType, opts?: { returning?: string[] | '*' }): string;
export function updateMany(table: string, query: Record<string, any>, update: Record<string, any>, db?: DBType, opts?: { returning?: string[] | '*' }): string;
export function deleteMany(table: string, query: Record<string, any>, db?: DBType, opts?: { allowDeleteAll?: boolean; returning?: string[] | '*' }): string;
export function collection(name: string, db?: DBType): CollectionApi;
export const FindQuery: new (table: string, query: Record<string, any>, projection?: any, db?: DBType) => FindQuery;
export const extend: ExtendApi;
export function db(collectionName: string, database?: DBType): CollectionApi;

declare const defaultCollection: (name: string, db?: DBType) => CollectionApi;
export default defaultCollection;