import terser from '@rollup/plugin-terser';
import bundleSize from 'rollup-plugin-bundle-size';
import fs from 'fs';

const copyTypes = (src, dest) => ({
    name: 'copy-types',
    writeBundle() {
        try {
            const dir = dest.split('/').slice(0, -1).join('/');
            if (dir) fs.mkdirSync(dir, { recursive: true });
            fs.copyFileSync(src, dest);
        } catch {}
    }
});

// Optional/peer drivers are loaded lazily at runtime and must never be
// inlined into a published bundle (they are devDependencies here).
const OPTIONAL_DEPS = ['dotenv', 'pg', 'mysql2/promise', 'better-sqlite3', 'mongodb'];

// The package is `"type": "module"`, so the CommonJS build has to use the
// `.cjs` extension or Node refuses to load it through the "require" condition.
const resolve = ({ name, input, output, types, external = [], globalName = name }) => ({
    input: `${input}.js`,
    external: [...external],
    treeshake: 'smallest',
    plugins: [
        bundleSize(),
        copyTypes(types, `${output}.d.ts`),
    ],
    output: [
        {
            file: `${output}.es.js`,
            format: 'es',
            exports: 'named',
        },
        {
            file: `${output}.cjs`,
            format: 'cjs',
            exports: 'named',
        },
        {
            file: `${output}.min.js`,
            format: 'iife',
            name: globalName,
            strict: false,
            compact: true,
            exports: 'named',
            plugins: [terser()]
        },
        {
            file: `${output}.umd.js`,
            format: 'umd',
            name: globalName,
            strict: false,
            compact: true,
            exports: 'named',
            plugins: [terser()]
        }
    ]
});

export default [
    resolve({ name: 'umosql', input: 'index', output: 'dist/index', types: 'index.d.ts' }),
    resolve({ name: 'umosql', input: 'lite/index', output: 'lite/dist/index', types: 'lite/index.d.ts' }),
    resolve({ name: 'umosql', input: 'tiny/index', output: 'tiny/dist/index', types: 'tiny/index.d.ts' }),
    resolve({ name: 'umosqlSchemaless', input: 'src/schemaless', output: 'dist/schemaless', types: 'schemaless.d.ts' }),
    resolve({ name: 'umosqlMemory', input: 'src/memory', output: 'dist/memory', types: 'memory.d.ts' }),
    resolve({ name: 'umosqlClient', input: 'src/client', output: 'dist/client', types: 'client.d.ts', external: OPTIONAL_DEPS }),
]
