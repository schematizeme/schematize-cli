//! Descoberta de projetos nos diretórios de desenvolvimento cadastrados.
//! O quê: dado 1..N diretórios (pasta de UM projeto ou guarda-chuva tipo `PROJETOS`),
//! varre recursivamente procurando MARCADORES de projeto (`.overdev`, `.schematize`,
//! `.git`, `<algo>_archive/`) e devolve a lista de projetos detectados. Onde: usado
//! pela GUI pra alimentar o seletor de projeto das abas Overdev e Grafo.
//!
//! Regras: um diretório que contenha um marcador É um projeto; ao achar um marcador
//! PARA de descer (não recursa dentro de um projeto já identificado). Limite de
//! profundidade sensato e ignora `node_modules`, `target`, `.git`, etc.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Profundidade máxima da varredura a partir de cada diretório de desenvolvimento.
/// 0 = o próprio diretório cadastrado (caso "pasta de UM projeto direto").
const MAX_DEPTH: usize = 4;

/// Nomes de subdiretório em que NUNCA descemos (ruído/build/deps pesados).
const IGNORE: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    ".hg",
    ".svn",
    ".cache",
    ".venv",
    "venv",
    "__pycache__",
    "vendor",
    "dist",
    "build",
    ".next",
    ".idea",
    ".vscode",
    ".terraform",
];

/// Um projeto detectado num diretório de desenvolvimento.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Nome de exibição (basename do diretório do projeto).
    pub name: String,
    /// Caminho absoluto (canônico quando possível) do projeto.
    pub path: String,
    /// Marcador que identificou o projeto (".overdev", ".overflow", ".schematize", ".git", "_archive").
    pub marker: String,
}

/// Se `dir` contém um marcador de projeto, devolve qual (ordem de prioridade).
/// None = não é um projeto (candidato a guarda-chuva → continuar descendo).
fn marker_of(dir: &Path) -> Option<&'static str> {
    if dir.join(".overdev").is_dir() {
        return Some(".overdev");
    }
    // `.overflow`/`.schematize` podem ser arquivo (marcador explícito) ou pasta (dir
    // operacional). Os dois valem: o app virou Overflow, mas projeto marcado com o
    // nome anterior não pode sumir da lista por causa de um rename nosso.
    if dir.join(".overflow").exists() {
        return Some(".overflow");
    }
    if dir.join(".schematize").exists() {
        return Some(".schematize");
    }
    if dir.join(".git").is_dir() {
        return Some(".git");
    }
    // Padrão de archive da casa: um filho `<algo>_archive/`.
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                    if name.ends_with("_archive") {
                        return Some("_archive");
                    }
                }
            }
        }
    }
    None
}

/// Basename de um caminho como String (fallback: o caminho inteiro).
fn basename(p: &Path) -> String {
    p.file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| p.to_string_lossy().into_owned())
}

/// Varre recursivamente `dir` (na profundidade `depth`), acumulando projetos em `out`
/// e deduplicando por caminho canônico via `seen`. `pins` é o conjunto de caminhos
/// canônicos FIXADOS: um diretório nele é tratado como projeto (marcador "pinned")
/// e PARA a descida — igual a um marcador — pra o guarda-chuva aparecer como UM projeto.
fn walk(
    dir: &Path,
    depth: usize,
    out: &mut Vec<Project>,
    seen: &mut HashSet<PathBuf>,
    pins: &HashSet<PathBuf>,
) {
    if !dir.is_dir() {
        return;
    }
    // Caminho canônico pra dedup e pra registrar o projeto de forma estável.
    let canon = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());

    // Pin tem prioridade sobre marcador: um dir pinado é projeto "pinned" e para de descer.
    if pins.contains(&canon) {
        if seen.insert(canon.clone()) {
            out.push(Project {
                name: basename(&canon),
                path: canon.to_string_lossy().into_owned(),
                marker: "pinned".to_string(),
            });
        }
        return;
    }

    if let Some(marker) = marker_of(&canon) {
        if seen.insert(canon.clone()) {
            out.push(Project {
                name: basename(&canon),
                path: canon.to_string_lossy().into_owned(),
                marker: marker.to_string(),
            });
        }
        // Achou um projeto: NÃO desce dentro dele.
        return;
    }

    if depth >= MAX_DEPTH {
        return;
    }

    // Guarda-chuva: desce nos subdiretórios (ordenados pra saída determinística).
    let Ok(rd) = std::fs::read_dir(&canon) else {
        return;
    };
    let mut children: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| {
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            !name.starts_with('.') || name == ".overdev" || name == ".schematize"
            // (marcadores ocultos já foram tratados por marker_of; aqui só evitamos
            //  descer em pastas ocultas genéricas, mas mantemos as raras exceções.)
        })
        .filter(|p| {
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            !IGNORE.contains(&name)
        })
        .collect();
    children.sort();

    // UMBRELLA (aplicação multi-repo): um dir SEM marcador próprio mas com ≥2 filhos que SÃO
    // projetos (marcador ou pinado) é a "aplicação" — selecionável no app pra o GRAFO GLOBAL
    // (`.schematize/grafos/`), rodando o reindex na raiz do umbrella. Ao contrário de um marcador,
    // NÃO para a descida: os sub-repos seguem listados pra os grafos POR microserviço.
    let child_projects = children
        .iter()
        .filter(|c| {
            let cc = std::fs::canonicalize(c).unwrap_or_else(|_| (*c).to_path_buf());
            pins.contains(&cc) || marker_of(&cc).is_some()
        })
        .count();
    if child_projects >= 2 && seen.insert(canon.clone()) {
        out.push(Project {
            name: basename(&canon),
            path: canon.to_string_lossy().into_owned(),
            marker: "umbrella".to_string(),
        });
    }

    for child in children {
        walk(&child, depth + 1, out, seen, pins);
    }
}

/// Descobre os projetos em todos os diretórios de desenvolvimento cadastrados.
/// Dedup por caminho canônico; saída determinística (ordem dos dev_dirs + nome).
pub fn scan(dev_dirs: &[String]) -> Vec<Project> {
    scan_with_pins(dev_dirs, &[])
}

/// Como `scan`, mas os projetos FIXADOS (`pinned`) SEMPRE entram na lista, mesmo
/// sem marcador git, e um dir pinado encontrado durante o walk vira UM projeto que
/// para de descer (não despeja os sub-repos dele). Dedup por caminho canônico; o
/// label "pinned" tem prioridade. Saída determinística.
pub fn scan_with_pins(dev_dirs: &[String], pinned: &[String]) -> Vec<Project> {
    // Conjunto de pins canônicos (dir precisa existir pra virar projeto listado).
    let pins: HashSet<PathBuf> = pinned
        .iter()
        .filter_map(|p| std::fs::canonicalize(p).ok())
        .collect();

    let mut out = Vec::new();
    let mut seen = HashSet::new();

    // 1) Os pins válidos entram primeiro, garantidamente listados como "pinned".
    //    Ordenados pra saída determinística.
    let mut pin_list: Vec<PathBuf> = pins.iter().cloned().collect();
    pin_list.sort();
    for canon in &pin_list {
        if canon.is_dir() && seen.insert(canon.clone()) {
            out.push(Project {
                name: basename(canon),
                path: canon.to_string_lossy().into_owned(),
                marker: "pinned".to_string(),
            });
        }
    }

    // 2) Varredura normal dos dev_dirs (dedup contra os pins já inseridos; um dir
    //    pinado alcançado no walk para de descer sem re-listar).
    for d in dev_dirs {
        let root = PathBuf::from(d);
        walk(&root, 0, &mut out, &mut seen, &pins);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    /// Cria um diretório e um marcador dentro dele.
    fn mkproj(base: &Path, name: &str, marker: &str) -> PathBuf {
        let dir = base.join(name);
        fs::create_dir_all(&dir).unwrap();
        match marker {
            ".overdev" | ".git" => fs::create_dir_all(dir.join(marker)).unwrap(),
            ".schematize" => fs::write(dir.join(".schematize"), "{}").unwrap(),
            "_archive" => fs::create_dir_all(dir.join(format!("{name}_archive"))).unwrap(),
            _ => panic!("marcador de teste desconhecido"),
        }
        dir
    }

    /// Tempdir isolado no diretório temp do sistema (único por pid + contador atômico,
    /// pra os testes rodarem em paralelo sem colidir).
    fn tmp() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let id = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("schematize_projtest_{}_{id}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn dev_dir_que_e_um_projeto_direto() {
        let root = tmp();
        // O próprio dev_dir tem um marcador.
        fs::create_dir_all(root.join(".overdev")).unwrap();
        let projs = scan(&[root.to_string_lossy().into_owned()]);
        assert_eq!(projs.len(), 1);
        assert_eq!(projs[0].marker, ".overdev");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn guarda_chuva_descobre_varios_marcadores() {
        let root = tmp();
        mkproj(&root, "alpha", ".overdev");
        mkproj(&root, "beta", ".git");
        mkproj(&root, "gama", ".schematize");
        mkproj(&root, "delta", "_archive");
        // Ruído: pasta sem marcador não vira projeto.
        fs::create_dir_all(root.join("vazia")).unwrap();

        let projs = scan(&[root.to_string_lossy().into_owned()]);
        let names: std::collections::HashSet<&str> = projs.iter().map(|p| p.name.as_str()).collect();
        // Os 4 sub-repos aparecem, cada um com seu marcador...
        for n in ["alpha", "beta", "delta", "gama"] {
            assert!(names.contains(n), "faltou o sub-repo {n}: {projs:?}");
        }
        // ...e o guarda-chuva (dev_dir com ≥2 sub-projetos) aparece como UMA aplicação "umbrella",
        // sem parar a descida (por isso os sub-repos seguem listados). Ruído "vazia" nunca vira projeto.
        let umbrellas: Vec<&Project> = projs.iter().filter(|p| p.marker == "umbrella").collect();
        assert_eq!(umbrellas.len(), 1, "esperava 1 umbrella: {projs:?}");
        assert!(!names.contains("vazia"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn nao_recursa_dentro_de_projeto_ja_identificado() {
        let root = tmp();
        let proj = mkproj(&root, "app", ".git");
        // Um "subprojeto" com marcador DENTRO de um projeto já identificado não deve
        // aparecer — paramos de descer ao achar o marcador do pai.
        mkproj(&proj, "sub", ".overdev");

        let projs = scan(&[root.to_string_lossy().into_owned()]);
        assert_eq!(projs.len(), 1);
        assert_eq!(projs[0].name, "app");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ignora_node_modules_e_target() {
        let root = tmp();
        // Marcadores enterrados em pastas ignoradas não contam.
        let nm = root.join("node_modules");
        fs::create_dir_all(&nm).unwrap();
        mkproj(&nm, "fantasma", ".git");

        let projs = scan(&[root.to_string_lossy().into_owned()]);
        assert!(projs.is_empty(), "não deveria descer em node_modules: {projs:?}");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn dedup_por_caminho_canonico() {
        let root = tmp();
        mkproj(&root, "app", ".git");
        let s = root.to_string_lossy().into_owned();
        // Mesmo dev_dir cadastrado duas vezes → projeto aparece uma vez só.
        let projs = scan(&[s.clone(), s]);
        assert_eq!(projs.len(), 1);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn pin_de_dir_sem_marcador_aparece() {
        let root = tmp();
        // Uma pasta comum, SEM nenhum marcador de projeto.
        let plain = root.join("solto");
        fs::create_dir_all(&plain).unwrap();
        let canon = fs::canonicalize(&plain).unwrap();

        // Sem pin: não aparece. Com pin: aparece como "pinned".
        let sem = scan_with_pins(&[root.to_string_lossy().into_owned()], &[]);
        assert!(sem.is_empty(), "sem marcador e sem pin não deveria listar: {sem:?}");

        let com = scan_with_pins(
            &[root.to_string_lossy().into_owned()],
            &[plain.to_string_lossy().into_owned()],
        );
        assert_eq!(com.len(), 1);
        assert_eq!(com[0].marker, "pinned");
        assert_eq!(com[0].path, canon.to_string_lossy());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn pin_de_guarda_chuva_lista_ele_e_nao_os_sub_repos() {
        let root = tmp();
        // Um guarda-chuva com vários sub-repos com marcador dentro.
        let umbrella = root.join("workspace");
        fs::create_dir_all(&umbrella).unwrap();
        mkproj(&umbrella, "svc-a", ".git");
        mkproj(&umbrella, "svc-b", ".git");
        mkproj(&umbrella, "svc-c", ".overdev");

        // Fixando o guarda-chuva: ele vira UM projeto e os sub-repos NÃO aparecem.
        let projs = scan_with_pins(
            &[root.to_string_lossy().into_owned()],
            &[umbrella.to_string_lossy().into_owned()],
        );
        assert_eq!(projs.len(), 1, "deveria listar só o guarda-chuva: {projs:?}");
        assert_eq!(projs[0].name, "workspace");
        assert_eq!(projs[0].marker, "pinned");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn respeita_limite_de_profundidade() {
        let root = tmp();
        // Enterra um projeto além do MAX_DEPTH.
        let mut deep = root.clone();
        for i in 0..(MAX_DEPTH + 2) {
            deep = deep.join(format!("n{i}"));
        }
        fs::create_dir_all(&deep).unwrap();
        fs::create_dir_all(deep.join(".git")).unwrap();

        let projs = scan(&[root.to_string_lossy().into_owned()]);
        assert!(projs.is_empty(), "projeto além do depth-limit não deveria aparecer: {projs:?}");
        fs::remove_dir_all(&root).ok();
    }
}
