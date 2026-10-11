#!/usr/bin/env python3
"""Guard de RELEASE: nenhum app delegado por hook pode ser inalcançável numa máquina limpa.

# O que este guard trava

Desde o commit ccb4bd0 o hub NÃO contém mais o motor do overdev: `schematize overdev check|guard`
(chamados pelos hooks Stop/PreToolUse do Claude Code) delega ao app `schematize-overdev`. Se o
app falta, o hub sai rc=1 — e o Claude Code trata rc=1 de hook como erro NÃO bloqueante: o veto
do overdev some em silêncio. Publicar um release do hub enquanto nada instala o
`schematize-overdev` numa máquina limpa desliga o overdev em toda máquina que atualizar.

Regra: só se corta tag se TODO app em `CRITICOS_DE_HOOK` (src/instalardica.rs) estiver, na tabela
`ORIGENS`, com `mercado: true` (o market o gere) E `publico: true` (o repo é clonável sem
credencial). Lê o Rust como texto (regex, sem cargo, sem rede, sem PyYAML) para rodar em segundos
no primeiro passo do release.yml, antes de qualquer build.

Não roda no CI comum (ci.yml) nem no `verificar.sh` do dia a dia: o desenvolvimento segue
livre; só o caminho de release fica trancado.

# Como rodar

    python3 scripts/release-delegados.py             # veredito por código de saída (0 ok, 1 bloqueia)
    python3 scripts/release-delegados.py --autoteste  # exercita a decisão nos dois sentidos
    sh scripts/verificar.sh --release                 # o mesmo, como etapa do gate local
"""

import re
import sys
from pathlib import Path

FONTE = Path(__file__).resolve().parent.parent / "src" / "instalardica.rs"


def criticos(fonte: str) -> list[str]:
    """Os bins de `CRITICOS_DE_HOOK`. ONDE: `main`. Falha alto se o formato mudar."""
    m = re.search(r"CRITICOS_DE_HOOK:\s*&\[&str\]\s*=\s*&\[(.*?)\];", fonte, re.S)
    if not m:
        raise SystemExit("release-delegados: não achei CRITICOS_DE_HOOK em instalardica.rs")
    return re.findall(r'"([^"]+)"', m.group(1))


def origens(fonte: str) -> dict[str, dict]:
    """A tabela `ORIGENS` como {bin: {repo, mercado, publico}}. ONDE: `main`."""
    m = re.search(r"pub const ORIGENS:[^=]*=\s*&\[(.*?)\n\];", fonte, re.S)
    if not m:
        raise SystemExit("release-delegados: não achei ORIGENS em instalardica.rs")
    out = {}
    for bloco in re.findall(r"Origem\s*\{(.*?)\}", m.group(1), re.S):
        campo = lambda k: re.search(rf"{k}:\s*([^,\n]+)", bloco).group(1).strip().strip('"')
        out[campo("bin")] = {
            "repo": campo("repo"),
            "mercado": campo("mercado") == "true",
            "publico": campo("publico") == "true",
        }
    return out


def bloqueios(crit: list[str], tabela: dict[str, dict]) -> list[str]:
    """A decisão, PURA: uma mensagem acionável por app crítico que não é instalável.

    ONDE: `main` e `autoteste`. Crítico ausente da tabela também bloqueia (não dá para provar
    que se instala). Lista vazia = release liberado.
    """
    msgs = []
    for bin_ in crit:
        o = tabela.get(bin_)
        if o is None:
            msgs.append(f"`{bin_}` não consta em ORIGENS (src/instalardica.rs): adicione a origem dele.")
            continue
        falta = []
        if not o["publico"]:
            falta.append(f"tornar o repo {o['repo']} público (GET anônimo = 200)")
        if not o["mercado"]:
            falta.append(
                "publicar uma versão do market que o gere (hoje: está no main do market, "
                "falta a tag) e então marcar `mercado: true`"
            )
        if falta:
            msgs.append(
                f"`{bin_}` (hook-crítico) não é instalável numa máquina limpa "
                f"[mercado={o['mercado']}, publico={o['publico']}]. Para liberar: "
                + " e ".join(falta)
                + ". Sem isso o hub, ao atualizar, sai rc=1 nos hooks e o veto do overdev some."
            )
    return msgs


def _texto_ok() -> bool:
    """Confere o TEXTO do bloqueio para cada combinação de flags. ONDE: `autoteste`."""
    base = {"repo": "schematizeme/x", "mercado": True, "publico": True}
    pub = "tornar o repo schematizeme/x público (GET anônimo = 200)"
    mer = "publicar uma versão do market que o gere"
    velho = ("cortar um release dele", "geri-lo", "publicar o repo")
    ok = True
    for publico, mercado in [(True, True), (False, True), (True, False), (False, False)]:
        msgs = bloqueios(["a"], {"a": {**base, "publico": publico, "mercado": mercado}})
        txt = " ".join(msgs)
        esperado = [(pub, not publico), (mer, not mercado)]
        errado = any((frag in txt) != deve for frag, deve in esperado)
        errado |= any(v in txt for v in velho) or bool(msgs) != (not (publico and mercado))
        if errado:
            print(f"autoteste: texto errado para publico={publico} mercado={mercado}: {txt!r}")
            ok = False
    return ok


def autoteste() -> int:
    """Vê o guard reprovar e aprovar. ONDE: `--autoteste`. Guard que nunca falhou está cego."""
    ok = {"a": {"repo": "x/a", "mercado": True, "publico": True}}
    casos = [
        (["a"], ok, 0),
        (["a"], {"a": {**ok["a"], "publico": False}}, 1),
        (["a"], {"a": {**ok["a"], "mercado": False}}, 1),
        (["a"], {}, 1),
        ([], {}, 0),
    ]
    falhou = False
    for crit, tab, esperado in casos:
        obtido = 1 if bloqueios(crit, tab) else 0
        if obtido != esperado:
            print(f"autoteste: {crit} {tab} -> {obtido}, esperado {esperado}")
            falhou = True
    falhou |= not _texto_ok()
    reais = origens(FONTE.read_text(encoding="utf-8"))
    if not {"schematize-overdev", "schematize-market"} <= set(reais):
        print("autoteste: o extrator não leu a tabela real")
        falhou = True
    print("autoteste do guard: " + ("VERMELHO" if falhou else "VERDE"))
    return 1 if falhou else 0


def main() -> int:
    if "--autoteste" in sys.argv:
        return autoteste()
    fonte = FONTE.read_text(encoding="utf-8")
    msgs = bloqueios(criticos(fonte), origens(fonte))
    if not msgs:
        print("release-delegados: VERDE — todo app delegado por hook é instalável numa máquina limpa.")
        return 0
    print("release-delegados: RELEASE BLOQUEADO")
    for m in msgs:
        print(f"  - {m}")
        if "GITHUB_ACTIONS" in __import__("os").environ:
            print(f"::error::{m}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
