#!/usr/bin/env bash
# Phase 5 failure drill: model-provider outage. Both provider keys are
# invalidated on purpose; the system must fail CLOSED — attempts exhaust,
# task becomes `failed`, exactly one `merge.gated`, gate NEVER opens.
set -u
cd "$(dirname "$0")/.."

set -a; source .env; set +a
export GROQ_API_KEY=gsk_invalid_on_purpose
export GOOGLE_API_KEY=google_invalid_on_purpose
export SWARM_MAX_ATTEMPTS=2

cargo build -p swarm-api || { echo "build failed"; exit 1; }
cargo run -p swarm-api > /tmp/swarm_drill_server.log 2>&1 &
SRV=$!
trap 'kill $SRV 2>/dev/null' EXIT

for i in $(seq 1 40); do curl -s -o /dev/null localhost:3000/tasks && break; sleep .5; done

RESP=$(curl -s -XPOST localhost:3000/tasks -H 'content-type: application/json' \
  -d '{"pr_id":"PR-DRILL","title":"Provider outage drill","bug_description":"ledger::sum skips the final element of the slice"}')
TID=$(echo "$RESP" | sed -E 's/.*"task_id":"([^"]+)".*/\1/')
echo "task=$TID"

S=""
for i in $(seq 1 40); do
  S=$(curl -s "localhost:3000/tasks/$TID" | grep -o '"status":"[a-z_]*"' | head -1)
  case "$S" in *merged*|*failed*) break ;; esac
  sleep 1
done
echo "final: $S"
GATE=$(grep -c 'MergeGated' /tmp/swarm_drill_server.log)
OPEN=$(grep -c 'MergeOpened' /tmp/swarm_drill_server.log)
echo "merge.gated events: $GATE | merge.opened events: $OPEN"

if [ "$S" = '"status":"failed"' ] && [ "$OPEN" -eq 0 ]; then
  echo "DRILL: PASS — system failed closed, gate never opened"
else
  echo "DRILL: FAIL"
fi
