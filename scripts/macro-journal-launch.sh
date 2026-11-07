#!/usr/bin/env bash
# Lanceur Macro Journal (version release).
#
# Démarré depuis l'entrée .desktop (ou la ligne de commande) :
#   1. garde le serveur release `target/release/macro-journal-server` vivant
#      — relance un binaire ancien ou absent via /api/health et /api/verdicts ;
#   2. ouvre le navigateur sur http://127.0.0.1:3000, que le serveur serve
#      l'interface compilée depuis apps/web/dist.
# Journal : ~/.local/state/macro-journal/server.log

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERVER_BIN="$ROOT_DIR/target/release/macro-journal-server"
URL="http://127.0.0.1:3000"
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/macro-journal"
LOG="$STATE_DIR/server.log"

notify() {
  if command -v notify-send >/dev/null 2>&1; then
    notify-send "Macro Journal" "$1"
  fi
  echo "Macro Journal : $1" >&2
}

health() {
  curl -s --noproxy '*' -o /dev/null -w '%{http_code}' "$1" 2>/dev/null || echo 000
}

mkdir -p "$STATE_DIR"

if [ ! -x "$SERVER_BIN" ]; then
  notify "Binaire release introuvable : $SERVER_BIN (lancez cargo build --release -p macro-journal-server)."
  exit 1
fi

# Binaire déjà en écoute ? Un ancien serveur (sans /api/verdicts) est remplacé.
if [ "$(health "$URL/api/health")" = "200" ]; then
  if [ "$(health "$URL/api/verdicts")" = "200" ]; then
    echo "Serveur Macro Journal déjà à jour sur $URL — ouverture du navigateur." >&2
  else
    echo "Serveur obsolète détecté sur :3000 — remplacement par la version release." >&2
    pkill -f "macro-journal-server" 2>/dev/null || true
    sleep 1
    setsid nohup "$SERVER_BIN" >> "$LOG" 2>&1 &
  fi
else
  echo "Démarrage du serveur Macro Journal (release)…" >&2
  if [ -x "$ROOT_DIR/cot/update.sh" ]; then
    "$ROOT_DIR/cot/update.sh" >> "$LOG" 2>&1 \
      || echo "Avertissement : l'actualisation COT a échoué ; derniers graphiques conservés." >&2
  fi
  setsid nohup "$SERVER_BIN" >> "$LOG" 2>&1 &
fi

for _ in $(seq 1 50); do
  [ "$(health "$URL/api/health")" = "200" ] && break
  sleep 0.2
done

if [ "$(health "$URL/api/health")" != "200" ]; then
  notify "Le serveur n'a pas répondu — journal : $LOG."
  exit 1
fi

if command -v xdg-open >/dev/null 2>&1; then
  xdg-open "$URL" >/dev/null 2>&1 &
else
  firefox "$URL" >/dev/null 2>&1 &
fi
disown
