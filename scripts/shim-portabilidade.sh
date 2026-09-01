#!/bin/sh
# Roda a bateria de comportamento do `schematize-ops-shell` sob VÁRIOS shells POSIX.
#
# POR QUE ISTO EXISTE
# -------------------
# O shim é a fronteira de verdade do acesso remoto: roda como `command=` no
# `authorized_keys` de um servidor ALHEIO, sob o `/bin/sh` que aquele host tiver. Não
# temos como escolher esse shell — pode ser BusyBox ash (Alpine), dash (Debian/Ubuntu),
# mksh, ksh, o `sh` do FreeBSD ou o bash-em-modo-posix do macOS.
#
# Um `[[ ]]`, um `local`, um `echo -e` que passe despercebido não dá erro visível: dá um
# shim que RECUSA o que devia aceitar (host inoperante) ou, muito pior, ACEITA o que devia
# recusar. Testar em um shell só é testar a máquina de quem escreveu.
#
# Uso:  scripts/shim-portabilidade.sh [shell ...]      (default: os que existirem na máquina)
#       docker run --rm -v "$PWD":/w -w /w debian:stable-slim sh -c \
#         'apt-get update -qq && apt-get install -y -qq dash mksh ksh zsh yash busybox >/dev/null &&
#          scripts/shim-portabilidade.sh sh dash bash mksh ksh zsh yash "busybox sh" "bash --posix"'
set -eu

R=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
SHIM="$R/packaging/ops-shell/schematize-ops-shell"
[ -f "$SHIM" ] || { echo "nao achei o shim em $SHIM" >&2; exit 2; }

SHELLS=${*:-}
if [ -z "$SHELLS" ]; then
    SHELLS=""
    for s in sh dash bash mksh ksh zsh yash; do
        command -v "$s" >/dev/null 2>&1 && SHELLS="$SHELLS $s"
    done
    command -v busybox >/dev/null 2>&1 && SHELLS="$SHELLS busybox_sh"
    command -v bash    >/dev/null 2>&1 && SHELLS="$SHELLS bash_posix"
fi

falhas=0

# Monta um sandbox com shim + catálogo e devolve o diretório.
montar() {
    d=$(mktemp -d)
    mkdir -p "$d/lib"
    cp "$SHIM" "$d/lib/schematize-ops-shell"
    chmod +x "$d/lib/schematize-ops-shell"
    printf 'deploy\techo DEPLOY-OK\nlogs\techo LOGS-OK\n' > "$d/lib/catalogo"
    printf '%s' "$d"
}

# Roda o shim com um pedido; imprime "<codigo>|<saida>".
invocar() {
    _sh=$1; _dir=$2; _pedido=$3
    _out=$(SSH_ORIGINAL_COMMAND="$_pedido" HOME="$_dir" \
           $_sh "$_dir/lib/schematize-ops-shell" 2>&1) && _rc=0 || _rc=$?
    printf '%s|%s' "$_rc" "$_out"
}

checar() {
    _nome=$1; _sh=$2; _dir=$3; _pedido=$4; _rc_esperado=$5; _contem=$6
    _r=$(invocar "$_sh" "$_dir" "$_pedido")
    _rc=${_r%%|*}; _out=${_r#*|}
    if [ "$_rc" != "$_rc_esperado" ]; then
        printf '    FALHA %-28s rc=%s (esperado %s)\n' "$_nome" "$_rc" "$_rc_esperado"
        falhas=$((falhas + 1)); return
    fi
    case $_out in
        *"$_contem"*) : ;;
        *) printf '    FALHA %-28s saida sem %s: %s\n' "$_nome" "$_contem" "$_out"
           falhas=$((falhas + 1)); return ;;
    esac
    printf '    ok    %s\n' "$_nome"
}

for entrada in $SHELLS; do
    case $entrada in
        busybox_sh) sh_cmd="busybox sh"; rotulo="busybox sh" ;;
        bash_posix) sh_cmd="bash --posix"; rotulo="bash --posix" ;;
        *) sh_cmd=$entrada; rotulo=$entrada ;;
    esac
    # `busybox sh` e `bash --posix` são duas palavras: sem aspas de propósito no `invocar`.
    command -v "${sh_cmd%% *}" >/dev/null 2>&1 || { printf '  (pulado: %s nao existe)\n' "$rotulo"; continue; }
    printf '== %s\n' "$rotulo"
    dir=$(montar)

    checar "recusa shell interativo"  "$sh_cmd" "$dir" ""                    126 "não abre shell interativo"
    checar "recusa verbo desconhecido" "$sh_cmd" "$dir" "rm"                 126 "verbo desconhecido"
    checar "recusa argumento"          "$sh_cmd" "$dir" "deploy --force"     126 "uma palavra só"
    checar "recusa encadeamento ;"     "$sh_cmd" "$dir" "deploy;id"          126 "uma palavra só"
    checar "recusa pipe"               "$sh_cmd" "$dir" "deploy|id"          126 "uma palavra só"
    checar "recusa substituicao"       "$sh_cmd" "$dir" 'deploy$(id)'        126 "uma palavra só"
    checar "recusa crase"              "$sh_cmd" "$dir" 'deploy`id`'         126 "uma palavra só"
    checar "recusa redirecionamento"   "$sh_cmd" "$dir" "deploy>/tmp/x"      126 "uma palavra só"
    checar "recusa quebra de linha"    "$sh_cmd" "$dir" "$(printf 'deploy\nid')" 126 "uma palavra só"
    checar "probe nao executa nada"    "$sh_cmd" "$dir" "schematize-probe"     0 "forced=sim"
    checar "verbo do catalogo roda"    "$sh_cmd" "$dir" "deploy"               0 "DEPLOY-OK"
    checar "segundo verbo roda"        "$sh_cmd" "$dir" "logs"                 0 "LOGS-OK"

    # O log do host tem que registrar tanto o allow quanto o deny.
    if grep -q "allow" "$dir/.schematize/ops-shell.log" 2>/dev/null &&
       grep -q "deny"  "$dir/.schematize/ops-shell.log" 2>/dev/null; then
        printf '    ok    log do host registra allow e deny\n'
    else
        printf '    FALHA log do host incompleto\n'; falhas=$((falhas + 1))
    fi

    rm -rf "$dir"
done

echo
if [ "$falhas" -ne 0 ]; then
    printf 'VERMELHO — %s verificacao(oes) falharam.\n' "$falhas"
    exit 1
fi
printf 'VERDE — o shim se comporta igual em todos os shells testados.\n'
