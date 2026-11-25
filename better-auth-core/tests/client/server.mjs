// Starting the real server for the compatibility tests.
//
// The binary under test is the one that ships: the same executable, the
// same routes, the same SQLite file. Nothing about the server is faked
// for these tests — the client talks to it over HTTP, exactly as it would
// talk to a deployed one.

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const crateRoot = resolve(here, "..", "..");

/** The server binary, release first. */
function binary() {
	for (const dir of ["release", "debug"]) {
		for (const name of ["better-auth-core", "better-auth-core.exe"]) {
			const path = join(crateRoot, "target", dir, name);
			if (existsSync(path)) return path;
		}
	}
	throw new Error(
		"the server binary is not built — run `cargo build --release` in better-auth-core/",
	);
}

/** A port nobody is using. */
function freePort() {
	return new Promise((resolvePort, reject) => {
		const probe = createServer();
		probe.on("error", reject);
		probe.listen(0, "127.0.0.1", () => {
			const { port } = probe.address();
			probe.close(() => resolvePort(port));
		});
	});
}

const SECRET = "0123456789abcdef0123456789abcdef0123456789";

/**
 * Start the server and wait until it answers.
 *
 * The path of the database is handed back as well: the interop test reads
 * the account row out of it, because the hash it holds is the one thing
 * the server does not hand out over HTTP.
 *
 * @returns {Promise<{ url: string, dbPath: string, stop: () => Promise<void>, output: () => string }>}
 */
export async function startServer() {
	const port = await freePort();
	const dir = await mkdtemp(join(tmpdir(), "ba-client-"));
	const dbPath = join(dir, "auth.db");
	const child = spawn(
		binary(),
		["--listen", `127.0.0.1:${port}`, "--data", dbPath],
		{
			env: {
				...process.env,
				BETTER_AUTH_SECRET: SECRET,
				BETTER_AUTH_URL: `http://127.0.0.1:${port}`,
				// No mailer: the tests read the one-time codes out of the
				// answers instead of out of an inbox.
				BETTER_AUTH_MAILER: "none",
			},
			stdio: ["ignore", "pipe", "pipe"],
		},
	);
	let output = "";
	child.stdout.on("data", (chunk) => {
		output += chunk;
	});
	child.stderr.on("data", (chunk) => {
		output += chunk;
	});

	const url = `http://127.0.0.1:${port}`;
	const deadline = Date.now() + 20_000;
	for (;;) {
		if (child.exitCode !== null) {
			throw new Error(`the server stopped:\n${output}`);
		}
		try {
			const reply = await fetch(`${url}/api/auth/ok`);
			if (reply.ok) break;
		} catch {
			// Not up yet.
		}
		if (Date.now() > deadline) {
			child.kill("SIGKILL");
			throw new Error(`the server did not come up:\n${output}`);
		}
		await new Promise((r) => setTimeout(r, 100));
	}

	return {
		url,
		dbPath,
		output: () => output,
		stop: () =>
			new Promise((resolveStop) => {
				if (child.exitCode !== null) return resolveStop();
				child.once("exit", () => resolveStop());
				child.kill("SIGTERM");
				setTimeout(() => child.kill("SIGKILL"), 2_000).unref?.();
			}),
	};
}
