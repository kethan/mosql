# A todo app, on a Windows desktop

Everything below runs on Windows in `cmd.exe` or in PowerShell, against a
Nanobase built from this repository. It takes about five minutes and
leaves you with a working todo list, a console you can manage apps from,
and a page that talks to the platform over HTTP.

Nothing here is a special mode: the console, the API and this page use the
same endpoints a script would.

---

## 1. Build it

```
git clone https://github.com/kethan/mosql.git
cd mosql\nanobase
cargo build --release
```

The binary is `.\target\release\nanobase.exe`.

> Building needs a C compiler, because SQLite is compiled from the
> vendored amalgamation rather than downloaded. Visual Studio Build Tools
> ("Desktop development with C++") is enough.

## 2. Create the owner and start the server

```
.\target\release\nanobase.exe init --email owner@example.com --password "correct-horse-battery"
.\target\release\nanobase.exe serve --host 127.0.0.1 --port 8090
```

`init` writes the platform's state to `.\data\nanobase.db` and prints the
owner. `serve` stays in the foreground — leave that window open and use a
second one for everything else.

You should see:

```
nanobase 0.1.0 serving on http://127.0.0.1:8090
  api:  http://127.0.0.1:8090/api
  ui:   http://127.0.0.1:8090/
```

## 3. Open the console

<http://127.0.0.1:8090/>

Sign in with `owner@example.com` and the password. The console is the
platform's own web interface: it creates apps, searches them, pages
through them, and opens each one's database, auth and access settings.

## 4. Create the app

Either in the console — **New app**, then name it `todos` — or from the
command line:

```
curl.exe -s -X POST http://127.0.0.1:8090/api/apps -H "Content-Type: application/json" -d "{\"name\":\"todos\"}"
```

It answers with the app's id:

```json
{"id":"app_7585e98765bed55cd83fce11","createdAt":"…","doc":{…}}
```

Keep that id: every command below needs it. In `cmd.exe`:

```
set APP=app_7585e98765bed55cd83fce11
```

In PowerShell:

```powershell
$app = "app_7585e98765bed55cd83fce11"
```

## 5. Get a token

The API needs a bearer token. The owner signs in for one.

**cmd.exe**

```
curl.exe -s -X POST http://127.0.0.1:8090/api/auth/login -H "Content-Type: application/json" -d "{\"email\":\"owner@example.com\",\"password\":\"correct-horse-battery\"}"
```

**PowerShell** (use `curl.exe`, not `curl` — in Windows PowerShell 5.1
`curl` is an alias for `Invoke-WebRequest`, which takes different
arguments)

```powershell
curl.exe -s -X POST http://127.0.0.1:8090/api/auth/login -H "Content-Type: application/json" -d '{"email":"owner@example.com","password":"correct-horse-battery"}'
```

Both answer with the token and the owner:

```json
{"token":"eyJhbGciOiJIUzI1NiIs…","user":{"id":"…","email":"owner@example.com",…}}
```

**cmd.exe**

```
set TOKEN=eyJhbGciOiJIUzI1NiIs…
```

**PowerShell**

```powershell
$token = "eyJhbGciOiJIUzI1NiIs…"
```

The token lasts three days. `nanobase login owner@example.com` prints
another one if you need one.

## 6. The four operations on rows

Everything below works in `cmd.exe` and in PowerShell with the same
quoting as step 5.

### List the rows

```
curl.exe -s "http://127.0.0.1:8090/api/apps/%APP%/db/todos?limit=50" -H "Authorization: Bearer %TOKEN%"
```

```json
{"items":[{"title":"buy milk","done":false,"id":"u8afa17a18a9df741791908e2eaf89576"}],"total":1}
```

### Insert a row

```
curl.exe -s -X POST "http://127.0.0.1:8090/api/apps/%APP%/db/todos" -H "Authorization: Bearer %TOKEN%" -H "Content-Type: application/json" -d "{\"row\":{\"title\":\"buy milk\",\"done\":false}}"
```

```json
{"title":"buy milk","done":false,"id":"u8afa17a18a9df741791908e2eaf89576"}
```

### Change the rows that match

The `filter` says which rows; the `changes` say what to do to them.

```
curl.exe -s -X PATCH "http://127.0.0.1:8090/api/apps/%APP%/db/todos" -H "Authorization: Bearer %TOKEN%" -H "Content-Type: application/json" -d "{\"filter\":{\"title\":\"buy milk\"},\"changes\":{\"done\":true}}"
```

```json
{"title":"buy milk","done":true,"id":"u8afa17a18a9df741791908e2eaf89576"}
```

### Delete the rows that match

```
curl.exe -s -X DELETE "http://127.0.0.1:8090/api/apps/%APP%/db/todos" -H "Authorization: Bearer %TOKEN%" -H "Content-Type: application/json" -d "{\"filter\":{\"title\":\"buy milk\"}}"
```

```json
{"removed":1}
```

## 7. Open the todo page

The page in this folder is a plain HTML file that talks to the app's rows.
It needs three things: the API's address, the app's id, and a token.

### Serve the page over HTTP

A page opened from the disk is a different origin from the API, and a
browser will not let it read the answer. Serve it instead — this script
needs nothing but Windows itself:

```
powershell -ExecutionPolicy Bypass -File serve.ps1
```

Then open <http://localhost:8000/>.

> If you have Python, `py -m http.server 8000` does the same thing.

### Let the page's origin through

The API only answers a browser that it has been told about. Tell it about
the page's origin, on the app:

```
curl.exe -s -X PATCH "http://127.0.0.1:8090/api/apps/%APP%" -H "Authorization: Bearer %TOKEN%" -H "Content-Type: application/json" -d "{\"config\":{\"cors\":{\"enabled\":true,\"origins\":[\"http://localhost:8000\"]}}}"
```

The page's own origin is now allowed, and the browser will let it read the
answers.

### Fill in the three fields

In the page:

| field | value |
| --- | --- |
| API address | `http://127.0.0.1:8090` |
| App id | the `app_…` from step 4 |
| Token | the token from step 5 |

Press **Connect**. The list appears. Add a todo, tick it, delete it —
each one is a request to the app's rows, and each one is visible in
`curl` at the same time.

---

## The security bits, and where they live

Three things came up while building the console, and all three are on by
the platform's own defaults.

### Security headers

Every answer the platform gives carries them, the console's HTML
included:

```
curl.exe -s -D - -o NUL http://127.0.0.1:8090/api/health
```

```
content-security-policy: default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'
x-content-type-options: nosniff
x-frame-options: DENY
referrer-policy: no-referrer
cross-origin-resource-policy: same-origin
permissions-policy: accelerometer=(), camera=(), geolocation=(), …
```

The console's own page gets a different policy, because it has to load
its own script and stylesheet:

```
content-security-policy: default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; frame-ancestors 'none'; …
```

### CORS

Two levels, and the app's is the one that matters for a front end:

* **The platform** — `--cors "http://localhost:8000,http://127.0.0.1:8000"`
  on `nanobase serve`, or `NANOBASE_CORS_ORIGINS` in the environment.
  With neither, no browser origin is allowed at all.
* **The app** — `config.cors` in the app's own document, which is what
  step 7 sets:

  ```json
  {"enabled": true, "origins": ["http://localhost:8000"], "allowCredentials": false, "maxAge": 600}
  ```

An origin that is not on the list gets no `Access-Control-Allow-Origin`
header, and the browser refuses to hand it the answer. That is the
correct answer, not a missing one.

### The rate limit

Off by default. Turn it on:

```
.\target\release\nanobase.exe serve --host 127.0.0.1 --port 8090 --rate-limit 120
```

A client that goes over the allowance gets `429` and a `Retry-After`:

```
curl.exe -s -D - http://127.0.0.1:8090/api/health
```

```
HTTP/1.1 429 Too Many Requests
retry-after: 59
{"code":"TOO_MANY_REQUESTS","message":"too many requests: slow down"}
```

The limit is per client address, over a sliding minute, so a burst does
not buy a second allowance immediately afterwards.

---

## What is still rough

* **The page uses the owner's token.** The app document already has the
  shape for per-app users (`config.auth.features.emailPassword`), but the
  platform does not yet serve an app's own sign-in endpoint, so the page
  asks for a platform token. An app's own users are the next thing to
  build.
* **The console edits the app document.** The database, auth and access
  tabs write into `config`, which is the same document `nanobase app
  export` writes. Nothing is lost, and nothing is hidden — but a change
  made in the console is a change to the app's document.
* **`serve` is single-file.** The platform's state is one SQLite file and
  the console is inside the binary, so there is nothing to install
  beside it. That also means the console is the version that shipped with
  the binary, not a live-updating one.
