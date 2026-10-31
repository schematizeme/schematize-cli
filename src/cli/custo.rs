//! `schematize cost`: despacho dos subcomandos sobre o módulo `schematize::cost`.

use crate::cli::args::custo::{CostCmd, Eixo};
use chrono::{Local, Utc};
use schematize::cost::agregar::{Custos, Item};
use schematize::cost::projeto::{self, Projeto};
use schematize::cost::relatorio::{markdown, milhar, usd};
use schematize::cost::sync::{self, Ambiente};
use schematize::cost::{ledger, tasks, AVISO_ESTIMATIVA};
use schematize::{config, projects, util};

/// Projetos que o app conhece (pra `--projeto <nome>`).
fn catalogo() -> Vec<(String, std::path::PathBuf)> {
    projects::scan_with_pins(&config::dev_dirs(), &config::projects())
        .into_iter()
        .map(|p| (p.name, std::path::PathBuf::from(p.path)))
        .collect()
}

fn projeto_de(arg: Option<&str>) -> Result<Projeto, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cwd: {e}"))?;
    projeto::resolver(arg, &cwd, &catalogo())
}

fn ambiente() -> Ambiente {
    Ambiente {
        claude_projects: util::claude_dir().join("projects"),
        pricing_usuario: Some(util::home().join(".config/schematize/pricing.toml")),
        agora: Utc::now(),
    }
}

fn sincronizar(p: &Projeto) -> Result<sync::Resultado, String> {
    sync::sincronizar(p, &ambiente(), &Local)
}

pub(crate) fn cost_cmd(sub: CostCmd) -> Result<(), String> {
    match sub {
        CostCmd::Sync { projeto } => {
            let p = projeto_de(projeto.as_deref())?;
            let r = sincronizar(&p)?;
            println!(
                "{}: {} nova(s), {} corrigida(s), {} transcript(s) vistos",
                p.nome(),
                r.novas,
                r.corrigidas,
                r.arquivos_lidos
            );
            for f in r.avisos.frases() {
                eprintln!("aviso: {f}");
            }
            Ok(())
        }
        CostCmd::Report { projeto, json, md } => {
            let p = projeto_de(projeto.as_deref())?;
            let c = sync::relatar(&p, &ambiente(), &Local)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&c.json()).map_err(|e| e.to_string())?);
            } else if md {
                print!("{}", markdown(&c));
            } else {
                println!(
                    "{}: {} · {} tokens → {}",
                    p.nome(),
                    usd(c.total.usd()),
                    milhar(c.total.total()),
                    p.custos().0.display()
                );
                eprintln!("({AVISO_ESTIMATIVA})");
            }
            Ok(())
        }
        CostCmd::Show { projeto, por } => {
            let p = projeto_de(projeto.as_deref())?;
            let c = sync::calcular(&p, &ambiente(), &Local)?;
            mostrar(&c, por);
            Ok(())
        }
        CostCmd::Tag { task_id, nome, projeto } => {
            let p = projeto_de(projeto.as_deref())?;
            // Fecha a contabilidade ANTES da virada: o uso já feito fica com a task antiga.
            sincronizar(&p)?;
            let dir = p.custos();
            std::fs::create_dir_all(&dir.0)
                .map_err(|e| format!("criar {}: {e}", dir.0.display()))?;
            let _trava = ledger::Trava::pegar(&dir.trava())?;
            let agora = Utc::now();
            if let Some(a) = tasks::tag_aberta(&tasks::ler_eventos(&dir.tasks())) {
                tasks::registrar(&dir.tasks(), agora, &a.task_id.clone(), None, "end")?;
            }
            tasks::registrar(&dir.tasks(), agora, &task_id, nome.as_deref(), "start")?;
            println!("{}: tag `{task_id}` aberta", p.nome());
            Ok(())
        }
        CostCmd::Untag { projeto } => {
            let p = projeto_de(projeto.as_deref())?;
            sincronizar(&p)?;
            let dir = p.custos();
            let _trava = ledger::Trava::pegar(&dir.trava())?;
            match tasks::tag_aberta(&tasks::ler_eventos(&dir.tasks())) {
                Some(a) => {
                    tasks::registrar(&dir.tasks(), Utc::now(), &a.task_id.clone(), None, "end")?;
                    println!("{}: tag `{}` fechada", p.nome(), a.task_id);
                }
                None => println!("{}: nenhuma tag aberta", p.nome()),
            }
            Ok(())
        }
        CostCmd::Pricing { check, projeto } => pricing(check, projeto.as_deref()),
    }
}

fn mostrar(c: &Custos, por: Eixo) {
    let (titulo, itens): (&str, &[Item]) = match por {
        Eixo::Modelo => ("modelo", &c.por_modelo),
        Eixo::Dia => ("dia", &c.por_dia),
        Eixo::Semana => ("semana", &c.por_semana),
        Eixo::Task => ("task", &c.por_task),
        Eixo::Sessao => ("sessao", &c.por_sessao),
        Eixo::Agente => ("agente", &c.por_agente),
    };
    println!("{:<44} {:>12} {:>16}", titulo, "US$", "tokens");
    for it in itens {
        println!("{:<44} {:>12} {:>16}", it.chave, usd(it.tot.usd()), milhar(it.tot.total()));
    }
    println!("{:<44} {:>12} {:>16}", "TOTAL", usd(c.total.usd()), milhar(c.total.total()));
    println!("\n{AVISO_ESTIMATIVA}");
    for a in &c.avisos {
        eprintln!("aviso: {a}");
    }
}

fn pricing(check: bool, projeto: Option<&str>) -> Result<(), String> {
    let env = ambiente();
    // `pricing` sem projeto resolvível ainda mostra o embutido + usuário.
    let p = projeto_de(projeto).ok();
    let (tab, mut avisos) = match &p {
        Some(p) => sync::tabela(p, env.pricing_usuario.as_deref()),
        None => schematize::cost::preco::Tabela::carregar(env.pricing_usuario.as_deref(), None),
    };
    println!("pricing_versao: {}", tab.versao);
    println!(
        "{:<22} {:>7} {:>7} {:>8} {:>8} {:>7}  {:<9} confirmado",
        "modelo", "input", "output", "cache5m", "cache1h", "read", "origem"
    );
    for (m, pr, o) in tab.modelos() {
        println!(
            "{:<22} {:>7} {:>7} {:>8} {:>8} {:>7}  {:<9} {}",
            m,
            pr.input,
            pr.output,
            pr.cache_write_5m,
            pr.cache_write_1h,
            pr.cache_read,
            o.nome(),
            pr.confirmado
        );
    }
    println!("\nUS$ por 1M de tokens. {AVISO_ESTIMATIVA}.");
    if !check {
        for a in &avisos {
            eprintln!("aviso: {a}");
        }
        return Ok(());
    }
    avisos.extend(tab.problemas());
    if let Some(p) = &p {
        let (linhas, _) = ledger::ler(&p.custos().ledger())?;
        let mut sem: Vec<&str> = linhas
            .iter()
            .filter(|l| tab.buscar(&l.modelo).is_none())
            .map(|l| l.modelo.as_str())
            .collect();
        sem.sort_unstable();
        sem.dedup();
        avisos.extend(sem.into_iter().map(|m| format!("modelo no ledger sem preço: {m}")));
    }
    if avisos.is_empty() {
        println!("check: ok");
        Ok(())
    } else {
        for a in &avisos {
            eprintln!("problema: {a}");
        }
        Err(format!("{} problema(s) no pricing", avisos.len()))
    }
}
