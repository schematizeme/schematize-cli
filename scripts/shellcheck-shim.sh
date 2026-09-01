#!/bin/sh
# Roda o `shellcheck -s sh` no shim — o MESMO comando que o CI roda.
#
# POR QUE E UM SCRIPT E NAO UMA LINHA NO verificar.sh
# ---------------------------------------------------
# O `shellcheck` nao esta instalado em toda maquina de desenvolvimento, mas ESTA no runner
# do GitHub. Uma etapa que simplesmente sumisse quando o binario falta deixaria o gate local
# afirmando VERDE sobre uma verificacao que ele nem tentou — e foi assim que o SC2016 chegou
# ao CI: a etapa local nunca rodou shellcheck, so a bateria de shells.
#
# Aqui: usa o binario se existir, senao cai pro container oficial, e se nao houver NENHUM
# dos dois REPROVA dizendo o que falta. Gate que nao consegue verificar nao diz VERDE.
set -eu

R=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
ALVO="packaging/ops-shell/schematize-ops-shell"
[ -f "$R/$ALVO" ] || { echo "nao achei $ALVO" >&2; exit 2; }

if command -v shellcheck >/dev/null 2>&1; then
    exec shellcheck -s sh "$R/$ALVO"
fi

if command -v docker >/dev/null 2>&1; then
    exec docker run --rm -v "$R/packaging/ops-shell":/w -w /w \
        koalaman/shellcheck:stable -s sh schematize-ops-shell
fi

echo "shellcheck AUSENTE e docker AUSENTE — nao da pra verificar o shim nesta maquina." >&2
echo "Instale o shellcheck (apt install shellcheck) ou o docker. Nao vou dizer VERDE" >&2
echo "sobre uma verificacao que nao rodou." >&2
exit 2
