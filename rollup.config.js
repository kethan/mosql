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

const resolve = (pkg, input = "src/index", output = "dist/index") => ({
    input: `${input}.js`,
    treeshake: 'smallest',
    plugins: [
        bundleSize(),
        copyTypes(
            input === 'index' ? 'index.d.ts' : input === 'lite/index' ? 'lite/index.d.ts' : 'tiny/index.d.ts',
            output === 'dist/index' ? 'dist/index.d.ts' : output === 'lite/dist/index' ? 'lite/dist/index.d.ts' : 'tiny/dist/index.d.ts'
        ),
    ],
	output: [
		{
			file: `${output}.es.js`,
			format: 'es',
			exports: 'named',
		},
		{
			file: `${output}.js`,
			format: 'cjs',
			exports: 'named',
		},
		{
			file: `${output}.min.js`,
			format: 'iife',
			name: pkg,
			strict: false,
			compact: true,
			exports: 'named',
			plugins: [terser()]
		},
		{
			file: `${output}.umd.js`,
			format: 'umd',
			name: pkg,
			strict: false,
			compact: true,
			exports: 'named',
			plugins: [terser()]
		}
	]
});

export default [
    resolve("umosql", "index", "dist/index"),
    resolve("umosql", "lite/index", "lite/dist/index"),
    resolve("umosql", "tiny/index", "tiny/dist/index")
]