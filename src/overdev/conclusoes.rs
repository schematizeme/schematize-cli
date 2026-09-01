//! LOG DE CONCLUSÕES: registra quando cada item fechou, pra o monitor mostrar o
//! avanço em tempo real sem reler o checklist inteiro.

use super::*;

/// Uma conclusão de item de MÁQUINA (`- [x]`) com a hora em que foi detectada.
/// `text` = conteúdo do item sem o prefixo `- [x] `; `ts` = epoch secs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Completion {
    pub text: String,
    pub ts: i64,
}

pub(crate) fn completions_file(root: &Path) -> PathBuf {
    dir_at(root).join("completions.json")
}

/// Epoch (secs) agora, como i64.
pub(crate) fn now_secs_i64() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// mtime (epoch secs) de um arquivo; None se indisponível.
pub(crate) fn mtime_secs(p: &Path) -> Option<i64> {
    let m = fs::metadata(p).ok()?.modified().ok()?;
    Some(m.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))
}

/// Texto de um item `- [x]` (máquina feito), sem o prefixo. None se a linha não
/// for `- [x]`. NÃO casa `- [H x]` (humano): esse começa por `- [H`, não `- [x]`.
pub(crate) fn done_item_text(line: &str) -> Option<String> {
    let t = line.trim_start();
    if t.starts_with("- [x]") || t.starts_with("- [X]") {
        Some(t[5..].trim().to_string())
    } else {
        None
    }
}

/// Ordena e materializa o mapa text->ts em `Vec<Completion>` (ts asc, nome desempata).
pub(crate) fn sorted_completions(map: std::collections::BTreeMap<String, i64>) -> Vec<Completion> {
    let mut out: Vec<Completion> =
        map.into_iter().map(|(text, ts)| Completion { text, ts }).collect();
    out.sort_by(|a, b| a.ts.cmp(&b.ts).then_with(|| a.text.cmp(&b.text)));
    out
}

/// Lê o registro `.overdev/completions.json` (mapa text->ts). Vazio se ausente/ilegível.
pub(crate) fn read_completions_map(root: &Path) -> std::collections::BTreeMap<String, i64> {
    fs::read_to_string(completions_file(root))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Detecta e REGISTRA (por DIFF) as conclusões de máquina do CHECKLIST de `<root>`.
/// - Lê os `- [x]` do CHECKLIST e o registro `.overdev/completions.json`.
/// - Para cada `- [x]` NOVO (fora do mapa) adiciona com `now`.
/// - SEED: se o registro ainda não existe, popula os `- [x]` JÁ presentes com o
///   mtime do CHECKLIST.md (aproxima "feito antes do log começar" em vez de perder).
/// - Salva o JSON e retorna TODAS as conclusões ordenadas por `ts` asc.
///
/// Best-effort: qualquer erro de IO é ignorado (nunca panica).
pub fn record_completions(root: &Path) -> Vec<Completion> {
    let checklist_path = dir_at(root).join("CHECKLIST.md");
    let content = match fs::read_to_string(&checklist_path) {
        Ok(c) => c,
        Err(_) => return sorted_completions(read_completions_map(root)),
    };
    let done_texts: Vec<String> = content.lines().filter_map(done_item_text).collect();

    let cf = completions_file(root);
    let existed = cf.exists();
    let mut map = read_completions_map(root);

    // 1ª vez (sem registro): os `- [x]` já presentes recebem o mtime do CHECKLIST.
    let seed_ts =
        if existed { 0 } else { mtime_secs(&checklist_path).unwrap_or_else(now_secs_i64) };
    let now = now_secs_i64();

    let mut changed = false;
    for txt in done_texts {
        if let std::collections::btree_map::Entry::Vacant(e) = map.entry(txt) {
            e.insert(if existed { now } else { seed_ts });
            changed = true;
        }
    }
    // Grava sempre na 1ª vez (marca que o seed já ocorreu) ou quando houver item novo.
    if changed || !existed {
        if let Some(d) = cf.parent() {
            let _ = fs::create_dir_all(d);
        }
        if let Ok(s) = serde_json::to_string_pretty(&map) {
            let _ = fs::write(&cf, s);
        }
    }
    sorted_completions(map)
}

/// Só LÊ o `.overdev/completions.json` (sem gravar), ordenado por `ts` asc —
/// pra a GUI/poll ler barato, sem tocar no CHECKLIST.
pub fn completions(root: &Path) -> Vec<Completion> {
    sorted_completions(read_completions_map(root))
}
