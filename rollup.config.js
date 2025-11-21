import terser from '@rollup/plugin-terser';
import bundleSize from 'rollup-plugin-bundle-size';

const resolve = (pkg, input = "src/index", output = "dist/index") => ({
	input: `${input}.js`,
	treeshake: 'smallest',
	plugins: [
		bundleSize(),
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