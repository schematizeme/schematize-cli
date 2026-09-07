#!/bin/sh
# Roda o `shellcheck` nos scripts de PRODUTO — a MESMA regua que o CI roda.
#
# POR QUE E UM SCRIPT E NAO UMA LINHA NO verificar.sh
# ---------------------------------------------------
# O `shellcheck` nao esta instalado em toda maquina de desenvolvimento, mas ESTA no runner
# do GitHub. Uma etapa que simplesmente sumisse quando o binario falta deixaria o gate local
# afirmando VERDE sobre uma verificacao que ele nem tentou — e foi assim que o SC2016 chegou
# ao CI: a etapa local nunca rodou shellcheck, so a bateria de shells.
#
# Aqui: roda sempre, com versao pinada, e se nao houver como rodar REPROVA dizendo o que
# falta. Gate que nao consegue verificar nao diz VERDE.
#
# POR QUE O install.sh ENTROU (2026-09-06)
# ----------------------------------------
# Ate entao este gate cobria SO o shim do ops. O `install.sh` — 550+ linhas que o CONTEXT.md
# chama de "codigo de produto, nao script auxiliar", e que sao o primeiro contato de todo
# usuario com a casa — nao era lido por lint nenhum. O que estava la dentro:
#
#   * `warn` USADA em quatro pontos e NUNCA definida. Sob `set -euo pipefail` o nome nao
#     resolve, o shell sai 127 e a instalacao ABORTA: avisar matava o instalador.
#   * uma continuacao `\` com comentario no meio, que engole o resto do comando.
#
# Nenhuma das duas e pegavel por `bash -n` nem pela bateria de shells: sao sintaticamente
# validas e semanticamente erradas. Sao exatamente o que o shellcheck existe pra ver.
#
# POR QUE DOIS DIALETOS: sao dois contratos diferentes. O shim do ops roda no `/bin/sh` de
# qualquer distro (POSIX estrito, `-s sh`); o `install.sh` declara `#!/usr/bin/env bash` e
# usa `local` e arrays, entao cobra-lo como POSIX daria centenas de falso-positivo.
set -eu

# VERSAO PINADA — e o ponto todo desta secao.
#
# Em 2026-09-06 o gate local disse VERDE e o CI REPROVOU o mesmo arquivo. Nao era
# flakiness: o local usava a tag `stable` (0.11.0) e o runner usa um shellcheck mais
# ANTIGO, que e MAIS ESTRITO em SC2015. O skew estava na direcao perigosa — o gate local
# absolvia o que o CI condenava, que e precisamente o que este script foi escrito pra
# impedir ("gate que nao consegue verificar nao diz VERDE" vale tambem pra gate que
# verifica com regua diferente).
#
# Por isso a versao e PINADA e o docker e PREFERIDO ao binario local, inclusive no CI:
# uma regua so, a mesma nas duas pontas. Puxar a imagem custa segundos; descobrir a
# divergencia so no push custa um main vermelho.
SC_VER=v0.10.0

R=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

# <caminho-relativo>:<dialeto>
ALVOS='packaging/ops-shell/schematize-ops-shell:sh install.sh:bash'

for par in $ALVOS; do
    [ -f "$R/${par%:*}" ] || { echo "nao achei ${par%:*}" >&2; exit 2; }
done

if command -v docker >/dev/null 2>&1; then
    T=$(mktemp -d)
    # `trap` e nao `rm` no fim: se um shellcheck reprovar, o `set -e` sai antes do fim.
    trap 'rm -rf "$T"' EXIT INT TERM
    rc=0
    for par in $ALVOS; do
        f=${par%:*}
        b=$(basename "$f")
        cp "$R/$f" "$T/$b"
        chmod 644 "$T/$b"
        # `:z` porque sem ele o container leva "permission denied" em distro com SELinux
        # (openSUSE, Fedora, RHEL). Rotula so o temp — montar a raiz do repo com `:z`
        # rerrotularia o repo inteiro, o que e lento e mexe em label que nao e do container.
        docker run --rm -v "$T":/w:z -w /w \
            "koalaman/shellcheck:$SC_VER" -s "${par##*:}" "$b" || rc=1
    done
    exit "$rc"
fi

if command -v shellcheck >/dev/null 2>&1; then
    # Sem docker: roda com o que tem, mas DIZ que a regua pode ser outra. Silenciar isso
    # e exatamente como o skew nasceu.
    echo "aviso: sem docker — usando o shellcheck local, que pode divergir do pinado ($SC_VER)." >&2
    shellcheck --version 2>&1 | sed -n 's/^version: /  local: /p' >&2
    rc=0
    for par in $ALVOS; do
        shellcheck -s "${par##*:}" "$R/${par%:*}" || rc=1
    done
    exit "$rc"
fi

echo "shellcheck AUSENTE e docker AUSENTE — nao da pra verificar os scripts nesta maquina." >&2
echo "Instale o docker (preferido, versao pinada) ou o shellcheck. Nao vou dizer VERDE" >&2
echo "sobre uma verificacao que nao rodou." >&2
exit 2
