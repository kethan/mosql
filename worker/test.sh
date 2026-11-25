#!/usr/bin/env bash
#
# The Worker, end to end.
#
# Builds the module, creates the local database, starts the Worker and
# talks to it: a sign-up, a sign-in, the session that comes back, a blob
# written and read, and a sign-out that makes the session null.
#
# Needs `wrangler` on the PATH. Everything it touches is local: the
# database, the cache and the bucket all live under `.wrangler/state`.
#
set -euo pipefail
cd "$(dirname "$0")"

PORT="${PORT:-8797}"
BASE="http://127.0.0.1:$PORT"
SECRET="a-local-development-secret-of-at-least-32-bytes"
EMAIL="worker-$(date +%s)@example.com"
PASSWORD="correct-horse-battery"

step() { printf '\n\033[1m==> %s\033[0m\n' "$1"; }
fail() { printf '\033[31mFAIL: %s\033[0m\n' "$1" >&2; exit 1; }

step "building the module"
./build.sh

step "creating the database"
wrangler d1 execute mosql-auth --local --file=schema.sql >/dev/null

step "starting the Worker"
wrangler dev --port "$PORT" --local --ip 127.0.0.1 \
  --var "AUTH_SECRET:$SECRET" \
  --var "AUTH_BASE_URL:$BASE" \
  --persist-to .wrangler/state >/tmp/mosql-worker-test.log 2>&1 &
WORKER_PID=$!
trap 'kill "$WORKER_PID" 2>/dev/null || true' EXIT

for _ in $(seq 1 60); do
  if curl -fsS "$BASE/api/auth/ok" >/dev/null 2>&1; then break; fi
  sleep 0.5
done
curl -fsS "$BASE/api/auth/ok" >/dev/null || fail "the Worker did not start"

post() {
  curl -fsS -X POST "$BASE$1" -H 'content-type: application/json' -d "$2"
}

step "signing up"
SIGNED_UP=$(post /api/auth/sign-up/email "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\",\"name\":\"Ada\"}")
TOKEN=$(printf '%s' "$SIGNED_UP" | python3 -c 'import sys,json; print(json.load(sys.stdin)["token"])')
[ -n "$TOKEN" ] || fail "no token came back: $SIGNED_UP"
echo "    signed up $EMAIL"

step "signing in"
SIGNED_IN=$(post /api/auth/sign-in/email "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\"}")
printf '%s' "$SIGNED_IN" | python3 -c '
import sys, json
body = json.load(sys.stdin)
assert body["user"]["email"], body
assert body["token"], body
' || fail "the sign-in did not answer with a user"
echo "    signed in"

step "reading the session"
SESSION=$(curl -fsS "$BASE/api/auth/get-session" -H "authorization: Bearer $TOKEN")
printf '%s' "$SESSION" | python3 -c '
import sys, json
body = json.load(sys.stdin)
assert body["session"], body
assert body["user"]["email"], body
' || fail "the session did not come back: $SESSION"
echo "    the session came back"

step "writing and reading a blob"
curl -fsS -X PUT "$BASE/api/blob/notes/hello.txt" -H 'content-type: text/plain' \
  -d 'hello from the worker' >/dev/null || fail "the blob was not written"
BLOB=$(curl -fsS "$BASE/api/blob/notes/hello.txt")
[ "$BLOB" = "hello from the worker" ] || fail "the blob came back as: $BLOB"
curl -fsS -X DELETE "$BASE/api/blob/notes/hello.txt" >/dev/null
CODE=$(curl -s -o /dev/null -w '%{http_code}' "$BASE/api/blob/notes/hello.txt")
[ "$CODE" = "404" ] || fail "the blob was not deleted (status $CODE)"
echo "    the blob round-tripped"

step "signing out"
curl -fsS -X POST "$BASE/api/auth/sign-out" -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' -d '{}' >/dev/null || fail "the sign-out failed"
AFTER=$(curl -fsS "$BASE/api/auth/get-session" -H "authorization: Bearer $TOKEN")
[ "$AFTER" = "null" ] || fail "the session is still there after signing out: $AFTER"
echo "    the session is gone"

printf '\n\033[32mThe Worker answers every route.\033[0m\n'
