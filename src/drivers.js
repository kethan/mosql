// ============================================
// OPTIONAL DRIVER LOADING
// ============================================
// umosql bundles NO driver code and declares NO driver dependency. You install
// the driver you want (or bring your own instance/module) and umosql only ever
// loads it lazily, at the moment you ask for that backend.

export const DRIVER_INSTALL = {
    'better-sqlite3': 'npm i better-sqlite3',
    'pg': 'npm i pg',
    'mysql2/promise': 'npm i mysql2',
    'mongodb': 'npm i mongodb',
};

const isMissingModule = (e) => {
    const code = String(e?.code || '');
    return code === 'ERR_MODULE_NOT_FOUND' || code === 'MODULE_NOT_FOUND'
        || /cannot find (package|module)/i.test(String(e?.message || ''));
};

/**
 * Runs a lazy `import()` of a driver and turns "not installed" into an
 * actionable message instead of a bare ERR_MODULE_NOT_FOUND.
 *
 * @param {string} backend  name used in the error (e.g. 'pg')
 * @param {string} specifier package specifier (e.g. 'pg', 'mysql2/promise')
 * @param {() => Promise<any>} load the actual dynamic import
 */
export const loadDriver = async (backend, specifier, load) => {
    try {
        return await load();
    } catch (e) {
        if (!isMissingModule(e)) throw e;
        const install = DRIVER_INSTALL[specifier] || `npm i ${specifier}`;
        throw new Error(
            `umosql: the "${backend}" backend needs the "${specifier}" driver, which is not installed.\n` +
            `  -> install it yourself: \`${install}\`\n` +
            `  -> or bring your own: createSchemalessClient("${backend}", { client }) / { driver }\n` +
            `umosql never bundles drivers.`
        );
    }
};

/** Reads `keys` off a module namespace or its `.default` (CJS interop). */
export const pick = (mod, ...keys) => {
    for (const candidate of [mod, mod?.default]) {
        if (!candidate) continue;
        for (const key of keys) {
            if (candidate[key]) return candidate[key];
        }
    }
    return null;
};

export default loadDriver;
