#!/bin/sh
set -eu
release=/home/zero/atlantis-auth/releases/20261111
data=/home/zero/atlantis-auth/data
test -x "$release/target/debug/auth_api"
mkdir -p "$data"
chmod 700 "$data"
if docker inspect atlantis-auth-api >/dev/null 2>&1; then
  echo "An auth container already exists; inspect it before replacing."
  exit 1
fi
docker run -d --name atlantis-auth-api --restart unless-stopped \
  --user 1000:1000 --read-only --tmpfs /tmp:rw,noexec,nosuid,size=16m \
  --cap-drop ALL --security-opt no-new-privileges \
  -p 127.0.0.1:8087:8087 \
  -e GOOGLE_OAUTH_CLIENT_ID=615673989451-f294t6230ml283pmcrd4uqcgmbd8m9jp.apps.googleusercontent.com \
  -e ATLANTIS_SESSION_DB=/data/auth-sessions.sqlite \
  -e ATLANTIS_AUTH_BIND=0.0.0.0:8087 \
  -e ATLANTIS_AUTH_ORIGINS=https://play.tr7n7t7.com,https://lite.tr7n7t7.com,https://tr7n7t7.com,https://heavenlysent-680ac.web.app,https://heavenlybound-lite.pages.dev,https://radio.tr7n7t7.com,http://localhost:8765,http://localhost:8766,http://127.0.0.1:8765,http://127.0.0.1:8766 \
  -v "$release:/work:ro" -v "$data:/data" \
  rust:latest /work/target/debug/auth_api
