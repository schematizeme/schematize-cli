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
    "schematize_updater_rs": dict(
        what="Bootstrapper/gestor de versão cross-OS: instala binário ou builda do fonte, purga cópias-fantasma.",
        stack="Rust", runs="binário local"),
    "schematize_updater_gui_rs": dict(
        what="Janela (Slint) do gestor de atualizações: casca fina sobre o binário do updater.",
        stack="Rust/Slint", runs="app desktop"),
}

# ---------------------------------------------------------------- limpeza lexica

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


BOUNDARY_BIN = {
    'schematize-updater-gui': 'schematize_updater_gui_rs',
    'schematize-updater': 'schematize_updater_rs',
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


def resolver_units(units, raw, repo):
    """Unidades que RESOLVEM o caminho de um binario de outro sub-repo.

    Por que existe: quem dispara escreve `Command::new(updater_bin())` — o nome do
    binario mora no resolver, nao no corpo do chamador. Sem seguir essa indirecao de
    um nivel, a fronteira real (o spawn) passaria despercebida.
    """
    out = {}
    for u in units:
        if not u.get('own_raw'):
            continue
        body_raw = u['own_raw']
        for binname, dest in BOUNDARY_BIN.items():
            if dest == repo:
                continue
            if binname in body_raw:
                out[u['name']] = (dest, binname)
    return out


def boundary_for(u, repo, resolvers, imported=frozenset()):
    """Detecta saida que CRUZA a fronteira do repo -> (destino, contrato) ou None.

    Duas formas: (a) SPAWN do binario de outro sub-repo — direto pelo nome ou via
    resolver (um nivel de indirecao); (b) uso do crate `schematize` (git-dep da lib
    do CLI). Citar o nome num rotulo NAO e fronteira: so produz saida quem dispara.
    """
    body_clean = u.get('own_clean') or ''
    body_raw = u.get('own_raw') or ''
    if not body_clean:
        return None
    if re.search(r'Command::new|process::Command', body_clean):
        for binname, dest in BOUNDARY_BIN.items():
            if dest != repo and binname in body_raw:
                return (dest, f'spawn `{binname}` (processo externo)')
        for m in re.finditer(r'Command::new\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*\(', body_clean):
            hit = resolvers.get(m.group(1))
            if hit:
                return (hit[0], f'spawn `{hit[1]}` via `{m.group(1)}()` (processo externo)')
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

    resolvers = resolver_units(units, raw, repo)
    imports = {f: imported_symbols(strip_noise(raw[f])) for f in raw}
    for u in units:
        u['boundary'] = boundary_for(u, repo, resolvers, imports.get(u['file'], frozenset()))

    called = {b for _, b in edges}
    externals = [u for u in units if u['display'] not in called]
    return dict(repo=repo, units=units, files=files, edges=sorted(edges),
                externals=externals, ambiguous=ambiguous)

# ---------------------------------------------------------------- entrypoints

def kebab(v: str) -> str:
    return re.sub(r'(?<!^)(?=[A-Z])', '-', v).lower()


def clap_entrypoints(repo: str):
    """Superficie de CLI: variantes dos enums `#[derive(Subcommand)]` com seu doc.

    Por que: no CLI o "entrypoint" que interessa ao grafo global nao e a `fn main`, e
    o SUBCOMANDO que o usuario digita. Aliases ocultos (`hide = true`) ficam de fora —
    sao compat, nao superficie.
    """
    args = ROOT / repo / 'src' / 'cli' / 'args.rs'
    if not args.is_file():
        return []
    src = args.read_text(encoding='utf-8', errors='replace')
    lines = src.split('\n')
    out, in_enum, depth, hidden, doc = [], False, 0, False, []
    for i, ln in enumerate(lines):
        st = ln.strip()
        if re.match(r'^(pub(\([^)]*\))?\s+)?enum\s+Cmd\s*\{', st):
            in_enum, depth = True, 1
            continue
        if not in_enum:
            continue
        depth += ln.count('{') - ln.count('}')
        if depth <= 0:
            break
        if st.startswith('///'):
            doc.append(st[3:].strip()); continue
        if 'hide = true' in st:
            hidden = True; continue
        if st.startswith('#['):
            continue
        m = re.match(r'^([A-Z][A-Za-z0-9]*)\s*[\{\(,]', st)
        if m and depth == 1:
            if not hidden:
                out.append(dict(name=f'schematize {kebab(m.group(1))}',
                                what=first_sentence(doc) or kebab(m.group(1)),
                                loc=f'{repo}/src/cli/args.rs:{i + 1}'))
            doc, hidden = [], False
        elif st.startswith('//'):
            continue
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

    # Espelho durável no archive (secao 28): INDEX_GLOBAL + INDEX_FUNCTIONS + MAPA.
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
