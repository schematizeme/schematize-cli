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

/// Dir de DADOS do app em `~/.claude/` — resolvido pela regra "ler ambos".
///
/// Canônico é `~/.claude/schematize/`. `overflow/` ainda é LIDO: houve um período
/// curto em que o app se chamou Overflow e escreveu lá — apagar esse caminho da
/// resolução tornaria invisível o estado de quem instalou naquela janela.
///
/// Resolve UMA vez e escreve onde leu; resolver por arquivo racharia o estado entre
/// os dois diretórios. E não move nada: migrar é decisão de quem opera, não efeito
/// colateral de atualizar.
pub fn dados_dir() -> PathBuf {
    let canonico = claude_dir().join("schematize");
    if canonico.is_dir() {
        return canonico;
    }
    let interregno = claude_dir().join("overflow");
    if interregno.is_dir() {
        return interregno;
    }
    canonico
}

/// Dir de dados do app no HOME (`~/.schematize/`) — mesma regra do [`dados_dir`],
/// para o que NÃO mora sob `~/.claude`: o DB do overdev, a sessão, o machine-id, o
/// orçamento de agents e os checkouts de build.
///
/// Existe separado de [`dados_dir`] porque são dois lugares distintos com o mesmo
/// problema — e um resolvedor por lugar é mais honesto que um genérico com flag.
pub fn home_app_dir() -> PathBuf {
    let canonico = home().join(".schematize");
    if canonico.is_dir() {
        return canonico;
    }
    let interregno = home().join(".overflow");
    if interregno.is_dir() {
        return interregno;
    }
    canonico
}

/// Estado do app (versões instaladas) em `<dados>/state.json`.
pub fn state_path() -> PathBuf {
    dados_dir().join("state.json")
}

/// settings.json do Claude Code (onde os hooks são registrados).
pub fn settings_path() -> PathBuf {
    claude_dir().join("settings.json")
}

/// Config do app (idioma etc.) em `<dados>/config.json`.
pub fn config_path() -> PathBuf {
    dados_dir().join("config.json")
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

/// Compara duas versões semver de forma SIMPLES (split por `.`), sem crate: `a < b`?
/// O quê: quebra cada versão em números por `.`, ignora sufixos não-numéricos (ex.: `1.2.0-rc`
/// vira 1,2,0), completa com zeros o mais curto e compara campo a campo. Onde: usado por
/// `upgrade::app_update_available` e por `notifications` pra decidir "há versão mais nova?".
/// Função PURA e determinística — testável sem rede.
pub fn semver_lt(a: &str, b: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        v.trim()
            .trim_start_matches('v')
            .split('.')
            .map(|p| {
                // Pega só o prefixo de dígitos do campo (descarta `-rc1`, `+build`, etc.).
                let digits: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
                digits.parse::<u64>().unwrap_or(0)
            })
            .collect()
    }
    let (pa, pb) = (parts(a), parts(b));
    let n = pa.len().max(pb.len());
    for i in 0..n {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        if x != y {
            return x < y;
        }
    }
    false
}

/// Define as permissões POSIX de um caminho. NO-OP em Windows.
///
/// O quê: aplica o modo POSIX quando a plataforma tem o conceito. Onde: todo lugar que
/// grava segredo ou dado sensível no HOME — sessão (`account`), chave SSH (`sshkeys`),
/// relatório de debug (`debugreport`).
///
/// Por que existe: os três chamadores importavam `std::os::unix::fs::PermissionsExt` no
/// TOPO do arquivo, sem `#[cfg(unix)]`. Isso quebra a compilação em Windows — é a causa
/// exata de `cannot find 'unix' in 'os'` + `no associated function 'from_mode'` que
/// derrubou o job de release do Windows em TODAS as 12 tentativas, deixando o asset
/// `schematize-windows-x86_64.zip` sem existir e o link do site em 404.
///
/// Concentrar aqui troca três `cfg` espalhados (que voltariam a ser esquecidos no próximo
/// arquivo que grave segredo) por um ponto único que já nasce correto nas duas plataformas.
///
/// **Entrada:** o caminho e o modo POSIX (`0o600` arquivo secreto, `0o700` diretório,
/// `0o644` arquivo público como uma chave `.pub`).
/// **Saída:** nenhuma. **Efeitos:** muda permissão no disco em Unix; erro é ignorado de
/// propósito (best-effort — falhar o chmod não pode abortar a gravação que já ocorreu).
///
/// **Limite honesto:** em Windows isto NÃO restringe nada. A ACL herdada do diretório do
/// usuário é a proteção efetiva ali; um equivalente real exigiria mexer em ACL, que é outro
/// trabalho e outra dependência.
pub fn definir_modo(path: &std::path::Path, modo: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(modo));
    }
    #[cfg(not(unix))]
    {
        let _ = (path, modo); // sem equivalente portátil; ver a nota no doc.
    }
}

#[cfg(test)]
mod tests {
    use super::semver_lt;

    #[test]
    fn semver_lt_basico() {
        assert!(semver_lt("0.25.0", "0.25.1"));
        assert!(semver_lt("0.25.1", "0.26.0"));
        assert!(semver_lt("1.0.0", "1.0.1"));
        assert!(semver_lt("0.9.0", "0.10.0")); // numérico, não lexicográfico
        // Iguais ou maiores → não é "<".
        assert!(!semver_lt("0.25.1", "0.25.1"));
        assert!(!semver_lt("0.26.0", "0.25.9"));
        assert!(!semver_lt("2.0.0", "1.9.9"));
    }

    #[test]
    fn semver_lt_tolera_prefixo_e_sufixo() {
        assert!(semver_lt("v0.25.0", "v0.25.1"));
        // Sufixo não-numérico é descartado (`1.2.3-rc` → 1,2,3).
        assert!(semver_lt("1.2.3-rc", "1.2.4"));
        // Comprimentos diferentes completam com zero.
        assert!(semver_lt("1.2", "1.2.1"));
        assert!(!semver_lt("1.2.0", "1.2"));
    }
}
