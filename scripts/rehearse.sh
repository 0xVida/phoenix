#!/usr/bin/env bash
# Phase 5 rehearsal: drives the EXACT demo flow over HTTP, the same requests
# the dashboard makes — submit → planner → implementer → ⚡KILL mid-flight →
# supervisor visibly reassigns → real cargo test passes → merge gate opens.
# Usage: scripts/rehearse.sh [runs] [kill_after_seconds]
set -u
cd "$(dirname "$0")/.."

set -a; source .env; set +a   # real providers; keys never printed
RUNS="${1:-2}"
KILL_AFTER="${2:-2}"

cargo build -p swarm-api || { echo "build failed"; exit 1; }
cargo run -p swarm-api > /tmp/swarm_rehearse_server.log 2>&1 &
SRV=$!
trap 'kill $SRV 2>/dev/null' EXIT

for i in $(seq 1 40); do curl -s -o /dev/null localhost:3000/tasks && break; sleep .5; done
timeout $(( RUNS * 60 + 30 )) curl -sN localhost:3000/events > /tmp/swarm_rehearse_sse.log &

PASS=0
for n in $(seq 1 "$RUNS"); do
  echo "===== REHEARSAL RUN $n/$RUNS ====="
  RESP=$(curl -s -XPOST localhost:3000/tasks -H 'content-type: application/json' \
    -d "{\"pr_id\":\"PR-REHEARSAL-$n\",\"title\":\"Ledger sum drops last element\",\"bug_description\":\"ledger::sum(&[1,2,3]) returns 3 but must return 6; it skips the final element of the slice\"}")
  echo "submit: $RESP"
  TID=$(echo "$RESP" | sed -E 's/.*"task_id":"([^"]+)".*/\1/')

  sleep "$KILL_AFTER"
  echo "⚡ kill -> $(curl -s -XPOST localhost:3000/tasks/$TID/kill)"

  S=""
  for i in $(seq 1 60); do
    S=$(curl -s "localhost:3000/tasks/$TID" | grep -o '"status":"[a-z_]*"' | head -1)
    case "$S" in *merged*|*failed*) break ;; esac
    sleep 1
  done
  echo "final: $S"
  if [ "$S" = '"status":"merged"' ]; then PASS=$((PASS+1)); echo "RESULT run $n: PASS"; else echo "RESULT run $n: FAIL"; fi
  sleep 1
done

echo "===== SUMMARY: $PASS/$RUNS rehearsals ended MERGED ====="
[ "$PASS" -eq "$RUNS" ]
