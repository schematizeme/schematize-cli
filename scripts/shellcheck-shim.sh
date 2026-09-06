#!/bin/sh
# Roda o `shellcheck` nos scripts de PRODUTO — o MESMO comando que o CI roda.
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
#
# POR QUE O install.sh ENTROU (2026-09-06)
# ----------------------------------------
# Ate hoje este gate cobria SO o shim do ops. O `install.sh` — 551 linhas que o CONTEXT.md
# chama de "codigo de produto, nao script auxiliar", e que e o primeiro contato de todo
# usuario com a casa — nao era lido por lint nenhum. O que estava la dentro:
#
#   * `warn` USADA em quatro pontos e NUNCA definida. Sob `set -euo pipefail` o nome nao
#     resolve, o shell sai 127 e a instalacao ABORTA — o aviso matava o instalador.
#   * uma continuacao `\` com comentario no meio, que engolia o resto do comando. O
#     `bash -n` passa nas duas: sao validas sintaticamente e erradas semanticamente.
#
# Nenhuma das duas e pegavel por `bash -n` nem pela bateria de shells. Sao exatamente o
# tipo de defeito que o shellcheck existe pra ver.
#
# POR QUE DOIS DIALETOS: sao dois contratos diferentes. O shim do ops roda no `/bin/sh` de
# qualquer distro (POSIX estrito, `-s sh`); o `install.sh` declara `#!/usr/bin/env bash` e
# usa `local`, entao cobra-lo como POSIX daria centenas de falso-positivo.
#
# POR QUE COPIA PRA UM TEMP ANTES DE MONTAR NO DOCKER: montar a raiz do repo com `:z` faz
# o SELinux RERROTULAR o repo inteiro (lento, e mexe em label de arquivo que nao e do
# container). Sem `:z`, em distro com SELinux ligado — openSUSE, Fedora, RHEL — o container
# leva "permission denied" e o gate nao roda. Copiar os dois arquivos pra um temp resolve
# os dois lados: rotulo so no temp, custo desprezivel.
set -eu

R=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

# <caminho-relativo>:<dialeto>
ALVOS='packaging/ops-shell/schematize-ops-shell:sh install.sh:bash'

for par in $ALVOS; do
    [ -f "$R/${par%:*}" ] || { echo "nao achei ${par%:*}" >&2; exit 2; }
done

if command -v shellcheck >/dev/null 2>&1; then
    rc=0
    for par in $ALVOS; do
        shellcheck -s "${par##*:}" "$R/${par%:*}" || rc=1
    done
    exit "$rc"
fi

if command -v docker >/dev/null 2>&1; then
    T=$(mktemp -d)
    # `trap` e nao `rm` no fim: se um shellcheck reprovar, o `set -e` sai antes do fim.
    trap 'rm -rf "$T"' EXIT INT TERM
    rc=0
    for par in $ALVOS; do
        f=${par%:*}
        cp "$R/$f" "$T/$(basename "$f")"
        chmod 644 "$T/$(basename "$f")"
        docker run --rm -v "$T":/w:z -w /w \
            koalaman/shellcheck:stable -s "${par##*:}" "$(basename "$f")" || rc=1
    done
    exit "$rc"
fi

echo "shellcheck AUSENTE e docker AUSENTE — nao da pra verificar os scripts nesta maquina." >&2
echo "Instale o shellcheck (apt install shellcheck) ou o docker. Nao vou dizer VERDE" >&2
echo "sobre uma verificacao que nao rodou." >&2
exit 2
