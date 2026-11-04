#!/usr/bin/env bash
# Runs two Caraxyn instances side by side, "a" and "b", each with its own data
# folder, to try invites and shares on one machine. Ctrl-C stops both.
#
# "a" runs the dev server (the UI reloads on changes); "b" uses the same server.
# Their data is kept in target/two-instances/ between runs; delete it to start fresh.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
app="$root/crates/caraxyn"
data="$root/target/two-instances"
dev_url="http://localhost:5173" # build.devUrl in tauri.conf.json
mkdir -p "$data/a" "$data/b"

[ -d "$app/ui/node_modules" ] || (cd "$app/ui" && npm ci)

# On exit (Ctrl-C, or an error), stop everything this script started.
trap 'trap - EXIT; kill 0' EXIT

(cd "$app" && CARAXYN_DATA_DIR="$data/a" exec ui/node_modules/.bin/tauri dev) &
a=$!

echo "waiting for the dev server at $dev_url ..."
until curl -s -o /dev/null "$dev_url"; do
  kill -0 "$a" 2>/dev/null || { echo "instance a stopped before the dev server came up" >&2; exit 1; }
  sleep 1
done

(cd "$root" && CARAXYN_DATA_DIR="$data/b" exec cargo run -p caraxyn) &

wait
