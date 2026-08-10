#!/usr/bin/env bash
# install.sh — bootstrap do schematize CLI (Linux-first).
# Baixa o binário do release latest pra ~/.local/bin e confirma o PATH.
set -euo pipefail

REPO="schematizeme/schematize-cli"
ASSET="schematize-linux-x86_64"
DEST="${SCHEMATIZE_BIN_DIR:-$HOME/.local/bin}"
URL="https://github.com/$REPO/releases/latest/download/$ASSET"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) : ;;
  *) echo "Por enquanto só Linux x86_64. Compile do fonte: cargo install --path ." >&2; exit 1 ;;
esac

mkdir -p "$DEST"
echo "baixando $ASSET → $DEST/schematize"
curl -fSL -o "$DEST/schematize" "$URL"
chmod +x "$DEST/schematize"

echo "✓ instalado em $DEST/schematize"
if ! command -v schematize >/dev/null 2>&1; then
  echo "  Adicione ao PATH:  export PATH=\"$DEST:\$PATH\"  (bote no ~/.bashrc)"
fi
echo "Agora: schematize install --all   (instala as skills)"
echo "       schematize overdev enable  (liga o modo overdev)"
