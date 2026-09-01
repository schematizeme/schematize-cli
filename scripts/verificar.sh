#!/bin/sh
# Verificação de verde/vermelho por CÓDIGO DE SAÍDA, nunca por contagem de texto.
#
# POR QUÊ ESTE ARQUIVO EXISTE
# ---------------------------
# Quatro vezes seguidas o que falhou não foi o código: foi a ferramenta que DIZ se o
# código está verde.
#
#   1. o mutantes.py morreu no timeout e deixou a mutação no fonte;
#   2. o `copy2` preservou o mtime e o cargo seguiu rodando o binário mutado — o que se
#      lia na tela deixou de ser o que rodava;
#   3. uma cópia solta do script resolveu a raiz errada e acusou "baseline VERMELHO";
#   4. um wrapper ad-hoc contou falha com `bc`, o `bc` errou a sintaxe, e o resultado
#      foi um "falhas: 3" inventado sobre uma suíte inteiramente verde.
#
# Todos os quatro têm a mesma forma: o veredito veio de PARSING, não do processo. Aqui o
# veredito é o `$?` de cada etapa e nada mais. O log fica em disco pra leitura humana,
# mas quem decide é o código de saída — texto não vota.
#
# Uso:  scripts/verificar.sh [diretorio-de-log]
set -eu

R=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
LOG=${1:-${TMPDIR:-/tmp}}
falhou=0

# Roda uma etapa, guarda o log e propaga SÓ o código de saída.
etapa() {
    nome=$1; shift
    printf '== %s ... ' "$nome"
    if (cd "$R" && "$@") > "$LOG/verificar-$nome.log" 2>&1; then
        printf 'VERDE\n'
    else
        printf 'VERMELHO (log: %s)\n' "$LOG/verificar-$nome.log"
        falhou=1
    fi
}

# `fmt` agora REPROVA.
#
# Ate 2026-08-31 esta etapa era informativa: o crate carregava ~1000 divergencias de
# rustfmt anteriores a qualquer disciplina de formatacao aqui, e um gate que reprova todo
# mundo desde o primeiro dia e um gate que alguem desliga na primeira semana.
#
# A divida foi paga numa passada dedicada, com um `rustfmt.toml` que faz o rustfmt
# concordar com o estilo da casa em vez do contrario (ver o arquivo). Divida paga, o gate
# entra: a partir daqui formatacao e binaria, e ninguem discute virgula em review.
etapa fmt     cargo fmt --check
etapa shim    sh scripts/shim-portabilidade.sh
etapa clippy  cargo clippy --all-targets -- -D warnings
etapa testes  cargo test --all-targets --quiet

if [ "$falhou" -ne 0 ]; then
    printf '\nVERMELHO — alguma etapa falhou. Nao mute, nao comite, nao publique.\n'
    exit 1
fi
printf '\nVERDE — todas as etapas passaram.\n'
