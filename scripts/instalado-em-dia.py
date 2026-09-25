#!/usr/bin/env python3
"""Guard do BINÁRIO INSTALADO: o que está na máquina é o commit que está no repo?

# O buraco que este guard fecha, medido em 2026-09-24

Os três apps desta máquina rodavam binários de **2026-09-09**. O repo estava 15 dias à frente.
E `--version` dizia a MESMA coisa nos dois — `schematize-market 0.2.1` —, porque o número não
foi bumpado entre os commits que mudaram comportamento.

O sintoma não foi "está velho": foi `schematize-market desktop --install` **rodar, dizer `✓` e
gravar a forma antiga do `.desktop`** (ícone abrindo terminal) com a janela instalada ao lado. A
lógica nova estava no repo; quem rodava era a antiga. Não havia estado observável que
distinguisse os dois — e por isso ninguém notou por duas semanas.

O `procedencia.rs` de cada app resolveu a metade "dar para saber": o `--version` passou a trazer
o SHA do build. Este script é a outra metade: **alguém tem de comparar.**

# Por que ele AVISA e não reprova o gate

Binário defasado na máquina de quem desenvolve é normal — você acabou de commitar e ainda não
reinstalou. Reprovar aí seria um gate vermelho a cada commit, que é um gate que se aprende a
ignorar. O veredito é código de saída **2** (aviso) para defasagem, e **1** (reprova) só para o
que é defeito de verdade: binário instalado que **não sabe dizer** de onde veio, existindo um
repo ao lado — porque aí a pergunta "está em dia?" deixou de ter resposta possível.

# Como rodar

    python3 scripts/instalado-em-dia.py            # aviso (2) se defasado
    python3 scripts/instalado-em-dia.py --exigir    # reprova (1) se defasado
    python3 scripts/instalado-em-dia.py --autoteste  # exercita a decisão nos dois sentidos
"""

import re
import shutil
import subprocess
import sys
from pathlib import Path

# binário instalado → repo que o produz
APPS = {
    "schematize": "schematize_cli_rs",
    "schematize-market": "schematize_market_rs",
    "schematize-optimizer": "schematize_optimizer_rs",
    "schematize-deployer": "schematize_deployer_rs",
}

SEM_GIT = "fonte sem git"


def sha_do_binario(exe: str) -> str | None:
    """O SHA que o binário reporta no `--version`, ou `None` se ele não sabe.

    ONDE: `main`, um por app. Lê `0.2.1 (4900767818ff)` → `4900767818ff`.
    """
    try:
        out = subprocess.run([exe, "--version"], capture_output=True, text=True, timeout=20)
    except Exception:
        return None
    m = re.search(r"\(([0-9a-f]{7,40})\)", out.stdout + out.stderr)
    return m.group(1) if m else None


def veredito(sha_bin: str | None, sha_head: str | None) -> tuple[str, str]:
    """A decisão, PURA: `("ok"|"defasado"|"mudo"|"sem-repo", explicação)`.

    ONDE: `main` e `autoteste`. Pura para poder ser vista errando — a versão anterior deste
    guard vivia dentro do laço e não tinha como ser exercitada sem reinstalar binário.
    """
    if sha_head is None:
        return "sem-repo", "não há repo ao lado para comparar"
    if sha_bin is None:
        return "mudo", (
            "o binário instalado não diz de qual commit é — então não há como saber se está em "
            "dia. Reinstale de um checkout com git (ver `procedencia.rs`)"
        )
    # Prefixos: o binário pode trazer 12 e o repo responder 40, ou o contrário.
    if sha_bin.startswith(sha_head) or sha_head.startswith(sha_bin):
        return "ok", f"instalado == repo ({sha_bin})"
    return "defasado", f"instalado {sha_bin}, repo {sha_head}"


def autoteste() -> int:
    casos = [
        (("4900767818ff", "4900767818ff"), "ok", "iguais"),
        (("4900767818ff", "4900767818ffaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), "ok", "prefixo de 12 vs 40"),
        (("aaaaaaaaaaaa", "4900767818ff"), "defasado", "diferentes → defasado"),
        ((None, "4900767818ff"), "mudo", "binário que não diz o commit → REPROVA"),
        (("4900767818ff", None), "sem-repo", "sem repo ao lado → não vota"),
    ]
    falhou = 0
    for (b, h), esperado, desc in casos:
        got, _ = veredito(b, h)
        ok = got == esperado
        falhou |= 0 if ok else 1
        print(f"  [{'ok' if ok else 'FALHOU'}] {desc} → {got}")
    print("autoteste: " + ("VERDE" if not falhou else "VERMELHO"))
    return falhou


def main() -> int:
    if "--autoteste" in sys.argv:
        return autoteste()
    exigir = "--exigir" in sys.argv
    raiz = Path(__file__).resolve().parents[2]

    defasados, mudos, ok = [], [], []
    for exe, repo in APPS.items():
        caminho = shutil.which(exe)
        if not caminho:
            continue
        r = raiz / repo
        sha_head = None
        if (r / ".git").exists():
            p = subprocess.run(["git", "-C", str(r), "rev-parse", "HEAD"],
                               capture_output=True, text=True)
            if p.returncode == 0:
                sha_head = p.stdout.strip()
        estado, msg = veredito(sha_do_binario(exe), sha_head)
        linha = f"{exe}: {msg}"
        if estado == "defasado":
            defasados.append(linha)
        elif estado == "mudo":
            mudos.append(linha)
        elif estado == "ok":
            ok.append(linha)

    for l in ok:
        print(f"  ok        {l}")
    for l in defasados:
        print(f"  DEFASADO  {l}", file=sys.stderr)
    for l in mudos:
        print(f"  MUDO      {l}", file=sys.stderr)

    if mudos:
        print("\nBinário que não reporta o commit deixa a pergunta sem resposta possível.",
              file=sys.stderr)
        return 1
    if defasados:
        print("\nO binário instalado é de outro commit. Isto NÃO é aviso cosmético: um binário "
              "\nantigo executa a lógica antiga dizendo `✓` — foi assim que o `desktop --install` "
              "\ngravou o ícone errado por 15 dias. Reinstale com "
              "\n`cargo build --release && install -m755 target/release/<bin> ~/.cargo/bin/`.",
              file=sys.stderr)
        return 1 if exigir else 2
    print(f"binários instalados em dia — {len(ok)} conferido(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
