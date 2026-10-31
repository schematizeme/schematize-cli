//! `cost sync`: transcripts → ledger. Incremental (offset por arquivo) e idempotente.
//!
//! Ordem de segurança: o ledger é gravado ANTES do cursor. Se o processo morrer entre os dois,
//! o próximo `sync` relê o trecho e o dedupe por `chave` descarta o que já entrou.

use super::agregar::{self, Custos};
use super::ledger::{self, Cursor, Linha, Trava};
use super::parser::{self, Avisos, CtxArquivo, Resposta};
use super::preco::Tabela;
use super::projeto::{self, Projeto};
use super::relatorio;
use super::tasks;
use super::tempo::{dia_e_semana, fmt_ts};
use chrono::{DateTime, TimeZone, Utc};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// O ambiente do `sync` — injetável pra teste (nada de `HOME` global).
pub struct Ambiente {
    /// `~/.claude/projects`.
    pub claude_projects: PathBuf,
    /// `~/.config/schematize/pricing.toml`.
    pub pricing_usuario: Option<PathBuf>,
    pub agora: DateTime<Utc>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Resultado {
    pub arquivos_lidos: usize,
    pub novas: usize,
    /// Respostas já no ledger que reapareceram com mais `output_tokens`.
    pub corrigidas: usize,
    pub avisos: Avisos,
}

/// Tabela de preços efetiva do projeto (+ avisos de override inválido).
pub fn tabela(p: &Projeto, usuario: Option<&Path>) -> (Tabela, Vec<String>) {
    Tabela::carregar(usuario, Some(&p.custos().pricing()))
}

/// Lê só os bytes novos de `abs` a partir de `offset`, devolvendo apenas LINHAS COMPLETAS
/// (até o último `\n`) e quantos bytes consumiu. Meia linha no fim fica pro próximo `sync`.
fn ler_novos(abs: &Path, offset: u64) -> Result<(String, u64), String> {
    let mut f = std::fs::File::open(abs).map_err(|e| format!("abrir {}: {e}", abs.display()))?;
    let tam = f.metadata().map_err(|e| format!("stat {}: {e}", abs.display()))?.len();
    // Arquivo menor que o offset = foi truncado/recriado: relê tudo (o dedupe protege).
    let ini = if tam < offset { 0 } else { offset };
    f.seek(SeekFrom::Start(ini)).map_err(|e| format!("seek {}: {e}", abs.display()))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).map_err(|e| format!("ler {}: {e}", abs.display()))?;
    let Some(fim) = buf.iter().rposition(|&b| b == b'\n') else {
        // nada completo ainda; se o arquivo encolheu, o novo offset é o início
        return Ok((String::new(), ini));
    };
    let usados = fim as u64 + 1;
    Ok((String::from_utf8_lossy(&buf[..fim]).into_owned(), ini + usados))
}

/// Sincroniza o ledger do projeto com os transcripts. `tz` define o "dia" (fuso local).
pub fn sincronizar<Tz: TimeZone>(p: &Projeto, env: &Ambiente, tz: &Tz) -> Result<Resultado, String>
where
    Tz::Offset: std::fmt::Display,
{
    let dir = p.custos();
    std::fs::create_dir_all(&dir.0).map_err(|e| format!("criar {}: {e}", dir.0.display()))?;
    let _trava = Trava::pegar(&dir.trava())?;

    let mut cursor = Cursor::carregar(&dir.cursor());
    let (existentes, _) = ledger::ler(&dir.ledger())?;
    let existentes = ledger::consolidar(existentes);
    // chave → (output_tokens, task) já gravados
    let conhecidas: HashMap<&str, (u64, &str)> =
        existentes.iter().map(|l| (l.chave.as_str(), (l.out, l.task.as_str()))).collect();

    let (tab, _) = tabela(p, env.pricing_usuario.as_deref());
    let eventos = tasks::ler_eventos(&dir.tasks());
    let overdev = tasks::item_ativo_overdev(&p.root);

    let mut res = Resultado::default();
    let mut lidas: Vec<Resposta> = Vec::new();
    let mut avisos = Avisos::default();
    let mut novos_offsets = Vec::new();
    for t in projeto::transcripts(&env.claude_projects, &p.root) {
        let off = cursor.arquivos.get(&t.rel).copied().unwrap_or(0);
        let (txt, novo) = ler_novos(&t.abs, off)?;
        let ctx = CtxArquivo { sessao: t.sessao.clone(), agente: t.agente.clone() };
        for l in txt.lines() {
            if let Some(r) = parser::ler_linha(l, &ctx, &mut avisos) {
                // Resposta de OUTRO projeto que caiu na mesma pasta por prefixo de slug.
                if r.cwd.as_deref().is_some_and(|c| !Path::new(c).starts_with(&p.root)) {
                    continue;
                }
                lidas.push(r);
            }
        }
        if novo != off {
            novos_offsets.push((t.rel.clone(), novo));
        }
        res.arquivos_lidos += 1;
    }

    let mut a_gravar: Vec<Linha> = Vec::new();
    for r in parser::dedupe(lidas) {
        let task = match conhecidas.get(r.chave.as_str()) {
            Some(&(out, _)) if r.tokens.out <= out => continue,
            Some(&(_, task)) => {
                res.corrigidas += 1;
                task.to_string()
            }
            None => {
                res.novas += 1;
                tasks::resolver(r.ts, &eventos, overdev.as_deref(), env.agora)
            }
        };
        let (dia, semana_iso) = dia_e_semana(r.ts, tz);
        a_gravar.push(Linha {
            usd: tab.custo(&r.modelo, &r.tokens),
            ts: fmt_ts(r.ts),
            dia,
            semana_iso,
            sessao: r.sessao,
            agente: r.agente,
            modelo: r.modelo,
            task,
            inp: r.tokens.inp,
            out: r.tokens.out,
            cache_w: r.tokens.cache_w(),
            cache_r: r.tokens.cache_r,
            chave: r.chave,
            cache_w1h: r.tokens.cache_w1h,
        });
    }
    a_gravar.sort_by(|a, b| a.ts.cmp(&b.ts));

    ledger::anexar(&dir.ledger(), &a_gravar)?;
    for (rel, off) in novos_offsets {
        cursor.arquivos.insert(rel, off);
    }
    cursor.avisos.somar(&avisos);
    cursor.salvar(&dir.cursor())?;
    res.avisos = avisos;
    Ok(res)
}

/// Carrega o ledger e monta o relatório (sem escrever nada).
pub fn calcular<Tz: TimeZone>(p: &Projeto, env: &Ambiente, tz: &Tz) -> Result<Custos, String>
where
    Tz::Offset: std::fmt::Display,
{
    let dir = p.custos();
    let (linhas, ruins) = ledger::ler(&dir.ledger())?;
    let linhas = ledger::consolidar(linhas);
    let (tab, mut extra) = tabela(p, env.pricing_usuario.as_deref());
    if ruins > 0 {
        extra.push(format!("{ruins} linha(s) corrompida(s) no ledger.jsonl ignorada(s)"));
    }
    let cursor = Cursor::carregar(&dir.cursor());
    let nomes = tasks::nomes(&tasks::ler_eventos(&dir.tasks()));
    let fuso = env.agora.with_timezone(tz).format("%:z").to_string();
    Ok(agregar::montar(agregar::Entrada {
        linhas: &linhas,
        tab: &tab,
        avisos_parse: &cursor.avisos,
        avisos_extra: extra,
        nomes_task: &nomes,
        projeto: &p.nome(),
        fuso: &fuso,
        gerado_em: &fmt_ts(env.agora),
    }))
}

/// Regenera `custos.json` e `custos.md` (atômico). Se NADA mudou além do `gerado_em`, não toca
/// nos arquivos — o hook/timer rodando de 30 em 30 min não pode sujar o git à toa.
pub fn relatar<Tz: TimeZone>(p: &Projeto, env: &Ambiente, tz: &Tz) -> Result<Custos, String>
where
    Tz::Offset: std::fmt::Display,
{
    let dir = p.custos();
    std::fs::create_dir_all(&dir.0).map_err(|e| format!("criar {}: {e}", dir.0.display()))?;
    let mut c = calcular(p, env, tz)?;
    let antigo = std::fs::read(dir.json()).ok();
    if let Some(prev) =
        antigo.as_deref().and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok())
    {
        if let Some(g) = prev.get("gerado_em").and_then(|g| g.as_str()) {
            let novo = c.gerado_em.clone();
            c.gerado_em = g.to_string();
            let igual = serde_json::to_vec_pretty(&c.json()).ok() == antigo && dir.md().is_file();
            if igual {
                return Ok(c);
            }
            c.gerado_em = novo;
        }
    }
    let j = serde_json::to_vec_pretty(&c.json()).map_err(|e| format!("custos.json: {e}"))?;
    ledger::escrever_atomico(&dir.json(), &j)?;
    ledger::escrever_atomico(&dir.md(), relatorio::markdown(&c).as_bytes())?;
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::parser::fixtures::assistant;
    use chrono::FixedOffset;

    /// Mundo sintético: `~/.claude/projects` + um projeto com archive.
    struct Mundo {
        _d: tempfile::TempDir,
        env: Ambiente,
        proj: Projeto,
        pasta: PathBuf,
    }

    fn mundo() -> Mundo {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("work/proj");
        std::fs::create_dir_all(root.join("proj_archive")).unwrap();
        let cp = d.path().join("claude/projects");
        let pasta = cp.join(projeto::slug(&root));
        std::fs::create_dir_all(&pasta).unwrap();
        let proj = Projeto { root: root.clone(), archive: root.join("proj_archive") };
        let env = Ambiente {
            claude_projects: cp,
            pricing_usuario: None,
            agora: crate::cost::tempo::parse_ts("2026-10-31T20:00:00Z").unwrap(),
        };
        Mundo { _d: d, env, proj, pasta }
    }

    /// Como `assistant`, mas com o `cwd` do mundo.
    fn linha(m: &Mundo, id: &str, model: &str, ts: &str, out: u64) -> String {
        assistant(id, model, ts, 10, out, 100, 1000)
            .replace("/work/proj", m.proj.root.to_str().unwrap())
    }

    fn brt() -> FixedOffset {
        FixedOffset::west_opt(3 * 3600).unwrap()
    }

    fn escrever(m: &Mundo, nome: &str, linhas: &[String]) {
        let mut s = linhas.join("\n");
        s.push('\n');
        std::fs::write(m.pasta.join(nome), s).unwrap();
    }

    fn ledger_de(m: &Mundo) -> Vec<Linha> {
        ledger::consolidar(ledger::ler(&m.proj.custos().ledger()).unwrap().0)
    }

    #[test]
    fn dedupe_streaming_e_precificacao() {
        let m = mundo();
        let a = |o| linha(&m, "m1", "claude-opus-5-5", "2026-10-31T10:00:00.000Z", o);
        escrever(
            &m,
            "s1.jsonl",
            &[
                a(5),
                a(50),
                a(20),
                linha(&m, "m2", "claude-sonnet-5-5", "2026-10-31T10:05:00.000Z", 7),
            ],
        );
        let r = sincronizar(&m.proj, &m.env, &brt()).unwrap();
        assert_eq!(r.novas, 2);
        let l = ledger_de(&m);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].out, 50);
        // opus-5-5: 10*4 + 50*20 + 100*5 + 1000*0.2 = 1740 / 1e6
        assert!((l[0].usd.unwrap() - 0.001740).abs() < 1e-12);
        assert_eq!(l[0].dia, "2026-10-31");
        assert_eq!(l[0].task, "sem-task");
    }

    #[test]
    fn sync_duas_vezes_gera_o_mesmo_ledger() {
        let m = mundo();
        escrever(&m, "s1.jsonl", &[linha(&m, "m1", "claude-opus-5-5", "2026-10-31T10:00:00Z", 5)]);
        sincronizar(&m.proj, &m.env, &brt()).unwrap();
        let antes = std::fs::read(m.proj.custos().ledger()).unwrap();
        let r2 = sincronizar(&m.proj, &m.env, &brt()).unwrap();
        assert_eq!((r2.novas, r2.corrigidas), (0, 0));
        assert_eq!(std::fs::read(m.proj.custos().ledger()).unwrap(), antes);
        // mesmo SEM cursor (apagado), o dedupe por chave segura
        std::fs::remove_file(m.proj.custos().cursor()).unwrap();
        sincronizar(&m.proj, &m.env, &brt()).unwrap();
        assert_eq!(std::fs::read(m.proj.custos().ledger()).unwrap(), antes);
    }

    #[test]
    fn incremental_le_so_o_novo_e_ignora_meia_linha() {
        let m = mundo();
        let p = m.pasta.join("s1.jsonl");
        let l1 = linha(&m, "m1", "claude-opus-5-5", "2026-10-31T10:00:00Z", 5);
        let l2 = linha(&m, "m2", "claude-opus-5-5", "2026-10-31T10:01:00Z", 6);
        // l2 ainda sem \n final (escrita em curso)
        std::fs::write(&p, format!("{l1}\n{l2}")).unwrap();
        assert_eq!(sincronizar(&m.proj, &m.env, &brt()).unwrap().novas, 1);
        std::fs::write(&p, format!("{l1}\n{l2}\n")).unwrap();
        assert_eq!(sincronizar(&m.proj, &m.env, &brt()).unwrap().novas, 1);
        assert_eq!(ledger_de(&m).len(), 2);
    }

    #[test]
    fn resposta_que_cresce_entre_syncs_vira_correcao() {
        let m = mundo();
        escrever(&m, "s1.jsonl", &[linha(&m, "m1", "claude-opus-5-5", "2026-10-31T10:00:00Z", 5)]);
        sincronizar(&m.proj, &m.env, &brt()).unwrap();
        let mut f =
            std::fs::OpenOptions::new().append(true).open(m.pasta.join("s1.jsonl")).unwrap();
        use std::io::Write;
        writeln!(f, "{}", linha(&m, "m1", "claude-opus-5-5", "2026-10-31T10:00:00Z", 90)).unwrap();
        let r = sincronizar(&m.proj, &m.env, &brt()).unwrap();
        assert_eq!((r.novas, r.corrigidas), (0, 1));
        let l = ledger_de(&m);
        assert_eq!((l.len(), l[0].out), (1, 90));
        // o arquivo cresceu (append-only), a leitura lógica não duplica
        assert_eq!(ledger::ler(&m.proj.custos().ledger()).unwrap().0.len(), 2);
    }

    #[test]
    fn modelo_sem_preco_fica_null() {
        let m = mundo();
        escrever(
            &m,
            "s1.jsonl",
            &[linha(&m, "m1", "claude-misterio-9", "2026-10-31T10:00:00Z", 5)],
        );
        sincronizar(&m.proj, &m.env, &brt()).unwrap();
        assert_eq!(ledger_de(&m)[0].usd, None);
    }

    #[test]
    fn linha_corrompida_no_meio_nao_perde_o_resto() {
        let m = mundo();
        escrever(
            &m,
            "s1.jsonl",
            &[
                linha(&m, "m1", "claude-opus-5-5", "2026-10-31T10:00:00Z", 5),
                "{isto nao e json".into(),
                linha(&m, "m2", "claude-opus-5-5", "2026-10-31T10:01:00Z", 6),
            ],
        );
        let r = sincronizar(&m.proj, &m.env, &brt()).unwrap();
        assert_eq!(r.novas, 2);
        assert_eq!(r.avisos.linhas_invalidas, 1);
        assert_eq!(Cursor::carregar(&m.proj.custos().cursor()).avisos.linhas_invalidas, 1);
    }

    #[test]
    fn subagent_vai_pra_sessao_do_pai_e_tem_agente() {
        let m = mundo();
        escrever(&m, "pai.jsonl", &[linha(&m, "m1", "claude-opus-5-5", "2026-10-31T10:00:00Z", 5)]);
        let sub = m.pasta.join("pai/subagents");
        std::fs::create_dir_all(&sub).unwrap();
        // a linha do subagent não traz sessionId: vem do caminho
        let l = linha(&m, "m9", "claude-sonnet-5-5", "2026-10-31T10:02:00Z", 8)
            .replace(r#""sessionId":"sess-1","#, "");
        std::fs::write(sub.join("agent-ab12.jsonl"), format!("{l}\n")).unwrap();
        sincronizar(&m.proj, &m.env, &brt()).unwrap();
        let l = ledger_de(&m);
        let s = l.iter().find(|x| x.chave == "m9").unwrap();
        assert_eq!((s.sessao.as_str(), s.agente.as_deref()), ("pai", Some("ab12")));
    }

    #[test]
    fn resposta_de_outro_projeto_na_pasta_de_prefixo_e_ignorada() {
        let m = mundo();
        let outra = linha(&m, "m1", "claude-opus-5-5", "2026-10-31T10:00:00Z", 5)
            .replace(m.proj.root.to_str().unwrap(), "/outro/lugar");
        escrever(&m, "s1.jsonl", &[outra]);
        assert_eq!(sincronizar(&m.proj, &m.env, &brt()).unwrap().novas, 0);
    }

    #[test]
    fn task_vem_da_tag_ativa_na_hora_da_resposta() {
        let m = mundo();
        let dir = m.proj.custos();
        std::fs::create_dir_all(&dir.0).unwrap();
        tasks::registrar(
            &dir.tasks(),
            crate::cost::tempo::parse_ts("2026-10-31T09:00:00Z").unwrap(),
            "T7",
            None,
            "start",
        )
        .unwrap();
        tasks::registrar(
            &dir.tasks(),
            crate::cost::tempo::parse_ts("2026-10-31T10:30:00Z").unwrap(),
            "T7",
            None,
            "end",
        )
        .unwrap();
        escrever(
            &m,
            "s1.jsonl",
            &[
                linha(&m, "m1", "claude-opus-5-5", "2026-10-31T10:00:00Z", 5),
                linha(&m, "m2", "claude-opus-5-5", "2026-10-31T11:00:00Z", 5),
            ],
        );
        sincronizar(&m.proj, &m.env, &brt()).unwrap();
        let l = ledger_de(&m);
        assert_eq!((l[0].task.as_str(), l[1].task.as_str()), ("T7", "sem-task"));
    }

    #[test]
    fn relatar_gera_arquivos_e_nao_regrava_sem_mudanca() {
        let mut m = mundo();
        escrever(&m, "s1.jsonl", &[linha(&m, "m1", "claude-opus-5-5", "2026-10-31T10:00:00Z", 5)]);
        sincronizar(&m.proj, &m.env, &brt()).unwrap();
        let c = relatar(&m.proj, &m.env, &brt()).unwrap();
        assert_eq!(c.total.respostas, 1);
        let dir = m.proj.custos();
        let j1 = std::fs::read(dir.json()).unwrap();
        let md1 = std::fs::read_to_string(dir.md()).unwrap();
        assert!(md1.contains("estimativa API-equivalente"));
        // relógio andou, dados iguais: arquivos intactos
        m.env.agora = crate::cost::tempo::parse_ts("2026-10-31T21:00:00Z").unwrap();
        relatar(&m.proj, &m.env, &brt()).unwrap();
        assert_eq!(std::fs::read(dir.json()).unwrap(), j1);
        // dado novo: regrava
        escrever(
            &m,
            "s1.jsonl",
            &[
                linha(&m, "m1", "claude-opus-5-5", "2026-10-31T10:00:00Z", 5),
                linha(&m, "m2", "claude-opus-5-5", "2026-10-31T10:00:01Z", 5),
            ],
        );
        sincronizar(&m.proj, &m.env, &brt()).unwrap();
        relatar(&m.proj, &m.env, &brt()).unwrap();
        assert_ne!(std::fs::read(dir.json()).unwrap(), j1);
    }
}
