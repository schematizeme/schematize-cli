//! Layout de diretórios OPERACIONAIS de um projeto — o ponto ÚNICO que resolve onde
//! ficam `overdev/`, `grafos/` etc. O quê: o dir vivo de um projeto é `.schematize/`
//! (contendo `overdev/`, `grafos/`); o antigo `.overdev/` é LEGADO e ainda lido por
//! compat ("ler ambos"). Onde: consumido por overdev.rs, agentrun.rs, panel.rs,
//! debugreport.rs, projects.rs e pela GUI (via o crate `schematize`).
//!
//! Dois contratos de path, ambos resolvidos aqui pra hook e GUI NUNCA divergirem:
//! - **cwd-relative** (`*_cwd`): usado pelos hooks `overdev check`/`guard`, que rodam
//!   no cwd do agente.
//! - **path-aware** (`*_at`): usado pela GUI/monitor, que observam um `root` explícito.
//!
//! Regra "ler ambos": para um `root`, o dir de overdev é `.schematize/overdev` se ele
//! existir; senão o legado `.overdev` se existir; senão o novo `.schematize/overdev`
//! (default de escrita). Assim projeto novo nasce no layout novo, projeto legado segue
//! funcionando, e `overdev start` migra o legado ([`migrate_legacy_overdev`]).

use std::path::{Path, PathBuf};

/// Nome do dir operacional canônico de um projeto.
pub const OVERFLOW_DIR: &str = ".overflow";
/// Nome anterior do dir operacional — ainda lido por compat (o app virou Overflow).
pub const SCHEMATIZE_DIR: &str = ".schematize";
/// Nome do dir operacional LEGADO (pré-migração) — ainda lido por compat.
pub const LEGACY_OVERDEV_DIR: &str = ".overdev";

/// Dirs operacionais aceitos, do mais novo pro mais antigo.
///
/// A ordem É o contrato: quem escreve usa o primeiro; quem lê aceita o primeiro que
/// existir no disco. Acrescentar um nome novo aqui no topo é tudo o que um rename
/// futuro precisa — e nenhum projeto existente sente.
const DIRS_ACEITOS: [&str; 2] = [OVERFLOW_DIR, SCHEMATIZE_DIR];

/// O dir operacional EM USO neste `root`: o primeiro de [`DIRS_ACEITOS`] que existe,
/// ou o canônico se nenhum existe (projeto novo nasce no nome novo).
///
/// NÃO move nada. Projeto que já tem `.schematize/` continua com `.schematize/` até
/// alguém decidir o contrário — atualizar o app não remexe a pasta de ninguém.
fn dir_em_uso(root: &Path) -> PathBuf {
    for nome in DIRS_ACEITOS {
        let c = root.join(nome);
        if c.is_dir() {
            return c;
        }
    }
    root.join(OVERFLOW_DIR)
}

// ---------------------------------------------------------------------------
// path-aware (root explícito) — GUI/monitor
// ---------------------------------------------------------------------------

/// O dir operacional do projeto (`.overflow/`, ou o nome anterior se já existir).
pub fn schematize_dir_at(root: &Path) -> PathBuf {
    dir_em_uso(root)
}

/// Dir de overdev de `root`, resolvido pela regra "ler ambos": `.schematize/overdev`
/// se existir; senão `.overdev` legado se existir; senão o novo `.schematize/overdev`.
pub fn overdev_dir_at(root: &Path) -> PathBuf {
    for nome in DIRS_ACEITOS {
        let c = root.join(nome).join("overdev");
        if c.is_dir() {
            return c;
        }
    }
    let legado = root.join(LEGACY_OVERDEV_DIR);
    if legado.is_dir() {
        return legado;
    }
    root.join(OVERFLOW_DIR).join("overdev")
}

/// `<root>/.schematize/grafos` — dir operacional dos grafos do index (global + por serviço).
pub fn grafos_dir_at(root: &Path) -> PathBuf {
    dir_em_uso(root).join("grafos")
}

// ---------------------------------------------------------------------------
// Documentos multi-arquivo — granularidade (checklist/decisões/plano como PASTA)
// ---------------------------------------------------------------------------
//
// Cada "documento" do overdev pode ser 1 arquivo (`CHECKLIST.md`) OU uma PASTA com vários `.md`
// (`checklist/part-1.md`, `checklist/auth.md`, …) — pra granularidade máxima e pro SPLIT em
// multiagents (cada agent cuida de um arquivo). A leitura CONCATENA: o arquivo único primeiro (se
// existir), depois os `.md` da pasta em ordem alfabética. Assim projetos antigos (arquivo único)
// seguem funcionando e os novos podem espalhar em vários arquivos. Mesma ideia do dir de grafos.

/// Lista os arquivos que compõem um documento do overdev: o `single` (ex.: `CHECKLIST.md`) se
/// existir, seguido dos `.md` da pasta `folder` (ex.: `checklist/`) em ordem. Vazio se nada existe.
pub fn multidoc_files(od_dir: &Path, single: &str, folder: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let s = od_dir.join(single);
    if s.is_file() {
        out.push(s);
    }
    let dir = od_dir.join(folder);
    if dir.is_dir() {
        let mut mds: Vec<PathBuf> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("md"))
            .collect();
        mds.sort();
        out.extend(mds);
    }
    out
}

/// Lê e CONCATENA um documento multi-arquivo (arquivo único + pasta). Junta com `\n` entre partes.
pub fn read_multidoc(od_dir: &Path, single: &str, folder: &str) -> String {
    let parts: Vec<String> = multidoc_files(od_dir, single, folder)
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect();
    parts.join("\n")
}

// ---------------------------------------------------------------------------
// cwd-relative — hooks (Stop/PreToolUse)
// ---------------------------------------------------------------------------

/// Dir de overdev relativo ao cwd, mesma regra "ler ambos" de [`overdev_dir_at`].
/// É o que os hooks `overdev check`/`guard` enxergam (rodam no cwd do agente).
pub fn overdev_dir_cwd() -> PathBuf {
    overdev_dir_at(Path::new("."))
}

// ---------------------------------------------------------------------------
// migração legado -> novo
// ---------------------------------------------------------------------------

/// Migra `<root>/.overdev` para `<root>/.schematize/overdev` se o legado existir e o
/// novo ainda não. Best-effort: qualquer erro é devolvido pra quem chamar decidir
/// (o `overdev start` só avisa e segue). `Ok(true)` = migrou; `Ok(false)` = nada a fazer.
pub fn migrate_legacy_overdev(root: &Path) -> std::io::Result<bool> {
    let legado = root.join(LEGACY_OVERDEV_DIR);
    let novo = dir_em_uso(root).join("overdev");
    if !legado.is_dir() || novo.exists() {
        return Ok(false);
    }
    if let Some(parent) = novo.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // rename é atômico no mesmo filesystem; se falhar (cross-device), cai pra copy+remove.
    match std::fs::rename(&legado, &novo) {
        Ok(()) => Ok(true),
        Err(_) => {
            copy_dir_all(&legado, &novo)?;
            std::fs::remove_dir_all(&legado)?;
            Ok(true)
        }
    }
}

/// Copia recursivamente `src` -> `dst` (fallback do rename cross-device na migração).
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ordem de resolução É o contrato: `.overflow` > `.schematize` > `.overdev`,
    /// e o default de escrita é o nome novo. Projeto que já tem um nome anterior no
    /// disco NUNCA é redirecionado — atualizar o app não remexe a pasta de ninguém.
    #[test]
    fn resolve_prefere_novo_depois_anterior_depois_legado() {
        let tmp = std::env::temp_dir().join(format!("ovf-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // (1) nada existe -> projeto novo nasce em `.overflow/overdev`
        assert_eq!(overdev_dir_at(&tmp), tmp.join(".overflow/overdev"));

        // (2) só o legado pré-rename existe -> usa `.overdev`
        std::fs::create_dir_all(tmp.join(".overdev")).unwrap();
        assert_eq!(overdev_dir_at(&tmp), tmp.join(".overdev"));

        // (3) o nome ANTERIOR existe -> fica nele, mesmo com o legado presente.
        //     É a garantia de não-disrupção: quem está em `.schematize/` continua lá.
        std::fs::create_dir_all(tmp.join(".schematize/overdev")).unwrap();
        assert_eq!(overdev_dir_at(&tmp), tmp.join(".schematize/overdev"));

        // (4) o nome NOVO existe -> ele ganha de todos.
        std::fs::create_dir_all(tmp.join(".overflow/overdev")).unwrap();
        assert_eq!(overdev_dir_at(&tmp), tmp.join(".overflow/overdev"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Migrar o `.overdev` legado alveja o dir EM USO, não um nome fixo: num projeto
    /// que já vive em `.schematize/`, o conteúdo tem de cair lá — não criar um
    /// terceiro diretório e espalhar o estado do projeto por dois lugares.
    #[test]
    fn migra_legado_para_o_dir_em_uso() {
        let tmp = std::env::temp_dir().join(format!("ovf-migra-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join(".overdev")).unwrap();
        std::fs::write(tmp.join(".overdev/CHECKLIST.md"), b"- [ ] x\n").unwrap();

        // projeto sem dir do app -> migra pro nome novo
        assert!(migrate_legacy_overdev(&tmp).unwrap());
        assert!(!tmp.join(".overdev").exists(), "legado removido");
        assert!(tmp.join(".overflow/overdev/CHECKLIST.md").is_file(), "conteúdo movido");
        assert!(!migrate_legacy_overdev(&tmp).unwrap(), "idempotente");
        let _ = std::fs::remove_dir_all(&tmp);

        // projeto que JÁ vive em `.schematize/` -> migra PRA LÁ, não pro nome novo
        let tmp2 = std::env::temp_dir().join(format!("ovf-migra2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp2);
        std::fs::create_dir_all(tmp2.join(".overdev")).unwrap();
        std::fs::create_dir_all(tmp2.join(".schematize")).unwrap();
        std::fs::write(tmp2.join(".overdev/CHECKLIST.md"), b"- [ ] x\n").unwrap();
        assert!(migrate_legacy_overdev(&tmp2).unwrap());
        assert!(tmp2.join(".schematize/overdev/CHECKLIST.md").is_file(), "foi pro dir em uso");
        assert!(!tmp2.join(".overflow").exists(), "não inventou um terceiro dir");
        let _ = std::fs::remove_dir_all(&tmp2);
    }
}
