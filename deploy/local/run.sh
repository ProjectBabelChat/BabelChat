#!/usr/bin/env bash
# Runs the cluster in cluster.toml on this machine: certificates on first
# run, then one process per service. Logs go to logs/<name>.log; Ctrl-C
# stops everything.
#
#   nix develop -c deploy/local/run.sh [--plan paid3]
#
# Options after the script name go to the Translation Cells. The operator
# certificate `ops` is made too:
#
#   target/debug/babel-admin --cluster deploy/local/cluster.toml --name ops placements
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
cd "$here"

cargo build --manifest-path "$root/Cargo.toml" --bins \
  -p babel-net -p babel-account -p babel-directory -p babel-message-cell \
  -p babel-translation-cell -p babel-engine -p babel-edge
bin="$root/target/debug"

"$bin/babel-pki" cluster --cluster cluster.toml
[ -f pki/ops.crt ] || "$bin/babel-pki" issue --dir pki --role admin --name ops
mkdir -p logs data

trap 'kill $(jobs -p) 2>/dev/null; wait' EXIT INT TERM

run() {
  local name=$1
  shift
  "$@" --cluster cluster.toml --name "$name" >"logs/$name.log" 2>&1 &
  echo "started $name (pid $!)"
}

run account "$bin/babel-account" --data data/account --backup-dir data/backups/account
run directory "$bin/babel-directory" --data data/directory --backup-dir data/backups/directory
for n in 0 1; do
  run "message-cell-$n" "$bin/babel-message-cell" --data "data/message-cell-$n" \
    --backup-dir "data/backups/message-cell-$n"
  run "translation-cell-$n" "$bin/babel-translation-cell" --data "data/translation-cell-$n" \
    --backup-dir "data/backups/translation-cell-$n" "$@"
done
run engine-0 "$bin/babel-engine"
run edge-0 "$bin/babel-edge"
run edge-1 "$bin/babel-edge"
# Nothing here calls the internet (the engine is a mock); the proxy is
# where an engine for a hosted model would send its requests.
"$bin/babel-egress" --listen 127.0.0.1:3128 >logs/egress.log 2>&1 &
echo "started egress (pid $!)"

echo "clients: http://127.0.0.1:7000 (edge-0), http://127.0.0.1:7001 (edge-1)"
wait
