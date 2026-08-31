//! Conserto do PATH: `~/.local/bin` no rc do shell quando o binário instalado não
//! está alcançável — o caso clássico de "instalei e o comando não existe".

/// A linha de export que garante ~/.local/bin no PATH do usuário.
pub(crate) const LOCAL_BIN_EXPORT: &str = "export PATH=\"$HOME/.local/bin:$PATH\"";

/// Decisão PURA: precisa consertar o PATH? Só quando o binário NÃO está no PATH
/// mas EXISTE em ~/.local/bin — nesse caso, pôr ~/.local/bin no PATH resolve. Se
/// nem no PATH nem no ~/.local/bin, o problema é outro (instalação falhou) e não
/// há PATH a consertar. Testável sem tocar o disco.
pub fn needs_path_fix(bin_in_path: bool, bin_in_local_bin: bool) -> bool {
    !bin_in_path && bin_in_local_bin
}

/// Decisão PURA e idempotente: o conteúdo de um rc já garante ~/.local/bin no PATH?
/// Considera presente qualquer linha NÃO-comentada que exporte um PATH mencionando
/// `.local/bin`. Assim não duplicamos a linha em quem já a tem. Testável com string.
pub fn rc_already_has_local_bin(content: &str) -> bool {
    content.lines().any(|l| {
        let l = l.trim();
        !l.starts_with('#') && l.contains(".local/bin") && l.contains("PATH")
    })
}

/// Garante (idempotente, best-effort) a linha de export num arquivo rc. Cria o
/// arquivo se não existir (ex.: ~/.bashrc ausente). Retorna Ok(true) se ADICIONOU
/// a linha, Ok(false) se já estava lá, Err se não deu pra escrever.
pub(crate) fn ensure_export_in_rc(path: &std::path::Path) -> Result<bool, String> {
    // `ler_para_modificar` e nao `unwrap_or_default`: um rc que nao seja UTF-8 valido
    // (comentario acentuado em Latin-1) ou sem permissao de leitura viraria `""` aqui, e o
    // `write` mais abaixo reescreveria o arquivo INTEIRO a partir do vazio.
    let existing = crate::util::ler_para_modificar(path)?;
    if rc_already_has_local_bin(&existing) {
        return Ok(false);
    }
    let mut new = existing;
    if !new.is_empty() && !new.ends_with('\n') {
        new.push('\n');
    }
    new.push_str("\n# schematize: garante ~/.local/bin no PATH\n");
    new.push_str(LOCAL_BIN_EXPORT);
    new.push('\n');
    std::fs::write(path, &new).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sandbox exclusivo deste teste.
    fn sandbox(nome: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("schematize-rc-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// **O caso que apagava o `.bashrc` da pessoa.**
    ///
    /// `read_to_string` devolve `InvalidData` para arquivo que não seja UTF-8 válido, e a
    /// versão anterior mapeava isso para `""` com `unwrap_or_default` — e então reescrevia o
    /// arquivo inteiro a partir do vazio. Um comentário acentuado em Latin-1 num `.bashrc`,
    /// coisa corriqueira, era suficiente pra destruir anos de configuração.
    #[test]
    fn rc_nao_utf8_sobrevive_intacto() {
        let d = sandbox("latin1");
        let p = d.join(".bashrc");
        let original = b"# meu ambiente, configura\xE7\xE3o de anos\nalias ll='ls -la'\n";
        std::fs::write(&p, original).unwrap();

        let r = ensure_export_in_rc(&p);
        assert!(r.is_err(), "tinha que recusar o rc ilegível, devolveu {r:?}");
        assert_eq!(std::fs::read(&p).unwrap(), original, "o .bashrc do usuário foi reescrito");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Num rc legível nada muda: acrescenta uma vez, preserva o que havia, é idempotente.
    #[test]
    fn rc_valido_ganha_a_linha_uma_vez_so() {
        let d = sandbox("ok");
        let p = d.join(".bashrc");
        std::fs::write(&p, "alias ll='ls -la'\n").unwrap();

        assert!(ensure_export_in_rc(&p).unwrap(), "devia ter adicionado");
        let uma = std::fs::read_to_string(&p).unwrap();
        assert!(uma.contains(".local/bin"), "não acrescentou: {uma}");
        assert!(uma.starts_with("alias ll="), "apagou o que já estava lá: {uma}");

        assert!(!ensure_export_in_rc(&p).unwrap(), "não é idempotente");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), uma, "a 2ª chamada mexeu no arquivo");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Arquivo ausente é criado — o caminho normal de quem não tem `.bashrc`.
    #[test]
    fn rc_ausente_e_criado() {
        let d = sandbox("ausente");
        let p = d.join(".bashrc");
        assert!(ensure_export_in_rc(&p).unwrap(), "devia ter criado");
        assert!(std::fs::read_to_string(&p).unwrap().contains(".local/bin"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
