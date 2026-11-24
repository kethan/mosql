// The bearer half of a Better Auth client.
//
// The published client keeps its session in a cookie, which a browser
// does and a server-to-server caller cannot. The `bearer` plugin on the
// server is what makes the cookie-free mode work: it reads the
// `Authorization: Bearer` header, and it answers with the session token
// in the `set-auth-token` header.
//
// This plugin is the other half — the client side of the same contract.
// It sends the token it was given as a bearer, and it remembers the one
// that comes back in `set-auth-token`. Nothing here is Better Auth's
// code: it is the twenty lines a native app or a service would write.

/**
 * @param {{
 *   onToken?: (token: string) => void,
 *   onRefreshToken?: (token: string) => void,
 * }} [options]
 */
export function bearerPlugin(options = {}) {
	/** @type {string | null} */
	let token = null;

	return {
		id: "bearer",
		name: "Bearer",
		hooks: {
			/**
			 * The request is built before this hook runs, and the fetch
			 * implementation is handed the context — so the header goes
			 * on `context.headers`.
			 *
			 * @param {{ headers: Headers }} context
			 */
			onRequest(context) {
				if (!token) return;
				if (context.headers.has("authorization")) return;
				context.headers.set("authorization", `Bearer ${token}`);
			},
			/**
			 * @param {{ response: Response }} context
			 */
			onResponse({ response }) {
				const header = response.headers.get("set-auth-token");
				if (!header) return;
				token = header;
				options.onToken?.(header);
			},
			/**
			 * @param {{ data: unknown }} context
			 */
			onSuccess({ data }) {
				if (!data || typeof data !== "object") return;
				const body = /** @type {{token?: string, refreshToken?: string}} */ (data);
				if (typeof body.token === "string") {
					token = body.token;
					options.onToken?.(body.token);
				}
				if (typeof body.refreshToken === "string") {
					options.onRefreshToken?.(body.refreshToken);
				}
			},
		},
	};
}
