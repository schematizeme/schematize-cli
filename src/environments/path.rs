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
    let existing = std::fs::read_to_string(path).unwrap_or_default();
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
