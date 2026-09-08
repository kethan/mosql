// ============================================
// OPTIONAL .env LOADING
// ============================================
// `dotenv` is a devDependency: it must never become a hard runtime requirement
// for published consumers, and it must not run at import time (the package is
// marked `"sideEffects": false`). Adapters therefore call `loadEnv()` from
// inside their async factories instead of doing `dotenv.config()` on load.

export const loadEnv = async (options) => {
    try {
        const mod = await import('dotenv');
        const dotenv = mod?.default || mod;
        // `quiet` keeps a library from printing dotenv's banner into the host
        // app's stdout; callers can override it.
        if (typeof dotenv?.config === 'function') dotenv.config({ quiet: true, ...options });
        return true;
    } catch {
        // dotenv is not installed - callers pass their config explicitly.
        return false;
    }
};

export default loadEnv;
