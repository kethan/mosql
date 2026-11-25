// Wire compatibility with the real Better Auth client.
//
// The client is the published package, not a copy of it: `createAuthClient`
// from `better-auth/client`, the same one an application would use. It
// maps `client.signUp.email(...)` onto `/sign-up/email`, `client.getSession()`
// onto `/get-session`, and so on — so every call below is a statement
// about the route names, the request bodies and the answer shapes.
//
// The client keeps its session in a cookie, which a server-to-server
// caller cannot do. The bearer plugin on the other side of this is the
// contract under test: the token comes back in the `set-auth-token`
// header and goes out again in `Authorization: Bearer`.
//
// The client answers with `{data, error}` rather than throwing, so every
// call below is checked on both halves.

import assert from "node:assert/strict";
import { DatabaseSync } from "node:sqlite";
import { after, before, describe, test } from "node:test";

import { createAuthClient } from "better-auth/client";
import { hashPassword, verifyPassword } from "better-auth/crypto";
import {
	adminClient,
	anonymousClient,
	emailOTPClient,
	magicLinkClient,
	twoFactorClient,
	usernameClient,
} from "better-auth/client/plugins";

import { bearerPlugin } from "./bearer.mjs";
import { startServer } from "./server.mjs";

let server;
let client;

/**
 * A fresh client, sharing nothing with the others.
 *
 * The plugin clients are the ones an application installs: each of them
 * says which method its routes take, which is how the client knows to
 * POST to `/sign-in/anonymous` rather than GET it.
 */
function newClient() {
	return createAuthClient({
		baseURL: `${server.url}/api/auth`,
		plugins: [
			anonymousClient(),
			usernameClient(),
			emailOTPClient(),
			magicLinkClient(),
			twoFactorClient(),
			adminClient(),
		],
		fetchOptions: { plugins: [bearerPlugin()] },
	});
}

/** The answer of a call that is expected to succeed. */
async function ok(promise) {
	const { data, error } = await promise;
	assert.equal(error, null, `expected no error, got ${JSON.stringify(error)}`);
	return data;
}

/** The error of a call that is expected to fail. */
async function fails(promise) {
	const { data, error } = await promise;
	assert.equal(data, null, "a failing call answers with no data");
	assert.ok(error, "a failing call answers with an error");
	return error;
}

before(async () => {
	server = await startServer();
	client = newClient();
});

after(async () => {
	await server?.stop();
});

const PASSWORD = "a good long password";

describe("the session", () => {
	test("sign-up answers with a token, a refresh token and the user", async () => {
		const data = await ok(
			client.signUp.email({
				email: "ada@example.com",
				password: PASSWORD,
				name: "Ada",
			}),
		);
		assert.equal(typeof data.token, "string");
		assert.ok(data.token.length > 40);
		assert.equal(typeof data.refreshToken, "string");
		assert.ok(data.refreshToken.length >= 40);
		assert.equal(data.user.email, "ada@example.com");
		assert.equal(data.user.name, "Ada");
		assert.equal(data.user.role, "user");
		assert.equal(data.user.emailVerified, false);
		// A password is never handed out.
		assert.equal(data.user.password, undefined);
	});

	test("the session is readable afterwards", async () => {
		await ok(client.signIn.email({ email: "ada@example.com", password: PASSWORD }));
		const session = await ok(client.getSession());
		assert.equal(session.user.email, "ada@example.com");
		assert.equal(session.session.userId, session.user.id);
		// The client turns the ISO string into a Date.
		const expires = new Date(session.session.expiresAt);
		assert.ok(expires.getTime() > Date.now(), "the session has an expiry in the future");
		// The refresh digest is not in the session.
		assert.equal(session.session.token, undefined);
	});

	test("the sessions can be listed, and the current one is marked", async () => {
		const sessions = await ok(client.listSessions());
		assert.ok(Array.isArray(sessions));
		assert.ok(sessions.length >= 1);
		assert.equal(sessions.filter((s) => s.isCurrent).length, 1);
		assert.ok(sessions.every((s) => s.token === undefined));
	});

	test("the profile can be changed", async () => {
		const data = await ok(client.updateUser({ name: "Ada L" }));
		assert.equal(data.status, true);
		assert.equal(data.user.name, "Ada L");
		const session = await ok(client.getSession());
		assert.equal(session.user.name, "Ada L");
	});

	test("signing out ends the session", async () => {
		const data = await ok(client.signOut());
		assert.equal(data.success, true);
		const session = await ok(client.getSession());
		assert.equal(session, null);
	});

	test("a wrong password is refused with the server's own wording", async () => {
		const error = await fails(
			client.signIn.email({ email: "ada@example.com", password: "wrong" }),
		);
		assert.equal(error.status, 401);
		assert.equal(error.code, "INVALID_EMAIL_OR_PASSWORD");
		assert.equal(error.message, "Invalid email or password");
	});

	test("an unknown address is refused the same way", async () => {
		const error = await fails(
			client.signIn.email({ email: "nobody@example.com", password: PASSWORD }),
		);
		assert.equal(error.status, 401);
		assert.equal(error.code, "INVALID_EMAIL_OR_PASSWORD");
	});
});

describe("the refresh token", () => {
	test("it is exchanged for a new pair, and cannot be replayed", async () => {
		const own = newClient();
		const signed = await ok(
			own.signIn.email({ email: "ada@example.com", password: PASSWORD }),
		);
		const refreshToken = signed.refreshToken;

		const refreshed = await ok(
			own.$fetch("/refresh-token", {
				method: "POST",
				headers: { authorization: `Bearer ${refreshToken}` },
			}),
		);
		assert.equal(typeof refreshed.token, "string");
		assert.equal(typeof refreshed.refreshToken, "string");
		assert.notEqual(refreshed.refreshToken, refreshToken);
		assert.equal(refreshed.user.email, "ada@example.com");

		// The old one is spent.
		const error = await fails(
			own.$fetch("/refresh-token", {
				method: "POST",
				headers: { authorization: `Bearer ${refreshToken}` },
			}),
		);
		assert.equal(error.status, 401);
		assert.equal(error.code, "INVALID_TOKEN");

		// The new one works.
		const again = await ok(
			own.$fetch("/refresh-token", {
				method: "POST",
				headers: { authorization: `Bearer ${refreshed.refreshToken}` },
			}),
		);
		assert.equal(typeof again.token, "string");
	});
});

describe("the plugins", () => {
	test("an anonymous visitor gets an account", async () => {
		const own = newClient();
		const data = await ok(own.signIn.anonymous());
		assert.equal(data.user.isAnonymous, true);
		assert.match(data.user.email, /@anonymous\.local$/);
		assert.equal(typeof data.token, "string");
	});

	test("a username signs in", async () => {
		const own = newClient();
		await ok(
			own.signUp.email({
				email: "grace@example.com",
				password: PASSWORD,
				name: "Grace",
			}),
		);
		await ok(own.updateUser({ username: "grace.h" }));
		const data = await ok(
			own.signIn.username({ username: "grace.h", password: PASSWORD }),
		);
		assert.equal(data.user.username, "grace.h");
		assert.equal(data.user.email, "grace@example.com");

		const error = await fails(
			own.signIn.username({ username: "grace.h", password: "wrong" }),
		);
		assert.equal(error.status, 401);
		assert.equal(error.code, "INVALID_USERNAME_OR_PASSWORD");
	});

	test("a one-time code signs in", async () => {
		const own = newClient();
		const sent = await ok(
			own.emailOtp.sendVerificationOTP({ email: "otp@example.com", type: "sign_in" }),
		);
		assert.equal(sent.success, true);
		// With no mailer on the server the code comes back in the answer,
		// which is exactly what a development server does.
		assert.equal(typeof sent.otp, "string");
		assert.equal(sent.otp.length, 6);

		const data = await ok(
			own.signIn.emailOtp({ email: "otp@example.com", otp: sent.otp }),
		);
		assert.equal(data.user.email, "otp@example.com");
		assert.equal(data.user.emailVerified, true);
	});

	test("two-factor is switched on and answers with the codes", async () => {
		const own = newClient();
		await ok(
			own.signUp.email({
				email: "two@example.com",
				password: PASSWORD,
				name: "Two",
			}),
		);
		const data = await ok(own.twoFactor.enable({ password: PASSWORD }));
		assert.match(data.totpURI, /^otpauth:\/\/totp\//);
		assert.equal(data.backupCodes.length, 10);
	});

	test("a magic link asks for mail, and says so when there is none", async () => {
		const own = newClient();
		const error = await fails(own.signIn.magicLink({ email: "magic@example.com" }));
		assert.equal(error.status, 400);
		assert.equal(error.code, "MAIL_NOT_CONFIGURED");
	});
});

describe("the admin routes", () => {
	test("an ordinary user is refused", async () => {
		const own = newClient();
		await ok(own.signIn.email({ email: "ada@example.com", password: PASSWORD }));
		const error = await fails(own.admin.listUsers());
		assert.equal(error.status, 403);
		assert.equal(error.code, "YOU_ARE_NOT_ALLOWED_TO_ACCESS_THIS");
	});
});

describe("the bearer contract", () => {
	test("the token comes back in a header, and no cookie is set", async () => {
		const reply = await fetch(`${server.url}/api/auth/sign-up/email`, {
			method: "POST",
			headers: { "content-type": "application/json" },
			body: JSON.stringify({
				email: "bearer@example.com",
				password: PASSWORD,
				name: "Bearer",
			}),
		});
		assert.equal(reply.status, 200);
		const token = reply.headers.get("set-auth-token");
		assert.equal(typeof token, "string");
		assert.ok(token.length > 40, "the header carries the token");
		// The session is not kept by a cookie: there is none.
		assert.equal(reply.headers.get("set-cookie"), null);
	});

	test("the token in the header is the one in the body", async () => {
		const reply = await fetch(`${server.url}/api/auth/sign-in/email`, {
			method: "POST",
			headers: { "content-type": "application/json" },
			body: JSON.stringify({ email: "bearer@example.com", password: PASSWORD }),
		});
		assert.equal(reply.status, 200);
		const body = await reply.json();
		assert.equal(reply.headers.get("set-auth-token"), body.token);
	});

	test("a token sent as a bearer is accepted", async () => {
		const signed = await fetch(`${server.url}/api/auth/sign-in/email`, {
			method: "POST",
			headers: { "content-type": "application/json" },
			body: JSON.stringify({ email: "bearer@example.com", password: PASSWORD }),
		});
		const { token } = await signed.json();
		const reply = await fetch(`${server.url}/api/auth/get-session`, {
			headers: { authorization: `Bearer ${token}` },
		});
		assert.equal(reply.status, 200);
		const session = await reply.json();
		assert.equal(session.user.email, "bearer@example.com");
	});
});

describe("the password hash", () => {
	test("the server writes a hash the JavaScript stack can read", async () => {
		const own = newClient();
		await ok(
			own.signUp.email({
				email: "hash@example.com",
				password: PASSWORD,
				name: "Hash",
			}),
		);

		const db = new DatabaseSync(server.dbPath);
		const row = db
			.prepare(
				`SELECT a."password" AS password FROM "account" a
				 JOIN "user" u ON u."id" = a."userId"
				 WHERE u."email" = ? AND a."providerId" = 'credential'`,
			)
			.get("hash@example.com");
		db.close();

		assert.ok(row, "the credential account is there");
		const stored = String(row.password);
		// Better Auth's own format: the salt, then the key.
		const [salt, key] = stored.split(":");
		assert.equal(salt.length, 32);
		assert.equal(key.length, 128);
		// And the JavaScript stack can verify it.
		assert.equal(await verifyPassword({ hash: stored, password: PASSWORD }), true);
		assert.equal(
			await verifyPassword({ hash: stored, password: "not the password" }),
			false,
		);
	});

	test("the server reads a hash the JavaScript stack wrote", async () => {
		const own = newClient();
		await ok(
			own.signUp.email({
				email: "js@example.com",
				password: PASSWORD,
				name: "JS",
			}),
		);

		// The hash Better Auth itself would have written.
		const jsHash = await hashPassword(PASSWORD);

		const db = new DatabaseSync(server.dbPath);
		db.prepare(
			`UPDATE "account" SET "password" = ?
			 WHERE "providerId" = 'credential' AND "userId" = (
				SELECT "id" FROM "user" WHERE "email" = ?)`,
		).run(jsHash, "js@example.com");
		db.close();

		// The Rust server has to accept it.
		const own2 = newClient();
		const data = await ok(
			own2.signIn.email({ email: "js@example.com", password: PASSWORD }),
		);
		assert.equal(data.user.email, "js@example.com");
	});
});

describe("the routes themselves", () => {
	test("the health route answers", async () => {
		const data = await ok(client.$fetch("/ok"));
		assert.equal(data.ok, true);
	});

	test("a path that does not exist is a 404", async () => {
		const error = await fails(client.$fetch("/not-a-route"));
		assert.equal(error.status, 404);
		assert.equal(error.code, "NOT_FOUND");
	});

	test("the wrong method is a 405", async () => {
		const error = await fails(
			client.$fetch("/sign-up/email", { method: "DELETE" }),
		);
		assert.equal(error.status, 405);
		assert.equal(error.code, "METHOD_NOT_ALLOWED");
	});
});
