#!/usr/bin/env python3
"""Gera o inventário de extradição: cada função do app principal, com destino e motivo."""
import re, collections, pathlib, sys

# destino -> (rotulo, se e app novo)
DESTINOS = {
    "hub":       ("FICA no hub", False),
    "optimizer": ("-> schematize_optimizer_rs", False),
    "market":    ("-> schematize_market_rs", False),
    "deployer":  ("-> schematize_deployer_rs", False),
    "overdev":   ("-> schematize_overdev_rs (NOVO)", True),
    "skills":    ("-> schematize_skills_rs (NOVO)", True),
    "database":  ("-> schematize_database_rs (NOVO)", True),
    "git":       ("-> schematize_git_rs (NOVO)", True),
}

# arquivo -> (destino, motivo). UM motivo por arquivo, e nenhum arquivo sem motivo.
C = {
# ---- CLI: o motor do overdev -------------------------------------------------
"src/overdev/mod.rs":        ("overdev", "motor do overdev; ADR-0012 F5 ja autorizou a saida"),
"src/overdev/caixa.rs":      ("overdev", "as caixas do checklist sao o dado central do overdev"),
"src/overdev/resposta.rs":   ("overdev", "responder/recusar item humano — nucleo da fila"),
"src/overdev/notas.rs":      ("overdev", "notas do run"),
"src/overdev/gate.rs":       ("overdev", "o gate por codigo de saida e do overdev"),
"src/overdev/supervisor.rs": ("overdev", "relanca o agente; so faz sentido dentro do overdev"),
"src/overdev/trava.rs":      ("overdev", "trava de concorrencia entre runs"),
"src/overdev/progresso.rs":  ("overdev", "progresso do run"),
"src/overdev/conclusoes.rs": ("overdev", "log de conclusoes do run"),
"src/overdev/arquivo.rs":    ("overdev", "leitura/escrita dos arquivos do run"),
"src/overdev/divisao.rs":    ("overdev", "divisao de trabalho do run"),
"src/overdevdb.rs":          ("overdev", "o SQLite do overdev; o unico consumidor do rusqlite"),
"src/overdev/pergunta.rs":   ("overdev", "a FILA DE QUIZ: modelo e invariantes das perguntas"),
"src/cli/perguntas.rs":      ("overdev", "os subcomandos `ask|questions|reply|review`"),
"src/cli/overdev.rs":        ("overdev", "os subcomandos `overdev`"),
"src/cli/caixa.rs":          ("overdev", "subcomando de caixa"),
"src/settings.rs":           ("overdev", "registra os HOOKS do overdev no settings.json do Claude"),
"src/panel/mod.rs":          ("overdev", "painel HTML do estado do overdev"),
"src/panel/parse.rs":        ("overdev", "parser dos arquivos do run para o painel"),
"src/panel/estado.rs":       ("overdev", "estado do run para o painel"),
"src/panel/html.rs":         ("overdev", "render HTML do painel"),
"src/panel/grafo.rs":        ("hub", "grafo force-directed do INDEX, nao do overdev — e do hub"),
"src/panel/obsidian.rs":     ("hub", "export Obsidian do grafo do index — do hub"),
"src/archive.rs":            ("overdev", "o archive e a memoria durável do overdev e dos runs"),
# ---- CLI: skills -------------------------------------------------------------
"src/skills.rs":             ("skills", "instalar/versionar skills; ADR-0012 F4 ja autorizou"),
"src/skilledit.rs":          ("skills", "autoria de skills"),
"src/skillsproj.rs":         ("skills", "qual versao de skill foi aplicada ao projeto"),
"src/registry.rs":           ("skills", "catalogo remoto de skills"),
"src/cli/skills.rs":         ("skills", "os subcomandos `skills`"),
"src/cli/skillsproj.rs":     ("skills", "os subcomandos de skill por projeto"),
"src/guiactions.rs":         ("skills", "contrato skill -> botao na GUI; o dono do contrato e a skill"),
# ---- CLI: disco e capacidade da maquina -> optimizer ------------------------
"src/disco/mod.rs":          ("optimizer", "inventario e limpeza de lixo recriavel = dominio da maquina"),
"src/disco/docker.rs":       ("optimizer", "camadas de Docker ocupando disco"),
"src/disco/artefatos.rs":    ("optimizer", "artefato de build recriavel"),
"src/disco/caches.rs":       ("optimizer", "cache de toolchain"),
"src/disco/tamanho.rs":      ("optimizer", "medicao de tamanho em disco"),
"src/disco/montagem.rs":     ("optimizer", "agrupamento por disco/montagem"),
"src/cli/disco.rs":          ("optimizer", "os subcomandos `disco`"),
"src/agents.rs":             ("optimizer", "governador de concorrencia por CPU/RAM/load = capacidade da maquina"),
# ---- CLI: database builder -> app proprio ----------------------------------
# E1 EM ANDAMENTO: estes dois JA EXISTEM em `schematize_database_rs`, com saida byte a byte
# identica. Seguem classificados aqui porque a COPIA do hub ainda nao foi apagada — ela so morre
# no M5, junto com a delegacao da aba. Enquanto as duas existem, o inventario conta as duas, que
# e o estado real do sistema e nao o desejado.
"src/database.rs":           ("database", "JA MIGRADO (E1); a copia do hub morre no M5"),
"src/cli/db.rs":             ("database", "JA MIGRADO (E1); a copia do hub morre no M5"),
# ---- CLI: contas git -> app proprio ---------------------------------------
"src/gitcontas/contas.rs":   ("git", "multiplas identidades git na maquina: dominio proprio"),
"src/gitcontas/deteccao.rs": ("git", "deteccao de conta por repo"),
"src/gitcontas/repos.rs":    ("git", "inventario de repos locais"),
"src/gitcontas/aplicar.rs":  ("git", "aplicar identidade no repo"),
"src/cli/git.rs":            ("git", "os subcomandos `git`"),
"src/githist.rs":            ("git", "historico de commits para a tela de contas"),
# ---- CLI: instalar/atualizar -> market (ADR-0013) ---------------------------
"src/selfupdate.rs":         ("market", "ADR-0013: o market e o dono de ATUALIZAR — isto e a copia que sobrou"),
"src/upgrade.rs":            ("market", "`upgrade` recompila o CLI/GUI: instalar/atualizar e do market"),
"src/versoes.rs":            ("market", "ultima versao de um repo da casa: o market ja resolve isso"),
"src/procedencia.rs":        ("hub", "de qual COMMIT este binario e: plataforma, cada app tem a sua (D4)"),
"src/guipin.rs":             ("market", "qual commit da GUI um release publica = esteira de release"),
"src/market.rs":             ("market", "notas/ratings do marketplace"),
"src/gestorboot.rs":         ("hub", "SHIM de delegacao ao market; e a ponte, nao a copia"),
"src/applink.rs":            ("hub", "SHIM de encaminhamento aos apps da casa"),
"src/deployerlink.rs":       ("hub", "SHIM de ponte com o deployer"),
# ---- CLI: nucleo do hub ----------------------------------------------------
"src/main.rs":               ("hub", "entrypoint"),
"src/lib.rs":                ("hub", "raiz do lib"),
"src/cli/diversos.rs":       ("hub", "comandos avulsos do hub"),
"src/cli/prompt.rs":         ("hub", "leitura de prompt no terminal (plataforma)"),
"src/cli/conta.rs":          ("hub", "subcomandos de conta"),
"src/cli/deployer.rs":       ("hub", "SHIM: encaminha ssh/vps/mcp ao deployer"),
"src/account.rs":            ("hub", "CLIENTE do IdP; o auth e app a parte (piso 16), o cliente fica"),
"src/util.rs":               ("hub", "plataforma"),
"src/config.rs":             ("hub", "config do usuario (idioma)"),
"src/paths.rs":              ("hub", "layout de diretorios do projeto: ponto UNICO, usado por todos"),
"src/i18n.rs":               ("hub", "catalogo de traducao"),
"src/projects.rs":           ("hub", "descoberta de projetos: o hub e o navegador de projetos"),
"src/links.rs":              ("hub", "URLs canonicas + `open`"),
"src/news.rs":              ("hub", "feed do blog"),
"src/usage.rs":              ("hub", "tokens/modelo do transcript do Claude: telemetria do ecossistema"),
"src/status.rs":             ("hub", "painel geral do ambiente: agrega os outros, e do hub"),
"src/doctor.rs":             ("hub", "diagnostico do ecossistema inteiro"),
"src/debug.rs":              ("hub", "modo debug"),
"src/diagnostics.rs":        ("hub", "envio de diagnostico"),
"src/debugreport/mod.rs":    ("hub", "relatorio de bug do ecossistema: agrega os 9 servicos"),
"src/debugreport/sonda.rs":  ("hub", "sondas do relatorio"),
"src/debugreport/secoes.rs": ("hub", "secoes do relatorio"),
"src/debugreport/redacao.rs":("hub", "redacao de segredo no relatorio: seguranca, fica no agregador"),
"src/debugreport/fmt.rs":    ("hub", "formatacao do relatorio"),
"src/agent.rs":              ("hub", "agente residente que notifica: do hub"),
"src/agentrun/mod.rs":       ("hub", "lancamento de runs do Claude: plataforma do hub"),
"src/agentrun/lancador.rs":  ("hub", "lancador de terminal"),
"src/agentrun/prompts.rs":   ("hub", "prompts dos runs"),
"src/notifications.rs":      ("hub", "notificacao de desktop"),
"src/notificacoes/cache.rs": ("hub", "cache de notificacao"),
"src/notificacoes/formato.rs":("hub", "formato de notificacao"),
"src/autostart.rs":          ("hub", "bind do agente ao login (plataforma)"),
"src/appicon.rs":            ("hub", "geracao do icone PNG (plataforma)"),
# ---- HUB GUI ---------------------------------------------------------------
"src/wire/overdev.rs":       ("overdev", "a tela do overdev vai com o app do overdev"),
"src/wire/caixa.rs":         ("overdev", "caixas do checklist na tela"),
"src/wire/odhistory.rs":     ("overdev", "historico de runs na tela"),
"src/wire/quiz.rs":          ("overdev", "os callbacks do quiz na aba Overdev"),
"src/quizmodel.rs":          ("overdev", "parser PURO da fila de quiz"),
"src/checklist.rs":          ("overdev", "modelo do checklist na GUI"),
"src/checklistview.rs":      ("overdev", "render do checklist"),
"src/odproj.rs":             ("overdev", "projeto do overdev na GUI"),
"src/odload.rs":             ("overdev", "carga dos arquivos do run"),
"src/odmonitor.rs":          ("overdev", "monitor do run em andamento"),
"src/odhistory.rs":          ("overdev", "historico na GUI"),
"src/wire/database.rs":      ("database", "a tela do database builder vai com o app dele"),
"src/dbbuilder.rs":          ("database", "modelo do builder na GUI"),
"src/wire/skills.rs":        ("skills", "a tela de skills vai com o app de skills"),
"src/skillrows.rs":          ("skills", "linhas da tela de skills"),
"src/skilljobs.rs":          ("skills", "jobs de instalacao de skill"),
"src/wire/disco.rs":         ("optimizer", "a tela de disco vai com o optimizer"),
"src/discorows.rs":          ("optimizer", "linhas da tela de disco"),
"src/wire/git.rs":           ("git", "a tela de contas git vai com o app dela"),
"src/gitrows.rs":            ("git", "linhas da tela de contas git"),
"src/wire/appversion.rs":    ("market", "versao dos apps na tela: o market e o dono de versao"),
"src/wire/envs.rs":          ("hub", "JA DELEGA ao market-gui (F3); o que sobrou e a ponte"),
"src/envrows.rs":            ("hub", "sobrou servindo outras telas depois da F3 — ponte"),
"src/marketlink.rs":         ("hub", "SHIM: pergunta ao market por subprocesso"),
"src/sysenv.rs":             ("hub", "resolucao de binario/ambiente do sistema (plataforma)"),
"src/wire/manage.rs":        ("hub", "TelaDelegada parametrizada: e o mecanismo de delegacao"),
"src/wire/account.rs":       ("hub", "tela de conta: cliente do IdP"),
"src/wire/settings.rs":      ("hub", "preferencias do hub"),
"src/wire/graph.rs":         ("hub", "tela do grafo do index: do hub"),
"src/graphview.rs":          ("hub", "render do grafo"),
"src/graphstate.rs":         ("hub", "estado do grafo"),
"src/repulsion.rs":          ("hub", "fisica do layout do grafo"),
"src/spiral.rs":             ("hub", "layout em espiral do grafo"),
"src/wire/mod.rs":           ("hub", "raiz do wire"),
"src/main.rs GUI":           ("hub", "entrypoint da janela"),
"src/i18nbind.rs":           ("hub", "bind do i18n na janela"),
"src/fmt.rs":                ("hub", "formatacao de numero/data na janela"),
"build.rs":                  ("hub", "compilacao dos .slint"),
}

def contagens():
    out = {}
    for repo in ("schematize_cli_rs", "schematize_gui_slint"):
        txt = pathlib.Path(f".schematize/grafos/{repo}.md").read_text(encoding="utf-8")
        corpo = txt[txt.index("## Funcoes (nos)"):txt.index("## Fronteira")]
        c = collections.Counter()
        for m in re.finditer(r"\|\s*([\w./-]+\.(?:rs|slint)):\d+\s*\|?\s*$", corpo, re.M):
            c[m.group(1)] += 1
        out[repo] = c
    return out

def main():
    cont = contagens()
    linhas, faltando, total = [], [], 0
    for repo, c in cont.items():
        for arq, n in sorted(c.items()):
            rel = arq.split("/", 1)[1]
            chave = "src/main.rs GUI" if (rel == "src/main.rs" and repo.endswith("slint")) else rel
            if chave not in C:
                faltando.append(f"{repo}/{rel} ({n} unidades)")
                continue
            dest, motivo = C[chave]
            linhas.append((repo, rel, n, dest, motivo))
            total += n
    if faltando:
        print("ARQUIVOS SEM CLASSIFICACAO — inventario nao e exaustivo:", file=sys.stderr)
        for f in faltando:
            print("  -", f, file=sys.stderr)
        return 1
    esperado = sum(sum(c.values()) for c in cont.values())
    if total != esperado:
        print(f"CONTAGEM NAO FECHA: classificadas {total}, grafo diz {esperado}", file=sys.stderr)
        return 1
    print(f"OK — {total} unidades classificadas em {len(linhas)} arquivos, e o total bate com o grafo")
    por_dest = collections.Counter()
    for _, _, n, d, _ in linhas:
        por_dest[d] += n
    for d, n in por_dest.most_common():
        print(f"  {n:5d}  {DESTINOS[d][0]}")
    pathlib.Path("/tmp/inventario.tsv").write_text(
        "\n".join(f"{r}\t{a}\t{n}\t{d}\t{m}" for r, a, n, d, m in linhas), encoding="utf-8")
    return 0

sys.exit(main())
