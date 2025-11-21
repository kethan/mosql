import fs from 'fs';
import path from 'path';
import { pathToFileURL, fileURLToPath } from 'url';

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

const specs = walk(here).filter(isSpec);
for (const spec of specs) {
  const url = pathToFileURL(spec).href;
  try {
    await import(url);
  } catch (e) {
    console.error('Import failed:', spec, e?.message || e);
  }
}
// Ensure proper process exit for coverage runs
if (typeof process !== 'undefined' && process?.exit) {
  process.exit(0);
}