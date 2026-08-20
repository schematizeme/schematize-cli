//! Formatação do relatório (cabeçalho, chave/valor, indentação, data).

use super::*;

/// Cabeçalho de seção (linha em branco antes).
pub(crate) fn hdr(o: &mut String, title: &str) {
    let _ = writeln!(o, "\n== {title} ==");
}

/// Linha chave/valor alinhada.
pub(crate) fn kv(o: &mut String, k: &str, v: &str) {
    let _ = writeln!(o, "  {k:<26} {v}");
}

/// Indenta cada linha de `s` com `pad`.
pub(crate) fn indent(s: &str, pad: &str) -> String {
    s.lines().map(|l| format!("{pad}{l}")).collect::<Vec<_>>().join("\n")
}

/// Epoch (s) → `YYYY-MM-DD HH:MM:SS UTC` (algoritmo civil de Howard Hinnant, sem crate de data).
pub(crate) fn fmt_epoch(secs: u64) -> String {
    let secs = secs as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02} UTC")
}
