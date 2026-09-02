#!/bin/sh
# DIAGNÓSTICO — "o comando `schematize` não existe no terminal".
#
# Rode NA MÁQUINA COM O PROBLEMA e cole a saída inteira. Não muda nada; só lê.
#
#   sh scripts/diagnostico-path.sh
#
# ou, sem o repo clonado:
#   curl -fsSL https://raw.githubusercontent.com/schematizeme/schematize-cli/main/scripts/diagnostico-path.sh | sh
#
# O QUE ELE PROCURA
# -----------------
# A hipótese principal é uma cadeia específica, e cada seção abaixo testa um elo dela:
#
#   1. openSUSE não tem binário pré-compilado compatível → o app compila do FONTE.
#   2. Build do fonte instala em `~/.cargo/bin`, não em `~/.local/bin`.
#   3. O `install.sh` NÃO escreve em nenhum rc de shell (conferido: zero ocorrências).
#   4. O rustup escreve o `~/.cargo/env` em `~/.profile` e `~/.bash_profile` — NÃO em
#      `~/.bashrc`.
#   5. Terminal gráfico abre shell interativo NÃO-login → lê só o `~/.bashrc`.
#   6. Logo: `~/.cargo/bin` nunca entra no PATH, e o binário existe mas some.
#
# Em Debian/Ubuntu o elo 5 não fecha porque o gerenciador de login lê `~/.profile` e o PATH
# se propaga pra tudo. É por isso que o sintoma seria só no openSUSE.
#
# As seções 7 e 8 existem pra REFUTAR a hipótese se ela estiver errada — binário ausente,
# binário que não executa (glibc), ou várias cópias brigando no PATH.
echo "===== 1. sistema"
# shellcheck source=/dev/null
. /etc/os-release 2>/dev/null && echo "  distro: ${PRETTY_NAME:-?} (ID=${ID:-?} ID_LIKE=${ID_LIKE:-})"
echo "  kernel: $(uname -sr)"
echo "  libc:   $(ldd --version 2>&1 | head -1)"
echo "  shell:  ${SHELL:-?}   \$0=$0"

echo "===== 2. onde o binário está (todos os lugares que o app usa)"
for d in "$HOME/.cargo/bin" "$HOME/.local/bin" /usr/local/bin /usr/bin; do
    for b in schematize schematize-gui schematize-updater; do
        [ -e "$d/$b" ] || continue
        # `stat` em vez de `ls | awk`: nome com espaco quebraria o parsing, e este script
        # roda na maquina de outra pessoa — onde o inesperado e a regra.
        printf '  %-38s %s\n' "$d/$b" "$(stat -c '%A %s bytes  %y' "$d/$b" 2>/dev/null | cut -c1-40)"
    done
done
echo "  (vazio acima = não achei binário nenhum: o problema é INSTALAÇÃO, não PATH)"

echo "===== 3. o que o PATH resolve AGORA"
echo "  command -v schematize: $(command -v schematize 2>/dev/null || echo '(NADA — é o sintoma)')"
echo "  PATH="
printf '%s\n' "$PATH" | tr ':' '\n' | sed 's/^/    /'

echo "===== 4. os dirs do app estão no PATH?"
for d in "$HOME/.cargo/bin" "$HOME/.local/bin"; do
    case ":$PATH:" in *":$d:"*) echo "  SIM  $d" ;; *) echo "  NAO  $d" ;; esac
done

echo "===== 5. o que cada rc diz sobre PATH (o elo 4/5 da hipótese)"
for f in .bashrc .bash_profile .profile .zshrc .zprofile .config/fish/config.fish; do
    p="$HOME/$f"
    if [ -e "$p" ]; then
        n=$(grep -cE 'cargo/bin|\.local/bin|cargo/env' "$p" 2>/dev/null || echo 0)
        printf '  %-30s existe, %s linha(s) de PATH do app\n' "$f" "$n"
        grep -nE 'cargo/bin|\.local/bin|cargo/env' "$p" 2>/dev/null | sed 's/^/      /'
    else
        printf '  %-30s NAO EXISTE\n' "$f"
    fi
done

echo "===== 6. este shell é de login? (decide QUAL rc é lido)"
case "$-" in *i*) echo "  interativo: sim" ;; *) echo "  interativo: nao" ;; esac
# `shopt` e do bash, nao do POSIX sh — por isso perguntamos ao bash, se houver.
if command -v bash >/dev/null 2>&1; then
    bash -lc 'shopt -q login_shell && echo "  bash -l: login SIM" || echo "  bash -l: login nao"' 2>/dev/null
    bash -c  'shopt -q login_shell && echo "  bash (terminal comum): login SIM" || echo "  bash (terminal comum): login NAO — le so o ~/.bashrc"' 2>/dev/null
fi

echo "===== 7. o binário EXECUTA? (refuta a hipótese se falhar por glibc)"
for d in "$HOME/.cargo/bin" "$HOME/.local/bin" /usr/local/bin /usr/bin; do
    if [ -x "$d/schematize" ]; then
        printf '  %s --version → ' "$d/schematize"
        "$d/schematize" --version 2>&1 | head -1
    fi
done

echo "===== 8. cópias em conflito (qual ganharia o PATH)"
n=0
for d in "$HOME/.cargo/bin" "$HOME/.local/bin" /usr/local/bin /usr/bin; do
    [ -e "$d/schematize" ] && n=$((n + 1))
done
echo "  cópias de schematize encontradas: $n"
[ "$n" -gt 1 ] && echo "  ATENCAO: mais de uma — quem ganha e quem estiver primeiro no PATH"

echo "===== 9. rustup/cargo"
echo "  rustup: $(command -v rustup 2>/dev/null || echo ausente)"
echo "  cargo:  $(command -v cargo 2>/dev/null || echo ausente)"
[ -f "$HOME/.cargo/env" ] && echo "  ~/.cargo/env existe" || echo "  ~/.cargo/env NAO existe"

echo
echo "===== VEREDITO PRELIMINAR"
if [ -x "$HOME/.cargo/bin/schematize" ] && ! command -v schematize >/dev/null 2>&1; then
    echo "  O binario EXISTE em ~/.cargo/bin e o PATH nao o alcanca."
    echo "  Isso confirma a hipotese: o instalador nao escreveu o PATH em rc nenhum."
    echo "  Alivio imediato (nao e a correcao):"
    echo "    echo 'export PATH=\"\$HOME/.cargo/bin:\$PATH\"' >> ~/.bashrc && exec bash"
elif ! [ -x "$HOME/.cargo/bin/schematize" ] && ! [ -x "$HOME/.local/bin/schematize" ]; then
    echo "  Nao achei binario em lugar nenhum: o problema e a INSTALACAO, nao o PATH."
    echo "  A secao 7 e a 9 dizem se o build do fonte chegou a rodar."
else
    echo "  O quadro nao casa com a hipotese principal. As secoes 7 e 8 sao as que importam."
fi
