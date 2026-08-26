//! schematize — gerenciador do ecossistema para o Claude (skills, overdev e mais).
//! O quê: CLI multi-idioma que instala/versiona skills, roda o overdev, diagnostica
//! o ambiente (doctor), atualiza a si mesmo (upgrade), mostra status e o blog.
//! Onde: ponto de entrada; despacha pros módulos da lib `schematize`.

// Subcomandos por área (piso da casa: <=750 linhas, uma unidade lógica por arquivo).
// `main` guarda só o arranque e o despacho.
mod cli;
use cli::args::*;
use cli::conta::*;
use cli::caixa::*;
use cli::db::*;
use cli::disco::*;
use cli::git::*;
use cli::diversos::*;
use cli::overdev::*;
use cli::skills::*;
use cli::ssh::*;

use clap::Parser;
use schematize::i18n::tf;
use schematize::{
    agent, autostart, doctor, environments, links, news, overdev, panel, status, upgrade, util,
};















































/// Um alvo de item humano digitado na linha de comando.
///
/// Número puro = a POSIÇÃO entre os humanos abertos (é o que a UI numera, e o que a
/// pessoa tem à mão ao olhar a lista). Qualquer outra coisa = trecho do texto. Aceitar
/// os dois evita obrigar alguém a copiar e colar um item inteiro pra respondê-lo.
fn alvo_humano(s: &str) -> schematize::overdev::resposta::Alvo {
    use schematize::overdev::resposta::Alvo;
    match s.trim().parse::<usize>() {
        Ok(n) if n > 0 => Alvo::Indice(n),
        _ => Alvo::Texto(s.trim().to_string()),
    }
}

fn main() {
    let cli = Cli::parse();
    let r: Result<(), String> = match cli.cmd {
        // Feature SKILLS, agrupada. O app é uma coisa; skills são uma funcionalidade.
        Cmd::Skills { sub } => skills_cmd(sub),
        // Aliases ocultos de compat (mesma lógica que o subcomando `skills`).
        Cmd::Install { names, all, with_recommended } => {
            skills_install(&names, all, with_recommended)
        }
        Cmd::Update { names, all } => skills_update(&names, all),
        Cmd::List => skills_list(),
        Cmd::Remove { name } => skills_remove(&name),
        Cmd::Status => {
            status::run();
            Ok(())
        }
        Cmd::Agents { json, split } => agents_cmd(json, split),
        Cmd::Diagnostics { yes } => schematize::diagnostics::send(yes),
        Cmd::Icon { emit, size, hicolor } => {
            let mut r: Result<(), String> = Ok(());
            if let Some(dir) = hicolor {
                r = schematize::appicon::write_hicolor(std::path::Path::new(&dir))
                    .map(|paths| paths.iter().for_each(|p| println!("{}", p.display())))
                    .map_err(|e| e.to_string());
            }
            if r.is_ok() {
                if let Some(path) = emit {
                    r = schematize::appicon::write_png(std::path::Path::new(&path), size)
                        .map(|_| println!("{path}"))
                        .map_err(|e| e.to_string());
                }
            }
            r
        }
        Cmd::Disco { sub } => disco_cmd(sub),
        Cmd::Git { sub } => git_cmd(sub),
        Cmd::Doctor { fix } => {
            doctor::run(fix);
            Ok(())
        }
        Cmd::Debug { collect, out, stdout, online } => debug_cmd(collect, out, stdout, online),
        Cmd::Upgrade { force } => upgrade::run(force),
        Cmd::News => {
            news::show();
            Ok(())
        }
        Cmd::Notifications { sync, historico, lidas, concluir } => match concluir {
            Some(id) => notifications_concluir(&id),
            None if lidas => {
                notifications_lidas();
                Ok(())
            }
            None => {
                notifications_cmd(sync, historico);
                Ok(())
            }
        }
        Cmd::Blog => links::open("blog"),
        Cmd::Open { target } => links::open(&target),
        Cmd::Lang { code, list } => lang_cmd(code, list),
        Cmd::Overdev { sub } => match sub {
            Over::Enable => overdev::enable(),
            Over::Disable => overdev::disable(),
            Over::Start { objetivo, max } => overdev::start(&objetivo.join(" "), max),
            Over::Terminal => cli::overdev::cwd_project().and_then(|p| {
                schematize::agentrun::abrir_terminal_no_projeto(&p)
                    .map(|t| println!("terminal `{t}` aberto em {} — o claude sobe com o bypass ligado.", p.display()))
            }),
            Over::Supervise { max } => cli::overdev::cwd_project().map(|projeto| {
                let teto = max.unwrap_or(schematize::overdev::supervisor::MAX_RELANCAMENTOS);
                println!(
                    "Supervisionando {} — relanço o agente se ele morrer com item de máquina aberto (teto {teto}).",
                    projeto.display()
                );
                let n = schematize::overdev::supervisor::supervise(&projeto, teto);
                println!("supervisor encerrado após {n} relançamento(s).");
            }),
            Over::Split { k, dispatch, force } => overdev_split(k, dispatch, force),
            Over::Check => {
                overdev::check();
                Ok(())
            }
            Over::Guard => {
                overdev::guard();
                Ok(())
            }
            Over::Status => {
                overdev::status();
                Ok(())
            }
            Over::Hold { texto } => overdev::hold(&texto.join(" ")),
            Over::Park { item, pergunta } => overdev::park(&item, &pergunta.join(" ")),
            Over::Human { texto, done } => {
                let t = texto.join(" ");
                let sub = if t.trim().is_empty() { None } else { Some(t.as_str()) };
                overdev::human_done(sub, done)
            }
            Over::Note { texto, kind } => overdev::note(&kind, &texto.join(" ")),
            Over::Answer { alvo, texto } => {
                overdev::resolver(alvo_humano(&alvo), overdev::resposta::Acao::Responder, &texto.join(" "))
            }
            Over::Refuse { alvo, texto } => {
                overdev::resolver(alvo_humano(&alvo), overdev::resposta::Acao::Recusar, &texto.join(" "))
            }
            Over::Add { texto } => caixa_add(&texto.join(" ")),
            Over::Caixa { sub } => caixa_cmd(sub),
            Over::Stop => overdev::stop(),
            Over::Run { max, yes } => overdev_run(max, yes),
            Over::Snapshot => overdev_snapshot(),
            Over::History { limit } => overdev_history(limit),
            Over::Restore { id } => overdev_restore(id),
            Over::Load => overdev_agent_cmd(overdev::load_cmd()),
            Over::Index => overdev_agent_cmd(overdev::index_cmd()),
            Over::Log => {
                overdev_log();
                Ok(())
            }
        },
        Cmd::GitLog { limit } => {
            git_log(limit);
            Ok(())
        }
        Cmd::Panel => panel::open(),
        Cmd::Graph { sub } => match sub {
            GraphCmd::Obsidian { out } => panel::export_obsidian(out),
        },
        Cmd::Db { sub } => db_cmd(sub),
        Cmd::Check { notify } => {
            agent::run_once(notify);
            Ok(())
        }
        Cmd::Agent => {
            // O agente é o processo LONGO da máquina (autostart): é o lugar certo
            // pra garantir o gestor de atualizações sem o usuário pedir. Sai na
            // hora se já estiver instalado (só um stat, sem rede).
            schematize::updaterboot::ensure_in_background();
            // Auto-cura dos hooks do overdev: quem ligou numa versão antiga carrega o
            // comando daquela versão no settings.json, e atualizar o app não regravava.
            // No-op se o overdev está desligado ou o comando já é o atual.
            // Auto-cura só do settings do USUÁRIO. O do projeto é reparado pelo
            // `doctor` — escrever no repo de alguém sem pedido explícito não é papel
            // de um daemon que subiu no login.
            let exe = util::self_exe();
            let _ = schematize::settings::refresh_hooks(&exe);
            let _ = schematize::settings::repara_hooks_em(&util::settings_path(), &exe);
            agent::run_loop();
            Ok(())
        }
        Cmd::Autostart { sub } => match sub {
            Auto::Enable => autostart::enable(&util::self_exe()),
            Auto::Disable => autostart::disable(),
        },
        Cmd::Env { sub } => match sub {
            EnvCmd::List => {
                environments::list();
                Ok(())
            }
            EnvCmd::Install { lang, method, dry_run, yes } => {
                environments::install(&lang, method, dry_run, yes)
            }
            EnvCmd::Remove { lang, method, dry_run } => {
                environments::remove(&lang, method, dry_run)
            }
        },
        Cmd::Gui => {
            // Mesma aplicação, outra face. A face gráfica DEFAULT é o binário
            // `schematize-gui` (Slint), instalado à parte pelo install.sh; executa-o.
            // Se ele não estiver no PATH (ex.: build do Slint falhou), cai na GUI egui
            // EMBUTIDA (fallback) — a virada é segura: nunca fica sem janela.
            match std::process::Command::new("schematize-gui").status() {
                Ok(st) if st.success() => Ok(()),
                Ok(st) => Err(format!("schematize-gui saiu com {st}")),
                // Sem fallback: a GUI é UMA só (Slint, binário `schematize-gui`).
                // Existia aqui uma segunda GUI (egui) como rede de segurança, e era
                // justamente ela que aparecia quando o PATH resolvia pro pacote em vez
                // do fonte — o "abre a versão antiga". Melhor uma mensagem clara do que
                // uma janela diferente da que o usuário espera.
                Err(_) => Err(
                    "não achei o `schematize-gui` no PATH. Rode `schematize doctor` \
                     (ele diagnostica e conserta) ou reinstale pelo install.sh."
                        .to_string(),
                ),
            }
        }
        Cmd::Archive => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            schematize::archive::sync(&cwd).map(|msg| println!("{msg}"))
        }
        Cmd::Ssh { sub } => ssh_cmd(sub),
        Cmd::Projects { sub } => projects_cmd(sub),
        Cmd::Login => login_cmd(),
        Cmd::Logout => {
            logout_cmd();
            Ok(())
        }
        Cmd::Whoami => {
            whoami_cmd();
            Ok(())
        }
    };
    if let Err(e) = r {
        eprintln!("{}", tf("err.prefix", &[("e", &e)]));
        std::process::exit(1);
    }
}
