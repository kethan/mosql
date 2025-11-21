import dotenv from 'dotenv';
dotenv.config();

import { createSchemalessAdapter, createSQLAdapter } from './schemaless.js';
import { createQueryBuilder, filterOps, exprOps, updateOps, stageHandlers } from './index.js';

export const createSchemalessClient = async (type = 'sqlite', cfg = {}) => {
  let adapter;
  let raw;

  if (type === 'sqlite') {
    const client = cfg.client || (await import('better-sqlite3')).default(cfg.filename || ':memory:');
    const init = createSchemalessAdapter(client, 'sqlite');
    adapter = init.adapter;
    raw = client;
  } else if (type === 'pg') {
    const pkg = await import('pg');
    const Client = pkg.Client || pkg.default?.Client;
    const client = cfg.client || new Client({ host: cfg.host, port: cfg.port || 5432, user: cfg.user, password: cfg.password, database: cfg.database });
    await client.connect();
    const init = createSchemalessAdapter(client, 'pg');
    adapter = init.adapter;
    raw = client;
  } else if (type === 'mysql') {
    const mysql2 = await import('mysql2/promise');
    const conn = cfg.conn || await mysql2.createConnection({ host: cfg.host, user: cfg.user, password: cfg.password, database: cfg.database });
    const init = createSchemalessAdapter(conn, 'mysql');
    adapter = init.adapter;
    raw = conn;
  } else if (type === 'sql' && typeof cfg.executor === 'function') {
    const qb = createQueryBuilder({ filterOps, exprOps, updateOps, stageHandlers });
    adapter = createSQLAdapter({ database: cfg.database || 'sqlite', execute: cfg.executor, queryBuilder: qb });
    raw = null;
  } else {
    throw new Error('Unknown type');
  }

  const client = {
    type,
    raw,
    db(name, options = {}) {
      const idColumn = options.id || options.idColumn || '_id';
      const idStrategy = options.idStrategy || (type === 'mongodb' ? 'mongo' : 'auto');
      const idGenerator = options.idGenerator || null;
      return {
        collection: (col, colOptions = {}) => adapter.collection(col, {
          idColumn,
          idStrategy,
          idGenerator,
          ...colOptions,
        }),
        adapter,
      };
    },
    adapter,
  };

  return client;
};

export default createSchemalessClient;