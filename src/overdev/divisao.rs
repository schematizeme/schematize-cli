//! SPLIT multiagente: divide o checklist em K fatias independentes, uma por agente.

use super::*;

/// Resultado de um split: os arquivos-parte criados e quantos itens foram pra cada um.
#[derive(Debug, Clone, Default)]
pub struct SplitResult {
    pub parts: Vec<PathBuf>,
    pub per_part: Vec<usize>,
    pub moved: usize,
}

/// SPLIT do checklist de `root` em `k` arquivos `checklist/part-N.md` (pastas multi-arquivo), pra
/// rodar multiagents em paralelo. MOVE os itens de MÁQUINA abertos (`- [ ]`) do `CHECKLIST.md` pros
/// parts (round-robin, balanceado), fazendo backup do original em `CHECKLIST.md.bak`. O resto
/// (feitos/on-hold/humanos/cabeçalho) fica no primário; a leitura multidoc reconstrói o total, então
/// as contagens e o Stop-hook seguem corretos. NÃO duplica itens. Idempotência: rerodar re-divide o
/// que estiver aberto no momento (inclui o que já está nos parts, pois o multidoc os enxerga).
pub fn split(root: &Path, k: usize) -> Result<SplitResult, String> {
    if k < 2 {
        return Err("split precisa de ao menos 2 partes (k >= 2).".into());
    }
    let od = dir_at(root);
    if !od.is_dir() {
        return Err("nenhum overdev neste projeto (rode `overdev start` antes).".into());
    }
    // Junta TODO o checklist (primário + parts já existentes) e separa itens-abertos do resto.
    let full = crate::paths::read_multidoc(&od, "CHECKLIST.md", "checklist");
    let mut open_items: Vec<String> = Vec::new();
    let mut keep: Vec<String> = Vec::new(); // linhas que NÃO são itens-abertos (ficam no primário)
    for line in full.lines() {
        if line.trim_start().starts_with("- [ ]") {
            open_items.push(line.trim_end().to_string());
        } else {
            keep.push(line.to_string());
        }
    }
    if open_items.is_empty() {
        return Err("não há item de máquina aberto (`- [ ]`) pra dividir.".into());
    }
    let k = k.min(open_items.len()); // não cria parts vazios

    // Limpa parts antigos (evita acúmulo entre re-splits) e recria a pasta.
    let cldir = od.join("checklist");
    if cldir.is_dir() {
        for e in std::fs::read_dir(&cldir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("md")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("part-"))
                    .unwrap_or(false)
            {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    std::fs::create_dir_all(&cldir).map_err(|e| e.to_string())?;

    // Distribui round-robin (balanceado).
    let mut buckets: Vec<Vec<String>> = vec![Vec::new(); k];
    for (i, item) in open_items.iter().enumerate() {
        buckets[i % k].push(item.clone());
    }

    // Backup do primário + reescreve sem os itens-abertos (mantém o resto).
    let prim = od.join("CHECKLIST.md");
    if prim.is_file() {
        let _ = std::fs::copy(&prim, od.join("CHECKLIST.md.bak"));
    }
    let mut primary_out = keep.join("\n");
    if !primary_out.ends_with('\n') {
        primary_out.push('\n');
    }
    std::fs::write(&prim, primary_out).map_err(|e| e.to_string())?;

    // Escreve os parts.
    let mut res = SplitResult::default();
    for (i, b) in buckets.iter().enumerate() {
        let f = cldir.join(format!("part-{:02}.md", i + 1));
        let body = format!(
            "# Split parte {}/{} — cuidada por um claude dedicado\n\
             > Gerado por `schematize overdev split`. Este agente fecha SÓ os itens abaixo.\n\n{}\n",
            i + 1,
            k,
            b.join("\n")
        );
        std::fs::write(&f, body).map_err(|e| e.to_string())?;
        res.per_part.push(b.len());
        res.parts.push(f);
    }
    res.moved = open_items.len();
    Ok(res)
}
