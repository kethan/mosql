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
  insertOne(doc: Record<string, any>, opts?: { returning?: string[] | '*' }): string;
  insertMany(docs: Record<string, any>[], opts?: { returning?: string[] | '*' }): string;
  updateOne(query: Record<string, any>, update: Record<string, any>, opts?: { returning?: string[] | '*' }): string;
  updateMany(query: Record<string, any>, update: Record<string, any>, opts?: { returning?: string[] | '*' }): string;
  deleteOne(query: Record<string, any>, opts?: { allowDeleteAll?: boolean; returning?: string[] | '*' }): string;
  deleteMany(query?: Record<string, any>, opts?: { allowDeleteAll?: boolean; returning?: string[] | '*' }): string;
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
  insertMany(table: string, docs: Record<string, any>[], db?: DBType, opts?: { returning?: string[] | '*' }): string;
  updateMany(table: string, query: Record<string, any>, update: Record<string, any>, db?: DBType, opts?: { returning?: string[] | '*' }): string;
  deleteMany(table: string, query: Record<string, any>, db?: DBType, opts?: { allowDeleteAll?: boolean; returning?: string[] | '*' }): string;
  collection(name: string, db?: DBType): CollectionApi;
  FindQuery: new (table: string, query: Record<string, any>, projection?: any, db?: DBType) => FindQuery;
  extend: ExtendApi;
  db(collectionName: string, database?: DBType): CollectionApi;
}

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