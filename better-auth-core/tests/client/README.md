# Client compatibility tests

These tests drive the **published** Better Auth client — `better-auth@1.7.27`
from npm, the same package an application installs — against the server this
crate builds. Nothing about the server is faked: the client talks to the
release binary over HTTP.

```console
$ ./run.sh
```

`run.sh` installs the client if `node_modules` is missing and builds the
server if the release binary is missing. The server is started on a free
port with a throwaway SQLite file, and stopped when the tests end.

## What is being proved

The client maps its methods onto route names — `client.signUp.email(...)` is
`POST /sign-up/email`, `client.getSession()` is `GET /get-session` — and the
plugin clients say which method each of their routes takes. So every test
here is a statement about a route name, a request body or an answer shape.

The client keeps its session in a cookie, which a server-to-server caller
cannot do. The `bearer` plugin on the server is what makes the cookie-free
mode work: it reads `Authorization: Bearer`, and it answers with the token
in the `set-auth-token` header. `bearer.mjs` is the other half of that
contract — the twenty lines a native app or a service would write — and it
is the only piece of code here that is not Better Auth's.

| test | what it pins down |
|---|---|
| sign-up | `{token, refreshToken, user}`, and no password in the user |
| get-session | `{session, user}` with the session's digest left out |
| list-sessions | the array, `isCurrent` on one of them |
| update-user | `{status: true, user}` |
| sign-out | `{success: true}`, and the session is gone afterwards |
| wrong password | 401 `INVALID_EMAIL_OR_PASSWORD` |
| refresh-token | rotation, and the old token refused |
| anonymous | a placeholder address and `isAnonymous` |
| username | sign-in by username, 401 `INVALID_USERNAME_OR_PASSWORD` |
| email OTP | the code, the sign-in, `emailVerified` |
| two-factor | the `otpauth://` URI and ten backup codes |
| magic link | 400 `MAIL_NOT_CONFIGURED` when there is no mailer |
| admin | 403 `YOU_ARE_NOT_ALLOWED_TO_ACCESS_THIS` |
| routes | 404 for an unknown path, 405 for the wrong method |
| bearer | the token comes back in `set-auth-token`, no cookie is set, and the header and the body agree |
| bearer | a token sent as `Authorization: Bearer` is accepted |

## Note on dependencies

The Rust crate itself has no dependencies from a registry, and nothing here
changes that: `node_modules` is a test-only dependency of the compatibility
suite and never reaches the binary. The `deps` stage of `verify.sh` still
checks the crate's own manifest.
