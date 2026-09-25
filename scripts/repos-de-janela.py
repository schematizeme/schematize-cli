#!/usr/bin/env python3
"""Guard do REPO CLONADO: o `release.yml` clona um repositório que EXISTE, e fala do app dono?

# O que este guard trava, e como a falha se esconde

Cada app da casa publica a janela dele **no próprio release**, clonando o repo dela dentro do
workflow (ADR-0014 D5 — o dono do binário é o dono da janela):

    git clone --depth 1 https://github.com/schematizeme/schematize_optimizer_gui_rs.git ../janela

Esse passo é `continue-on-error: true`, **e isso é deliberado**: a janela é chrome, e falhar o
release do gestor headless por causa dela inverteria a prioridade. O efeito colateral é que o
clone de uma URL que **não existe** não reprova nada. O release sai verde com um asset a menos,
e o 404 chega na máquina de quem instalou — porque o `install.sh` baixa
`schematize-<app>-gui-<plataforma>` de `releases/latest/download/`.

Foi exatamente assim que o `v0.2.0` do market saiu com 7 assets em vez de 8: faltavam as libs de
X11 no runner Linux, `outcome: failure` com `conclusion: success`, e o único asset ausente era o
da plataforma principal. Este guard é o nível acima daquele: lá faltava dependência, aqui falta
o repositório.

Medido em 2026-09-24: `schematize_optimizer_gui_rs` e `schematize_deployer_gui_rs` respondiam
**404**, e nenhuma verificação do ecossistema notava.

# A divisão de trabalho com o `assets-esperados.py`

São duas perguntas diferentes, e cobrir uma deixa a outra livre para divergir:

- `assets-esperados.py` — **os NOMES casam?** O que o `install.sh` baixa é o que algum
  `release.yml` declara publicar. Não toca a rede: compara intenção declarada em dois arquivos.
- este script — **a FONTE do nome existe?** O repo que o `release.yml` clona para produzir aquele
  asset é alcançável, e o workflow fala do app que o repo é dono.

O primeiro aprova um `asset_gui:` perfeitamente escrito cuja janela nunca será compilada. O
segundo é o que pega isso.

# As três respostas, e por que não são duas

`200` aprova. `404` reprova. **Sem rede não é nenhuma das duas** — e tratar "não pude verificar"
como "está bom" é o que transforma guard em decoração. O script sonda `github.com` primeiro: se
o controle também falha, ele diz PULADO em voz alta e sai 0 (um gate que reprova todo mundo no
avião é um gate que alguém desliga). Se o controle responde e o repo não, é 404 de verdade.

# A lista de pendências, e por que ela não apodrece

Repo que ainda não foi publicado é um ato humano pendente (push publica). Reprovar para sempre
seria livelock: o agente não pode fechar, e o gate travaria a entrega inteira. Então a pendência
é **declarada** em `packaging/repos-pendentes.txt`, com motivo — e a declaração tem prazo natural:
**entrada que volta a resolver REPROVA**, com a mensagem "saiu da pendência, tire da lista".
Exceção que ninguém revisa é exceção que vira mentira; esta se auto-cobra.

# Como rodar

    python3 scripts/repos-de-janela.py              # veredito por código de saída
    python3 scripts/repos-de-janela.py --autoteste   # exercita a decisão nos dois sentidos

Sem argumento procura os repos irmãos ao lado deste; `--repos <dir>` aponta outra raiz.
"""

import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

# Os apps com janela, que o `install.sh` instala — o mesmo conjunto do `assets-esperados.py`.
APPS_COM_JANELA = {
    "market": "schematize_market_rs",
    "optimizer": "schematize_optimizer_rs",
    "deployer": "schematize_deployer_rs",
}


def apps_com_release(raiz):
    """Todo repo irmão que TEM um `release.yml`, descoberto pelo disco.

    ONDE: `main`, para a checagem de nome de irmão na prosa.

    **Era uma lista fixa de três, e a lista fixa deixou o bug passar.** A fase E2 do ADR-0018
    criou `schematize_database_rs` e `schematize_git_rs` copiando o workflow do optimizer — e as
    notas de release dos DOIS saíram dizendo *"schematize optimizer"*. O guard não viu, porque os
    repos novos não estavam na lista. O defeito é o mesmo que ele existe para pegar, e ele foi
    cego para a própria classe por causa de um literal.

    Descobrir pelo disco não tem esse modo de falha: um app novo entra no guard no instante em
    que ganha um `release.yml`, sem ninguém lembrar de nada.
    """
    achados = {}
    for d in sorted(raiz.iterdir()):
        if not (d / ".github" / "workflows" / "release.yml").is_file():
            continue
        # `schematize_market_rs` → `market`; `schematize_cli_rs` → `cli`.
        nome = d.name.removeprefix("schematize_").removesuffix("_rs")
        if nome and nome != d.name:
            achados[nome] = d.name
    return achados

PENDENTES = Path("packaging/repos-pendentes.txt")


def clones_do_workflow(texto: str) -> list[str]:
    """As URLs de `github.com` que um workflow clona.

    Regex e não parser de YAML, de propósito, e pela mesma razão do `assets-esperados.py`:
    exigir PyYAML numa máquina de desenvolvimento é a diferença entre um guard que roda e um
    que alguém pula. O que se lê é uma linha de `run:`, não estrutura de YAML.
    """
    return re.findall(r"git clone[^\n]*?(https://github\.com/[\w.\-]+/[\w.\-]+?)(?:\.git)?\s", texto)


def nome_de_irmao_na_prosa(texto: str, app: str, irmaos: list[str] | None = None) -> list[str]:
    """Os irmãos citados como se fossem o dono — `schematize <irmao>` na nota do release.

    Procura só onde o nome é AFIRMADO sobre o artefato (`--notes`, `--title`), nunca em
    comentário: comentário que cita o irmão é procedência ("o mesmo desenho que o market usa"),
    e reprovar isso ensinaria a apagar a explicação em vez de o erro.
    """
    achados = []
    for linha in texto.splitlines():
        nu = linha.strip()
        if nu.startswith("#"):
            continue
        if "--notes" not in nu and "--title" not in nu:
            continue
        for irmao in irmaos if irmaos is not None else []:
            if f"schematize {irmao}" in nu or f"schematize-{irmao} " in nu:
                achados.append(f"{irmao}: {nu[:90]}")
    return achados


def veredito(existe: bool | None, declarado: bool) -> tuple[bool, str]:
    """A decisão, como função PURA — para poder ser vista reprovando.

    `existe` é `True` (200), `False` (404) ou `None` (não deu para saber). `declarado` diz se o
    repo está em `repos-pendentes.txt`.

    ONDE: `main`, um par por URL de clone; e `autoteste`, que exercita os quatro casos.
    """
    if existe is None:
        return True, "não verificado"
    if existe and declarado:
        return False, ("está em `packaging/repos-pendentes.txt` e JÁ RESOLVE — "
                       "saiu da pendência, tire da lista")
    if existe:
        return True, "existe"
    if declarado:
        return True, "404, e a pendência está declarada"
    return False, ("404 — o `git clone` do release falha, o `continue-on-error` engole, e o "
                   "asset da janela NUNCA é publicado")


def resolve(url: str) -> bool | None:
    """`True`/`False`/`None` para 200 / 404 / não deu para saber."""
    req = urllib.request.Request(url, method="HEAD")
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            return 200 <= r.status < 400
    except urllib.error.HTTPError as e:
        return False if e.code == 404 else None
    except Exception:
        return None


def autoteste() -> int:
    """Exercita `veredito` nos quatro casos, nos DOIS sentidos.

    ONDE: `--autoteste`, no gate e à mão. Guard que nunca foi visto reprovando é guard cego.
    """
    casos = [
        ((True, False), True, "repo existe e não está na lista → aprova"),
        ((False, False), False, "repo 404 e não declarado → REPROVA"),
        ((False, True), True, "repo 404 com pendência declarada → aprova"),
        ((True, True), False, "repo existe MAS está na lista → REPROVA (lista apodrecida)"),
        ((None, False), True, "sem rede → não vota"),
    ]
    falhou = 0
    for (ex, dec), esperado, desc in casos:
        ok, _ = veredito(ex, dec)
        marca = "ok" if ok == esperado else "FALHOU"
        if ok != esperado:
            falhou = 1
        print(f"  [{marca}] {desc}")
    print("autoteste do guard: " + ("VERDE" if not falhou else "VERMELHO"))
    return falhou


def main() -> int:
    if "--autoteste" in sys.argv:
        return autoteste()

    raiz = Path(sys.argv[sys.argv.index("--repos") + 1]) if "--repos" in sys.argv \
        else Path(__file__).resolve().parents[2]

    pend_txt = Path(__file__).resolve().parents[1] / PENDENTES
    declarados = set()
    if pend_txt.is_file():
        for linha in pend_txt.read_text(encoding="utf-8").splitlines():
            alvo = linha.split("#", 1)[0].strip()
            if alvo:
                declarados.add(alvo)

    controle = resolve("https://github.com/schematizeme/schematize_market_rs")
    if controle is None:
        print("repos de janela: PULADO — github.com não respondeu (sem rede?). "
              "Nada foi verificado, e isto NÃO é um verde.")
        return 0

    # Os apps com `release.yml`, descobertos pelo disco — a checagem de nome de irmão vale para
    # TODOS eles, não só para os três que instalam janela.
    todos_apps = apps_com_release(raiz)

    problemas = []

    # AS PENDÊNCIAS PRIMEIRO, E INDEPENDENTES DO QUE OS WORKFLOWS CLONAM.
    #
    # A primeira versão deste bloco só conferia as URLs que algum `git clone` citava — e por
    # isso uma linha da lista apontando um repo que NENHUM workflow clona nunca era verificada:
    # ficaria lá para sempre, apodrecida, sem uma palavra. Foi visto acontecendo (uma entrada de
    # teste para `schematize_market_rs`, que existe, passou batida). A lista tem de se cobrar
    # sozinha, não de carona no laço dos clones.
    citados = set()
    vistos = 0

    for slug in sorted(declarados):
        ok, motivo = veredito(resolve(f"https://github.com/{slug}"), True)
        if not ok:
            problemas.append(f"`packaging/repos-pendentes.txt` declara `{slug}` — {motivo}")

    # 1) Os apps COM JANELA precisam de `release.yml` — é ele que publica a janela.
    for app, repo in APPS_COM_JANELA.items():
        if not (raiz / repo / ".github" / "workflows" / "release.yml").is_file():
            problemas.append(
                f"{repo}: sem release.yml — a janela do `{app}` não é publicada por ninguém"
            )

    # 2) A prosa e os clones, em TODO app que tem release.
    for app, repo in todos_apps.items():
        wf = raiz / repo / ".github" / "workflows" / "release.yml"
        texto = wf.read_text(encoding="utf-8")
        irmaos = [a for a in todos_apps if a != app]

        for trocado in nome_de_irmao_na_prosa(texto, app, irmaos):
            problemas.append(
                f"{repo}/release.yml afirma o nome de um IRMÃO no texto do release → {trocado}\n"
                f"    Esse texto fica na página do release para sempre."
            )

        for url in clones_do_workflow(texto):
            vistos += 1
            slug = "/".join(url.rstrip("/").split("/")[-2:])
            citados.add(slug)
            if slug in declarados:
                continue  # já julgado, e com mais rigor, no bloco das pendências
            ok, motivo = veredito(resolve(url), False)
            if not ok:
                problemas.append(f"{repo}/release.yml clona `{slug}` — {motivo}")

    # Entrada que nenhum workflow clona é entrada MORTA: ela não protege nada e ninguém a revisa.
    for orfa in sorted(declarados - citados):
        problemas.append(
            f"`packaging/repos-pendentes.txt` declara `{orfa}`, e nenhum `release.yml` o clona.\n"
            f"    A linha não cobre risco nenhum — tire-a, ou conserte o workflow que devia citá-lo."
        )

    if problemas:
        print("REPOS DE JANELA / RELEASE INCOERENTE:", file=sys.stderr)
        for p in problemas:
            print(f"  - {p}", file=sys.stderr)
        print("\nPendência de publicação se declara em `packaging/repos-pendentes.txt`, "
              "com motivo — e sai da lista quando o repo passa a existir.", file=sys.stderr)
        return 1

    print(f"repos de janela OK — {vistos} clone(s) de release conferido(s), "
          f"{len(declarados)} ainda por publicar (declarado), "
          f"{vistos - len(declarados & citados)} já resolvendo; "
          f"nenhum nome de irmão na prosa")
    return 0


if __name__ == "__main__":
    sys.exit(main())
