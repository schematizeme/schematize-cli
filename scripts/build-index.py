#!/usr/bin/env python3
"""Gerador do GRAFO de funcionalidades (secao 39) dos repos Rust deste workspace.

O QUE: varre cada sub-repo, enumera EXAUSTIVAMENTE toda unidade chamavel (fn livre,
metodo de impl/trait com corpo, closure nomeada, handler de UI), extrai o doc-comment
como descricao, resolve as chamadas intra-servico (arestas) e as saidas que cruzam a
fronteira do repo, e emite `.schematize/grafos/<servico>.md` + `GRAFO_GLOBAL.md`.

DE ONDE VEM: os arquivos `.rs` de cada sub-repo (fonte unica de verdade).
PRA ONDE VAI: `.schematize/grafos/` (operacional, o que o app desenha) e o espelho
durable em `<projeto>_archive/index/`.

INVARIANTE: `nº de entradas na tabela == nº de unidades encontradas no codigo` (M == N).
As arestas saem SEMPRE em ASCII (`A -> B`), nunca a seta unicode — o parser do app
(`panel/parse.rs`) le ASCII.
"""
import os, re, sys, json
from pathlib import Path
from collections import defaultdict

# scripts/ -> schematize_cli_rs/ -> raiz do umbrella (workspace com os 4 repos)
ROOT = Path(__file__).resolve().parents[2]

REPOS = {
    "schematize_cli_rs": dict(
        what="App schematize (CLI Rust): instala/versiona skills, motor overdev, grafos, contas, envs, ssh, db.",
        stack="Rust/clap", runs="binário local"),
    "schematize_gui_slint": dict(
        what="Face gráfica do app (Slint): mesma engine da CLI, painel de projetos/overdev/skills.",
        stack="Rust/Slint", runs="app desktop"),
    # APOSENTADO pelo ADR-0013: absorvido pelo `schematize_market_rs`. Continua no indice de
    # proposito — o grafo conta a evolucao do sistema, e um servico que some sem deixar rastro
    # apaga justamente a pergunta "e o updater, para onde foi?". A descricao diz o destino.
    "schematize_updater_rs": dict(
        what="APOSENTADO (ADR-0013) — absorvido pelo schematize_market_rs. Era o bootstrapper/gestor de versão cross-OS.",
        stack="Rust", runs="arquivado"),
    "schematize_updater_gui_rs": dict(
        what="Janela (Slint) do gestor: casca fina sobre o `schematize-market`, lendo o contrato `status --json` (ADR-0014). Binário: schematize-market-gui.",
        stack="Rust/Slint", runs="app desktop"),
    # Os apps que saíram do hub (ADR-0010/0011/0012). Entram aqui porque o índice é
    # EXAUSTIVO por contrato (§39/C3): um serviço da casa fora da tabela é um buraco no
    # grafo, e o grafo é a fonte que se consulta ANTES de criar algo — um serviço invisível
    # é um serviço que alguém vai reimplementar sem saber que já existe.
    "schematize_deployer_rs": dict(
        what="App schematize Deployer: chaves SSH, VPS, DNS (Cloudflare) e cofre de credenciais.",
        stack="Rust/clap", runs="binário local"),
    "schematize_optimizer_rs": dict(
        what="App schematize Optimizer: mede o ambiente de dev e põe cada software no seu teto de recurso.",
        stack="Rust/clap", runs="binário local"),
    "schematize_market_rs": dict(
        what="App schematize Market: DONO de instalar e atualizar (ADR-0013) — runtimes, ferramentas de dev, os apps da casa, o app e ele mesmo.",
        stack="Rust/clap", runs="binário local"),
    # As JANELAS dos apps que ganharam uma. Cada uma vive em repo próprio e NÃO depende do
    # crate do app: fala só com o binário headless, pelo `--json`. Depender do crate as faria
    # embutir a versão via git-dep — o bug que fez a janela do market "abrir a versão antiga".
    "schematize_optimizer_gui_rs": dict(
        what="Janela (Slint) do Optimizer: diagnóstico da máquina e tetos por software, lendo `diag/limits/services --json`. Mostra o que MUDA antes de aplicar.",
        stack="Rust/Slint", runs="app desktop"),
    "schematize_deployer_gui_rs": dict(
        what="Janela (Slint) do Deployer: chaves SSH, hosts e estado do cofre, lendo `ssh list/vps list/vault status --json`. Segredo NUNCA aparece; passphrase vai para o terminal.",
        stack="Rust/Slint", runs="app desktop"),
    # Os apps da EXTRADICAO (ADR-0018): o que saiu do app principal porque nao fala de skill,
    # de overdev nem de instalacao. Entram aqui no MESMO commit em que nascem — um servico que
    # existe e nao esta no grafo e um servico que alguem reimplementa sem saber que ja existe,
    # e o grafo e justamente a fonte que se consulta ANTES de criar algo.
    "schematize_database_rs": dict(
        what="App schematize Database (E1 do ADR-0018): modela o schema relacional, introspecta SQLite/Postgres e gera SQL e migration expand-contract. Le e emite `--json`.",
        stack="Rust/clap", runs="binário local"),
    "schematize_git_rs": dict(
        what="App schematize Git (E2 do ADR-0018): mais de uma identidade git na mesma maquina, sem commitar com a errada. Contas, alias SSH por conta, e o que ainda nao saiu da maquina.",
        stack="Rust/clap", runs="binário local"),
    "schematize_skills_rs": dict(
        what="App schematize Skills (E5 do ADR-0018, ADR-0012 F4): o catalogo, o que esta instalado, a autoria e o fork. E o unico dono do `gui.json`, o formato em que uma skill de TERCEIRO declara um botao na interface.",
        stack="Rust/clap + Slint", runs="binário local"),
}

# ---------------------------------------------------------------- limpeza lexica

def strip_comments(src: str) -> str:
    """Substitui SO os comentarios por espaco, PRESERVANDO os literais e os offsets.

    Por que existe, separado do [`strip_noise`]: a deteccao de fronteira precisa VER o nome do
    binario, que e um literal — entao nao pode usar o `strip_noise`, que apaga literais. Mas
    usar o corpo CRU faz um nome citado num COMENTARIO virar aresta, e foi exatamente isso que
    pos 46 nos do CLI apontando para o `schematize_updater_rs`: um servico APOSENTADO, que o
    CLI so procura no disco e nunca executa. Os comentarios que explicam a aposentadoria eram a
    "prova" da dependencia.

    Citar o nome num comentario nao e fronteira. So produz saida quem dispara.
    """
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == '/' and i + 1 < n and src[i + 1] == '/':
            j = src.find('\n', i)
            j = n if j < 0 else j
            for k in range(i, j):
                out[k] = ' '
            i = j
            continue
        if c == '/' and i + 1 < n and src[i + 1] == '*':
            j = src.find('*/', i + 2)
            j = n if j < 0 else j + 2
            for k in range(i, j):
                if out[k] != '\n':
                    out[k] = ' '
            i = j
            continue
        # Dentro de literal: pula ate fechar, para nao confundir um `//` de URL com comentario.
        if c == '"':
            i += 1
            while i < n and src[i] != '"':
                i += 2 if src[i] == '\\' else 1
            i += 1
            continue
        i += 1
    return ''.join(out)


def strip_noise(src: str) -> str:
    """Substitui comentarios e literais de string por espaco, PRESERVANDO offsets.

    Por que: a deteccao de chamadas varre identificadores; sem isso, um nome citado
    num comentario ou numa string virava aresta falsa. Preserva o comprimento pra
    que os offsets (e portanto os numeros de linha) continuem validos.
    """
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == '/' and i + 1 < n and src[i+1] == '/':
            j = src.find('\n', i)
            j = n if j < 0 else j
            for k in range(i, j):
                out[k] = ' '
            i = j
        elif c == '/' and i + 1 < n and src[i+1] == '*':
            depth, j = 1, i + 2
            while j < n and depth:
                if src[j] == '/' and j + 1 < n and src[j+1] == '*':
                    depth += 1; j += 2
                elif src[j] == '*' and j + 1 < n and src[j+1] == '/':
                    depth -= 1; j += 2
                else:
                    j += 1
            for k in range(i, min(j, n)):
                if src[k] != '\n':
                    out[k] = ' '
            i = j
        elif c == 'r' and i + 1 < n and src[i+1] in '#"':
            m = re.match(r'r(#*)"', src[i:])
            if m:
                hashes = m.group(1)
                close = '"' + hashes
                j = src.find(close, i + m.end() - m.start())
                j = n if j < 0 else j + len(close)
                for k in range(i, min(j, n)):
                    if src[k] != '\n':
                        out[k] = ' '
                i = j
            else:
                i += 1
        elif c == '"':
            j = i + 1
            while j < n:
                if src[j] == '\\':
                    j += 2; continue
                if src[j] == '"':
                    j += 1; break
                j += 1
            for k in range(i, min(j, n)):
                if src[k] != '\n':
                    out[k] = ' '
            i = j
        else:
            i += 1
    return ''.join(out)


def body_span(clean: str, start: int):
    """Do offset `start`, acha o `{` da abertura e devolve (ini, fim) do corpo balanceado."""
    i = clean.find('{', start)
    if i < 0:
        return None
    depth, j, n = 0, i, len(clean)
    while j < n:
        if clean[j] == '{':
            depth += 1
        elif clean[j] == '}':
            depth -= 1
            if depth == 0:
                return (i + 1, j)
        j += 1
    return (i + 1, n)

# ---------------------------------------------------------------- extracao

FN_RE = re.compile(
    r'(?m)^(?P<indent>[ \t]*)'
    r'(?P<vis>pub(?:\s*\([^)]*\))?\s+)?'
    r'(?:default\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?'
    r'(?:extern\s+"[^"]*"\s+)?'
    r'fn\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)')

# closure nomeada: `let nome = |..|` / `let nome = move |..|`
CLOSURE_RE = re.compile(
    r'(?m)^[ \t]*let\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*=\s*(?:move\s+)?\|')

# handler de UI (Slint): `.on_algo(move |..|` ou `.on_algo(|..|`
HANDLER_RE = re.compile(r'\.(?P<name>on_[A-Za-z0-9_]+)\s*\(\s*(?:move\s*)?\|')


def line_of(src: str, off: int) -> int:
    return src.count('\n', 0, off) + 1


def doc_above(lines, idx):
    """Colhe o doc-comment (`///`) imediatamente acima da linha `idx` (0-based)."""
    out = []
    k = idx - 1
    while k >= 0:
        s = lines[k].strip()
        if s.startswith('///'):
            out.append(s[3:].strip())
            k -= 1
        elif s.startswith('#[') or s.startswith('#!['):
            k -= 1                      # atributos nao cortam o doc
        elif s == '':
            break
        else:
            break
    return list(reversed(out))


def humanize(name: str) -> str:
    """Fallback quando nao ha doc-comment: nome -> frase legivel."""
    if name.startswith('on_'):
        return 'handler da UI para ' + name[3:].replace('_', ' ')
    return name.replace('_', ' ')


def first_sentence(doc_lines):
    """Primeira frase util do doc — vira a coluna 'O que' (UMA linha, secao 39)."""
    txt = ' '.join(d for d in doc_lines if d).strip()
    if not txt:
        return ''
    txt = re.sub(r'\s+', ' ', txt)
    for marker in ('. ', '? ', '! '):
        p = txt.find(marker)
        if 0 < p < 180:
            txt = txt[:p + 1]
            break
    txt = txt.rstrip('.').strip()
    return txt[:200]


def module_of(relpath: str) -> str:
    """Caminho de modulo legivel a partir do arquivo (pra qualificar nome ambiguo)."""
    p = Path(relpath)
    parts = list(p.parts)
    if parts and parts[0] == 'src':
        parts = parts[1:]
    if not parts:
        return p.stem
    parts[-1] = Path(parts[-1]).stem
    if parts[-1] == 'mod' and len(parts) > 1:
        parts = parts[:-1]
    return '::'.join(parts)


IMPL_RE = re.compile(r'(?m)^[ \t]*impl(?:\s*<[^>]*>)?\s+(?:(?P<trait>[A-Za-z_][\w:]*(?:\s*<[^>]*>)?)\s+for\s+)?(?P<ty>[A-Za-z_][\w:]*)')


def impl_spans(clean: str):
    """Spans dos blocos `impl` com o tipo — usado pra qualificar metodo homonimo.

    Por que: `new`/`run`/`fmt` se repetem em varios `impl` do mesmo modulo. Sem o tipo,
    duas unidades distintas ganham o MESMO id e o parser as funde num no so — some
    unidade do grafo e o gate de contagem (M == N) nao fecha.
    """
    out = []
    for m in IMPL_RE.finditer(clean):
        sp = body_span(clean, m.end())
        if sp:
            out.append((sp[0], sp[1], m.group('ty').split('::')[-1]))
    return out


CFG_TEST_RE = re.compile(r'#\[cfg\(test\)\]')


def test_spans(clean: str):
    """Spans dos modulos `#[cfg(test)]` — enumeracao os EXCLUI.

    Por que: o grafo (secao 39) mapeia o SISTEMA — o que existe pra ser chamado em
    producao. Funcao de teste e andaime de verificacao, nao superficie: contá-la
    inflaria o total e faria o N do gate variar a cada teste novo (foi o que
    aconteceu quando este proprio gerador ganhou um teste). Regra declarada no
    cabecalho de cada grafo pra a contagem ser reproduzivel e auditavel.
    """
    spans = []
    for m in CFG_TEST_RE.finditer(clean):
        sp = body_span(clean, m.end())
        if sp:
            spans.append(sp)
    return spans


def scan_repo(repo: str):
    """Enumera TODA unidade chamavel do repo. Devolve (units, files_scanned)."""
    base = ROOT / repo
    units = []
    files = []
    for path in sorted(base.rglob('*.rs')):
        rel = path.relative_to(base).as_posix()
        if rel.startswith('target/') or '/target/' in rel:
            continue
        if rel.startswith('tests/') or rel.startswith('benches/') or rel.startswith('examples/'):
            continue
        files.append(rel)
        src = path.read_text(encoding='utf-8', errors='replace')
        clean = strip_noise(src)
        lines = src.split('\n')
        mod = module_of(rel)
        tspans = test_spans(clean)
        in_test = lambda off: any(a <= off <= b for a, b in tspans)
        ispans = impl_spans(clean)

        def impl_ty(off):
            """Tipo do `impl` mais INTERNO que contem `off` (None fora de impl)."""
            best, bestlen = None, None
            for a, b, ty in ispans:
                if a <= off <= b and (bestlen is None or (b - a) < bestlen):
                    best, bestlen = ty, b - a
            return best

        for m in FN_RE.finditer(clean):
            off = m.start('name')
            if in_test(off):
                continue
            ln = line_of(src, off)
            # trait sem corpo (`fn foo(&self);`) nao e unidade com corpo, mas E
            # unidade chamavel declarada; o `;` antes de `{` denuncia a assinatura pura.
            tail = clean[m.end('name'): m.end('name') + 400]
            semi, brace = tail.find(';'), tail.find('{')
            has_body = not (semi >= 0 and (brace < 0 or semi < brace))
            span = body_span(clean, m.end('name')) if has_body else None
            units.append(dict(
                name=m.group('name'), mod=mod, file=rel, line=ln,
                vis='publica' if m.group('vis') else 'interna',
                kind='fn', span=span, clean=clean, doc=doc_above(lines, ln - 1),
                has_body=has_body, ity=impl_ty(off)))

        for m in CLOSURE_RE.finditer(clean):
            off = m.start('name')
            if in_test(off):
                continue
            ln = line_of(src, off)
            units.append(dict(
                name=m.group('name'), mod=mod, file=rel, line=ln, vis='interna',
                kind='closure', span=None, clean=clean, doc=doc_above(lines, ln - 1),
                has_body=True, ity=impl_ty(off)))

        for m in HANDLER_RE.finditer(clean):
            off = m.start('name')
            if in_test(off):
                continue
            ln = line_of(src, off)
            span = body_span(clean, m.end('name'))
            units.append(dict(
                name=m.group('name'), mod=mod, file=rel, line=ln, vis='publica',
                kind='handler', span=span, clean=clean, doc=doc_above(lines, ln - 1),
                has_body=True, ity=impl_ty(off)))

    # nome de exibicao: qualifica com o modulo quando o nome bare colide no repo
    # Display UNICO no repo, do mais curto ao mais qualificado: `nome` ->
    # `Tipo::nome` -> `mod::Tipo::nome` -> `mod::Tipo::nome@linha`. Id duplicado faz o
    # parser fundir duas unidades num no so e derruba o gate M == N.
    seen = defaultdict(int)
    for u in units:
        seen[u['name']] += 1
    for u in units:
        u['what'] = first_sentence(u['doc']) or humanize(u['name'])
        if seen[u['name']] == 1:
            u['display'] = u['name']
        elif u.get('ity'):
            u['display'] = f"{u['ity']}::{u['name']}"
        else:
            u['display'] = f"{u['mod']}::{u['name']}"
    for _ in range(3):
        cnt = defaultdict(int)
        for u in units:
            cnt[u['display']] += 1
        dups = {d for d, n in cnt.items() if n > 1}
        if not dups:
            break
        for u in units:
            if u['display'] in dups and not u['display'].startswith(u['mod'] + '::'):
                u['display'] = f"{u['mod']}::{u['display']}"
    cnt = defaultdict(int)
    for u in units:
        cnt[u['display']] += 1
    for u in units:
        if cnt[u['display']] > 1:
            u['display'] = f"{u['display']}@{u['line']}"    # ultimo desempate: a linha
    # dedupe: mesma unidade no mesmo arquivo:linha (regex sobreposta)
    uniq, key_seen = [], set()
    for u in units:
        k = (u['file'], u['line'], u['name'])
        if k in key_seen:
            continue
        key_seen.add(k)
        uniq.append(u)
    return uniq, files

# ---------------------------------------------------------------- arestas

def direct_all(u, units_in_file):
    """Todas as unidades aninhadas dentro de `u` (netos inclusive)."""
    if not u['span']:
        return []
    a, b = u['span']
    return [v for v in units_in_file
            if v is not u and v['span'] and v['span'][0] >= a and v['span'][1] <= b
            and (v['span'][1] - v['span'][0]) < (b - a)]


def own_text(u, units_in_file):
    """Corpo da unidade SEM os corpos das unidades aninhadas nela.

    Por que: o corpo de uma `fn` engloba o de cada handler/closure declarado dentro.
    Sem descontar, as chamadas do handler seriam atribuidas tambem a fn que o cerca,
    inflando as arestas. O pai ganha, em vez disso, uma aresta explicita PRA o aninhado.
    """
    if not u['span']:
        return '', []
    a, b = u['span']
    txt = list(u['clean'][a:b])
    nested = []
    for v in units_in_file:
        if v is u or not v['span']:
            continue
        va, vb = v['span']
        if va >= a and vb <= b and (vb - va) < (b - a):
            nested.append(v)
    # so os DIRETAMENTE aninhados (nao os netos) viram aresta do pai
    direct = []
    for v in nested:
        if not any(w is not v and w['span'][0] <= v['span'][0] and v['span'][1] <= w['span'][1]
                   for w in nested):
            direct.append(v)
    for v in nested:
        va, vb = v['span']
        for k in range(va - a, min(vb - a, len(txt))):
            if txt[k] != '\n':
                txt[k] = ' '
    return ''.join(txt), direct


# Nome do binario -> repo que o produz. E o que transforma um `Command::new(...)` numa
# ARESTA ENTRE SERVICOS no grafo global.
#
# ORDEM IMPORTA: a deteccao usa `binname in body_raw`, entao os nomes mais LONGOS vem
# primeiro. Com `schematize-updater` antes de `schematize-updater-gui`, todo spawn da janela
# seria atribuido ao updater — a aresta apontaria para o servico errado, e nada reprovaria.
#
# ESTAVA INCOMPLETA (corrigido em 2026-09-08, ADR-0013): faltavam market, deployer e
# optimizer. O efeito era silencioso do pior jeito — o hub passou a disparar o
# `schematize-market`, e a aresta simplesmente NAO APARECIA no grafo. Um grafo que omite a
# dependencia mais nova e um grafo que se consulta e engana; o proposito dele e justamente
# responder "quem chama quem" ANTES de alguem mexer.
#
# ESTAVA INCOMPLETA DE NOVO (2026-09-10): faltavam as JANELAS dos tres apps. O hub passou a
# abrir `schematize-market-gui` e `schematize-deployer-gui`, e as arestas nao apareciam — o
# mesmo modo de falha silencioso descrito acima, um ciclo depois. A licao que fica e sobre a
# ORDEM: `schematize-deployer-gui` CONTEM `schematize-deployer`, entao a janela precisa vir
# antes, ou todo spawn dela seria atribuido ao CLI e nada reprovaria.
BOUNDARY_BIN = {
    'schematize-updater-gui': 'schematize_updater_gui_rs',
    'schematize-updater': 'schematize_updater_rs',
    # As janelas ANTES dos CLIs de mesmo prefixo — ver a nota sobre ordem, acima.
    'schematize-deployer-gui': 'schematize_deployer_gui_rs',
    'schematize-deployer': 'schematize_deployer_rs',
    'schematize-optimizer-gui': 'schematize_optimizer_gui_rs',
    'schematize-optimizer': 'schematize_optimizer_rs',
    # A janela do market vive no repo `schematize_updater_gui_rs` (o repo manteve o nome
    # historico; o binario, nao). Apontar para o repo, e nao para o nome do binario, e o que
    # mantem o grafo falando de SERVICOS.
    'schematize-market-gui': 'schematize_updater_gui_rs',
    'schematize-market': 'schematize_market_rs',
    'schematize-database': 'schematize_database_rs',
    'schematize-git': 'schematize_git_rs',
    'schematize-skills': 'schematize_skills_rs',
    'schematize-skills-gui': 'schematize_skills_rs',
    'schematize-gui': 'schematize_gui_slint',
}


USE_SCHEMATIZE_RE = re.compile(r'use\s+schematize::(?:\{([^}]*)\}|([A-Za-z_][A-Za-z0-9_:]*))')


def imported_symbols(clean: str):
    """Símbolos trazidos por `use schematize::...` num arquivo.

    Por que: a GUI e a git-dep da lib do CLI e quase sempre importa (`use
    schematize::overdev::{caixa, trava}`) em vez de escrever o caminho completo na
    chamada. Sem resolver o import, a aresta de fronteira GUI -> CLI — a mais
    importante do grafo global — passaria batida.
    """
    syms = set()
    for m in USE_SCHEMATIZE_RE.finditer(clean):
        blob = m.group(1) or m.group(2) or ''
        for part in blob.split(','):
            part = part.strip()
            if not part or part == 'self':
                continue
            part = part.split(' as ')[-1].strip()
            leaf = part.split('::')[-1].strip()
            if leaf and leaf != 'self' and re.match(r'^[A-Za-z_][A-Za-z0-9_]*$', leaf):
                syms.add(leaf)
            head = part.split('::')[0].strip()
            if head and re.match(r'^[A-Za-z_][A-Za-z0-9_]*$', head):
                syms.add(head)
    return syms


CONST_BIN_RE = re.compile(
    r'const\s+([A-Z_][A-Z0-9_]*)\s*:[^=]*=\s*[\[&"]([^;]*);', re.S)


def nomeia_binario(texto: str, binname: str) -> bool:
    """O texto contem o nome do binario COMO NOME, e nao dentro de uma frase?

    **Onde:** [`resolver_units`] e [`boundary_for`].

    **Por que a distincao decide a aresta.** O `doctor` tem a mensagem
    `"gestor ANTIGO ainda instalado (schematize-updater)"` — uma FRASE que cita o binario para
    avisar que ele sobrou na maquina. Com uma busca de substring, essa frase marcava a funcao
    como "resolve o caminho do updater", e daí todo `run(` do mesmo arquivo virava fronteira
    para um servico que o CLI nunca executa.
    
    A regra: o nome tem de terminar o literal, ou ser seguido de `.` (extensao), `/` (caminho)
    ou `{` (o sufixo `.exe` interpolado — `format!("schematize-gui{s}")`, que e como a casa
    escreve o nome por plataforma). Antes dele, aspa ou barra.

    Isso aceita `"schematize-updater"`, `"~/.cargo/bin/schematize-updater"`,
    `"schematize-updater.exe"` e `format!("schematize-gui{s}")`; e recusa tanto o nome no meio
    de uma frase quanto `"schematize-gui-linux-x86_64"`, que e nome de ASSET de download e nao
    de binario a executar.
    """
    for m in re.finditer(re.escape(binname), texto):
        antes = texto[m.start() - 1] if m.start() > 0 else '"'
        depois = texto[m.end()] if m.end() < len(texto) else '"'
        if antes in '"\'/' and depois in '"\'/.{':
            return True
    return False


def const_resolvers(raw_por_arquivo, repo):
    """Constantes que GUARDAM o nome de um binario de outro sub-repo -> (destino, nome).

    Por que existe: o nome do binario raramente esta no corpo de quem dispara. As janelas
    escrevem `const DEPLOYER_GUI_BINS: [&str; 1] = ["schematize-deployer-gui"]`, e a funcao
    que resolve o caminho menciona a CONSTANTE, nao a string. Sem este passo, a cadeia
    quebra logo no primeiro elo.
    """
    out = {}
    for src in raw_por_arquivo:
        for m in CONST_BIN_RE.finditer(src):
            nome, corpo = m.group(1), m.group(2)
            for binname, dest in BOUNDARY_BIN.items():
                if dest != repo and nomeia_binario(corpo, binname):
                    out[nome] = (dest, binname)
                    break
    return out


def nomes_ambiguos(units):
    """Nomes de unidade que se repetem no repo — os que NAO podem ter alcance global.

    **Onde:** [`escolher_no_escopo`]. Um `run` existe em varios modulos deste workspace, e foi
    um deles que pos 46 nos apontando para um servico aposentado.
    """
    vistos, repetidos = set(), set()
    for u in units:
        if u['name'] in vistos:
            repetidos.add(u['name'])
        vistos.add(u['name'])
    return repetidos


def resolver_units(units, raw, repo, consts=None):
    """Unidades que RESOLVEM o caminho de um binario de outro sub-repo.

    Por que existe: quem dispara escreve `Command::new(updater_bin())` — o nome do
    binario mora no resolver, nao no corpo do chamador. Sem seguir essa indirecao, a
    fronteira real (o spawn) passaria despercebida.

    ## Por que a resolucao e TRANSITIVA, e nao de um nivel so
    
    Um nivel bastava quando a cadeia era `bin() -> "nome"`. As janelas novas tem tres elos:
    `const NOMES = [...]` -> `gui_de_app(&NOMES)` -> `deployer_gui_bin()` -> quem dispara.
    Com um nivel, as tres janelas apareciam no grafo SEM aresta nenhuma para a CLI que elas
    disparam — e a aresta de uma janela para o binario dela e a coisa mais importante que o
    grafo tem a dizer sobre ela.
    
    O laco roda ate estabilizar. Ele para porque cada volta so ACRESCENTA, e o conjunto de
    unidades e finito — nao ha como oscilar.
    """
    # nome -> [(arquivo, modulo, destino, binario)]. Uma LISTA, e com o escopo junto, porque
    # nome de funcao colide: ha varios `run` neste workspace, e um deles — em `selfupdate.rs` —
    # menciona o gestor aposentado. Com a chave sendo so o nome, TODA chamada a `run(` no repo
    # virava fronteira para um servico que o CLI nunca executa: 46 nos de uma vez.
    ambiguos = nomes_ambiguos(units)
    out = {}
    for nome, (dest, binname) in (consts or {}).items():
        # Constante: vale no repo inteiro (o escopo dela e o modulo, mas o nome e unico o
        # bastante — sao SCREAMING_CASE e nomeiam o binario).
        out[nome] = [(None, None, dest, binname)]
    for u in units:
        if not u.get('own_raw'):
            continue
        body_raw = strip_comments(u['own_raw'])
        for binname, dest in BOUNDARY_BIN.items():
            if dest == repo:
                continue
            if nomeia_binario(body_raw, binname):
                out.setdefault(u['name'], []).append((u['file'], u['mod'], dest, binname))
                break

    # Fecho transitivo: quem CHAMA um resolver tambem resolve.
    #
    # ## Por que ha um TETO de profundidade, e por que ele e 3
    #
    # Sem teto, o fecho vira contagio. A primeira versao rodava ate estabilizar e produziu 46
    # nos de fronteira do CLI para o `schematize_updater_rs` — um servico APOSENTADO, que o CLI
    # so PROCURA no disco (`updater_aposentado_presente`) e nunca executa. A funcao que le o
    # nome virou resolver, quem a chama virou resolver, e daí em diante metade do repo.
    #
    # A CAUSA do contagio era outra, e foi consertada na raiz: a deteccao lia o corpo CRU, e um
    # binario citado em COMENTARIO virava elo. Agora ela le o corpo sem comentario, e a cadeia
    # so anda por quem de fato nomeia o binario ou CHAMA quem o nomeia.
    #
    # O teto fica assim mesmo, como cinto: cadeia real mais longa da casa tem quatro elos
    # (`const NOMES` -> `gui_de_app(&NOMES)` -> `deployer_gui_bin()` -> o callback -> quem
    # dispara), e um numero fixo torna impossivel um laco patologico num repo futuro.
    MAX_ELOS = 6
    fronteira = {k: list(v) for k, v in out.items()}
    for _ in range(MAX_ELOS):
        novos = {}
        for u in units:
            if u['name'] in out or u['name'] in novos or not u.get('own_clean'):
                continue
            corpo = u['own_clean']
            achou = None
            for alvo, entradas in fronteira.items():
                # FUNCAO: `alvo(` e nao `alvo` — CHAMAR o resolver e a cadeia; citar nao e.
                # CONSTANTE: o nome nu, porque constante nao se chama. A primeira versao do
                # fecho exigia `(` para tudo, e com isso a cadeia das janelas quebrava no
                # PRIMEIRO elo — `gui_de_app(&DEPLOYER_GUI_BINS)` nunca era reconhecido — e o
                # hub ficava sem aresta nenhuma para as janelas que ele abre.
                e_const = any(arq is None for arq, _, _, _ in entradas)
                padrao = r'\b' + re.escape(alvo) + (r'\b' if e_const else r'\s*\(')
                if not re.search(padrao, corpo):
                    continue
                achou = escolher_no_escopo(alvo, entradas, u, ambiguos)
                if achou:
                    break
            if achou:
                novos[u['name']] = [(u['file'], u['mod'], achou[0], achou[1])]
        if not novos:
            break
        out.update(novos)
        fronteira = novos
    return out


def escolher_no_escopo(nome, entradas, u, ambiguos=frozenset()):
    """A entrada de resolver que vale PARA ESTA unidade -> (destino, binario) ou None.

    **Onde:** o fecho de [`resolver_units`] e a deteccao de fronteira.

    **Por que o escopo importa.** Nome de funcao colide: ha varios `run` neste workspace, e um
    deles menciona o gestor aposentado. Sem escopo, toda chamada a `run(` virava fronteira para
    um servico que o CLI nunca executa — 46 nos de uma vez, e uma dependencia inventada no
    grafo. Aresta falsa e pior que aresta faltando: quem consulta passa a evitar o que nao ha.

    A ordem e a mesma que a resolucao de chamadas ja usa: mesmo ARQUIVO, depois mesmo MODULO, e
    so entao alcance global — e o global exige que o nome NAO seja ambiguo no repo. Um `run`
    precisa de escopo; um `market_gui_bin` nao precisa, e e por isso que a cadeia longa das
    janelas continua fechando.
    """
    if not entradas:
        return None
    for arq, mod, dest, binname in entradas:
        if arq is None or arq == u['file']:
            return (dest, binname)
    for arq, mod, dest, binname in entradas:
        if mod == u['mod']:
            return (dest, binname)
    if len(entradas) == 1 and nome not in ambiguos:
        _, _, dest, binname = entradas[0]
        return (dest, binname)
    return None


def assinatura_de(u):
    """Os NOMES dos parametros de uma unidade. Vazio quando nao da para ler.

    **Onde:** a deteccao de lancador generico. Sem isto, "recebe o binario por parametro" seria
    adivinhacao — e a versao que adivinhava produziu dez arestas falsas.
    """
    clean = u.get('clean') or ''
    i = clean.find('(', clean.find(u['name']))
    if i < 0:
        return set()
    prof, j = 0, i
    while j < len(clean):
        if clean[j] == '(':
            prof += 1
        elif clean[j] == ')':
            prof -= 1
            if prof == 0:
                break
        j += 1
    params = clean[i + 1:j]
    return {m.group(1) for m in re.finditer(r'(?:^|,)\s*(?:mut\s+)?([a-z_][a-z0-9_]*)\s*:', params)}


def boundary_for(u, repo, resolvers, imported=frozenset(), ambiguos=frozenset()):
    """Detecta saida que CRUZA a fronteira do repo -> (destino, contrato) ou None.

    Duas formas: (a) SPAWN do binario de outro sub-repo — direto pelo nome ou via
    resolver (um nivel de indirecao); (b) uso do crate `schematize` (git-dep da lib
    do CLI). Citar o nome num rotulo NAO e fronteira: so produz saida quem dispara.
    """
    body_clean = u.get('own_clean') or ''
    # O corpo sem COMENTARIO, mas com os literais: e o unico que pode responder "este corpo
    # nomeia o binario de outro servico?" sem contar mencao em prosa como dependencia.
    body_sem_comentario = strip_comments(u.get('own_raw') or '')
    if not body_clean:
        return None
    if re.search(r'Command::new|process::Command', body_clean):
        # (a) O nome do binario NO ARGUMENTO do spawn.
        #
        # A versao anterior aceitava o nome em QUALQUER lugar do corpo, desde que houvesse um
        # `Command::new` em algum ponto. Isso pos cinco nos do `doctor` apontando para o gestor
        # APOSENTADO: eles listam o nome dele para PROCURA-LO no disco (e avisar que sobrou), e
        # spawnam outra coisa. Nomear nao e disparar — a diferenca e a aresta inteira.
        for m in re.finditer(r'(?:process::)?Command::new\s*\(', body_sem_comentario):
            arg = body_sem_comentario[m.end(): m.end() + 200]
            for binname, dest in BOUNDARY_BIN.items():
                if dest != repo and nomeia_binario(arg.split(')')[0], binname):
                    return (dest, f'spawn `{binname}` (processo externo)')
        # (b.1) `Command::new(resolver())` — a indirecao direta.
        for m in re.finditer(r'Command::new\(\s*&?\s*([A-Za-z_][A-Za-z0-9_]*)\s*\(', body_clean):
            hit = escolher_no_escopo(m.group(1), resolvers.get(m.group(1), []), u, ambiguos)
            if hit:
                return (hit[0], f'spawn `{hit[1]}` via `{m.group(1)}()` (processo externo)')
        # (b.2) `let b = resolver(); ... Command::new(&b)` — a MESMA indirecao com uma
        # variavel no meio, que e como as janelas novas escrevem (o caminho e reusado na
        # mensagem de erro, entao vale a pena guarda-lo).
        #
        # Sem este caso, as tres janelas apareciam no grafo SEM aresta nenhuma para a CLI que
        # elas disparam — e um grafo que omite a dependencia central de um servico e um grafo
        # que se consulta e engana.
        for m in re.finditer(
            r'let\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*([A-Za-z_][A-Za-z0-9_]*)\s*\(\s*\)',
            body_clean,
        ):
            var, fn = m.group(1), m.group(2)
            hit = escolher_no_escopo(fn, resolvers.get(fn, []), u, ambiguos)
            if hit and re.search(r'Command::new\(\s*&?\s*' + re.escape(var) + r'\b', body_clean):
                return (hit[0], f'spawn `{hit[1]}` via `{fn}()` (processo externo)')
    if repo != 'schematize_cli_rs':
        api = sorted(set(re.findall(r'\bschematize::([a-z_]+)', body_clean)))
        hits = sorted({s for s in imported if re.search(r'\b' + re.escape(s) + r'\b', body_clean)})
        via = api or hits
        if via:
            return ('schematize_cli_rs', 'chama a lib `schematize::' + '/'.join(via[:3]) + '`')
    return None


WHY_EXTERNAL = [
    (re.compile(r'^main$'), 'entrypoint do binario (chamado pelo SO)'),
    (re.compile(r'^on_'), 'handler de UI (chamado pelo framework)'),
    (re.compile(r'^(test_|.*_test)$'), 'teste (chamado pelo runner)'),
]


def why_external(u):
    for rx, why in WHY_EXTERNAL:
        if rx.search(u['name']):
            return why
    if u['kind'] == 'handler':
        return 'handler de UI (chamado pelo framework)'
    if u['file'].startswith('build.rs') or u['file'] == 'build.rs':
        return 'entrypoint do binario (chamado pelo SO)'
    if u['vis'] == 'publica':
        return 'API publica do crate (chamada de fora)'
    if not u['has_body']:
        return 'assinatura de trait (implementada em outro lugar)'
    return 'sem chamador interno — suspeita de codigo morto'


def build_graph(repo):
    units, files = scan_repo(repo)
    raw = {}
    base = ROOT / repo
    for f in {u['file'] for u in units}:
        raw[f] = (base / f).read_text(encoding='utf-8', errors='replace')

    by_file = defaultdict(list)
    for u in units:
        by_file[u['file']].append(u)
    by_name = defaultdict(list)
    for u in units:
        by_name[u['name']].append(u)

    # Corpo PROPRIO (sem os aninhados) — base tanto das arestas quanto da fronteira.
    for u in units:
        txt, direct = own_text(u, by_file[u['file']])
        u['own_clean'] = txt
        u['own_direct'] = direct
        if u['span']:
            a, b = u['span']
            rawtxt = list(raw[u['file']][a:b])
            for v in direct_all(u, by_file[u['file']]):
                va, vb = v['span']
                for k in range(va - a, min(vb - a, len(rawtxt))):
                    if rawtxt[k] != '\n':
                        rawtxt[k] = ' '
            u['own_raw'] = ''.join(rawtxt)
        else:
            u['own_raw'] = ''

    edges = set()
    ambiguous = 0
    for u in units:
        txt, direct = u['own_clean'], u['own_direct']
        for v in direct:
            if v is not u:
                edges.add((u['display'], v['display']))
        if not txt:
            continue
        for m in re.finditer(r'\b([A-Za-z_][A-Za-z0-9_]*)\s*\(', txt):
            nm = m.group(1)
            cands = by_name.get(nm)
            if not cands or nm == u['name']:
                continue
            same_file = [c for c in cands if c['file'] == u['file']]
            same_mod = [c for c in cands if c['mod'] == u['mod']]
            pick = None
            if len(cands) == 1:
                pick = cands[0]
            elif len(same_file) == 1:
                pick = same_file[0]
            elif len(same_mod) == 1:
                pick = same_mod[0]
            else:
                ambiguous += 1
            if pick and pick is not u:
                edges.add((u['display'], pick['display']))

    ambiguos = nomes_ambiguos(units)
    resolvers = resolver_units(units, raw, repo, const_resolvers(raw.values(), repo))
    imports = {f: imported_symbols(strip_noise(raw[f])) for f in raw}
    for u in units:
        u['boundary'] = boundary_for(
            u, repo, resolvers, imports.get(u['file'], frozenset()), ambiguos)

    # LANCADORES GENERICOS, e a aresta que so existe em quem os CHAMA.
    #
    # `abrir_gui(bin, aba)` dispara um `Command::new(bin)` — e nao sabe de qual app: o binario
    # chega por PARAMETRO. A fronteira nao esta nele, esta em quem escolhe o parametro. Sem
    # este passo, o hub aparecia no grafo sem nenhuma aresta para as janelas que ele abre, e a
    # unica funcao com `Command::new` era justamente a que nao tem destino.
    #
    # A regra e estreita de proposito: so conta quem chama um lancador generico E um resolver
    # de binario de outro servico. Citar o nome num rotulo continua nao sendo fronteira.
    # A regra e ESTREITA de proposito, e a primeira versao dela nao era — ela contava como
    # generico qualquer unidade com `Command::new`, e o grafo ganhou dez arestas falsas, entre
    # elas um `deployer -> deployer-gui` que nunca existiu (a CLI escreve o caminho da janela
    # num `.desktop`; nao a executa). Aresta falsa e pior que aresta faltando: quem consulta o
    # grafo passa a evitar uma dependencia que nao ha.
    #
    # Generico e so quem recebe o binario por PARAMETRO: `Command::new(x)` com `x` na
    # assinatura e sem `let x` no corpo. Quem monta o proprio caminho ja e pego pelas regras
    # anteriores, e tem destino conhecido.
    genericos = set()
    for u in units:
        corpo = u.get('own_clean') or ''
        if u.get('boundary') or not re.search(r'Command::new', corpo):
            continue
        assinatura = assinatura_de(u)
        for m in re.finditer(r'Command::new\(\s*&?\s*([A-Za-z_][A-Za-z0-9_]*)\s*\)', corpo):
            arg = m.group(1)
            # A exclusao e `let x =` (uma ligacao NOVA), e nao `let` seguido do nome em
            # qualquer forma. O idioma `let Some(bin) = bin else { return }` — desembrulhar um
            # `Option` sombreando o parametro — casava a versao anterior e escondia o unico
            # lancador generico da casa, deixando o hub sem aresta nenhuma para as janelas que
            # ele abre.
            if arg in assinatura and not re.search(
                r'\blet\s+(?:mut\s+)?' + re.escape(arg) + r'\s*=', corpo
            ):
                genericos.add(u['name'])
                break
    if genericos:
        for u in units:
            if u.get('boundary') or not u.get('own_clean'):
                continue
            corpo = u['own_clean']
            if not any(re.search(r'\b' + re.escape(g) + r'\s*\(', corpo) for g in genericos):
                continue
            for alvo, entradas in resolvers.items():
                if not re.search(r'\b' + re.escape(alvo) + r'\s*\(', corpo):
                    continue
                hit = escolher_no_escopo(alvo, entradas, u, ambiguos)
                if hit and hit[0] != repo:
                    u['boundary'] = (hit[0], f'spawn `{hit[1]}` via `{alvo}()` (processo externo)')
                    break

    called = {b for _, b in edges}
    externals = [u for u in units if u['display'] not in called]
    return dict(repo=repo, units=units, files=files, edges=sorted(edges),
                externals=externals, ambiguous=ambiguous)

# ---------------------------------------------------------------- entrypoints

def kebab(v: str) -> str:
    return re.sub(r'(?<!^)(?=[A-Z])', '-', v).lower()


def clap_entrypoints(repo: str):
    """Superficie de CLI, lida do SNAPSHOT gerado pelo proprio clap.

    De onde vem: `tests/superficie-cli.txt`, escrito por `superficie_da_cli_nao_mudou`
    percorrendo a arvore de `clap::Command`. Pra onde vai: os entrypoints do grafo global.

    POR QUE NAO PARSEIA MAIS O FONTE
    --------------------------------
    A versao anterior lia `src/cli/args.rs` com regex e capturava **10 de 126 comandos**,
    com a descricao ERRADA em varios (`schematize archive` aparecia como "Envia o relatorio
    de diagnostico"). Duas falhas somadas:

    1. O `depth` era atualizado com a propria linha da variante, entao `Skills {` ja subia
       pra 2 antes do teste `depth == 1`. So variantes SEM corpo (`Archive,`) casavam — e
       nenhum enum aninhado (`SkillsCmd`, `VpsCmd`, ...) era visitado.
    2. O acumulador `doc` nao era limpo nas variantes puladas, entao a variante seguinte
       herdava a descricao da anterior.

    Ninguem percebeu porque o indice nao tinha como se conferir: 10 linhas plausiveis num
    arquivo de milhares. O corte do `args.rs` em submodulos levou de 10 pra 0 e SO ENTAO o
    buraco apareceu — a falha total foi mais honesta que a parcial.

    Ler o snapshot troca uma heuristica sobre texto Rust por um artefato que o clap gerou de
    si mesmo: nao ha o que interpretar errado, e o `cargo test` garante que ele esta atual.
    """
    snap = ROOT / repo / 'tests' / 'superficie-cli.txt'
    if not snap.is_file():
        return []
    out, atual = [], None
    for i, ln in enumerate(snap.read_text(encoding='utf-8', errors='replace').split('\n')):
        if ln.startswith('CMD '):
            corpo = ln[4:]
            oculto = corpo.endswith(' (oculto)')
            if oculto:
                corpo = corpo[: -len(' (oculto)')]
            nome = corpo.split(' aliases=[')[0].strip()
            # Aliases ocultos sao compat, nao superficie — mesma regra de antes.
            atual = None if oculto else dict(name=nome, what=nome,
                                             loc=f'{repo}/tests/superficie-cli.txt:{i + 1}')
            if atual:
                out.append(atual)
        elif ln.strip().startswith('SOBRE ') and atual is not None:
            atual['what'] = first_sentence([ln.strip()[6:]]) or atual['what']
            atual = None
    return out

def entrypoints(g):
    """Superficie publica do servico — o que aparece como no no grafo global."""
    repo = g['repo']
    eps = clap_entrypoints(repo)
    for u in g['units']:
        if u['name'] == 'main' or u['kind'] == 'handler':
            eps.append(dict(name=u['display'], what=u['what'],
                            loc=f"{repo}/{u['file']}:{u['line']}"))
    seen, out = set(), []
    for e in eps:
        if e['name'] in seen:
            continue
        seen.add(e['name']); out.append(e)
    return out

# ---------------------------------------------------------------- emissao

HEADER = """# Grafo DETALHADO — {repo}

> Grafo detalhado interno do sub-repo `{repo}` (secao 39). As **funcoes sao os nos**, as
> **chamadas intra-servico sao as arestas**, e **cada no traz `arquivo:linha`** (caminho
> relativo a raiz do umbrella, para o app resolver o microservico pelo 1o segmento).
>
> **O que e:** {what}
> **Stack:** {stack} · **Onde roda:** {runs}
>
> **Completude:** enumeracao EXAUSTIVA — uma entrada por unidade chamavel (funcao, metodo,
> handler, closure nomeada, job), publica e privada, EXCLUINDO andaime de teste
> (`#[cfg(test)]`, `tests/`, `benches/`, `examples/`). Total: **{n} unidades**.
>
> **Fronteira:** funcao que produz saida para OUTRO sub-repo aparece na secao "Fronteira"
> marcada com o repo de destino — e a ponta local da aresta que reaparece no
> `GRAFO_GLOBAL.md`. Arestas SEMPRE em ASCII (hifen + maior-que), nunca a seta unicode.
>
> Grafo global: [`GRAFO_GLOBAL.md`](GRAFO_GLOBAL.md). Gerado por `scripts/build-index.py` em {date}.
"""


ARROWS = ['-->', '-.->', '==>', '->', '\u2192', '\u27f6', '\u21d2', '\u279c', '\u2794']


def esc(s: str) -> str:
    """Prepara texto pra uma celula de tabela do grafo.

    Duas coisas, ambas por causa do parser (`panel/parse.rs`):
    (1) o pipe e escapado — o parser corta a linha em `|`;
    (2) TODA seta vira a palavra "para". `parse_edge` normaliza `-->`/`==>`/`\u2192`/`\u21d2`
        para `->` antes de tentar ler adjacencia, entao uma seta na DESCRICAO pode ser
        lida como aresta e virar no lixo — foi exatamente o defeito corrigido na
        v0.50.1. Descricao nao carrega seta; adjacencia mora nos blocos ```.
    """
    t = (s or '').replace('\n', ' ')
    for a in ARROWS:
        t = t.replace(a, 'para')
    return t.replace('|', '\\|').strip()


def emit_service(g, date):
    repo, units = g['repo'], g['units']
    meta = REPOS[repo]
    out = [HEADER.format(repo=repo, n=len(units), date=date, **meta)]

    out.append('\n## Superficie publica (entrypoints) — o que aparece no grafo global\n')
    out.append('| Entrypoint | O que | arquivo:linha |')
    out.append('|---|---|---|')
    for e in entrypoints(g):
        out.append(f"| `{esc(e['name'])}` | {esc(e['what'])} | {e['loc']} |")

    out.append('\n## Funcoes (nos) — enumeracao exaustiva por arquivo\n')
    bydir = defaultdict(list)
    for u in units:
        d = str(Path(u['file']).parent)
        bydir['(raiz do repo)' if d == '.' else d].append(u)
    for d in sorted(bydir, key=lambda x: (x != '(raiz do repo)', x)):
        us = sorted(bydir[d], key=lambda z: (z['file'], z['line']))
        out.append(f'\n### `{d}` — {len(us)} unidades\n')
        out.append('| Funcao | O que | Visibilidade | Fronteira | arquivo:linha |')
        out.append('|---|---|---|---|---|')
        for u in us:
            fr = f"saida para {u['boundary'][0]}" if u['boundary'] else '-'
            out.append(f"| `{esc(u['display'])}` | {esc(u['what'])} | {u['vis']} | {fr} "
                       f"| {repo}/{u['file']}:{u['line']} |")

    fr = [u for u in units if u['boundary']]
    out.append('\n## Fronteira — nos com saida para OUTRO sub-repo (auto-referencia ao global)\n')
    if fr:
        out.append('| Funcao | Destino | Contrato | arquivo:linha |')
        out.append('|---|---|---|---|')
        for u in sorted(fr, key=lambda z: (z['file'], z['line'])):
            out.append(f"| `{esc(u['display'])}` | `{u['boundary'][0]}` | {esc(u['boundary'][1])} "
                       f"| {repo}/{u['file']}:{u['line']} |")
        out.append('\n```')
        for u in sorted(fr, key=lambda z: z['display']):
            out.append(f"{u['display']} -> {u['boundary'][0]} ({esc(u['boundary'][1])})")
        out.append('```')
    else:
        out.append('> Nenhuma: este sub-repo nao produz saida direta para outro sub-repo '
                   '(e o mais a jusante da cadeia).')

    ext = sorted(g['externals'], key=lambda z: (z['file'], z['line']))
    out.append('\n## Entradas externas — unidades sem chamador DENTRO do repo\n')
    out.append(f'> {len(ext)} de {len(units)} unidades nao sao chamadas por nenhuma outra unidade DESTE repo.')
    out.append('> Isso nao e defeito por si: entrypoint de binario, handler de framework, API publica')
    out.append('> do crate e teste sao chamados de FORA. A coluna "Por que" separa esses da suspeita')
    out.append('> de codigo morto.\n')
    out.append('| Funcao | Por que nao tem chamador interno | arquivo:linha |')
    out.append('|---|---|---|')
    for u in ext:
        out.append(f"| `{esc(u['display'])}` | {why_external(u)} | {repo}/{u['file']}:{u['line']} |")
    out.append('\n```')
    for u in ext:
        out.append(f"{repo} -> {u['display']} (entrada externa)")
    out.append('```')

    out.append('\n## Arestas — chamadas intra-servico (ASCII)\n')
    out.append(f"> {len(g['edges'])} chamadas resolvidas estaticamente dentro do repo.")
    if g['ambiguous']:
        out.append(f"> {g['ambiguous']} sitios de chamada ficaram de fora por AMBIGUIDADE de nome")
        out.append('> (varias unidades homonimas, ex. `new`/`run`/`parse` em impls distintos):')
        out.append('> preferimos perder a aresta a inventar uma falsa.')
    out.append('\n```')
    for a, b in g['edges']:
        out.append(f'{a} -> {b}')
    out.append('```')

    out.append('\n## Ancoras — o repo e seus entrypoints (ASCII)\n')
    out.append('```')
    for e in entrypoints(g)[:40]:
        out.append(f"{repo} -> {e['name']} (entrypoint)")
    out.append('```')
    return '\n'.join(out) + '\n'

GLOBAL_HEADER = """# GRAFO GLOBAL — schematize (app)

> O grafo GLOBAL da aplicacao (secao 39): **cada sub-repo e um NO**, mostrando suas
> **funcoes principais** (a superficie de contrato — nao todas as funcoes; essas estao no
> grafo detalhado de cada servico). As **arestas sao os CONTRATOS** — a saida de dados de
> um servico para outro.
>
> Arestas SEMPRE em ASCII (hifen + maior-que), NUNCA a seta unicode: o parser do app
> (`schematize_cli_rs/src/panel/parse.rs`) le ASCII, e o unicode quebra a leitura.
>
> **Nao ha aresta obsoleta.** Houve uma — `schematize_updater_gui_rs -> schematize_updater_rs`,
> apontando para o servico aposentado pelo ADR-0013 — e ela ficou marcada aqui enquanto o
> destino da janela era decisao humana em aberto. O ADR-0014 (D4) respondeu: a janela virou a
> do market, e a aresta virou `-> schematize_market_rs`. Este paragrafo fica como o registro de
> que o grafo mostra o sistema como ele E, e nao como se gostaria que fosse.
>
> Detalhe por servico: {links}.
> Gerado por `scripts/build-index.py` em {date}.
"""


def contrato_agregado(us):
    """Rotulo do contrato de uma aresta global, somando TODAS as pontas locais.

    Por que: pegar o rotulo do primeiro no de fronteira subestima a aresta — a GUI nao
    chama so `schematize::disco`, chama uma dezena de modulos da lib. O rotulo tem que
    dizer a superficie real do contrato, que e o que se quebra ao mexer.
    """
    mods, spawns = set(), set()
    for u in us:
        c = u['boundary'][1]
        if c.startswith('chama a lib'):
            mods.update(re.findall(r'schematize::([a-z_]+)', c))
        else:
            spawns.add(c.split(' via ')[0].replace(' (processo externo)', '').strip())
    if mods:
        ms = sorted(mods)
        head = '/'.join(ms[:6]) + (f' (+{len(ms) - 6})' if len(ms) > 6 else '')
        return f'chama a lib `schematize::{{{head}}}`'
    return ' · '.join(sorted(spawns)) + ' (processo externo)' if spawns else ''


def emit_global(graphs, date):
    links = ', '.join(f'[`{g["repo"]}`]({g["repo"]}.md)' for g in graphs)
    out = [GLOBAL_HEADER.format(links=links, date=date)]

    out.append('\n## Servicos (nos) — todos, nenhum de fora\n')
    out.append('| Servico | O que faz | Stack | Onde roda | Unidades | Arquivos |')
    out.append('|---|---|---|---|---|---|')
    for g in graphs:
        meta = REPOS[g['repo']]
        out.append(f"| `{g['repo']}` | {esc(meta['what'])} | {meta['stack']} | {meta['runs']} "
                   f"| {len(g['units'])} | {len(g['files'])} |")

    out.append('\n## Funcoes principais por servico (superficie de contrato)\n')
    for g in graphs:
        eps = entrypoints(g)
        out.append(f"\n### `{g['repo']}` — {len(eps)} entrypoints\n")
        out.append('| Entrypoint | O que | arquivo:linha |')
        out.append('|---|---|---|')
        for e in eps:
            out.append(f"| `{esc(e['name'])}` | {esc(e['what'])} | {e['loc']} |")

    out.append('\n## Contratos (arestas) — quem produz saida para quem\n')
    pairs = defaultdict(list)
    for g in graphs:
        for u in g['units']:
            if u['boundary']:
                pairs[(g['repo'], u['boundary'][0])].append(u)
    out.append('| De | Para | Contrato | Origem (nos de fronteira) |')
    out.append('|---|---|---|---|')
    for (a, b), us in sorted(pairs.items()):
        contrato = esc(contrato_agregado(us))
        origem = ', '.join(f"`{u['display']}`" for u in sorted(us, key=lambda z: z['display'])[:6])
        if len(us) > 6:
            origem += f' (+{len(us) - 6})'
        out.append(f'| `{a}` | `{b}` | {contrato} | {origem} |')

    out.append('\n```')
    for (a, b), us in sorted(pairs.items()):
        out.append(f'{a} -> {b} ({esc(contrato_agregado(us))})')
    out.append('```')

    out.append('\n## Fronteira detalhada — a ponta local de cada aresta global\n')
    out.append('```')
    for g in graphs:
        for u in sorted((x for x in g['units'] if x['boundary']), key=lambda z: z['display']):
            out.append(f"{g['repo']}::{u['display']} -> {u['boundary'][0]}")
    out.append('```')

    out.append('\n## Mermaid\n')
    out.append('```mermaid')
    out.append('graph LR')
    for g in graphs:
        out.append(f"  {g['repo']}[{g['repo']}]")
    for (a, b), us in sorted(pairs.items()):
        out.append(f'  {a} --> {b}')
    out.append('```')
    return '\n'.join(out) + '\n'


def emit_mapa(graphs, date):
    """MAPA.md — o resumo navegavel (secao 4), no archive."""
    tot = sum(len(g['units']) for g in graphs)
    out = [f"""# MAPA — schematize (app)

> O resumo navegavel do sistema (secao 4). A enumeracao completa esta em
> `INDEX_FUNCTIONS.md` (uma entrada por funcao) e o grafo de servicos em
> `INDEX_GLOBAL.md`. A versao OPERACIONAL que o app desenha vive em
> `.schematize/grafos/`; este diretorio e o espelho durável.
>
> **{len(graphs)} servicos · {tot} unidades chamaveis.** Gerado por `scripts/build-index.py` em {date}.

## Camada global — os servicos e como se comunicam
"""]
    out.append('| Servico | O que faz | Unidades | Grafo detalhado |')
    out.append('|---|---|---|---|')
    for g in graphs:
        out.append(f"| `{g['repo']}` | {esc(REPOS[g['repo']]['what'])} | {len(g['units'])} "
                   f"| `.schematize/grafos/{g['repo']}.md` |")
    out.append('\n```')
    seen = set()
    for g in graphs:
        for u in g['units']:
            if u['boundary']:
                e = f"{g['repo']} -> {u['boundary'][0]}"
                if e not in seen:
                    seen.add(e); out.append(e)
    out.append('```')

    out.append('\n## Onde tocar — pastas por servico\n')
    for g in graphs:
        dirs = defaultdict(int)
        for u in g['units']:
            d = str(Path(u['file']).parent)
            dirs['(raiz)' if d == '.' else d] += 1
        out.append(f"\n### `{g['repo']}`\n")
        out.append('| Pasta | Unidades |')
        out.append('|---|---|')
        for d, n in sorted(dirs.items(), key=lambda kv: -kv[1]):
            out.append(f'| `{d}` | {n} |')
    return '\n'.join(out) + '\n'


def desambigua_entre_repos(graphs):
    """Qualifica com o repo todo nome que colide ENTRE servicos.

    Por que e defeito, e nao cosmetica: o parser do app indexa no por `id`. Dois
    `main::main` — um do `gui_slint`, outro do `updater_gui` — viram UM no so, e o
    grafo passa a mostrar uma aresta entre servicos que nao existe no codigo. Dentro
    de um mesmo repo o display ja e unico, entao a substituicao nas arestas daquele
    repo e segura.
    """
    donos = defaultdict(set)
    for g in graphs:
        for u in g['units']:
            donos[u['display']].add(g['repo'])
    colididos = {n for n, rs in donos.items() if len(rs) > 1}
    if not colididos:
        return 0
    for g in graphs:
        ren = {}
        for u in g['units']:
            if u['display'] in colididos:
                ren[u['display']] = f"{g['repo']}::{u['display']}"
                u['display'] = ren[u['display']]
        if ren:
            g['edges'] = sorted({(ren.get(a, a), ren.get(b, b)) for a, b in g['edges']})
    return len(colididos)


def main():
    date = os.environ.get('INDEX_DATE') or __import__('datetime').date.today().isoformat()
    graphs = [build_graph(r) for r in REPOS]
    n_col = desambigua_entre_repos(graphs)
    if n_col:
        print(f'{n_col} nome(s) colidiam entre servicos e foram qualificados com o repo.')

    live = ROOT / '.schematize' / 'grafos'
    live.mkdir(parents=True, exist_ok=True)
    mirror = ROOT / 'schematize_app_archive' / 'index'
    mirror.mkdir(parents=True, exist_ok=True)

    ok = True
    print(f"{'servico':28} {'N (codigo)':>11} {'M (grafo)':>10}  veredito")
    for g in graphs:
        body = emit_service(g, date)
        (live / f"{g['repo']}.md").write_text(body, encoding='utf-8')
        # GATE: conta as linhas de tabela da secao de enumeracao e compara com N.
        rows = 0
        inside = False
        for ln in body.split('\n'):
            if ln.startswith('## Funcoes (nos)'):
                inside = True; continue
            if inside and ln.startswith('## '):
                inside = False
            if inside and ln.startswith('| `'):
                rows += 1
        good = rows == len(g['units'])
        ok = ok and good
        print(f"{g['repo']:28} {len(g['units']):>11} {rows:>10}  {'OK' if good else 'FALHA'}")

    (live / 'GRAFO_GLOBAL.md').write_text(emit_global(graphs, date), encoding='utf-8')

    # Espelho durável no archive (secao 28): INDEX_GLOBAL + INDEX_FUNCTIONS + MAPA, MAIS
    # um arquivo por servico.
    #
    # POR QUE OS ARQUIVOS POR SERVICO ENTRARAM (2026-09-08, ADR-0013): o espelho gravava so os
    # tres agregados, e os `<servico>.md` do archive eram sobra de uma versao anterior deste
    # script — congelados em QUATRO servicos enquanto `.schematize/grafos/` ja tinha SETE.
    # Market, deployer e optimizer nunca chegaram la. Espelho que espelha parte e pior que
    # espelho nenhum: ele parece completo. O `GRAFO_GLOBAL.md` tambem passa a ser espelhado
    # com o proprio nome, porque e ele que os `<servico>.md` referenciam.
    for g in graphs:
        (mirror / f"{g['repo']}.md").write_text(emit_service(g, date), encoding='utf-8')
    (mirror / 'GRAFO_GLOBAL.md').write_text(emit_global(graphs, date), encoding='utf-8')
    (mirror / 'INDEX_GLOBAL.md').write_text(emit_global(graphs, date), encoding='utf-8')
    (mirror / 'INDEX_FUNCTIONS.md').write_text(
        '\n\n---\n\n'.join(emit_service(g, date) for g in graphs), encoding='utf-8')
    (mirror / 'MAPA.md').write_text(emit_mapa(graphs, date), encoding='utf-8')

    tot = sum(len(g['units']) for g in graphs)
    print(f"\n{len(graphs)} servicos · {tot} unidades · "
          f"{sum(len(g['edges']) for g in graphs)} arestas intra-servico")
    if not ok:
        print('GATE REPROVADO: M != N em algum servico.', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
