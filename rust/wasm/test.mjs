// Replays the JS-generated fixture corpus through the WASM build.
import { loadMosql } from './mosql.js';
import { loadFixtures, replayBuilder, replayMemory, replayBehaviour } from '../tests/replay.mjs';

const mosql = await loadMosql(new URL('./mosql.wasm', import.meta.url));
const here = (p) => new URL(p, import.meta.url);

const fixtures = loadFixtures(here('../tests/fixtures.json'));
const memFixtures = loadFixtures(here('../tests/memory-fixtures.json'));

const failures = [
  ...replayBuilder(mosql, fixtures),
  ...replayMemory(mosql, memFixtures),
  ...replayBehaviour(mosql),
];

for (const f of failures) console.log(`FAIL ${f}`);
console.log(`${fixtures.length - failures.length}/${fixtures.length} wasm fixtures matched`);
console.log(`memory engine: ${failures.length} failures`);
if (failures.length) process.exit(1);
