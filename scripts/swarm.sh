#!/usr/bin/env bash
# swarm.sh — one-command launcher for Swarm CI.
#
#   ./scripts/swarm.sh              load .env → build → serve dashboard+API
#   ./scripts/swarm.sh --mock       force offline mode (simulated workers)
#   ./scripts/swarm.sh --release    optimized build
#
# Loads the repo-root .env automatically (never overrides already-exported
# vars), pre-flights provider keys and the port, then execs the server in the
# foreground. Ctrl-C stops it.
set -euo pipefail
cd "$(dirname "$0")/.."

RELEASE=0
MOCK=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    -r|--release)  RELEASE=1 ;;
    --mock)        MOCK=1 ;;
    -h|--help)     awk 'NR>1 && /^#/ { sub(/^# ?/, ""); print }' "$0" ; exit 0 ;;
    *) echo "✖ unknown flag: $1 (try --help)" >&2 ; exit 2 ;;
  esac
  shift
done

# --- env ---------------------------------------------------------------------
# .env is gitignored and optional; already-exported variables win over it.
if [[ -f .env ]]; then
  set -a
  # shellcheck disable=SC1091
  source .env
  set +a
fi
(( MOCK )) && export LLM_PROVIDER=mock
export LLM_PROVIDER="${LLM_PROVIDER:-mock}"

# --- provider pre-flight -------------------------------------------------------
case "$LLM_PROVIDER" in
  groq)      [[ -n "${GROQ_API_KEY:-}" ]]      || { echo "✖ LLM_PROVIDER=groq but GROQ_API_KEY is not set (.env loaded?)" >&2 ; exit 1 ; } ;;
  google)    [[ -n "${GOOGLE_API_KEY:-}" ]]    || { echo "✖ LLM_PROVIDER=google but GOOGLE_API_KEY is not set" >&2 ; exit 1 ; } ;;
  anthropic) [[ -n "${ANTHROPIC_API_KEY:-}" ]] || { echo "✖ LLM_PROVIDER=anthropic but ANTHROPIC_API_KEY is not set" >&2 ; exit 1 ; } ;;
  mock)      echo "ℹ  mock provider: simulated implementer, dev-mode merge gate" ;;
  *)         echo "⚠  unknown LLM_PROVIDER '$LLM_PROVIDER' — attempting anyway" >&2 ;;
esac

# --- port pre-flight -----------------------------------------------------------
BIND="${SWARM_BIND:-0.0.0.0:3000}"
PORT="${BIND##*:}"
if (exec 3<>"/dev/tcp/127.0.0.1/$PORT") 2>/dev/null; then
  echo "✖ port $PORT is already in use — another swarm-api running? try: pkill -f swarm-api" >&2
  exit 1
fi

# --- banner ----------------------------------------------------------------------
MODEL=""
case "$LLM_PROVIDER" in
  groq)      MODEL="${GROQ_MODEL:-openai/gpt-oss-120b}" ;;
  google)    MODEL="${GOOGLE_MODEL:-gemini-3.6-flash}" ;;
  anthropic) MODEL="${ANTHROPIC_MODEL:-claude-sonnet-4-20250514}" ;;
esac
echo ""
echo "  🔥 PHOENIX CI — self-healing PR review"
echo "     provider : $LLM_PROVIDER${MODEL:+ ($MODEL)}"
echo "     bind     : http://localhost:$PORT   (landing at / · dashboard at /app)"
echo "     demo     : pick a PR on /app, submit, then hit ⚡KILL mid-flight"
echo ""

RUN=(cargo run -p swarm-api)
(( RELEASE )) && RUN+=(--release)
exec "${RUN[@]}"
