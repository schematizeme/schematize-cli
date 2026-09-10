#!/usr/bin/env python3
"""Guard dos PINS ENTRE REPOS: a GUI compila contra o CLI de hoje, ou contra um de meses atrás?

# O que este guard trava

Há dois pinos cruzados neste ecossistema, e nenhum deles tinha verificação:

1. **`schematize_gui_slint/Cargo.lock`** fixa o COMMIT do CLI contra o qual a GUI compila.
   Ela consome o CLI por git-dep, e o lock é o que faz dois clones do mesmo SHA compilarem
   contra o mesmo CLI.
2. **`schematize_cli_rs/packaging/gui-pin.txt`** fixa o COMMIT da GUI que o release do CLI
   compila.

# Por que nada pegava, e por que isso é pior que um erro barulhento

O workflow de release roda `cargo update --precise` na hora de publicar. Ele conserta o pino
**para o artefato publicado** — e esconde a defasagem de todo mundo que **clona o repo**. Quem
compila do fonte pega o pino commitado, que pode estar seis versões atrás; e o
`cargo build --locked`, que é o modo do build reproduzível, quebra com um erro que fala de
resolução de dependência e não de "seu pino está velho".

O sintoma já apareceu: um `cargo build` local quebrou com o lock seis versões atrás do CLI.

# O que ele NÃO faz

Não exige que o pino aponte para o HEAD. Ficar alguns commits atrás é normal enquanto se
desenvolve os dois lados — o `[patch]` do `.cargo/config.toml` existe exatamente para isso. O
que ele cobra é que a defasagem esteja dentro de uma versão MINOR: `0.62.x` contra `0.65.0` é
outro software.

# Como rodar

    python3 scripts/pins-em-dia.py

Sem argumento, procura os repos irmãos ao lado deste. Sem eles, diz que não pôde verificar e
sai 0 — um checkout isolado não tem o outro lado para comparar, e reprovar ali seria um gate
sempre vermelho, que é um gate que as pessoas aprendem a ignorar.
"""

import re
import subprocess
import sys
from pathlib import Path


def versao_do_toml(caminho: Path) -> str | None:
    """A `version` da seção `[package]` de um `Cargo.toml`."""
    if not caminho.is_file():
        return None
    for linha in caminho.read_text(encoding="utf-8").splitlines():
        if linha.startswith("version = "):
            return linha.split("=", 1)[1].strip().strip('"')
    return None


def pino_do_lock(lock: Path) -> tuple[str, str] | None:
    """`(versão, sha)` do CLI fixados no `Cargo.lock` da GUI, ou `None`.

    Lê o bloco `[[package]]` cujo `name` é `schematize`. O `source` traz o SHA depois do `#`.
    """
    if not lock.is_file():
        return None
    for bloco in lock.read_text(encoding="utf-8").split("[[package]]"):
        if 'name = "schematize"\n' not in bloco:
            continue
        ver = re.search(r'^version = "([^"]+)"', bloco, re.M)
        sha = re.search(r'source = "git\+[^"]*#([0-9a-f]{40})"', bloco)
        if ver and sha:
            return (ver.group(1), sha.group(1))
        # Sem `source`: o lock foi reescrito com o `[patch]` ligado e deixou de pinar. O
        # `lockpin.rs` da GUI já cobre esse caso com um teste; aqui só nao ha o que comparar.
        return None
    return None


def minor(v: str) -> tuple[int, int]:
    """`(major, minor)` de uma versão semver. `(0, 0)` quando não dá para ler."""
    p = v.split(".")
    try:
        return (int(p[0]), int(p[1]))
    except (IndexError, ValueError):
        return (0, 0)


def commit_existe(repo: Path, sha: str) -> bool:
    """O commit está neste clone? (Um clone raso pode não ter.)"""
    r = subprocess.run(
        ["git", "-C", str(repo), "cat-file", "-e", f"{sha}^{{commit}}"],
        capture_output=True,
    )
    return r.returncode == 0


def main() -> int:
    raiz = (
        Path(sys.argv[sys.argv.index("--repos") + 1])
        if "--repos" in sys.argv
        else Path(__file__).resolve().parents[2]
    )
    cli, gui = raiz / "schematize_cli_rs", raiz / "schematize_gui_slint"
    if not (cli.is_dir() and gui.is_dir()):
        print("pins: repos irmãos não estão ao lado — nada a comparar neste checkout")
        return 0

    problemas = []

    # 1) A GUI compila contra qual CLI?
    pino = pino_do_lock(gui / "Cargo.lock")
    atual = versao_do_toml(cli / "Cargo.toml")
    if pino and atual:
        fixada, sha = pino
        if minor(fixada) != minor(atual):
            problemas.append(
                f"a GUI compila contra o CLI {fixada} (commit {sha[:12]}), e o CLI está em "
                f"{atual}.\n    Quem clonar a GUI hoje compila contra outro software. "
                f"Atualize o pino:\n      cd schematize_gui_slint && cargo update -p schematize"
            )
        elif not commit_existe(cli, sha):
            problemas.append(
                f"o commit {sha[:12]} que a GUI fixa não existe neste clone do CLI.\n"
                f"    Ou ele nunca foi publicado, ou o clone está raso — nos dois casos, um "
                f"`cargo build --locked` da GUI falha."
            )

    # 2) O release do CLI compila qual GUI?
    pin_txt = cli / "packaging" / "gui-pin.txt"
    if pin_txt.is_file():
        sha_gui = pin_txt.read_text(encoding="utf-8").strip()
        if not re.fullmatch(r"[0-9a-f]{40}", sha_gui):
            problemas.append(f"`packaging/gui-pin.txt` não é um SHA de 40 dígitos: {sha_gui!r}")
        elif not commit_existe(gui, sha_gui):
            problemas.append(
                f"o release do CLI fixa a GUI em {sha_gui[:12]}, que não existe neste clone "
                f"da GUI.\n    O release compilaria um commit que ninguém tem."
            )

    if problemas:
        print("PINOS ENTRE REPOS DEFASADOS:", file=sys.stderr)
        for p in problemas:
            print(f"  - {p}", file=sys.stderr)
        print(
            "\nO release conserta o pino na hora de publicar — e é justamente por isso que "
            "\nninguém vê a defasagem: ela só aparece para quem CLONA e compila do fonte.",
            file=sys.stderr,
        )
        return 1

    print("pinos entre repos OK — a GUI compila contra o CLI da série atual")
    return 0


if __name__ == "__main__":
    sys.exit(main())
