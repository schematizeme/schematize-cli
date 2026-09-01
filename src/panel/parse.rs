//! Parsers do índice/MAPA: arestas, nós de função (tabela e bullet) e descrições.
//! Tudo PURO (string -> dado) — é o que torna o formato do índice testável.

use super::*;

/// Tira a decoração de MARKDOWN do começo de uma linha: bullet (`- `, `* `, `+ `) e
/// lista numerada (`1. `). PURA.
///
/// Existe porque no MAPA a lista de adjacência é escrita como bullets — `- \`front\` →
/// \`api\`` é uma aresta legítima. Sem tirar o marcador, o id do nó virava
/// "- \`front", e o grafo enchia de nós fantasma com pontuação de markdown no nome.
pub(crate) fn sem_marcador_md(s: &str) -> &str {
    let t = s.trim();
    for m in ["- ", "* ", "+ "] {
        if let Some(r) = t.strip_prefix(m) {
            return r.trim();
        }
    }
    // "1. ", "12. " …
    let digitos = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digitos > 0 && t[digitos..].starts_with(". ") {
        return t[digitos + 2..].trim();
    }
    t
}

/// Limpa a decoração mermaid de um nó (`id[label]`, `id((label))`, aspas, crases).
pub(crate) fn clean_node(s: &str) -> String {
    let s = sem_marcador_md(s).trim_matches('`').trim();
    for (o, c) in [("[[", "]]"), ("((", "))"), ("[", "]"), ("(", ")"), ("{", "}")] {
        if let (Some(op), Some(cl)) = (s.find(o), s.rfind(c)) {
            if cl > op + o.len() - 1 {
                let inner = s[op + o.len()..cl].trim().trim_matches('"').trim();
                if !inner.is_empty() {
                    return inner.to_string();
                }
            }
        }
    }
    s.trim_matches('"').trim().to_string()
}

/// Tenta ler uma aresta `A -> B` (adjacência) ou `A -->|label| B` (mermaid).
pub(crate) fn parse_edge(l: &str) -> Option<(String, String, Option<String>)> {
    // CABEÇALHO nunca é aresta. O MAPA tem títulos que DOCUMENTAM o formato — por
    // exemplo `## Microfunções (função → arquivo:linha)` — e a seta ali é prosa. Sem
    // esta guarda, o título vira um nó chamado "## Microfunções (função".
    if l.trim_start().starts_with('#') {
        return None;
    }
    // Normaliza toda variante de seta pra `->`: mermaid (`-->`,`-.->`,`==>`) E unicode (`→`,`⟶`,`⇒`),
    // que o índice às vezes grava na lista de adjacência pesquisável e que o parser antes ignorava.
    let s = l
        .replace("-->", "->")
        .replace("-.->", "->")
        .replace("==>", "->")
        .replace(['→', '⟶', '⇒'], "->");
    let idx = s.find("->")?;
    let left = s[..idx].trim();
    let mut right = s[idx + 2..].trim().to_string();
    if left.is_empty() || right.is_empty() {
        return None;
    }
    let mut label: Option<String> = None;
    // mermaid: right pode vir "|contrato| B"
    if right.starts_with('|') {
        if let Some(end) = right[1..].find('|') {
            label = Some(right[1..1 + end].trim().to_string());
            right = right[1 + end + 1..].trim().to_string();
        }
    }
    // adjacência: "B (contrato)"
    if right.ends_with(')') {
        if let Some(op) = right.rfind('(') {
            label = Some(right[op + 1..right.len() - 1].trim().to_string());
            right = right[..op].trim().to_string();
        }
    }
    let a = clean_node(left);
    let b = clean_node(&right);
    if a.is_empty() || b.is_empty() || a.len() > 48 || b.len() > 48 {
        return None;
    }
    // Resíduo de MARKDOWN na ponta = a linha não era adjacência, era prosa que por
    // acaso tinha uma seta. Um id de nó não carrega crase, `#` nem travessão: o `—` é
    // justamente o separador "nome — descrição" do formato, então vê-lo aqui significa
    // que a descrição vazou pra dentro do nó.
    let residuo =
        |s: &str| s.contains('`') || s.contains('—') || s.starts_with('#') || s.starts_with('|');
    if residuo(&a) || residuo(&b) {
        return None;
    }
    // `|` = célula de tabela vazando um "->" interno (ex.: "api->db"); não é aresta de grafo.
    if a.contains('|') || b.contains('|') {
        return None;
    }
    // nós são identificadores/serviços curtos; frase com muitos espaços é legenda, não nó.
    let spaces = |s: &str| s.matches(' ').count();
    if spaces(&a) > 3 || spaces(&b) > 3 {
        return None;
    }
    Some((a, b, label.filter(|s| !s.is_empty())))
}

/// True se a célula parece `caminho/arquivo.ext:123`.
pub(crate) fn looks_like_loc(c: &str) -> bool {
    if let Some(idx) = c.rfind(':') {
        let after = &c[idx + 1..];
        return !after.is_empty()
            && after.chars().all(|ch| ch.is_ascii_digit())
            && (c.contains('/') || c.contains('.'));
    }
    false
}

/// Lê uma microfunção em BULLET do MAPA/§39: `- \`nome\` — descrição · arquivo:linha` (ou sem loc).
/// Devolve `(nome, Option<loc>)`. None se não for um bullet com nome curto e um em-dash de descrição.
/// NUNCA trata aresta (`->`) como função. É o que faz as microfunções em lista virarem nós do grafo.
pub(crate) fn parse_func_bullet(l: &str) -> Option<(String, Option<String>)> {
    // Precisa ser bullet (`-`/`*`) — evita casar linhas de texto corrido.
    let t = l.trim_start();
    if !(t.starts_with("- ") || t.starts_with("* ")) {
        return None;
    }
    let s = t[2..].trim();
    if s.is_empty() || s.contains("->") {
        return None;
    }
    // Precisa do em-dash separando nome — descrição (mesma convenção de `parse_desc_line`).
    let i = s.find('—')?;
    let name = clean_node(s[..i].trim());
    if name.is_empty() || name.len() > 80 || name.matches(' ').count() > 4 {
        return None;
    }
    // Localização opcional: a última célula que parecer `arquivo.ext:linha` (após `·`, `—` ou espaço).
    let rest = &s[i + '—'.len_utf8()..];
    let loc = rest
        .split(['·', '—', '|', '(', ')'])
        .flat_map(|seg| seg.split_whitespace())
        .map(|c| c.trim_matches('`').trim())
        .find(|c| looks_like_loc(c))
        .map(|c| c.to_string());
    Some((name, loc))
}

/// O nome da 1ª coluna parece um IDENTIFICADOR de função?
///
/// O quê: recusa o que nenhuma linguagem aceitaria como nome — começa com dígito, ou tem
/// espaço no meio. Onde: [`parse_func_row`], antes de aceitar a linha como nó.
///
/// Por que existe: exigir uma célula `arquivo:linha` não basta pra saber que a tabela é de
/// FUNÇÕES. Uma tabela que documenta códigos HTTP e cita onde eles são emitidos tem as duas
/// coisas — e o parser criava nós chamados `201 created` e `401 UNAUTHORIZED`, lixo que ia
/// direto pro grafo. Este é o resíduo que sobrou do conserto de arestas da v0.50.1.
///
/// **Entrada:** o texto da 1ª célula, já sem crases. **Saída:** `true` se pode ser nome de
/// função. **Efeitos:** nenhum.
///
/// Limite conhecido: um nome curto e maiúsculo (`AX`) é indistinguível de identificador
/// real pela FORMA, então não é filtrado aqui — separá-lo exigiria saber a linguagem.
fn parece_identificador(nome: &str) -> bool {
    let Some(primeiro) = nome.chars().next() else {
        return false;
    };
    // Nenhuma linguagem da casa aceita identificador começando por dígito — mata `201 created`.
    if primeiro.is_ascii_digit() {
        return false;
    }
    // Espaço no meio é prosa/rótulo, não símbolo — mata `401 UNAUTHORIZED`.
    !nome.chars().any(char::is_whitespace)
}

/// Lê uma linha de tabela de índice `nome | ... | arquivo:linha` → (nome, loc).
pub(crate) fn parse_func_row(l: &str) -> Option<(String, String)> {
    if !l.contains('|') {
        return None;
    }
    let cells: Vec<&str> = l.trim().trim_matches('|').split('|').map(|c| c.trim()).collect();
    if cells.len() < 2 {
        return None;
    }
    let name = cells[0].trim_matches('`').trim();
    if name.is_empty()
        || name.len() > 80
        || name.eq_ignore_ascii_case("função")
        || name.eq_ignore_ascii_case("funcao")
        || name.starts_with("---")
        || name.starts_with(":--")
        || !parece_identificador(name)
    {
        return None;
    }
    let loc = cells.iter().find(|c| looks_like_loc(c))?;
    Some((name.to_string(), loc.trim().to_string()))
}

/// Varre TODOS os `.md` do dir do index e monta (nós, arestas) — funde tudo (visão legada).
pub fn parse_graph(dir: &Path) -> (Vec<Node>, Vec<Edge>) {
    let mut files: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("md") {
                files.push(p);
            }
        }
    }
    files.sort();
    parse_graph_files(&files)
}

/// Monta (nós, arestas) parseando SÓ os arquivos dados (ex.: apenas `GRAFO_GLOBAL.md`, ou um
/// `<servico>.md`). Base compartilhada por [`parse_graph`], [`load_graph`] e [`load_service_graph`].
pub fn parse_graph_files(files: &[PathBuf]) -> (Vec<Node>, Vec<Edge>) {
    let mut locs: BTreeMap<String, String> = BTreeMap::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut ids: BTreeSet<String> = BTreeSet::new();
    let mut seen_edge: BTreeSet<(String, String)> = BTreeSet::new();
    {
        for p in files {
            let text = fs::read_to_string(p).unwrap_or_default();
            for line in text.lines() {
                let l = line.trim();
                // Arestas: linhas de adjacência/mermaid (ASCII `->`/`-->` OU unicode `→`/`⟶`/`⇒`) —
                // nunca linhas de tabela (`|`).
                let has_arrow =
                    l.contains("->") || l.contains('→') || l.contains('⟶') || l.contains('⇒');
                if has_arrow && !l.starts_with('|') {
                    if let Some((a, b, lab)) = parse_edge(l) {
                        ids.insert(a.clone());
                        ids.insert(b.clone());
                        if seen_edge.insert((a.clone(), b.clone())) {
                            edges.push(Edge { from: a, to: b, label: lab });
                        }
                    }
                }
                if l.contains('|') {
                    if let Some((name, loc)) = parse_func_row(l) {
                        ids.insert(name.clone());
                        locs.insert(name, loc);
                    }
                } else if let Some((name, loc)) = parse_func_bullet(l) {
                    // Microfunção em bullet `- \`nome\` — desc · arquivo:linha` (formato comum do
                    // MAPA §39) — vira nó com localização, senão essas funções nunca apareciam.
                    ids.insert(name.clone());
                    if let Some(loc) = loc {
                        locs.insert(name, loc);
                    }
                }
            }
        }
    }
    let nodes = ids
        .into_iter()
        .map(|id| {
            let loc = locs.get(&id).cloned();
            Node { id, loc }
        })
        .collect();
    (nodes, edges)
}

/// Lê uma linha de tabela de índice e extrai (nó, descrição curta): o nome é a 1ª
/// célula (mesma normalização de `parse_func_row`), a descrição é a 1ª célula
/// seguinte que NÃO é um `arquivo:linha`, separador ou vazia (tipicamente a coluna
/// "O quê"). Best-effort: None se não parecer uma linha de dado.
pub(crate) fn parse_desc_row(l: &str) -> Option<(String, String)> {
    if !l.contains('|') {
        return None;
    }
    let cells: Vec<&str> = l.trim().trim_matches('|').split('|').map(|c| c.trim()).collect();
    if cells.len() < 2 {
        return None;
    }
    let name = cells[0].trim_matches('`').trim();
    if name.is_empty()
        || name.len() > 80
        || name.starts_with("---")
        || name.starts_with(":--")
        || name.eq_ignore_ascii_case("função")
        || name.eq_ignore_ascii_case("funcao")
        || name.eq_ignore_ascii_case("repo/serviço")
        || name.eq_ignore_ascii_case("origem")
        || name.eq_ignore_ascii_case("pasta top-level")
    {
        return None;
    }
    let desc = cells[1..].iter().map(|c| c.trim_matches('`').trim()).find(|c| {
        !c.is_empty() && !looks_like_loc(c) && !c.starts_with("---") && !c.starts_with(":--")
    })?;
    if desc.is_empty() || desc.len() > 240 {
        return None;
    }
    Some((name.to_string(), desc.to_string()))
}

/// Lê uma linha de doc/hub `nó — descrição` (aceita bullet e `[[nó]]`); usa o
/// em-dash como separador. None se não casar. Nunca trata aresta (`->`) como desc.
pub(crate) fn parse_desc_line(l: &str) -> Option<(String, String)> {
    let s = l.trim_start_matches(['-', '*', '>']).trim();
    if s.is_empty() || s.contains("->") {
        return None;
    }
    let i = s.find('—')?;
    let left = s[..i].trim();
    let right = s[i + '—'.len_utf8()..].trim().trim_matches('`').trim();
    if left.is_empty() || right.is_empty() || right.len() > 240 {
        return None;
    }
    let name = clean_node(left);
    if name.is_empty() || name.len() > 80 {
        return None;
    }
    Some((name, right.to_string()))
}
