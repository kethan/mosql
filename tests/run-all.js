import fs from 'fs';
import path from 'path';
import { pathToFileURL, fileURLToPath } from 'url';
import { getSkipCount } from './common.js';

const here = path.dirname(fileURLToPath(import.meta.url));

// Every *.spec.js in the tree runs in one process. Files are written so they
// also work under `bun test` (see common.js), so nothing is excluded here:
// "unified" runtime specs degrade to SQLite/Memory and SKIP the backends that
// are not reachable in the current environment (pg/mysql/mongodb are enabled
// via PG_HOST / MYSQL_HOST / MONGO_HOST, or via the embedded PGlite / mysqld
// fallbacks in db-helpers.js).
const isSpec = (f) => f.endsWith('.spec.js');

const walk = (dir) => {
  const out = [];
  for (const entry of fs.readdirSync(dir)) {
    const full = path.join(dir, entry);
    const stat = fs.statSync(full);
    if (stat.isDirectory()) out.push(...walk(full));
    else out.push(full);
  }
  return out;
};

const specs = walk(here).filter(isSpec);
// A hung spec used to wedge the whole run until the CI job timed out with no
// clue which file was responsible. Bound each import instead and abort with
// the culprit named. Override with SPEC_TIMEOUT_MS=<ms> (0 disables).
const SPEC_TIMEOUT_MS = Number(process.env.SPEC_TIMEOUT_MS || 8 * 60 * 1000);
let failed = false;
for (const spec of specs) {
  const url = pathToFileURL(spec).href;
  let timer;
  try {
    if (SPEC_TIMEOUT_MS > 0) {
      await Promise.race([
        import(url),
        new Promise((_, reject) => {
          timer = setTimeout(() => reject(new Error(`spec timed out after ${SPEC_TIMEOUT_MS}ms (hung? leaked await?)`)), SPEC_TIMEOUT_MS);
        }),
      ]);
    } else {
      await import(url);
    }
  } catch (e) {
    failed = true;
    console.error('Import failed:', spec, e?.message || e);
    if (/spec timed out/.test(e?.message || '')) {
      // The hung import still dangles; continuing would interleave specs, so abort.
      console.error(`[run-all] aborting run (hung spec): ${spec}`);
      await new Promise((r) => setTimeout(r, 500)); // let stdio flush
      process.exit(1);
    }
  } finally {
    clearTimeout(timer);
  }
}

const skipped = getSkipCount();
console.log(`\n[run-all] ${specs.length} spec files, ${skipped} skipped tests${failed ? ', FAILURES' : ''}`);
// Always exit explicitly: open DB driver handles (e.g. an unclosed mongodb
// client's heartbeat timers) would otherwise keep the event loop alive and
// hang CI until the 15m timeout, even after every spec has completed.
if (typeof process !== 'undefined' && process?.exit) {
  process.exit(failed ? 1 : 0);
}
