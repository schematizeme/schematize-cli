#!/usr/bin/env python3
"""Guard do ÍCONE: com a janela instalada, o `.desktop` abre a janela — ou ainda abre terminal?

# O estado real desta máquina em 2026-09-24, e por que nada acusava

| ícone | `Exec=` | `Terminal=` | janela instalada? |
|---|---|---|---|
| market | `schematize-market list --wait` | true | **SIM** |
| optimizer | `schematize-optimizer diag --wait` | true | não |
| deployer | `schematize-deployer panel --wait` | true | não |

A primeira linha é o defeito: a janela do market ESTAVA em `~/.cargo/bin` e o ícone abria um
terminal. O usuário descreveu isso como *"nenhuma das aplicações novas tem gui, só tem terminal
bem vagabundo"* — e estava certo.

**As caixas do checklist que cobriam isso estavam verdes, e não mentiam:** elas provavam que o
GERADOR do `.desktop` escreve a forma certa, num `HOME` temporário, com e sem janela. O gerador
está correto. O que ninguém verificava era **o arquivo que está no disco desta máquina** — e ele
só é regravado quando alguém chama `desktop --install`. Instale a janela depois do app, e o ícone
fica preso na forma de terminal até a próxima chamada.

**Prova de gerador não é prova de instalação.** Este guard olha o disco.

# Como rodar

    python3 scripts/icone-aponta-janela.py             # reprova (1) se algum ícone está errado
    python3 scripts/icone-aponta-janela.py --autoteste   # exercita a decisão nos dois sentidos
"""

import shutil
import sys
from pathlib import Path

APPS = ["market", "optimizer", "deployer"]


def veredito(tem_janela: bool, exec_linha: str | None, terminal: str | None,
             gui_bin: str) -> tuple[bool, str]:
    """A decisão, PURA. ONDE: `main` (um por app) e `autoteste`.

    Sem janela instalada, a forma de terminal é a CERTA — e dizer que está errada mandaria
    alguém "consertar" para um `Exec=` que aponta um arquivo inexistente, o que deixa o ícone
    morto. Esse é o desenho, não o defeito.
    """
    if exec_linha is None:
        return True, "sem .desktop instalado — nada a conferir"
    aponta_janela = exec_linha.strip().endswith(gui_bin)
    if not tem_janela:
        if aponta_janela:
            return False, (f"`Exec=` aponta `{gui_bin}`, que NÃO está instalado — ícone morto. "
                           "A forma de terminal é a correta sem a janela")
        return True, "sem janela; ícone no terminal, que é a forma certa"
    if not aponta_janela:
        return False, (f"a janela `{gui_bin}` ESTÁ instalada e o ícone ainda abre terminal "
                       f"(`Exec={exec_linha.strip()}`)")
    if (terminal or "").strip() != "false":
        return False, (f"`Exec=` aponta a janela mas `Terminal={terminal}` — abre um terminal "
                       "preto pendurado atrás dela")
    return True, "abre a janela, Terminal=false"


def autoteste() -> int:
    G = "schematize-market-gui"
    casos = [
        ((True, f"/b/{G}", "false", G), True, "janela instalada + Exec na janela → ok"),
        ((True, "/b/schematize-market list --wait", "true", G), False,
         "janela instalada + ícone no terminal → REPROVA (o bug desta máquina)"),
        ((True, f"/b/{G}", "true", G), False, "Exec na janela com Terminal=true → REPROVA"),
        ((False, "/b/schematize-market list --wait", "true", G), True,
         "sem janela + terminal → ok, é o desenho"),
        ((False, f"/b/{G}", "false", G), False, "sem janela + Exec na janela → REPROVA, ícone morto"),
        ((True, None, None, G), True, "sem .desktop → não vota"),
    ]
    falhou = 0
    for args, esperado, desc in casos:
        ok, _ = veredito(*args)
        bom = ok == esperado
        falhou |= 0 if bom else 1
        print(f"  [{'ok' if bom else 'FALHOU'}] {desc}")
    print("autoteste: " + ("VERDE" if not falhou else "VERMELHO"))
    return falhou


def campo(texto: str, chave: str) -> str | None:
    for l in texto.splitlines():
        if l.startswith(f"{chave}="):
            return l[len(chave) + 1:]
    return None


def main() -> int:
    if "--autoteste" in sys.argv:
        return autoteste()

    apps_dir = Path.home() / ".local/share/applications"
    problemas, oks = [], []
    for app in APPS:
        gui_bin = f"schematize-{app}-gui"
        d = apps_dir / f"schematize-{app}.desktop"
        texto = d.read_text(encoding="utf-8") if d.is_file() else None
        ok, msg = veredito(
            shutil.which(gui_bin) is not None,
            campo(texto, "Exec") if texto else None,
            campo(texto, "Terminal") if texto else None,
            gui_bin,
        )
        (oks if ok else problemas).append(f"{app}: {msg}")

    for l in oks:
        print(f"  ok      {l}")
    for l in problemas:
        print(f"  ERRADO  {l}", file=sys.stderr)
    if problemas:
        print("\nConserto: `schematize-<app> desktop --install` — e confira que o binário do app "
              "\nestá em dia (`scripts/instalado-em-dia.py`), porque um binário anterior à lógica "
              "\nda janela regrava a forma de terminal dizendo `✓`.", file=sys.stderr)
        return 1
    print(f"ícones apontam para onde devem — {len(oks)} conferido(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
