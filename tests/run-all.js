import fs from 'fs';
import path from 'path';
import { spawnSync } from 'child_process';
import { fileURLToPath } from 'url';

const here = path.dirname(fileURLToPath(import.meta.url));

const isSpec = (f) => f.endsWith('.spec.js') && !f.includes('bun.');

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

// Every spec runs in its own Node process and its exit code is honoured.
// An earlier version imported the specs in-process and then called
// `process.exit(0)` unconditionally, so a thrown assertion - or a spec that only
// set `process.exitCode = 1` - was printed and dropped: `npm test`, the
// pre-commit hook and CI could never go red. Isolating the files also keeps one
// hung or hard-exiting spec from taking the rest of the run down with it.
// tests/runner.spec.js guards exactly that behaviour.
const timeoutMs = Number(process.env.MOSQL_TEST_TIMEOUT_MS || 120_000);

const specs = walk(here).filter(isSpec).sort();

const failures = [];
let passed = 0;

for (const spec of specs) {
  const rel = path.relative(here, spec);
  const started = Date.now();
  const run = spawnSync(process.execPath, [spec], {
    cwd: path.resolve(here, '..'),
    stdio: 'inherit',
    timeout: timeoutMs,
    killSignal: 'SIGKILL',
  });

  if (run.signal) {
    failures.push({ rel, reason: `killed by ${run.signal} after ${timeoutMs}ms` });
    continue;
  }
  if (run.error) {
    failures.push({ rel, reason: run.error.message });
    continue;
  }
  if (run.status !== 0) {
    failures.push({ rel, reason: `exit code ${run.status}` });
    continue;
  }
  passed++;
  console.log(`OK  ${rel} (${((Date.now() - started) / 1000).toFixed(1)}s)`);
}

console.log(`\n${specs.length} spec file(s): ${passed} passed, ${failures.length} failed`);

if (failures.length) {
  for (const f of failures) console.error(`FAILED ${f.rel} - ${f.reason}`);
  console.error('\nEach failing file above printed its own error output.');
  process.exitCode = 1;
}
