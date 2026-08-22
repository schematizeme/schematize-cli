//! Painel auxiliar (HTML no browser) + export Obsidian do grafo do index.
//! O quê: `schematize panel` gera um HTML self-contained (sem CDN) com o estado do
//! overdev (objetivo, checklist, decisões, plano, perguntas parkeadas) e um grafo
//! force-directed estilo Obsidian do index (nós linkados a arquivo:linha); abre no
//! browser. `schematize graph obsidian` exporta o index como vault Obsidian
//! (markdown + [[wikilinks]]). Onde: chamado por main; lê `.overdev/*` e
//! `<projeto>_archive/index/*.md`. É read-mostly — o juiz do "terminou" segue sendo o
//! checklist+gate (overdev.rs); o painel só dá visão.

use crate::util::open_url;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

// Submódulos (piso da casa: <=750 linhas, uma unidade lógica por arquivo).
mod parse;
mod grafo;
mod estado;
mod html;
mod obsidian;
pub use parse::*;
pub use grafo::*;
pub use estado::*;
pub use html::*;
pub use obsidian::*;


/// Nó do grafo: id (função/serviço) e, se conhecido, `arquivo:linha`.
#[derive(Clone)]
pub struct Node {
    pub id: String,
    pub loc: Option<String>,
}
/// Aresta dirigida do grafo, com rótulo opcional (contrato/rota/evento).
#[derive(Clone)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
}

/// Estado do overdev de um projeto (lido do control-plane) — consumido pela GUI e pelo HTML.
pub struct Overdev {
    pub objetivo: String,
    pub mode: String,
    pub items: Vec<(char, String)>, // (' '|'x'|'~', texto)
    pub decisoes: String,
    pub plano: String,
    pub perguntas: String,
}
impl Overdev {
    pub fn counts(&self) -> (u32, u32, u32) {
        let (mut o, mut d, mut h) = (0, 0, 0);
        for (s, _) in &self.items {
            match s {
                'x' => d += 1,
                '~' => h += 1,
                _ => o += 1,
            }
        }
        (o, d, h)
    }
}

// ---------------------------------------------------------------------------
// Descoberta do index e parsing do grafo (best-effort, tolerante a formato).
// ---------------------------------------------------------------------------












// ---------------------------------------------------------------------------
// Estado do overdev (lido direto dos arquivos do control-plane).
// ---------------------------------------------------------------------------










// ---------------------------------------------------------------------------
// `schematize panel` — gera o HTML e abre no browser.
// ---------------------------------------------------------------------------





// ---------------------------------------------------------------------------
// `schematize graph obsidian` — exporta o index como vault Obsidian.
// ---------------------------------------------------------------------------




#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Setas UNICODE (`→`) na adjacência viram arestas — antes o parser só via ASCII `->`/`-->`.
    #[test]
    pub(crate) fn parse_edge_aceita_seta_unicode() {
        let (a, b, lab) = parse_edge("front → api (login)").expect("aresta unicode");
        assert_eq!((a.as_str(), b.as_str()), ("front", "api"));
        assert_eq!(lab.as_deref(), Some("login"));
        // ASCII segue funcionando.
        let (c, d, _) = parse_edge("api -> db").expect("aresta ascii");
        assert_eq!((c.as_str(), d.as_str()), ("api", "db"));
    }

    /// Microfunção em BULLET vira nó com localização (`- \`nome\` — desc · arquivo:linha`).
    #[test]
    pub(crate) fn parse_func_bullet_extrai_no_e_loc() {
        let (n, loc) = parse_func_bullet("- `login` — autentica o usuário · auth.rs:42").expect("bullet");
        assert_eq!(n, "login");
        assert_eq!(loc.as_deref(), Some("auth.rs:42"));
        // Sem loc: nó sem localização.
        let (n2, loc2) = parse_func_bullet("- `logout` — encerra a sessão").expect("bullet sem loc");
        assert_eq!(n2, "logout");
        assert!(loc2.is_none());
        // Aresta não é função.
        assert!(parse_func_bullet("- api -> db").is_none());
        // Linha sem bullet não casa.
        assert!(parse_func_bullet("login — algo").is_none());
    }

    /// `load_graph` prefere `GRAFO_GLOBAL.md` (visão global limpa) e ignora os demais `.md` do dir.
    #[test]
    pub(crate) fn load_graph_prioriza_grafo_global() {
        let root = std::env::temp_dir().join(format!("schz-graph-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let grafos = root.join(".schematize/grafos");
        fs::create_dir_all(&grafos).unwrap();
        // Global: só a fronteira entre serviços.
        fs::write(grafos.join("GRAFO_GLOBAL.md"), "front -> api (login)\napi -> auth (token)\n").unwrap();
        // Detalhe por serviço: NÃO deve entrar na visão global.
        fs::write(grafos.join("api.md"), "handler -> repo\nrepo -> pg\n").unwrap();

        let (nodes, edges, dir) = load_graph(&root);
        assert!(dir.is_some());
        let ids: std::collections::HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains("front") && ids.contains("api") && ids.contains("auth"));
        assert!(!ids.contains("repo") && !ids.contains("pg"), "não pode fundir o grafo por-serviço no global");
        assert_eq!(edges.len(), 2);

        // Drill-down: o grafo detalhado do serviço é acessível à parte.
        let (snodes, _sedges) = load_service_graph(&root, "api");
        let sids: std::collections::HashSet<&str> = snodes.iter().map(|n| n.id.as_str()).collect();
        assert!(sids.contains("repo") && sids.contains("pg"));

        let _ = fs::remove_dir_all(&root);
    }

    /// `service_of` extrai o microserviço do 1º componente do path do loc.
    #[test]
    pub(crate) fn service_of_pega_o_primeiro_componente() {
        assert_eq!(service_of(&Some("svc_a/src/x.rs:12".into())).as_deref(), Some("svc_a"));
        assert_eq!(service_of(&Some("front/app/page.tsx:1".into())).as_deref(), Some("front"));
        // Sem diretório (nó de raiz) → None.
        assert!(service_of(&Some("main.rs:3".into())).is_none());
        // Sem loc → None.
        assert!(service_of(&None).is_none());
    }

    /// Índice flat grande (> CAP) sem `GRAFO_GLOBAL.md` → agrega em 1 nó por serviço, e o drill
    /// devolve o subgrafo do serviço a partir do flat. É o fix do "1600 nós ilegíveis".
    /// O `GRAFO_GLOBAL.md` AUTORADO também respeita o cap: um global que arrasta as
    /// tabelas de função de cada repo (o caso real do archive da casa) passa de 60
    /// nós e tem de virar a visão agregada — antes essa origem furava o cap e a GUI
    /// carregava centenas de nós na física.
    #[test]
    pub(crate) fn load_graph_global_autorado_grande_tambem_agrega() {
        let root = std::env::temp_dir().join(format!("schz-gg-cap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let grafos = root.join(".schematize/grafos");
        fs::create_dir_all(&grafos).unwrap();
        // Global autorado: 2 arestas de fronteira + 80 linhas de tabela de função
        // (40 em cada serviço) — exatamente o formato do índice §39.
        let mut md = String::from("svc_a/main.rs -> svc_b/main.rs (chamada)\n");
        md.push_str("| Função | O quê | arquivo:linha |\n|---|---|---|\n");
        for i in 0..40 {
            md.push_str(&format!("| `a{i}` | faz a | svc_a/x.rs:{} |\n", i + 1));
            md.push_str(&format!("| `b{i}` | faz b | svc_b/y.rs:{} |\n", i + 1));
        }
        fs::write(grafos.join("GRAFO_GLOBAL.md"), md).unwrap();

        let (nodes, _edges, dir, aggregated) = load_graph_global(&root);
        assert!(dir.is_some());
        assert!(aggregated, "global autorado com >60 nós tem de agregar");
        assert!(nodes.len() <= GLOBAL_NODE_CAP, "{} nós passaram do cap", nodes.len());
        let ids: Vec<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.iter().any(|i| i.starts_with("svc_a ·")), "faltou o nó agregado de svc_a: {ids:?}");
        assert!(ids.iter().any(|i| i.starts_with("svc_b ·")), "faltou o nó agregado de svc_b: {ids:?}");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    pub(crate) fn load_graph_global_agrega_flat_grande_e_drilla() {
        let root = std::env::temp_dir().join(format!("schz-agg-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let grafos = root.join(".schematize/grafos");
        fs::create_dir_all(&grafos).unwrap();
        // 40 funções em svc_a + 21 em svc_b = 61 nós (> GLOBAL_NODE_CAP=60) → deve agregar.
        let mut s = String::from("# MAPA\n");
        for i in 0..40 {
            s.push_str(&format!("- `fn_a{i}` — faz A{i} · svc_a/src/a.rs:{}\n", i + 1));
        }
        for i in 0..21 {
            s.push_str(&format!("- `fn_b{i}` — faz B{i} · svc_b/src/b.rs:{}\n", i + 1));
        }
        s.push_str("fn_a0 -> fn_b0\n"); // aresta cross-serviço
        fs::write(grafos.join("INDEX_FUNCTIONS.md"), s).unwrap();

        let (nodes, edges, dir, aggregated) = load_graph_global(&root);
        assert!(dir.is_some());
        assert!(aggregated, "flat com 61 nós tem que agregar");
        assert_eq!(nodes.len(), 2, "1 nó por serviço");
        let ids: std::collections::HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains("svc_a · 40") && ids.contains("svc_b · 21"), "rótulo com contagem: {ids:?}");
        assert_eq!(edges.len(), 1, "1 aresta serviço→serviço");

        // Drill no serviço agregado (fallback pelo flat, sem `svc_a.md` autorado).
        let (sn, _se) = load_service_graph(&root, "svc_a");
        assert_eq!(sn.len(), 40, "subgrafo do svc_a = suas 40 funções");
        assert!(sn.iter().all(|n| n.id.starts_with("fn_a")));

        let _ = fs::remove_dir_all(&root);
    }

    /// `find_index_dir` prioriza `.schematize/grafos/` sobre o `_archive/index/` legado.
    #[test]
    pub(crate) fn find_index_dir_prefere_schematize_grafos() {
        let root = std::env::temp_dir().join(format!("schz-idxdir-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".schematize/grafos")).unwrap();
        fs::create_dir_all(root.join("proj_archive/index")).unwrap();
        assert_eq!(find_index_dir(&root), Some(root.join(".schematize/grafos")));
        let _ = fs::remove_dir_all(&root);
    }
    /// Casos REAIS colhidos do MAPA de um projeto da casa — o grafo estava com 127 nós
    /// e a maioria era pontuação de markdown virando nome de nó.
    #[test]
    fn markdown_nao_vira_no() {
        // Cabeçalho que DOCUMENTA o formato: a seta ali é prosa, não adjacência.
        assert_eq!(parse_edge("## Microfunções (função → arquivo:linha)"), None);
        assert_eq!(parse_edge("### fluxo: a -> b"), None);

        // Bullet É adjacência legítima no MAPA — o marcador sai, a aresta fica.
        let (a, b, _) = parse_edge("- `front` → `api`").unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("front", "api"));
        let (a, b, _) = parse_edge("* api --> db").unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("api", "db"));
        let (a, b, _) = parse_edge("1. api -> cache").unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("api", "cache"));

        // Prosa com travessão: o `—` é o separador "nome — descrição" do formato, então
        // vê-lo dentro de um nó significa que a descrição vazou.
        assert_eq!(parse_edge("- `row_to_view` — linha → view"), None);
    }

    /// O marcador de lista sai; o resto do id é preservado como está.
    #[test]
    fn sem_marcador_md_so_tira_o_marcador() {
        assert_eq!(sem_marcador_md("- api"), "api");
        assert_eq!(sem_marcador_md("  * api"), "api");
        assert_eq!(sem_marcador_md("12. api"), "api");
        assert_eq!(sem_marcador_md("api"), "api", "sem marcador, não mexe");
        assert_eq!(sem_marcador_md("a-b"), "a-b", "hífen no meio do nome sobrevive");
        assert_eq!(sem_marcador_md("-api"), "-api", "sem espaço não é bullet");
    }

    /// Aresta normal continua funcionando — a correção não podia estreitar o que já lia.
    #[test]
    fn arestas_legitimas_continuam_passando() {
        assert!(parse_edge("front -> api").is_some());
        assert!(parse_edge("api -->|contrato| db").is_some());
        assert!(parse_edge("api ⇒ fila").is_some());
        let (_, _, lab) = parse_edge("front -> api (Bearer)").unwrap();
        assert_eq!(lab.as_deref(), Some("Bearer"));
    }

    /// O QUE: tabela de CÓDIGOS HTTP não vira nó de função.
    ///
    /// POR QUE: exigir uma célula `arquivo:linha` não prova que a tabela é de funções —
    /// uma que documenta status HTTP e cita onde cada um é emitido tem as duas coisas.
    /// O parser criava nós `201 created` e `401 UNAUTHORIZED`, que iam direto pro grafo.
    /// É o resíduo que a v0.50.1 deixou ao consertar as arestas (§8 do CONTEXT).
    #[test]
    fn tabela_de_codigo_http_nao_vira_funcao() {
        // Linhas REAIS do formato que produziu o lixo: 1ª coluna é o status, e há loc.
        assert_eq!(parse::parse_func_row("| 201 created | recurso criado | src/http.rs:88 |"), None);
        assert_eq!(parse::parse_func_row("| 401 UNAUTHORIZED | sem sessão | src/auth.rs:12 |"), None);
        // E a linha de função LEGÍTIMA continua passando — o filtro não pode comer o sinal.
        assert_eq!(
            parse::parse_func_row("| `criar_usuario` | cria o usuário | src/users.rs:42 |"),
            Some(("criar_usuario".to_string(), "src/users.rs:42".to_string())),
        );
        // Caminhos com `::` e genéricos seguem sendo identificadores válidos.
        assert!(parse::parse_func_row("| cache::ler | lê o cache | src/cache.rs:7 |").is_some());
    }
}
