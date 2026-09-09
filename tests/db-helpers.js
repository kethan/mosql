import dotenv from 'dotenv';

dotenv.config();

const log = (...args) => console.log('[db-helpers]', ...args);

/**
 * PostgreSQL client for specs.
 * - PG_HOST env set -> real `pg` Client (CI / local server), caller connects.
 * - otherwise       -> PGlite (WASM Postgres, in-memory) wrapped in a pg-like API
 *   (`connect`/`query`/`end`/`on`), so specs need no code changes between backends.
 * Returns null (with a warning) when no Postgres backend is available.
 */
export async function createPGClient() {
  const host = process.env.PG_HOST;
  if (host) {
    const { Client } = await import('pg');
    const client = new Client({
      host,
      port: Number(process.env.PG_PORT || 5432),
      user: process.env.PG_USER,
      password: process.env.PG_PASSWORD,
      database: process.env.PG_DB,
    });
    return { client, label: `pg-server:${host}` };
  }

  let PGlite;
  try {
    ({ PGlite } = await import('@electric-sql/pglite'));
  } catch (e) {
    log('PostgreSQL unavailable (no PG_HOST, and PGlite not installed — npm i -D @electric-sql/pglite), skipping pg tests:', e?.message || e);
    return null;
  }

  const db = await PGlite.create(); // in-memory WASM Postgres
  let ended = false;
  const client = {
    on() {},
    async connect() {},
    async query(sql, params = []) {
      const res = await db.query(sql, params);
      return {
        rows: res.rows,
        rowCount: res.affectedRows ?? res.rows?.length ?? 0,
        fields: res.fields,
      };
    },
    async end() {
      if (!ended) {
        ended = true;
        await db.close();
      }
    },
  };
  return { client, label: 'pglite(wasm-postgres)' };
}

/**
 * MySQL connection for specs.
 * - MYSQL_HOST env set -> real `mysql2` connection (CI / local server).
 * - otherwise          -> mysql-memory-server: ephemeral real mysqld (no Docker,
 *   binary downloaded once and cached), connected via the same `mysql2` API.
 *   Set MYSQL_EMBED=0 to disable the embedded fallback (e.g. in CI unit jobs).
 * Returns `{ conn, label, stop }` — `stop()` shuts the embedded server down —
 * or null (with a warning) when no MySQL backend is available.
 */
export async function createMySQLConn() {
  const host = process.env.MYSQL_HOST;
  if (host) {
    const mysql = await import('mysql2/promise');
    try {
      const conn = await mysql.createConnection({
        host,
        port: Number(process.env.MYSQL_PORT || 3306),
        user: process.env.MYSQL_USER,
        password: process.env.MYSQL_PASS,
        database: process.env.MYSQL_DB,
      });
      return { conn, label: `mysql-server:${host}`, stop: async () => {} };
    } catch (e) {
      log('MySQL server configured but unreachable, skipping mysql tests:', e?.message || e);
      return null;
    }
  }

  if (process.env.MYSQL_EMBED === '0') {
    log('MYSQL_EMBED=0, skipping embedded mysqld');
    return null;
  }

  let createDB;
  try {
    ({ createDB } = await import('mysql-memory-server'));
  } catch (e) {
    log('MySQL unavailable (no MYSQL_HOST, and mysql-memory-server not installed — npm i -D mysql-memory-server), skipping mysql tests:', e?.message || e);
    return null;
  }

  log('starting embedded mysqld (no Docker) — first run downloads the MySQL binary, please wait');
  try {
    const db = await createDB({ dbName: 'testdb' });
    const mysql = await import('mysql2/promise');
    const conn = await mysql.createConnection({
      host: '127.0.0.1',
      port: db.port,
      user: db.username,
      password: '',
      database: db.dbName,
    });
    return { conn, label: 'mysql-embedded(mysqld)', stop: () => db.stop() };
  } catch (e) {
    log('embedded mysqld failed to start, skipping mysql tests:', e?.message || e);
    return null;
  }
}
