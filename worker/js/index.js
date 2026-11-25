// The Worker: the host half of the deployment.
//
// The authentication engine is a WebAssembly module, imported here at
// source phase so the platform compiles it once and hands over a
// `WebAssembly.Module`. The module has no bindings of its own — it cannot
// have any — so this script is what reaches the database, the cache, the
// bucket and the network on its behalf.
//
// The engine asks for things rather than doing them: it hands over a
// question, this script answers it, and the engine continues. The loop
// below is that conversation.
//
// Two imports are given to the module, under the name `host`:
// `host_now_ms` and `host_random`. They are the clock and the randomness,
// which a Worker has no other way to get.

import wasmModule from "./mosql_worker.wasm";

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** The module, and the engine inside it, built once per isolate. */
let runtime = null;

/** The statuses the module answers with. */
const DONE = 0;
const ASK = 1;

/**
 * Build the module, and the engine inside it.
 *
 * The bindings are read from the environment the first time a request
 * arrives, because a Worker has no environment of its own until then.
 */
async function getRuntime(env) {
  if (runtime) return runtime;

  const instance = await WebAssembly.instantiate(wasmModule, {
    host: {
      host_now_ms: () => Date.now(),
      host_random: (ptr, len) => {
        // `getRandomValues` refuses a buffer of more than 64 KiB, so a
        // long buffer is filled in pieces.
        const memory = new Uint8Array(instance.exports.memory.buffer, ptr, len);
        for (let offset = 0; offset < len; offset += 65536) {
          crypto.getRandomValues(memory.subarray(offset, Math.min(offset + 65536, len)));
        }
      },
    },
  });
  const exports = instance.exports;

  const config = {
    baseUrl: env.AUTH_BASE_URL || env.BETTER_AUTH_URL || "http://localhost:8787",
    secret: env.AUTH_SECRET || env.BETTER_AUTH_SECRET || "",
    allowedOrigins: (env.ALLOWED_ORIGINS || "")
      .split(",")
      .map((origin) => origin.trim())
      .filter(Boolean),
    cache: Boolean(env.CACHE),
    blobs: Boolean(env.BUCKET),
  };

  const status = call(exports, "mosql_worker_init", JSON.stringify(config));
  if (status !== DONE) {
    throw new Error(`the engine did not start: ${read(exports)}`);
  }
  runtime = { exports };
  return runtime;
}

/** Read what the module last said. */
function read(exports) {
  const length = exports.mosql_worker_alloc(4);
  const pointer = exports.mosql_worker_take(length);
  const size = new Uint32Array(exports.memory.buffer, length, 1)[0];
  if (!size) {
    exports.mosql_worker_free(length, 4);
    return "";
  }
  const text = decoder.decode(new Uint8Array(exports.memory.buffer, pointer, size));
  exports.mosql_worker_free(length, 4);
  return text;
}

/** Call one of the module's functions with a string, and say what it said. */
function call(exports, name, text) {
  const bytes = encoder.encode(text);
  const pointer = exports.mosql_worker_alloc(bytes.length);
  new Uint8Array(exports.memory.buffer, pointer, bytes.length).set(bytes);
  const status = exports[name](pointer, bytes.length);
  exports.mosql_worker_free(pointer, bytes.length);
  return status;
}

/** Call one of the module's functions with a string, and read the answer. */
function callAndRead(exports, name, text) {
  const bytes = encoder.encode(text);
  const pointer = exports.mosql_worker_alloc(bytes.length);
  new Uint8Array(exports.memory.buffer, pointer, bytes.length).set(bytes);
  const status = exports[name](pointer, bytes.length);
  exports.mosql_worker_free(pointer, bytes.length);
  return { status, text: read(exports) };
}

/** Answer the engine's question. */
async function answer(env, op) {
  try {
    return await perform(env, op);
  } catch (error) {
    // A failure is an answer: the engine decides what to do with it. The
    // cache treats one as a miss; the database turns one into an error.
    return { error: String(error && error.message ? error.message : error) };
  }
}

/** Answer the engine's question, and let a failure through. */
async function perform(env, op) {
  switch (op.kind) {
    case "query":
      return query(env, op);
    case "fetch":
      return fetchAnswer(op);
    case "cacheGet":
      return { value: env.CACHE ? await env.CACHE.get(op.key, "text") : null };
    case "cachePut":
      if (env.CACHE) {
        await env.CACHE.put(op.key, op.value, { expirationTtl: op.ttl });
      }
      return {};
    case "cacheDelete":
      if (env.CACHE) {
        await env.CACHE.delete(op.key);
      }
      return {};
    case "cacheList":
      if (!env.CACHE) return { keys: [] };
      const entries = await env.CACHE.list({ prefix: op.prefix });
      return { keys: entries.keys.map((key) => key.name) };
    case "blobGet":
      return blobGet(env, op);
    case "blobPut":
      if (env.BUCKET) {
        await env.BUCKET.put(op.key, op.body, {
          httpMetadata: op.contentType ? { contentType: op.contentType } : undefined,
        });
      }
      return {};
    case "blobDelete":
      if (env.BUCKET) {
        await env.BUCKET.delete(op.key);
      }
      return {};
    case "blobList":
      if (!env.BUCKET) return { keys: [] };
      const objects = await env.BUCKET.list({ prefix: op.prefix });
      return { keys: objects.objects.map((object) => object.key) };
    default:
      return { error: `the engine asked for something unknown: ${op.kind}` };
  }
}

/** Run a statement against the database. */
async function query(env, op) {
  const statement = env.DB.prepare(op.sql);
  const bound = op.params && op.params.length ? statement.bind(...op.params.map(bindable)) : statement;
  try {
    if (op.rows) {
      const result = await bound.all();
      const rows = (result.results || []).map((row) => ({
        columns: Object.keys(row),
        values: Object.values(row),
      }));
      return { rows, changes: changesOf(result) };
    }
    const result = await bound.run();
    return { rows: [], changes: changesOf(result) };
  } catch (error) {
    return { error: String(error && error.message ? error.message : error) };
  }
}

/** How many rows a statement changed. */
function changesOf(result) {
  const changes = result && result.meta ? result.meta.changes : undefined;
  return typeof changes === "number" ? changes : 0;
}

/** A value D1 will accept. */
function bindable(value) {
  if (value === null || value === undefined) return null;
  if (typeof value === "boolean" || typeof value === "number" || typeof value === "string") {
    return value;
  }
  return JSON.stringify(value);
}

/** Make an outbound request. */
async function fetchAnswer(op) {
  try {
    const response = await fetch(op.url, {
      method: op.method || "GET",
      headers: Object.fromEntries(op.headers || []),
      body: op.body ? op.body : undefined,
    });
    return {
      status: response.status,
      headers: [...response.headers].map(([name, value]) => [name, value]),
      body: await response.text(),
    };
  } catch (error) {
    return { error: String(error && error.message ? error.message : error) };
  }
}

/** Read a blob. */
async function blobGet(env, op) {
  if (!env.BUCKET) return { error: "this deployment has no bucket" };
  const object = await env.BUCKET.get(op.key);
  if (!object) return { body: null };
  return {
    body: await object.text(),
    contentType: object.httpMetadata ? object.httpMetadata.contentType : undefined,
  };
}

export default {
  async fetch(request, env) {
    const { exports } = await getRuntime(env);
    const url = new URL(request.url);
    const payload = {
      method: request.method,
      path: url.pathname,
      query: [...url.searchParams].map(([name, value]) => [name, value]),
      headers: [...request.headers].map(([name, value]) => [name, value]),
      body: await request.text(),
      remote: remoteOf(request),
    };

    let { status, text } = callAndRead(exports, "mosql_worker_run", JSON.stringify(payload));
    while (status === ASK) {
      const op = JSON.parse(text);
      const reply = await answer(env, op);
      ({ status, text } = callAndRead(exports, "mosql_worker_resume", JSON.stringify(reply)));
    }
    if (status !== DONE) {
      return Response.json({ code: "INTERNAL_ERROR", message: text || "the engine failed" }, { status: 500 });
    }

    const response = JSON.parse(text);
    const headers = new Headers(response.headers || []);
    if (!headers.has("content-type")) {
      headers.set("content-type", "application/json");
    }
    return new Response(response.body ?? "", {
      status: response.status,
      headers,
    });
  },
};

/** Where the request came from. */
function remoteOf(request) {
  const address = request.headers.get("cf-connecting-ip") || request.headers.get("x-forwarded-for");
  if (!address) return "0.0.0.0:0";
  const first = address.split(",")[0].trim();
  // An IPv6 address needs brackets before a port can be added to it.
  return first.includes(":") ? `[${first}]:0` : `${first}:0`;
}
