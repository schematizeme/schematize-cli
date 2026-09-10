#!/usr/bin/env python3
"""Guard da MATRIZ DE ASSETS: o que os workflows publicam bate com o que o install.sh baixa?

# Por que isto existe

Os dois lados desta ponte moram em arquivos diferentes, em repos diferentes, e nada os
casava. O `install.sh` baixa `schematize-<app>-gui-linux-x86_64` de
`releases/latest/download/`; quem produz esse nome é a matriz do `release.yml` do app. Se um
dos dois mudar sozinho, o download falha em 404 — e falha **na máquina de quem instalou**,
não no CI.

Já aconteceu neste ecossistema, e o modo de falha foi pior que 404: o passo da janela do
market rodava com `continue-on-error: true`, faltavam as libs de X11 no runner Linux, e o
`conclusion` do passo virava `success` mesmo com `outcome: failure`. O release saía **verde**,
com 7 assets em vez de 8, e o único que faltava era o da plataforma principal.

Este guard não alcança o release publicado — alcança a INTENÇÃO declarada nos dois arquivos,
que é onde a divergência nasce. Um release que sai com menos assets do que declarou é outro
problema, e o `continue-on-error` do workflow é quem o cria.

# Como rodar

    python3 scripts/assets-esperados.py

Sem argumento, procura os repos irmãos ao lado deste. `--repos <dir>` aponta outra raiz.
"""

import re
import sys
from pathlib import Path

# app → (repo do release, repo da janela)
APPS = {
    "market": "schematize_market_rs",
    "optimizer": "schematize_optimizer_rs",
    "deployer": "schematize_deployer_rs",
}

# As plataformas que o `install.sh` sabe baixar. Windows fica de fora de propósito: o
# `install.sh` é um script de shell, e quem instala no Windows não passa por ele.
PLATAFORMAS_BAIXAVEIS = ["linux-x86_64", "macos-arm64", "macos-x86_64"]


def assets_do_workflow(release_yml: Path) -> set:
    """Os nomes de asset declarados na matriz de um `release.yml`.

    Lê `asset:` e `asset_gui:` com regex e não com um parser de YAML, de propósito: este
    script roda no gate local, e exigir PyYAML numa máquina de desenvolvimento é a diferença
    entre um guard que roda e um que alguém pula. O formato lido é uma linha `chave: valor`
    dentro de uma lista, que é o que a matriz sempre é.
    """
    texto = release_yml.read_text(encoding="utf-8")
    return set(re.findall(r"^\s*asset(?:_gui)?:\s*(\S+)\s*$", texto, re.M))


def assets_que_o_install_baixa(install_sh: Path) -> set:
    """Os nomes de asset que o `install.sh` monta, expandindo `$app` e `$arch`.

    O script monta `schematize-$app-gui-linux-x86_64`; aqui isso vira os três nomes
    concretos, um por plataforma, para cada app que ele instala.
    """
    texto = install_sh.read_text(encoding="utf-8")
    apps = set(re.findall(r"^\s*instala_janela\s+(\w+)\s", texto, re.M))
    if not apps:
        raise SystemExit("não achei nenhuma chamada a `instala_janela` no install.sh")
    return {f"schematize-{a}-gui-{p}" for a in apps for p in PLATAFORMAS_BAIXAVEIS}


def main() -> int:
    raiz = Path(sys.argv[sys.argv.index("--repos") + 1]) if "--repos" in sys.argv \
        else Path(__file__).resolve().parents[2]

    install_sh = raiz / "schematize_cli_rs" / "install.sh"
    if not install_sh.is_file():
        raise SystemExit(f"não achei {install_sh}")

    querem = assets_que_o_install_baixa(install_sh)

    publicam = set()
    faltam_workflows = []
    for app, repo in APPS.items():
        wf = raiz / repo / ".github" / "workflows" / "release.yml"
        if not wf.is_file():
            faltam_workflows.append(f"{repo}: sem release.yml — nada é publicado para `{app}`")
            continue
        publicam |= assets_do_workflow(wf)

    orfaos = sorted(querem - publicam)
    problemas = faltam_workflows + [
        f"o install.sh baixa `{a}`, e nenhum release.yml o publica" for a in orfaos
    ]

    if problemas:
        print("MATRIZ DE ASSETS DIVERGENTE:", file=sys.stderr)
        for p in problemas:
            print(f"  - {p}", file=sys.stderr)
        print(
            "\nCada linha acima é um download que dará 404 na máquina de quem instalar.",
            file=sys.stderr,
        )
        return 1

    print(f"matriz de assets OK — {len(querem)} downloads, todos publicados por algum release")
    return 0


if __name__ == "__main__":
    sys.exit(main())
