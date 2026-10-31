//! Datas do ledger: `dia` e `semana_iso` no fuso LOCAL (o dia de trabalho de quem usa, não o
//! UTC dos transcripts — uma sessão às 22h em São Paulo cai no dia certo).

use chrono::{DateTime, Datelike, TimeZone, Utc};

/// RFC 3339 → UTC. `None` = timestamp ilegível (quem chama conta como aviso).
pub fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
}

/// `(dia, semana_iso)` de `ts` no fuso `tz`: `("2026-10-31", "2026-W44")`.
///
/// O ano da semana é o ANO ISO (31/12 pode cair na `W01` do ano seguinte), não o do calendário.
pub fn dia_e_semana<Tz: TimeZone>(ts: DateTime<Utc>, tz: &Tz) -> (String, String)
where
    Tz::Offset: std::fmt::Display,
{
    let local = ts.with_timezone(tz);
    let iso = local.iso_week();
    (local.format("%Y-%m-%d").to_string(), format!("{}-W{:02}", iso.year(), iso.week()))
}

/// RFC 3339 em UTC com milissegundos e `Z` — o formato dos transcripts, e ordenável como texto.
pub fn fmt_ts(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn brt() -> FixedOffset {
        FixedOffset::west_opt(3 * 3600).unwrap()
    }

    #[test]
    fn dia_segue_o_fuso_local() {
        // 01:30Z do dia 1 ainda é dia 31 às 22:30 em UTC-3.
        let ts = parse_ts("2026-11-01T01:30:00.000Z").unwrap();
        assert_eq!(dia_e_semana(ts, &brt()).0, "2026-10-31");
        assert_eq!(dia_e_semana(ts, &Utc).0, "2026-11-01");
    }

    #[test]
    fn semana_iso_usa_o_ano_iso_na_virada() {
        // 2026-12-31 é quinta: W53 de 2026. 2027-01-01 (sexta) ainda é W53.
        let a = parse_ts("2026-12-31T12:00:00Z").unwrap();
        let b = parse_ts("2027-01-01T12:00:00Z").unwrap();
        assert_eq!(dia_e_semana(a, &Utc).1, "2026-W53");
        assert_eq!(dia_e_semana(b, &Utc).1, "2026-W53");
        // 2024-12-30 (segunda) já é 2025-W01.
        let c = parse_ts("2024-12-30T12:00:00Z").unwrap();
        assert_eq!(dia_e_semana(c, &Utc).1, "2025-W01");
    }

    #[test]
    fn semana_vira_na_segunda_no_fuso_local() {
        // Domingo 23:30 em UTC-3 (= segunda 02:30Z): ainda é a semana anterior.
        let ts = parse_ts("2026-11-02T02:30:00Z").unwrap();
        let (dia, sem) = dia_e_semana(ts, &brt());
        assert_eq!(dia, "2026-11-01");
        assert_eq!(sem, "2026-W44");
        assert_eq!(dia_e_semana(ts, &Utc).1, "2026-W45");
    }

    #[test]
    fn ts_invalido_e_none() {
        assert!(parse_ts("ontem").is_none());
    }
}
