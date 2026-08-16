//! sshkeys — gestão de chaves SSH da máquina do usuário (gerar/listar/exportar/remover).
//! O quê: envolve o `ssh-keygen` (via `util`) pra agilizar setup de GitHub/servidores;
//! guarda o par em `~/.ssh/<name>` (privada 600) e `~/.ssh/<name>.pub` (644).
//! Onde: lógica COMPARTILHADA (o CLI usa via `schematize ssh`; a GUI consumirá depois).
//!
//! SEGURANÇA (piso, deny-by-default):
//! - a chave PRIVADA nunca é lida, impressa, copiada nem exportada — só a PÚBLICA sai;
//! - nome validado por allow-list (sem `/`, sem `..`, sem `~`) pra não escapar de `~/.ssh`;
//! - permissões forçadas: privada 600, pública 644, diretório `~/.ssh` 700;
//! - passphrase repassada ao `ssh-keygen` sem ecoar (nunca vai pra log/stdout);
//! - não sobrescreve chave existente sem `force` explícito.

use crate::util;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Tipo de chave suportado. Padrão da casa: ed25519 (moderno, curto, recomendado);
/// rsa 4096 fica para servidores/dispositivos legados que ainda não falam ed25519.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    /// Ed25519 — recomendado (default).
    Ed25519,
    /// RSA 4096 bits — compatibilidade com sistemas antigos.
    Rsa4096,
}

impl KeyKind {
    /// Algoritmo como o `ssh-keygen -t` espera.
    pub fn algo(&self) -> &'static str {
        match self {
            KeyKind::Ed25519 => "ed25519",
            KeyKind::Rsa4096 => "rsa",
        }
    }

    /// Bits para o `-b` (só o RSA precisa; ed25519 tem tamanho fixo).
    pub fn bits(&self) -> Option<u32> {
        match self {
            KeyKind::Ed25519 => None,
            KeyKind::Rsa4096 => Some(4096),
        }
    }

    /// Interpreta uma escolha textual (usado por CLI/GUI). Deny-by-default: só o rol.
    pub fn parse(s: &str) -> Result<KeyKind, String> {
        match s.trim().to_lowercase().as_str() {
            "ed25519" | "ed" | "" => Ok(KeyKind::Ed25519),
            "rsa" | "rsa4096" | "rsa-4096" => Ok(KeyKind::Rsa4096),
            other => Err(format!("tipo de chave desconhecido: {other} (use: ed25519 | rsa)")),
        }
    }
}

/// Metadados PÚBLICOS de uma chave — nunca inclui material da privada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInfo {
    /// Nome do arquivo base em `~/.ssh` (ex.: "github").
    pub name: String,
    /// Algoritmo reportado pela pública (ex.: "ED25519", "RSA").
    pub kind: String,
    /// Fingerprint SHA256 (via `ssh-keygen -lf`).
    pub fingerprint: String,
    /// Comentário embutido na pública (ex.: "schematize:user@host").
    pub comment: String,
    /// Caminho absoluto da chave PÚBLICA.
    pub public_path: String,
}

/// Diretório `~/.ssh`.
fn ssh_dir() -> PathBuf {
    util::home().join(".ssh")
}

/// Garante `~/.ssh` existente com permissão 700 (padrão do OpenSSH).
fn ensure_ssh_dir() -> Result<PathBuf, String> {
    let dir = ssh_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("não consegui criar ~/.ssh: {e}"))?;
    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    Ok(dir)
}

/// Valida o nome da chave (allow-list). Falha fechada: só `[A-Za-z0-9._-]`, começando
/// por alfanumérico, sem `..`, sem separador de caminho — o par NUNCA escapa de `~/.ssh`.
pub fn valid_name(name: &str) -> Result<(), String> {
    let bad = || Err(format!("nome de chave inválido: {name:?} (use letras, números, '.', '_' ou '-')"));
    if name.is_empty() || name.len() > 64 {
        return bad();
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return bad();
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return bad(),
    }
    if name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        Ok(())
    } else {
        bad()
    }
}

/// Caminho da PRIVADA (`~/.ssh/<name>`), já validado.
fn private_path(name: &str) -> Result<PathBuf, String> {
    valid_name(name)?;
    Ok(ssh_dir().join(name))
}

/// Caminho da PÚBLICA (`~/.ssh/<name>.pub`), já validado.
fn public_path(name: &str) -> Result<PathBuf, String> {
    valid_name(name)?;
    Ok(ssh_dir().join(format!("{name}.pub")))
}

/// Comentário padrão `schematize:<user>@<host>`. Sem timestamp de propósito
/// (o sandbox proíbe relógio; data só entraria via `$(date)` no shell).
pub fn default_comment() -> String {
    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    let host = std::env::var("HOSTNAME")
        .ok()
        .filter(|h| !h.trim().is_empty())
        .or_else(|| util::run("hostname", &[]).ok().map(|s| s.trim().to_string()))
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "host".to_string());
    format!("schematize:{user}@{host}")
}

/// Monta os argumentos do `ssh-keygen` (função PURA — testável sem I/O nem `~/.ssh`).
/// Passphrase vazia = chave sem senha; qualquer valor é repassado via `-N` (sem eco).
pub fn keygen_args(kind: KeyKind, priv_path: &str, comment: &str, passphrase: &str) -> Vec<String> {
    let mut a: Vec<String> = vec!["-t".into(), kind.algo().into()];
    if let Some(bits) = kind.bits() {
        a.push("-b".into());
        a.push(bits.to_string());
    }
    a.push("-f".into());
    a.push(priv_path.into());
    a.push("-C".into());
    a.push(comment.into());
    // -N: passphrase (string vazia = sem senha). Nunca ecoamos este valor.
    a.push("-N".into());
    a.push(passphrase.into());
    // -q: silencioso (não imprime a arte/fingerprint no stdout).
    a.push("-q".into());
    a
}

/// Deriva o algoritmo (ED25519/RSA/…) da 1ª palavra de uma linha de chave pública.
pub fn kind_from_publine(line: &str) -> String {
    match line.split_whitespace().next() {
        Some("ssh-ed25519") => "ED25519".into(),
        Some("ssh-rsa") => "RSA".into(),
        Some("ssh-dss") => "DSA".into(),
        Some(t) if t.starts_with("ecdsa-") => "ECDSA".into(),
        Some(t) if t.starts_with("sk-") => "FIDO".into(),
        _ => "?".into(),
    }
}

/// Extrai o comentário (tudo após `<algo> <base64>`) de uma linha de pública.
pub fn comment_from_publine(line: &str) -> String {
    line.splitn(3, char::is_whitespace).nth(2).unwrap_or("").trim().to_string()
}

/// Fingerprint SHA256 de uma pública, via `ssh-keygen -lf`. Best-effort ("?" se falhar).
fn fingerprint_of(pub_path: &Path) -> String {
    match util::run("ssh-keygen", &["-lf", &pub_path.to_string_lossy()]) {
        Ok(out) => out.split_whitespace().nth(1).unwrap_or("?").to_string(),
        Err(_) => "?".to_string(),
    }
}

/// Gera um par de chaves. Salva a privada (600) e a pública (644) em `~/.ssh`.
/// Recusa sobrescrever um par existente sem `force`. NUNCA imprime a privada.
pub fn generate(
    name: &str,
    kind: KeyKind,
    comment: Option<&str>,
    passphrase: Option<&str>,
    force: bool,
) -> Result<KeyInfo, String> {
    valid_name(name)?;
    let dir = ensure_ssh_dir()?;
    let priv_p = dir.join(name);
    let pub_p = dir.join(format!("{name}.pub"));

    if priv_p.exists() || pub_p.exists() {
        if !force {
            return Err(format!(
                "a chave '{name}' já existe em ~/.ssh — use --force para sobrescrever"
            ));
        }
        // Com force: apaga o par antigo (senão o ssh-keygen abre um prompt interativo).
        let _ = fs::remove_file(&priv_p);
        let _ = fs::remove_file(&pub_p);
    }

    let owned_comment = match comment {
        Some(c) if !c.trim().is_empty() => c.to_string(),
        _ => default_comment(),
    };
    let pass = passphrase.unwrap_or("");
    let args = keygen_args(kind, &priv_p.to_string_lossy(), &owned_comment, pass);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

    // Chama o ssh-keygen. Em erro, NUNCA embutimos a passphrase na mensagem.
    util::run("ssh-keygen", &arg_refs).map_err(|e| format!("ssh-keygen falhou: {e}"))?;

    // Reforça as permissões (o ssh-keygen já as aplica; garantimos o piso).
    let _ = fs::set_permissions(&priv_p, fs::Permissions::from_mode(0o600));
    let _ = fs::set_permissions(&pub_p, fs::Permissions::from_mode(0o644));

    read_info(name)
}

/// Lê o `KeyInfo` de uma pública já existente (sem tocar a privada).
fn read_info(name: &str) -> Result<KeyInfo, String> {
    let pub_p = public_path(name)?;
    let body = fs::read_to_string(&pub_p)
        .map_err(|e| format!("não consegui ler a pública de '{name}': {e}"))?;
    let line = body.trim();
    Ok(KeyInfo {
        name: name.to_string(),
        kind: kind_from_publine(line),
        fingerprint: fingerprint_of(&pub_p),
        comment: comment_from_publine(line),
        public_path: pub_p.to_string_lossy().into_owned(),
    })
}

/// Lista as chaves em `~/.ssh` varrendo os `*.pub`. NUNCA lê/expõe a privada.
pub fn list() -> Vec<KeyInfo> {
    let dir = ssh_dir();
    let mut out = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_pub = path.extension().and_then(|e| e.to_str()) == Some("pub");
        if !is_pub {
            continue;
        }
        let name = match path.file_stem().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if valid_name(&name).is_err() {
            continue;
        }
        if let Ok(info) = read_info(&name) {
            out.push(info);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Devolve o conteúdo da chave PÚBLICA (pra colar no GitHub/servidor). Só a pública.
pub fn export_public(name: &str) -> Result<String, String> {
    let pub_p = public_path(name)?;
    let body = fs::read_to_string(&pub_p)
        .map_err(|_| format!("chave pública '{name}' não encontrada em ~/.ssh"))?;
    Ok(body.trim().to_string())
}

/// Copia um texto pro clipboard via `wl-copy` (Wayland) ou `xclip` (X11). Best-effort:
/// devolve true se algum copiador existia e rodou. Usado só com a PÚBLICA.
pub fn copy_to_clipboard(text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};
    for (bin, args) in [("wl-copy", &[][..]), ("xclip", &["-selection", "clipboard"][..])] {
        let child = Command::new(bin)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(mut c) = child {
            if let Some(mut stdin) = c.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if c.wait().map(|s| s.success()).unwrap_or(false) {
                return true;
            }
        }
    }
    false
}

/// Remove o par (privada + pública). A confirmação é responsabilidade do chamador (CLI).
pub fn remove(name: &str) -> Result<(), String> {
    let priv_p = private_path(name)?;
    let pub_p = public_path(name)?;
    if !priv_p.exists() && !pub_p.exists() {
        return Err(format!("chave '{name}' não encontrada em ~/.ssh"));
    }
    if priv_p.exists() {
        fs::remove_file(&priv_p).map_err(|e| format!("falha ao remover a privada: {e}"))?;
    }
    if pub_p.exists() {
        fs::remove_file(&pub_p).map_err(|e| format!("falha ao remover a pública: {e}"))?;
    }
    Ok(())
}

/// Adiciona a chave ao `ssh-agent` (`ssh-add`). Best-effort: devolve true se rodou ok.
pub fn add_to_agent(name: &str) -> bool {
    let priv_p = match private_path(name) {
        Ok(p) => p,
        Err(_) => return false,
    };
    if !priv_p.exists() {
        return false;
    }
    util::run("ssh-add", &[&priv_p.to_string_lossy()]).is_ok()
}

/// Adiciona a chave PÚBLICA à conta do GitHub do usuário via `gh ssh-key add`.
/// Exige `gh` instalado e autenticado (erro claro caso contrário).
pub fn add_to_github(name: &str) -> Result<(), String> {
    let pub_p = public_path(name)?;
    if !pub_p.exists() {
        return Err(format!("chave pública '{name}' não encontrada em ~/.ssh"));
    }
    // Checa autenticação do gh antes de tentar (mensagem clara se faltar).
    if util::run("gh", &["auth", "status"]).is_err() {
        return Err(
            "gh não está autenticado (ou não instalado) — rode `gh auth login` primeiro".to_string(),
        );
    }
    util::run(
        "gh",
        &["ssh-key", "add", &pub_p.to_string_lossy(), "--title", name],
    )
    .map(|_| ())
    .map_err(|e| format!("gh ssh-key add falhou: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keykind_rol_e_default() {
        assert_eq!(KeyKind::parse("ed25519").unwrap(), KeyKind::Ed25519);
        assert_eq!(KeyKind::parse("ED").unwrap(), KeyKind::Ed25519);
        assert_eq!(KeyKind::parse("").unwrap(), KeyKind::Ed25519); // default
        assert_eq!(KeyKind::parse("rsa").unwrap(), KeyKind::Rsa4096);
        assert_eq!(KeyKind::parse("rsa4096").unwrap(), KeyKind::Rsa4096);
        assert!(KeyKind::parse("dsa").is_err()); // deny-by-default
        assert!(KeyKind::parse("ecdsa").is_err());
    }

    #[test]
    fn nome_invalido_e_recusado_deny_by_default() {
        // Válidos.
        assert!(valid_name("github").is_ok());
        assert!(valid_name("id_ed25519").is_ok());
        assert!(valid_name("srv-01.prod").is_ok());
        // Inválidos — nada que escape de ~/.ssh.
        assert!(valid_name("").is_err());
        assert!(valid_name("../evil").is_err());
        assert!(valid_name("a/b").is_err());
        assert!(valid_name("a\\b").is_err());
        assert!(valid_name(".hidden").is_err()); // não começa por alfanumérico
        assert!(valid_name("-flag").is_err());
        assert!(valid_name("foo..bar").is_err());
        assert!(valid_name("nome com espaco").is_err());
        assert!(valid_name(&"x".repeat(65)).is_err()); // longo demais
    }

    #[test]
    fn keygen_args_ed25519_nao_tem_bits() {
        let a = keygen_args(KeyKind::Ed25519, "/home/u/.ssh/k", "c@h", "");
        assert!(a.contains(&"-t".to_string()));
        assert!(a.contains(&"ed25519".to_string()));
        assert!(!a.contains(&"-b".to_string()), "ed25519 não usa -b");
        // -f aponta pro caminho da privada e -C traz o comentário.
        let fi = a.iter().position(|s| s == "-f").unwrap();
        assert_eq!(a[fi + 1], "/home/u/.ssh/k");
        let ci = a.iter().position(|s| s == "-C").unwrap();
        assert_eq!(a[ci + 1], "c@h");
        // -N presente (passphrase vazia = sem senha).
        let ni = a.iter().position(|s| s == "-N").unwrap();
        assert_eq!(a[ni + 1], "");
    }

    #[test]
    fn keygen_args_rsa_tem_bits_4096_e_passphrase() {
        let a = keygen_args(KeyKind::Rsa4096, "/home/u/.ssh/k", "c", "s3cr3t");
        let bi = a.iter().position(|s| s == "-b").expect("rsa usa -b");
        assert_eq!(a[bi + 1], "4096");
        assert!(a.contains(&"rsa".to_string()));
        let ni = a.iter().position(|s| s == "-N").unwrap();
        assert_eq!(a[ni + 1], "s3cr3t"); // passphrase repassada via -N
    }

    #[test]
    fn parse_de_linha_publica() {
        let ed = "ssh-ed25519 AAAAC3NzaC1lZDI1 schematize:lucas@box";
        assert_eq!(kind_from_publine(ed), "ED25519");
        assert_eq!(comment_from_publine(ed), "schematize:lucas@box");
        let rsa = "ssh-rsa AAAAB3Nza comentario com espacos";
        assert_eq!(kind_from_publine(rsa), "RSA");
        assert_eq!(comment_from_publine(rsa), "comentario com espacos");
        // Sem comentário.
        let semc = "ssh-ed25519 AAAAC3NzaC1lZDI1";
        assert_eq!(comment_from_publine(semc), "");
    }

    #[test]
    fn comentario_padrao_tem_prefixo_schematize() {
        let c = default_comment();
        assert!(c.starts_with("schematize:"), "comentário: {c}");
        assert!(c.contains('@'));
    }
}
