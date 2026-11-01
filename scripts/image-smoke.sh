#!/usr/bin/env bash
# image-smoke.sh — the smoke check of the server image (KAIROS-T-0336).
#
#   scripts/image-smoke.sh IMAGE
#
# Starts the image with a Postgres next to it on a private Docker network (no
# port of the host), and checks:
#
#   1. the image gets healthy;
#   2. PID 1 is the init of the image (tini), not kairos-server;
#   3. orphaned children are reaped: the check leaves processes that end later
#      (as a curl of the healthcheck or a git of the code index builder does),
#      runs the healthcheck again and again for KAIROS_SMOKE_WATCH_SECS
#      (default 60), and then finds no zombie process;
#   4. `docker stop` ends the server cleanly: exit code 0, within the stop
#      timeout, and "shutdown complete" in the log.
#
# With KAIROS_SMOKE_EXPECT_CLI=1 (the release workflow), it also checks that
# the image serves the four CLI tarballs at /cli/<file>, that each matches
# its checksum, and that /version names them (KAIROS-T-0745).
#
# It removes its containers and its network at the end. The release workflow
# runs it on each image before the tag is published.

set -euo pipefail

image="${1:?usage: scripts/image-smoke.sh IMAGE}"
watch_secs="${KAIROS_SMOKE_WATCH_SECS:-60}"
stop_timeout=30
name="kairos-smoke-$$"
db="${name}-db"
app="${name}-app"

cleanup() {
  docker rm -f "$app" "$db" >/dev/null 2>&1 || true
  docker network rm "$name" >/dev/null 2>&1 || true
}
trap cleanup EXIT

fail() {
  echo "image smoke: FAIL: $*" >&2
  docker logs "$app" 2>&1 | tail -n 40 >&2 || true
  exit 1
}

docker network create "$name" >/dev/null
docker run -d --name "$db" --network "$name" \
  -e POSTGRES_USER=kairos -e POSTGRES_PASSWORD=smoke -e POSTGRES_DB=kairos \
  pgvector/pgvector:pg16 >/dev/null
for _ in $(seq 1 60); do
  docker exec "$db" pg_isready -U kairos -d kairos >/dev/null 2>&1 && break
  sleep 1
done

docker run -d --name "$app" --network "$name" \
  -e "DATABASE_URL=postgres://kairos:smoke@${db}:5432/kairos" \
  -e KAIROS_LOCAL_AUTH=true \
  -e KAIROS_SINGLE_TENANT=smoke \
  "$image" >/dev/null

# 1. Healthy. The HEALTHCHECK of the image runs each 10 s.
health=""
for _ in $(seq 1 90); do
  [ "$(docker inspect -f '{{.State.Running}}' "$app")" = "true" ] \
    || fail "the container stopped before it was healthy"
  health="$(docker inspect -f '{{.State.Health.Status}}' "$app")"
  [ "$health" = "healthy" ] && break
  sleep 2
done
[ "$health" = "healthy" ] || fail "the container is not healthy (${health})"
echo "image smoke: healthy"

# 1b. The CLI tarballs (KAIROS-T-0745).
if [ "${KAIROS_SMOKE_EXPECT_CLI:-0}" = "1" ]; then
  docker exec "$app" sh -c '
    set -e
    files="$(cd /usr/share/kairos/cli && ls ./*.tar.gz)"
    [ "$(echo "$files" | wc -l)" = "4" ] || { echo "not 4 tarballs: $files"; exit 1; }
    mkdir -p /tmp/cli-smoke && cd /tmp/cli-smoke
    for file in $files; do
      file="${file#./}"
      curl -fsS -o "$file" "http://localhost:8080/cli/$file"
      curl -fsS -o "$file.sha256" "http://localhost:8080/cli/$file.sha256"
      sha256sum -c "$file.sha256" >/dev/null
    done
    links="$(curl -fsS http://localhost:8080/version | grep -o "/cli/kairos-[^\"]*\.tar\.gz\"" | wc -l)"
    [ "$links" = "4" ] || { echo "/version names $links tarballs of the server, not 4"; exit 1; }
    rm -rf /tmp/cli-smoke
  ' || fail "the image does not serve the CLI tarballs"
  echo "image smoke: serves the 4 CLI tarballs"
fi

# 2. PID 1 is the init.
pid1="$(docker exec "$app" cat /proc/1/comm)"
[ "$pid1" = "tini" ] || fail "PID 1 is ${pid1}, not tini"
echo "image smoke: PID 1 is tini"

# 3. Orphans are reaped. Each `sh` ends at once, so its `sleep` goes to
# PID 1 and ends 2 s later.
for _ in 1 2 3 4 5; do
  docker exec "$app" sh -c 'sleep 2 >/dev/null 2>&1 & exit 0'
done
end=$((SECONDS + watch_secs))
while [ "$SECONDS" -lt "$end" ]; do
  docker exec "$app" curl -fsS http://localhost:8080/healthz >/dev/null \
    || fail "the healthcheck failed"
  sleep 5
done
zombies="$(docker exec "$app" sh -c '
  for stat in /proc/[0-9]*/stat; do
    read -r pid comm state rest < "$stat" 2>/dev/null || continue
    [ "$state" = "Z" ] && echo "$pid $comm"
  done
  true')"
[ -z "$zombies" ] || fail "zombie processes: ${zombies}"
echo "image smoke: no zombie after ${watch_secs} s of healthchecks"

# 4. A clean stop.
started=$SECONDS
docker stop -t "$stop_timeout" "$app" >/dev/null
took=$((SECONDS - started))
code="$(docker inspect -f '{{.State.ExitCode}}' "$app")"
[ "$code" = "0" ] || fail "exit code ${code} after docker stop"
[ "$took" -lt "$stop_timeout" ] || fail "the stop took ${took} s (the timeout is ${stop_timeout} s)"
# Read the log once into a variable: `docker logs | grep -q` fails under
# pipefail when grep exits at the match before docker logs is done (SIGPIPE).
log="$(docker logs "$app" 2>&1)"
grep -q '"signal":"SIGTERM"' <<<"$log" || fail "no SIGTERM in the log"
grep -q 'shutdown complete' <<<"$log" || fail "no clean shutdown in the log"
echo "image smoke: docker stop gave a clean shutdown in ${took} s"
echo "image smoke: PASS"
