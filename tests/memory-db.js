// Boots an ephemeral MySQL and runs the whole suite against it:
//
//   npm run db:memory
//
// This exists because MySQL, unlike PostgreSQL, has no WASM build - there is no way
// to run its server inside the Node process - so covering the MySQL specs means
// either Docker (`npm run db:up`) or a real mysqld. mysql-memory-server is the
// second one without anything left installed: it starts mysqld in a temporary data
// directory on a random free port and stops it with this process.
//
// The first run fetches the server binary from cdn.mysql.com and caches it; a mysqld
// already on PATH is reused instead. Both are optional: point MYSQL_HOST (and
// MYSQL_PORT / MYSQL_USER / MYSQL_DB) at a server you already have and this just
// forwards it to the suite. None of it is required to work on - `npm test` skips
// MySQL out loud when there is no server, which is why the specs read this exact set
// of variables through tests/common.js.
import { spawnSync } from 'child_process';

const configured = process.env.MYSQL_HOST != null && process.env.MYSQL_HOST !== '';

const runSuite = (env) => {
  const r = spawnSync(process.execPath, ['./tests/run-all.js'], { stdio: 'inherit', env: { ...process.env, ...env } });
  return r.status ?? 1;
};

if (configured) {
  console.log(`using the MySQL already reachable at ${process.env.MYSQL_HOST}:${process.env.MYSQL_PORT || 3306}`);
  process.exit(runSuite({}));
}

let createDB;
try {
  ({ createDB } = await import('mysql-memory-server'));
} catch {
  console.error('db:memory needs the dev dependency: npm i -D mysql-memory-server');
  console.error('or start a server some other way: npm run db:up, then npm test');
  process.exit(1);
}

// Two retries rather than the default ten: a machine that cannot reach the CDN should
// find that out now instead of after ten download attempts.
const db = await createDB({ version: '8.0.x', dbName: 'testdb', port: 0, downloadRetries: 2, logLevel: 'ERROR' })
  .catch((e) => {
    console.error(`db:memory could not start mysqld: ${String(e?.message || e).split('\n')[0]}`);
    console.error('the first run needs access to cdn.mysql.com to fetch the server binary.');
    console.error('otherwise: npm run db:up (Docker), or set MYSQL_HOST/MYSQL_PORT for a server you have.');
    process.exit(1);
  });

// The database is created with an empty password, which is fine for a server that
// lives only as long as this process and listens on localhost.
console.log(`MySQL ${db.mysql.version} on 127.0.0.1:${db.port} (database ${db.dbName}, user ${db.username}, empty password)`);

const code = runSuite({
  MYSQL_HOST: '127.0.0.1',
  MYSQL_PORT: String(db.port),
  MYSQL_USER: db.username,
  MYSQL_PASS: '',
  MYSQL_DB: db.dbName,
});

await db.stop();
process.exit(code);
