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
    // Piso de entropia: ed25519 (256-bit CSPRNG) sempre ok; RSA só >= 4096 bits.
    validate_entropy(kind)?;
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

// ------------------------------------------------------------------------------------------------
// ENTROPIA — piso de segurança na geração.
// ------------------------------------------------------------------------------------------------

/// Bits mínimos aceitos para RSA (deny-by-default: nada abaixo disso).
pub const RSA_MIN_BITS: u32 = 4096;

/// Nota (legível) sobre o NÍVEL de entropia/segurança de um tipo de chave — pra mostrar como
/// prova ao usuário. Toda geração usa o CSPRNG do `ssh-keygen` (getrandom do kernel).
pub fn entropy_note(kind: KeyKind) -> String {
    match kind {
        KeyKind::Ed25519 =>
            "ed25519 (Curve25519): chave de 256 bits do CSPRNG do ssh-keygen (getrandom do kernel), \
             ~128 bits de segurança efetiva — o padrão recomendado da casa."
                .to_string(),
        KeyKind::Rsa4096 => format!(
            "RSA {RSA_MIN_BITS} bits do CSPRNG do ssh-keygen, ~140+ bits de segurança efetiva — \
             só para hosts/dispositivos legados que ainda não falam ed25519."
        ),
    }
}

/// Valida o PISO de entropia do pedido (falha fechada). ed25519 é sempre aceito (256-bit fixo,
/// CSPRNG); RSA só com bits >= `RSA_MIN_BITS`. Chamado por `generate` antes de invocar o ssh-keygen.
pub fn validate_entropy(kind: KeyKind) -> Result<(), String> {
    match kind {
        KeyKind::Ed25519 => Ok(()),
        KeyKind::Rsa4096 => {
            let bits = kind.bits().unwrap_or(0);
            if bits < RSA_MIN_BITS {
                Err(format!(
                    "RSA com {bits} bits é fraco — o piso da casa é {RSA_MIN_BITS} bits \
                     (prefira ed25519, o default)"
                ))
            } else {
                Ok(())
            }
        }
    }
}

/// Linha de PROVA da chave: `ssh-keygen -l -f <pub>` → "bits SHA256:… comentário (TIPO)".
/// Mostra bits + fingerprint + tipo de uma vez (o usuário confere a força visualmente).
/// Best-effort: erro claro se a pública some ou o ssh-keygen falha.
pub fn proof_line(name: &str) -> Result<String, String> {
    let pub_p = public_path(name)?;
    if !pub_p.exists() {
        return Err(format!("chave pública '{name}' não encontrada em ~/.ssh"));
    }
    util::run("ssh-keygen", &["-l", "-f", &pub_p.to_string_lossy()])
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("ssh-keygen -l falhou: {e}"))
}

// ------------------------------------------------------------------------------------------------
// DEPLOY sem chave inline — usar a chave gerenciada pra logar/rodar comando remoto, e instalar a
// PÚBLICA no host. A privada NUNCA vai pra stdout/log: só é referenciada pelo caminho (`ssh -i`).
// ------------------------------------------------------------------------------------------------

/// Caminho da chave PRIVADA gerenciada (`~/.ssh/<name>`), com o nome já validado (allow-list).
/// Público pra a GUI/deploy referenciarem a chave por caminho, SEM nunca ler seu conteúdo.
pub fn key_path(name: &str) -> Result<PathBuf, String> {
    private_path(name)
}

/// Valida um alvo `user@host` (ou `host`) do ssh. Falha fechada: não-vazio, sem espaço e
/// SEM começar por `-` (senão o ssh interpretaria como opção — injeção de flag).
fn valid_target(target: &str) -> Result<(), String> {
    let t = target.trim();
    if t.is_empty() || t.starts_with('-') || t.chars().any(|c| c.is_whitespace()) {
        return Err(format!("alvo ssh inválido: {target:?} (use user@host)"));
    }
    Ok(())
}

/// Roda `ssh -i <privada gerenciada> <alvo> [comando...]` HERDANDO o terminal (stdin/out/err).
/// Sem comando → sessão interativa. A chave é referenciada só pelo CAMINHO (`-i`): o conteúdo da
/// privada NUNCA é lido nem impresso. `IdentitiesOnly=yes` força usar só a nossa chave;
/// `StrictHostKeyChecking=accept-new` aceita host novo sem prompt (mas trava se a fingerprint mudar).
/// Retorna o exit code do ssh (128+sinal se morto por sinal).
pub fn run_ssh(name: &str, target: &str, args: &[String]) -> Result<i32, String> {
    use std::process::Command;
    let key = key_path(name)?;
    if !key.exists() {
        return Err(format!(
            "chave privada '{name}' não encontrada em ~/.ssh — gere com `schematize ssh gen {name}`"
        ));
    }
    valid_target(target)?;
    let mut cmd = Command::new("ssh");
    cmd.arg("-i").arg(&key)
        .arg("-o").arg("IdentitiesOnly=yes")
        .arg("-o").arg("StrictHostKeyChecking=accept-new")
        .arg(target);
    // `--` não vai: o ssh já trata tudo após o alvo como o comando remoto.
    for a in args {
        cmd.arg(a);
    }
    let status = cmd.status().map_err(|e| format!("falha ao executar ssh: {e}"))?;
    // code() é None quando morto por sinal — reporta 128+sinal (convenção shell) ou 1.
    Ok(status.code().unwrap_or(1))
}

/// Instala a chave PÚBLICA no `~/.ssh/authorized_keys` do host remoto (bootstrap de acesso).
/// Requer que você JÁ tenha acesso ao host (outra chave/senha/agent). Usa `ssh-copy-id -i <pub>`
/// se existir (melhor: lida com prompt de senha), senão faz append por ssh (pública via stdin).
/// Só a PÚBLICA é enviada — a privada nunca sai.
pub fn authorize(name: &str, target: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let pub_p = public_path(name)?;
    if !pub_p.exists() {
        return Err(format!("chave pública '{name}' não encontrada em ~/.ssh"));
    }
    valid_target(target)?;

    // Caminho feliz: ssh-copy-id (herda o terminal pra pedir senha de bootstrap se preciso).
    if in_path("ssh-copy-id") {
        let status = Command::new("ssh-copy-id")
            .arg("-i").arg(&pub_p)
            .arg("-o").arg("StrictHostKeyChecking=accept-new")
            .arg(target)
            .status()
            .map_err(|e| format!("falha ao executar ssh-copy-id: {e}"))?;
        return if status.success() {
            Ok(())
        } else {
            Err(format!("ssh-copy-id falhou (exit {})", status.code().unwrap_or(-1)))
        };
    }

    // Fallback: append remoto por ssh, com a pública entrando pelo stdin (umask 077).
    let pubkey = fs::read_to_string(&pub_p)
        .map_err(|e| format!("não consegui ler a pública: {e}"))?;
    let remote = "umask 077; mkdir -p ~/.ssh && cat >> ~/.ssh/authorized_keys";
    let mut child = Command::new("ssh")
        .arg("-o").arg("StrictHostKeyChecking=accept-new")
        .arg(target)
        .arg(remote)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("falha ao executar ssh: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(pubkey.trim_end().as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .map_err(|e| format!("falha ao enviar a pública: {e}"))?;
    }
    let status = child.wait().map_err(|e| format!("ssh não finalizou: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("append remoto falhou (exit {})", status.code().unwrap_or(-1)))
    }
}

/// `true` se um binário está no PATH (via `which`). Best-effort.
fn in_path(bin: &str) -> bool {
    std::process::Command::new("which")
        .arg(bin)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ------------------------------------------------------------------------------------------------
// EXPORT pro Bitwarden — via CLI `bw` (se destravado) OU arquivo de IMPORT (fallback).
// A chave PRIVADA só vai pro cofre/arquivo (mode 600) — NUNCA pro stdout/log.
// ------------------------------------------------------------------------------------------------

/// Roda um comando alimentando `input` pelo stdin e capturando o stdout. Usado pelo fluxo do `bw`
/// (`bw encode`). Erro traz o stderr. A privada passa por aqui só rumo ao `bw` (nunca é impressa).
fn run_with_stdin(cmd: &str, args: &[&str], input: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("falha ao executar {cmd}: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .map_err(|e| format!("falha ao escrever no stdin de {cmd}: {e}"))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("{cmd} não finalizou: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "{cmd} falhou: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// O `bw` está no PATH e DESTRAVADO? (`bw status` traz `"status":"unlocked"`.) Só então
/// criamos item direto no cofre; caso contrário caímos no arquivo de import.
fn bw_unlocked() -> bool {
    if !in_path("bw") {
        return false;
    }
    match util::run("bw", &["status"]) {
        Ok(s) => s.contains("\"status\":\"unlocked\""),
        Err(_) => false,
    }
}

/// Monta o corpo (notas) legível do item — inclui a privada. NUNCA é impresso em stdout;
/// só entra no item do cofre / arquivo de import.
fn bw_notes(name: &str, kind: &str, fingerprint: &str, pubkey: &str, privkey: &str) -> String {
    format!(
        "Chave SSH gerenciada pelo schematize\n\
         nome: {name}\n\
         tipo: {kind}\n\
         fingerprint: {fingerprint}\n\n\
         --- CHAVE PÚBLICA ---\n{pubkey}\n\n\
         --- CHAVE PRIVADA (secreta) ---\n{privkey}\n"
    )
}

/// JSON de UM item de cofre (secure note, type 2) pro `bw encode | bw create item`.
/// Campos: public_key + fingerprint (visíveis) e private_key (oculto, type 1).
fn bw_item_json(name: &str, notes: &str, pubkey: &str, fingerprint: &str, privkey: &str) -> String {
    let item = serde_json::json!({
        "type": 2,
        "name": format!("SSH schematize:{name}"),
        "notes": notes,
        "secureNote": { "type": 0 },
        "fields": [
            { "name": "public_key",  "value": pubkey,      "type": 0 },
            { "name": "fingerprint", "value": fingerprint, "type": 0 },
            { "name": "private_key", "value": privkey,     "type": 1 }
        ]
    });
    item.to_string()
}

/// JSON no formato de IMPORT do Bitwarden (`{items:[...]}`) — o fallback quando o `bw` não está
/// destravado. Mesma modelagem do item (secure note com os campos), envelopado em `items`.
fn bw_import_json(name: &str, notes: &str, pubkey: &str, fingerprint: &str, privkey: &str) -> String {
    let doc = serde_json::json!({
        "items": [ serde_json::from_str::<serde_json::Value>(
            &bw_item_json(name, notes, pubkey, fingerprint, privkey)
        ).unwrap_or(serde_json::Value::Null) ]
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into())
}

/// Caminho default do arquivo de import (`~/.schematize/bw-import-<name>.json`).
fn bw_import_path(name: &str) -> PathBuf {
    util::home().join(".schematize").join(format!("bw-import-{name}.json"))
}

/// Exporta a chave `name` pro Bitwarden. Se o `bw` estiver destravado, cria um item (secure note)
/// no cofre com nome/tipo/fingerprint/pública/PRIVADA. Senão, grava um JSON no formato de import
/// (default `~/.schematize/bw-import-<name>.json`, mode 600) e instrui a importar. A privada é lida
/// do arquivo e SÓ vai pro cofre/arquivo — NUNCA pro stdout/log. Retorna uma mensagem do que ocorreu.
pub fn export_bitwarden(name: &str, out: Option<&Path>) -> Result<String, String> {
    valid_name(name)?;
    let info = read_info(name)?;
    let priv_p = private_path(name)?;
    if !priv_p.exists() {
        return Err(format!("chave privada '{name}' não encontrada em ~/.ssh"));
    }
    let privkey = fs::read_to_string(&priv_p)
        .map_err(|e| format!("não consegui ler a privada de '{name}': {e}"))?;
    let privkey = privkey.trim_end().to_string();
    let pubkey = export_public(name)?;
    let notes = bw_notes(name, &info.kind, &info.fingerprint, &pubkey, &privkey);

    // Caminho feliz: cofre destravado → cria o item direto (bw encode | bw create item).
    if bw_unlocked() {
        let item_json = bw_item_json(name, &notes, &pubkey, &info.fingerprint, &privkey);
        let encoded = run_with_stdin("bw", &["encode"], &item_json)?;
        // `bw create item` lê o base64 pelo stdin.
        match run_with_stdin("bw", &["create", "item"], encoded.trim()) {
            Ok(_) => {
                return Ok(format!(
                    "item 'SSH schematize:{name}' criado no seu cofre Bitwarden \
                     (pública + fingerprint + privada oculta). A privada não foi impressa."
                ));
            }
            // Falhou no cofre: não perde a exportação — cai no arquivo de import.
            Err(e) => {
                eprintln!("(aviso: bw create item falhou: {e} — gerando arquivo de import)");
            }
        }
    }

    // Fallback: arquivo de import (mode 600).
    let target = match out {
        Some(p) => p.to_path_buf(),
        None => bw_import_path(name),
    };
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("falha ao criar {}: {e}", dir.display()))?;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    let body = bw_import_json(name, &notes, &pubkey, &info.fingerprint, &privkey);
    fs::write(&target, body).map_err(|e| format!("falha ao gravar {}: {e}", target.display()))?;
    let _ = fs::set_permissions(&target, fs::Permissions::from_mode(0o600));
    Ok(format!(
        "arquivo de import do Bitwarden gravado (mode 600) em {}\n\
         importe em: Bitwarden → Tools → Import data → formato 'Bitwarden (json)'.\n\
         Apague o arquivo depois de importar (ele contém a chave privada).",
        target.display()
    ))
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

    #[test]
    fn entropia_ed25519_sempre_ok_rsa_no_piso() {
        // ed25519 é aceito e a nota cita 256/128 bits.
        assert!(validate_entropy(KeyKind::Ed25519).is_ok());
        let n = entropy_note(KeyKind::Ed25519);
        assert!(n.contains("256"));
        assert!(n.contains("128"));
        // RSA do rol é 4096 (>= piso) → aceito; a nota cita 4096.
        assert!(validate_entropy(KeyKind::Rsa4096).is_ok());
        assert!(entropy_note(KeyKind::Rsa4096).contains("4096"));
        assert_eq!(RSA_MIN_BITS, 4096);
    }

    #[test]
    fn alvo_ssh_valido_recusa_injecao_de_flag() {
        assert!(valid_target("root@host").is_ok());
        assert!(valid_target("host.local").is_ok());
        assert!(valid_target("deploy@10.0.0.1").is_ok());
        // Falha fechada: vazio, começando por '-' (flag), ou com espaço.
        assert!(valid_target("").is_err());
        assert!(valid_target("-oProxyCommand=evil").is_err());
        assert!(valid_target("user@host extra").is_err());
    }

    #[test]
    fn key_path_valida_nome_e_fica_em_ssh() {
        let p = key_path("deploy").expect("nome válido");
        assert!(p.ends_with("deploy"));
        assert!(p.to_string_lossy().contains(".ssh"));
        // Nome que escaparia de ~/.ssh é recusado.
        assert!(key_path("../evil").is_err());
    }

    #[test]
    fn bw_item_json_tem_secure_note_e_campos() {
        let notes = bw_notes("deploy", "ED25519", "SHA256:abc", "ssh-ed25519 AAA c", "PRIV");
        let j = bw_item_json("deploy", &notes, "ssh-ed25519 AAA c", "SHA256:abc", "PRIV");
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["type"], 2); // secure note
        assert_eq!(v["secureNote"]["type"], 0);
        assert_eq!(v["name"], "SSH schematize:deploy");
        // Campos: public_key/fingerprint visíveis, private_key oculto (type 1).
        let fields = v["fields"].as_array().unwrap();
        assert!(fields.iter().any(|f| f["name"] == "public_key" && f["type"] == 0));
        assert!(fields.iter().any(|f| f["name"] == "fingerprint" && f["type"] == 0));
        assert!(fields.iter().any(|f| f["name"] == "private_key" && f["type"] == 1));
        // A privada está presente no item (vai pro cofre), mas o notes também a carrega.
        assert!(notes.contains("PRIV"));
    }

    #[test]
    fn bw_import_json_envelopa_em_items() {
        let notes = bw_notes("k", "RSA", "SHA256:z", "ssh-rsa AAA", "PRIV");
        let j = bw_import_json("k", &notes, "ssh-rsa AAA", "SHA256:z", "PRIV");
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        let items = v["items"].as_array().expect("items é array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["type"], 2);
        assert_eq!(items[0]["name"], "SSH schematize:k");
    }
}
