#!/usr/bin/env python3
"""Guard das FEATURES do install.sh: toda `--features X` que o script manda ao cargo existe?

# Por que existe

O install.sh compila CLI e GUI do fonte. Quando as features de SQLite saíram do CLI (E6) e da
GUI, o script seguiu mandando `--no-default-features --features sqlite-do-sistema` em toda
máquina com libsqlite3 — e o build do fonte morria com "the package does not contain this
feature". Nada pegava: o CI não roda o ramo do install.sh que depende do que há na máquina.

# O que ele faz

Para cada linha do install.sh que chama `cargo build|install`, descobre o CRATE (pelo
`cd '$var'`), expande `$var` de `var="...--features X..."` e confere que cada X existe em
`[features]` (ou é dependência opcional) do Cargo.toml daquele crate. Crate cujo checkout
irmão não está ao lado -> SKIP explícito com o motivo (nunca verde mudo).

Saída: 0 = ok; 1 = feature inexistente.
"""
import re
import sys
import tomllib
from pathlib import Path

RAIZ = Path(__file__).resolve().parent.parent
CRATES = {
    "cli": RAIZ / "Cargo.toml",
    "gui": RAIZ.parent / "schematize_gui_slint" / "Cargo.toml",
    "mkt": RAIZ.parent / "schematize_market_rs" / "Cargo.toml",
}


def features_do(cargo: Path) -> set[str]:
    """O quê: nomes de feature válidos (tabela [features] + deps opcionais). Onde: `main`."""
    dados = tomllib.loads(cargo.read_text(encoding="utf-8"))
    nomes = set(dados.get("features", {}))
    for sec in ("dependencies", "build-dependencies"):
        for nome, spec in dados.get(sec, {}).items():
            if isinstance(spec, dict) and spec.get("optional"):
                nomes.add(nome)
    return nomes


def main() -> int:
    """O quê: varre o install.sh e confere cada feature. Onde: etapa `features` do verificar.sh."""
    linhas = (RAIZ / "install.sh").read_text(encoding="utf-8").splitlines()
    atribuicoes: dict[str, list[str]] = {}
    for ln in linhas:
        m = re.match(r'\s*(\w+)="([^"]*--(?:no-default-features|features)[^"]*)"', ln)
        if m:
            atribuicoes.setdefault(m.group(1), []).append(m.group(2))
    falhas = 0
    vistos = 0
    for n, ln in enumerate(linhas, 1):
        if ln.lstrip().startswith("#") or not re.search(r"cargo (build|install)", ln):
            continue
        crate = re.search(r"cd '\$(\w+)'", ln)
        texto = ln
        for v in re.findall(r"\$(\w+)", ln):
            texto += " " + " ".join(atribuicoes.get(v, []))
        pedidas = re.findall(r"--features[ =]([\w,.\-/]+)", texto)
        usa_nd = "--no-default-features" in texto
        if not pedidas and not usa_nd:
            continue
        vistos += 1
        nome = crate.group(1) if crate else "?"
        cargo = CRATES.get(nome)
        if cargo is None or not cargo.exists():
            print(f"SKIP install.sh:{n} crate '{nome}': Cargo.toml nao encontrado "
                  f"({cargo}); features nao conferidas")
            continue
        validas = features_do(cargo)
        for lista in pedidas:
            for f in lista.split(","):
                if f not in validas:
                    falhas += 1
                    print(f"FALHA install.sh:{n}: --features {f} nao existe em {cargo} "
                          f"(existem: {sorted(validas) or 'nenhuma'})")
        if usa_nd and not pedidas and not validas:
            print(f"FALHA install.sh:{n}: --no-default-features em {cargo}, que nao tem [features]")
            falhas += 1
    print(f"features-install: {vistos} chamada(s) com flags conferida(s), {falhas} falha(s)")
    return 1 if falhas else 0


if __name__ == "__main__":
    sys.exit(main())
