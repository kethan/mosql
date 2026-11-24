# better-auth-core

A complete authentication server that speaks the [Better Auth](https://better-auth.com)
route contract, with **no cookies**: clients hold a bearer token and send
it in `Authorization: Bearer …`.

It is written in Rust with no dependencies from crates.io — the HTTP
server, the JSON, the SQLite driver, the JWT signer, the TOTP generator
and the OAuth client are all in this repository. The release binary is
about 1.8 MB.

```console
$ cargo build --release
$ BETTER_AUTH_SECRET=$(head -c 32 /dev/urandom | base64) \
  BETTER_AUTH_URL=https://app.example.com \
  ./target/release/better-auth-core --data ./auth.db
```

The API hangs off `/api/auth`.

---

## Contents

- [Tokens](#tokens)
- [Configuration](#configuration)
- [Routes](#routes)
- [Sign-in flows](#sign-in-flows)
- [Two-factor](#two-factor)
- [OAuth](#oauth)
- [Admin](#admin)
- [Data](#data)
- [As a library](#as-a-library)
- [Tests](#tests)

---

## Tokens

There are no cookies and no session cookie jar. Two things come back
from every successful sign-in, refresh and verification:

| field | what it is |
|---|---|
| `token` | an HS256 JWT, valid for 15 minutes, carrying `sub`, `sid`, `email`, `role`, `anon`, `imp`, `iat`, `exp` |
| `refreshToken` | an opaque 32-byte string, stored only as a SHA-256 digest in `session.token`, valid for 30 days |

The access token also arrives in the `set-auth-token` response header, so
a client can read it either way.

The refresh token is **rotated**: every refresh issues a new one and
destroys the old. Presenting a spent refresh token is refused with
`401 INVALID_TOKEN` — a stolen token cannot be replayed.

Because the access token carries the identity and the role, the
middleware that answers "who is this?" never touches the database. A
route that needs the user row fetches it; a route that only needs "is
this an admin?" does not.

Sign-out revokes the session, which also ends the refresh token.

### A second factor in the middle

A user with two-factor authentication enabled gets a **pending token**
(5 minutes, `purpose: "2fa_pending"`) instead of a session. It opens the
second-factor routes and nothing else: every ordinary route answers
`401 TWO_FACTOR_PENDING`.

### Errors

Every error is `{"code": "…", "message": "…"}` with a meaningful status:

| status | when |
|---|---|
| 400 | the request is malformed, or the code/field is wrong |
| 401 | no token, a bad token, an expired token, a wrong password or code |
| 403 | the caller is known but not allowed (banned, not an admin, email not verified) |
| 404 | the route or the record does not exist |

---

## Configuration

Everything is read from the environment.

| variable | default | meaning |
|---|---|---|
| `BETTER_AUTH_SECRET` | — | the HS256 key; at least 32 characters. Required. |
| `BETTER_AUTH_URL` | — | the public origin of this server, e.g. `https://app.example.com`. Required. |
| `BETTER_AUTH_DATA` | `./better-auth.db` | the SQLite file; `:memory:` for an ephemeral server |
| `BETTER_AUTH_LISTEN` | `0.0.0.0:8787` | the address to bind |
| `BETTER_AUTH_TRUSTED_ORIGINS` | — | comma-separated origins allowed in CORS and in redirects |
| `BETTER_AUTH_ISSUER` | `better-auth` | the `iss` claim and the issuer in TOTP URIs |
| `BETTER_AUTH_REQUIRE_EMAIL_VERIFICATION` | `0` | refuse sign-in until the address is verified |
| `BETTER_AUTH_DISABLE_ANONYMOUS` | `0` | turn anonymous sign-in off |
| `BETTER_AUTH_HTTP` | `curl` if present | `curl`, `plain` (http only) or `none` |
| `BETTER_AUTH_MAILER` | `console` | `console` prints the mail, `none` disables it |
| `ACCESS_TTL_SECS` | `900` | the life of an access token |
| `REFRESH_TTL_SECS` | `2592000` | the life of a refresh token |
| `BETTER_AUTH_SCRYPT_N` | `16384` | the cost of the password hash |
| `BETTER_AUTH_STORE` | `sqlite` | `sqlite`, `memory`, `http` or `tcp` |
| `BETTER_AUTH_STORE_URL` | — | the store's address, for `http` and `tcp` |
| `BETTER_AUTH_STORE_TOKEN` | — | a bearer token for the store, if it wants one |

Command line: `--listen ADDR`, `--data PATH`, `--store-server [http|tcp]`,
`--help`, `--version`.

### Storing the data elsewhere

The engine does not have to keep its records in a file next to itself.
`BETTER_AUTH_STORE` picks where they go:

| store | where the records live |
|---|---|
| `sqlite` | the SQLite file named by `--data` |
| `memory` | in the process, and gone when it stops |
| `http` | a database service behind an HTTP API |
| `tcp` | a database service on a socket |

Both remote stores speak one protocol — a JSON request per operation, a
JSON reply per operation — so a service can answer over HTTP, over TCP,
or both. The binary serves that protocol itself:

```console
$ better-auth-core --store-server http --listen 0.0.0.0:8801 --data auth.db
$ BETTER_AUTH_STORE=http BETTER_AUTH_STORE_URL=http://db.internal:8801 \
  better-auth-core --listen 0.0.0.0:8787
```

The rows land in `auth.db` on the other machine, and the engine's routes
do not know the difference.

---

## Routes

Sixty-two paths, all under `/api/auth` — sixty-four method-and-path
pairs, because the two callback paths answer both GET and POST.

### Core

| method | path | answers |
|---|---|---|
| GET | `/ok` | `{ok: true}` |
| GET | `/error` | a deliberate 500, for testing |
| GET | `/get-session` | `{session, user}`, or `null` |
| GET | `/list-sessions` | the caller's sessions, without their tokens |
| POST | `/revoke-session` | `{status}` — takes a `token` |
| POST | `/revoke-sessions` | `{status: true}` — ends every session |
| POST | `/revoke-other-sessions` | `{status: true}` — ends all but this one |
| POST | `/sign-up/email` | `{token, refreshToken, user}` |
| POST | `/sign-in/email` | `{token, refreshToken, user}` |
| POST | `/sign-out` | `{success: true}` |
| POST | `/update-user` | `{status: true, user}` |
| POST | `/change-password` | a fresh `{token, refreshToken, user}` |
| POST | `/set-password` | `{status: true}` |
| POST | `/change-email` | `{status: true}` — by mail |
| POST | `/delete-user` | `{success: true}` |
| GET | `/verify-email` | tokens, or a redirect |
| POST | `/send-verification-email` | `{status: true}` |
| POST | `/request-password-reset` | `{status: true, message}` |
| POST | `/forget-password` | the same, under its other name |
| POST | `/reset-password` | `{status: true}` |
| GET | `/reset-password/:token` | a redirect to the app |

### Username

| method | path | answers |
|---|---|---|
| POST | `/sign-in/username` | `{token, refreshToken, user}` |
| POST | `/is-username-available` | `{available}` |

### Anonymous

| method | path | answers |
|---|---|---|
| POST | `/sign-in/anonymous` | `{token, refreshToken, user}` |

### Bearer

| method | path | answers |
|---|---|---|
| POST | `/refresh-token` | `{token, refreshToken, user}` — the refresh token is the bearer |

### Magic link

| method | path | answers |
|---|---|---|
| POST | `/sign-in/magic-link` | `{status: true}` — and a mail |
| GET | `/magic-link/verify` | tokens, or a redirect |

### Email OTP

| method | path | answers |
|---|---|---|
| POST | `/email-otp/send-verification-otp` | `{success: true}` |
| POST | `/email-otp/check-verification-otp` | `{success: true}` — does not consume the code |
| POST | `/email-otp/verify-email` | `{status: true, token, user}` |
| POST | `/sign-in/email-otp` | `{token, refreshToken, user}` |
| POST | `/email-otp/reset-password` | `{status: true}` |

### OAuth

| method | path | answers |
|---|---|---|
| POST | `/sign-in/social` | `{url, redirect: true}` |
| GET, POST | `/callback/:providerId` | the provider's answer |
| GET | `/list-accounts` | the caller's linked providers |
| POST | `/unlink-account` | `{status: true}` |
| POST | `/link-social-account` | `{url, redirect: true}` |
| POST | `/sign-in/oauth2` | the older name for `/sign-in/social` |
| POST | `/oauth2/link` | the older name for `/link-social-account` |
| GET, POST | `/oauth2/callback/:providerId` | the older name for `/callback/:providerId` |

### Two factor

| method | path | answers |
|---|---|---|
| POST | `/two-factor/enable` | `{totpURI, backupCodes}` |
| POST | `/two-factor/disable` | `{status: true}` |
| POST | `/two-factor/get-totp-uri` | `{totpURI}` |
| POST | `/two-factor/send-otp` | `{status: true}` — and a mail |
| POST | `/two-factor/verify-otp` | tokens |
| POST | `/two-factor/verify-totp` | tokens |
| POST | `/two-factor/verify-backup-code` | tokens |
| POST | `/two-factor/generate-backup-codes` | `{status: true, backupCodes}` |

### Admin

Every one of these needs `role == "admin"`; anything else is
`403 YOU_ARE_NOT_ALLOWED_TO_ACCESS_THIS`.

| method | path | answers |
|---|---|---|
| POST | `/admin/create-user` | `{user}` |
| GET | `/admin/list-users` | `{users, total}` |
| GET | `/admin/get-user` | `{user}` — by `userId` or `email` |
| POST | `/admin/update-user` | `{user}` |
| POST | `/admin/set-role` | `{user}` |
| POST | `/admin/set-user-password` | `{status: true}` — and the user's sessions end |
| POST | `/admin/ban-user` | `{user}` — and the user's sessions end |
| POST | `/admin/unban-user` | `{user}` |
| POST | `/admin/list-user-sessions` | `{sessions}` |
| POST | `/admin/revoke-user-session` | `{success: true}` |
| POST | `/admin/revoke-user-sessions` | `{success: true}` |
| POST | `/admin/impersonate-user` | a token for the target, marked `imp` |
| POST | `/admin/stop-impersonating` | the admin's own fresh token |
| POST | `/admin/remove-user` | `{success: true}` |

`/admin/list-users` understands `searchValue`, `searchField`,
`searchOperator`, `filterField`, `filterValue`, `filterOperator`,
`sortBy`, `sortDirection`, `limit` and `offset`.

A ban ends the user's sessions at once. A ban with `banExpiresIn` lifts
itself: the next sign-in after the expiry finds the ban over and clears
it.

---

## Sign-in flows

**The password hash is Better Auth's.** scrypt at `N=16384, r=16, p=1`,
stored as `<salt-hex>:<key-hex>` — the same bytes the JavaScript stack
writes, so the two can share a table and a user can sign in with either
one. The salt is the *characters* of the hex string rather than the bytes
it stands for, because that is what Node's `crypto.scrypt` is handed.
Hashes written by this crate before it moved to scrypt still verify, and
are replaced with a scrypt hash the next time the password is given.

**Email and password.** `POST /sign-up/email` with `{email, password,
name}` creates the user and signs them in. The address is unverified
until the link in the mail is followed; the link is single use and
lasts a day.

**Password reset.** `POST /request-password-reset` always answers the
same way, whether or not the address exists — no address is confirmed
by the answer. The link lasts fifteen minutes and works once.

**Magic link.** `POST /sign-in/magic-link` mails a link with 32 random
bytes in it; the server keeps only its SHA-256. Following the link
signs the reader in and consumes it.

**Email OTP.** A six-digit code, good for five minutes, keyed by
`(type, address)`. Five wrong tries delete the code: the address is
locked out until a new one is requested.

**Anonymous.** `POST /sign-in/anonymous` creates a user with a
placeholder address (`<id>@anonymous.local`) and no credentials. When
that visitor later signs up with an email, a code, a magic link or a
provider, the *same user row* is upgraded — the id never changes, so
nothing they did is lost.

---

## Two-factor

`POST /two-factor/enable` returns a `otpauth://` URI and ten backup
codes. The shared secret is sealed with AES-256-GCM under a key derived
from the server secret; only the seal is stored. The factor is not
switched on until `verify-totp` accepts a code, so a mistyped secret
cannot lock anyone out.

Backup codes are stored as SHA-256 digests and are spent as they are
used. `generate-backup-codes` replaces the set.

`send-otp` and `verify-otp` do the same job by mail, for people who
cannot run an authenticator.

---

## OAuth

Thirty-two providers are built in. A provider is offered when both of
its variables are set, for example:

```sh
export GOOGLE_CLIENT_ID=…
export GOOGLE_CLIENT_SECRET=…
```

The names are `<PROVIDER>_CLIENT_ID` and `<PROVIDER>_CLIENT_SECRET`
for: `google`, `github`, `apple`, `atlassian`, `cognito`, `discord`,
`dropbox`, `facebook`, `figma`, `gitlab`, `huggingface`, `kakao`,
`kick`, `line`, `linear`, `linkedin`, `microsoft`, `naver`, `notion`,
`paybin`, `paypal`, `polar`, `reddit`, `roblox`, `salesforce`, `slack`,
`spotify`, `tiktok`, `twitch`, `twitter`, `vk` and `zoom`.

Each provider brings its own details: PKCE where the provider wants it,
Basic or body authentication at the token endpoint, JSON or form token
requests, `client_key` instead of `client_id` for TikTok, `form_post`
callbacks for Apple, the `Client-Id` header for Twitch, a second call
for GitHub's address, and a synthetic `{provider}_{id}@oauth.local`
address for providers that do not give one.

The round trip:

1. `POST /sign-in/social` with `{provider}` answers `{url, redirect: true}`.
   The URL carries the state and, where the provider wants it, the PKCE
   challenge.
2. The provider sends the browser back to `/api/auth/callback/:providerId`
   with a code and the state. The state is stored under its hash, lives
   ten minutes and works once.
3. The code is exchanged, the profile is read, the account is linked and
   the user is signed in. The browser gets *our* token pair — the
   provider's access token is never shown to it.

With a `callbackURL`, the answer is a `302` to that address with
`?token=…&refreshToken=…` on it.

---

## Admin

An admin is a user whose `role` is `admin`. The first one has to be
made in the database (or through the library); after that the admin
routes can make more.

Impersonation issues a real session for the target, with the admin
named in the `imp` claim and in the session's `impersonatedBy`. The
token works, and every request made with it says who is really behind
it. `stop-impersonating` gives the admin their own session back.

---

## Data

Five tables, created on first use.

| table | columns |
|---|---|
| `user` | id, name, email, emailVerified, image, username, displayUsername, isAnonymous, twoFactorEnabled, role, banned, banReason, banExpires, createdAt, updatedAt, data |
| `session` | id, userId, token, expiresAt, ipAddress, userAgent, impersonatedBy, createdAt, updatedAt, data |
| `account` | id, userId, accountId, providerId, accessToken, refreshToken, idToken, accessTokenExpiresAt, refreshTokenExpiresAt, scope, password, createdAt, updatedAt, data |
| `verification` | id, identifier, value, expiresAt, createdAt, updatedAt, data |
| `twoFactor` | id, userId, secret, backupCodes, data |

Everything one-time — email verification, password reset, magic links,
OAuth state, one-time codes — is a `verification` row whose `value` is
the SHA-256 of the token. The token itself is never stored.

---

## As a library

```rust
use std::sync::Arc;
use better_auth_core::auth::{Auth, AuthConfig};
use better_auth_core::store::SqliteAuthStore;
use better_auth_core::routes;

let config = AuthConfig::new("https://app.example.com", &secret);
let store = SqliteAuthStore::open("auth.db")?;
let auth = Arc::new(Auth::new(Arc::new(store), config));

let response = routes::respond(Arc::clone(&auth), request).await;
```

`AuthStore` is an eight-method async trait, so the engine can sit on
any storage: `MemoryStore` and `SqliteAuthStore` ship with the crate,
and `HttpAuthStore` and `TcpAuthStore` reach a database service over a
socket. `HttpClient` and `Mailer` are traits too — `PlainHttpClient`,
`CurlHttpClient`, `ConsoleMailer` and `MemoryMailer` are the ones that
come with it.

---

## Tests

```console
$ cargo test
```

116 tests: the unit tests of the engine, and eight integration suites
that drive the real route table — tokens and sessions, the email flows,
two-factor, one-time codes and magic links, anonymous sign-in and
usernames, OAuth, the admin plugin, and the route table itself.

```console
$ ./tests/client/run.sh
```

20 more, against the **published** Better Auth client: `better-auth@1.7.27`
from npm, the same package an application installs, talking to the
release binary over HTTP. They pin down the route names, the request
bodies and the answer shapes — and the `set-auth-token` header that
carries the session in cookie-free mode. See
[`tests/client/README.md`](tests/client/README.md).

The Rust crate has no dependencies from a registry, and the client suite
does not change that: `node_modules` is a test-only dependency and never
reaches the binary.
