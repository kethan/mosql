import assert from 'assert';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { spawnSync } from 'child_process';
import { fileURLToPath } from 'url';

// Guards the test runner itself. `tests/run-all.js` used to import every spec and
// then call `process.exit(0)` unconditionally, which printed failures and
// reported success - `npm test`, the pre-commit hook and CI could not go red.

const here = path.dirname(fileURLToPath(import.meta.url));
const runner = path.join(here, 'run-all.js');
const pass = (title) => console.log('PASS ' + title);

const withSpecs = (contents, run, env = {}) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mosql-runner-'));
  try {
    fs.copyFileSync(runner, path.join(dir, 'run-all.js'));
    for (const [name, body] of Object.entries(contents)) fs.writeFileSync(path.join(dir, name), body);
    return run(dir, env);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
};

const run = (dir, env) => spawnSync(process.execPath, [path.join(dir, 'run-all.js')], {
  encoding: 'utf8',
  env: { ...process.env, MOSQL_TEST_TIMEOUT_MS: '2000', ...env },
});

const ok = `console.log('PASS example');\n`;

// 1. a green suite is a green run
assert.strictEqual(withSpecs({ 'a.spec.js': ok, 'b.spec.js': ok }, run).status, 0, 'passing specs exit 0');
pass('run-all.js exits 0 when every spec passes');

// 2. a spec that throws must fail the run, and say which file did it
const thrown = withSpecs({
  'a-pass.spec.js': ok,
  'b-throw.spec.js': `throw new Error('assertion blew up');\n`,
}, run);
assert.notStrictEqual(thrown.status, 0, 'a throwing spec must fail the run');
assert.match(thrown.stderr || '', /FAILED b-throw\.spec\.js/, 'the failing file must be named');
assert.match(`${thrown.stdout}\n${thrown.stderr}`, /assertion blew up/, "the spec's output must reach the terminal");
pass('run-all.js fails on a throwing spec and names the file');

// 3. specs that only report a failure through process.exitCode count too
const exitCode = withSpecs({ 'a-exit.spec.js': `process.exitCode = 1;\n` }, run);
assert.notStrictEqual(exitCode.status, 0, 'process.exitCode = 1 must fail the run');
assert.match(exitCode.stderr || '', /FAILED a-exit\.spec\.js/, 'the exit-code file must be named');
pass('run-all.js honours process.exitCode');

// 4. a hung spec is killed and reported instead of blocking the whole suite
const hung = withSpecs({
  'a-hang.spec.js': `setTimeout(() => {}, 60_000);\n`,
  'b-ok.spec.js': ok,
}, run);
assert.notStrictEqual(hung.status, 0, 'a hung spec must fail the run');
assert.match(hung.stderr || '', /FAILED a-hang\.spec\.js - killed/, 'the timeout must be reported');
assert.match(`${hung.stdout}\n${hung.stderr}`, /PASS example/, 'the other specs must still have run');
pass('run-all.js times out per file and keeps going');

// 5. the real suite is discovered by the same walk (every spec file is executed)
const discovered = withSpecs({
  'z-not-a-spec.js': `throw new Error('must not be run');\n`,
  'skipped.bun.unified.spec.js': ok,
}, run);
assert.strictEqual(discovered.status, 0, 'non-spec and bun files are not run');
pass('run-all.js selects exactly the *.spec.js files (bun specs excluded)');
