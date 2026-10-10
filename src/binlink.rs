//! Achar um BINÁRIO no disco — PLATAFORMA, e nada de domínio.
//!
//! **O quê:** o caminho absoluto de um executável, procurando no `$PATH` e nos diretórios que
//! um processo aberto pelo lançador do desktop não recebe.
//!
//! **Onde:** o [`crate::applink`], que é quem encaminha um comando para o app dono — e os dois
//! links (`deployerlink`, `skillslink`).
//!
//! ## Por que ele ficou no hub quando o `agentrun` saiu (E6)
//!
//! O `agentrun` inteiro é do overdev: PTY, prompts, supervisão. Este pedaço não — é o
//! resolvedor que **a própria delegação usa** para achar o app dono. Se ele tivesse ido junto,
//! o hub precisaria do app de overdev instalado para conseguir encaminhar qualquer comando a
//! qualquer app, inclusive ao deployer e ao market. Uma dependência circular disfarçada de
//! arrumação.

/// Diretórios onde ferramentas de usuário caem mas que o PATH de um processo
/// aberto pelo LANÇADOR DO DESKTOP não inclui — o desktop não carrega
/// `~/.profile`/`~/.bashrc`. Checar aqui (além do `$PATH`) é o que faz o app
/// achar o `claude` mesmo quando foi aberto pelo menu de apps e não pelo terminal.
pub(crate) fn fallback_bin_dirs() -> Vec<std::path::PathBuf> {
    // `util::home()` e não `env HOME`: no Windows a variável é outra, e a resolução certa
    // mora num lugar só (ver o doc de `util::home`).
    let h = crate::util::home();
    let mut dirs = vec![h.join(".local/bin"), h.join(".claude/local"), h.join(".cargo/bin")];
    #[cfg(windows)]
    {
        // O OpenSSH do Windows vem aqui desde o Windows 10 1803, e NÃO está no PATH de todo
        // perfil — sem este fallback, o `vps exec` falha em máquina recém-instalada.
        if let Some(sysroot) = std::env::var_os("SystemRoot") {
            let sr = std::path::PathBuf::from(sysroot);
            dirs.push(sr.join("System32").join("OpenSSH"));
            dirs.push(sr.join("SysNative").join("OpenSSH"));
        }
        for pf in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(d) = std::env::var_os(pf) {
                dirs.push(std::path::PathBuf::from(d).join("OpenSSH"));
            }
        }
    }
    dirs.push(std::path::PathBuf::from("/usr/local/bin"));
    dirs.push(std::path::PathBuf::from("/usr/bin"));
    dirs.push(std::path::PathBuf::from("/opt/homebrew/bin"));
    dirs.push(std::path::PathBuf::from("/usr/local/opt/openssh/bin"));
    dirs
}

/// Resolve o caminho ABSOLUTO de `bin`: primeiro nos diretórios do `$PATH` do
/// processo, depois nos de fallback ([`fallback_bin_dirs`]). `None` se não achar
/// em lugar nenhum. É o que permite achar/rodar o `claude` com o PATH mínimo da GUI.
pub fn resolve_bin(bin: &str) -> Option<std::path::PathBuf> {
    for dir in std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(fallback_bin_dirs())
    {
        for nome in nomes_de_executavel(bin) {
            let p = dir.join(&nome);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Nomes de arquivo que um executável `bin` pode ter neste sistema.
///
/// No Unix é só o nome. **No Windows o arquivo é `ssh.exe`**, e a versão anterior procurava
/// `ssh` — nunca achava. Como todo `vps exec` passa por `resolve_bin("ssh")`, o gestor de VPS
/// simplesmente não funcionava no Windows, que é uma das plataformas que o app publica
/// (`selfupdate.rs` distribui `schematize-windows-x86_64.exe`).
///
/// A lista segue o `PATHEXT` padrão, na ordem em que o próprio Windows resolve.
fn nomes_de_executavel(bin: &str) -> Vec<String> {
    nomes_de_executavel_em(bin, cfg!(windows))
}

/// A REGRA de [`nomes_de_executavel`], com a plataforma como PARÂMETRO.
///
/// Existe separada porque a versão anterior era `#[cfg(windows)]`: no Linux o corpo nem
/// compilava, então **nenhum teste podia exercitá-lo** — o mutation testing flagrou que
/// desligar a correção do Windows não quebrava teste nenhum. Com a plataforma como argumento,
/// a regra é verificável em qualquer máquina, e o `cfg!` só escolhe o argumento.
///
/// **Onde:** [`nomes_de_executavel`] em produção, e os testes com os dois valores.
pub fn nomes_de_executavel_em(bin: &str, windows: bool) -> Vec<String> {
    if !windows {
        return vec![bin.to_string()];
    }
    // Já veio com extensão: respeita o que o chamador pediu.
    if bin.contains('.') {
        return vec![bin.to_string()];
    }
    let mut v = vec![bin.to_string()];
    for ext in [".exe", ".cmd", ".bat", ".com"] {
        v.push(format!("{bin}{ext}"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A regra do Windows é verificável no Linux**, e por isso ela existe separada.
    ///
    /// A versão anterior era `#[cfg(windows)]`: no Linux o corpo nem compilava, então nenhum
    /// teste podia exercitá-lo — o mutation testing flagrou que desligar a correção do Windows
    /// não quebrava teste nenhum. Com a plataforma como argumento, a regra é afirmável em
    /// qualquer máquina.
    #[test]
    fn no_windows_o_executavel_tem_extensao() {
        assert_eq!(nomes_de_executavel_em("ssh", false), ["ssh"]);
        let w = nomes_de_executavel_em("ssh", true);
        assert!(w.contains(&"ssh.exe".to_string()), "{w:?}");
        assert!(w.contains(&"ssh.cmd".to_string()), "o npm instala `.cmd`: {w:?}");
        // Já veio com extensão: respeita o que o chamador pediu, sem inventar `ssh.exe.exe`.
        assert_eq!(nomes_de_executavel_em("ssh.exe", true), ["ssh.exe"]);
    }

    /// O que é achado vem ABSOLUTO — relativo dependeria do diretório de quem chamou.
    #[test]
    fn o_que_e_achado_vem_absoluto() {
        let p = resolve_bin("sh").expect("sh existe em máquina POSIX");
        assert!(p.is_absolute() && p.is_file(), "{p:?}");
        assert!(resolve_bin("nao-existe-em-lugar-nenhum-xyz").is_none());
    }
}
