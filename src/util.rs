//! Utilitários: caminhos do ~/.claude, execução de processos e tempo.
//! O quê: helpers compartilhados por install/overdev. Onde: usado por main/skills/overdev.

use std::path::PathBuf;
use std::process::Command;

/// HOME do usuário (Linux). Falha explícita se não definido.
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("HOME não definido — schematize é Linux-first e precisa de $HOME")
}

/// Diretório base do Claude Code (`~/.claude`).
pub fn claude_dir() -> PathBuf {
    home().join(".claude")
}

/// Onde as skills instaladas moram (`~/.claude/skills`).
pub fn skills_dir() -> PathBuf {
    claude_dir().join("skills")
}

/// Onde os comandos achatados moram (`~/.claude/commands`).
pub fn commands_dir() -> PathBuf {
    claude_dir().join("commands")
}

/// Estado do schematize (versões instaladas) em `~/.claude/schematize/state.json`.
pub fn state_path() -> PathBuf {
    claude_dir().join("schematize").join("state.json")
}

/// settings.json do Claude Code (onde os hooks são registrados).
pub fn settings_path() -> PathBuf {
    claude_dir().join("settings.json")
}

/// Config do schematize (idioma etc.) em `~/.claude/schematize/config.json`.
pub fn config_path() -> PathBuf {
    claude_dir().join("schematize").join("config.json")
}

/// Abre uma URL no navegador padrão (xdg-open), sem bloquear. Best-effort.
pub fn open_url(url: &str) {
    let _ = Command::new("xdg-open")
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Caminho absoluto do próprio binário (pra registrar nos hooks sem depender do PATH).
pub fn self_exe() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| "schematize".to_string())
}

/// Roda um comando externo capturando stdout; erro traz stderr.
/// Fluxo: usado pra chamar curl/unzip/cp/rm — ferramentas presentes no Linux.
pub fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("falha ao executar {cmd}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "{cmd} falhou ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Executa um comando de shell HERDANDO o terminal (stdin/stdout/stderr).
/// Ao contrário de `run` (que captura), este deixa o processo interagir com o usuário —
/// essencial pra `sudo` pedir senha e pra instaladores oficiais mostrarem progresso.
/// Fluxo: usado APENAS pelo engine de environments, DEPOIS do consentimento explícito.
pub fn run_shell(cmd: &str) -> Result<(), String> {
    let status = Command::new("bash")
        .arg("-lc")
        .arg(cmd)
        .status()
        .map_err(|e| format!("falha ao executar shell: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("comando falhou ({status}): {cmd}"))
    }
}

/// Segundos desde a época (timestamp sem depender de crate de data).
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
