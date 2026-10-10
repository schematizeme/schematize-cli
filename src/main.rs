//! schematize — gerenciador do ecossistema para o Claude (skills, overdev e mais).
//! O quê: CLI multi-idioma que instala/versiona skills, roda o overdev, diagnostica
//! o ambiente (doctor), atualiza a si mesmo (upgrade), mostra status e o blog.
//! Onde: ponto de entrada; despacha pros módulos da lib `schematize`.

// Subcomandos por área (piso da casa: <=750 linhas, uma unidade lógica por arquivo).
// `main` guarda só o arranque e o despacho.
mod cli;
use cli::args::*;
use cli::conta::*;
use cli::disco::*;
use cli::diversos::*;

use clap::Parser;
use schematize::i18n::tf;
use schematize::overdevlink;
use schematize::{agent, autostart, doctor, links, news, status, upgrade, util};

// O `alvo_humano` foi com o domínio (E6): ele interpreta o alvo de um item HUMANO, e quem
// resolve item humano é o `schematize-overdev`. Lá ele ganhou teste — aqui vivia no meio do
// despacho, sem nenhum.

fn main() {
    // ANTES do clap: `ssh`, `vps`, `mcp` e `env` são dos apps da casa, e os argumentos vão
    // CRUS para eles. Se o hub parseasse, precisaria conhecer cada flag dos apps — e foi
    // exatamente essa sincronia que falhou duas vezes num dia só (`--method distro` do csharp
    // e `--paste` do ssh import, ambos existindo no app e não aqui). Ver `applink`.
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = schematize::applink::interceptar(&argv) {
        std::process::exit(code);
    }
    let cli = Cli::parse();
    let r: Result<(), String> = match cli.cmd {
        // O braço de SKILLS saiu daqui na E5: `skills`, `install`, `update`, `list` e `remove`
        // são interceptados pelo `applink` antes do clap e vão para o `schematize-skills`.
        Cmd::Status => {
            status::run();
            Ok(())
        }
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
        },
        Cmd::Blog => links::open("blog"),
        Cmd::Open { target } => links::open(&target),
        Cmd::Lang { code, list } => lang_cmd(code, list),
        // O braço do OVERDEV saiu daqui na E6 — 71 linhas de despacho. `schematize overdev …`
        // é interceptado pelo `applink` antes do clap, e com ele os HOOKS do Claude Code.
        Cmd::Check { notify } => {
            agent::run_once(notify);
            Ok(())
        }
        Cmd::Agent => {
            // O agente é o processo LONGO da máquina (autostart): é o lugar certo
            // pra garantir o gestor de atualizações sem o usuário pedir. Sai na
            // hora se já estiver instalado (só um stat, sem rede).
            schematize::gestorboot::ensure_in_background();
            // Auto-cura dos hooks do overdev: quem ligou numa versão antiga carrega o comando
            // daquela versão no `settings.json`, e atualizar o app não regravava.
            //
            // **Quem regrava agora é o app do overdev** (E6), e este daemon apenas PEDE. A
            // chamada é best-effort: sem o app instalado, não há hook para curar — e um
            // daemon que falha no login por causa de um app opcional é pior que um hook velho.
            if let Err(e) = overdevlink::reparar_hooks() {
                eprintln!("aviso: não reparei os hooks do overdev: {e}");
            }
            agent::run_loop();
            Ok(())
        }
        Cmd::Autostart { sub } => match sub {
            Auto::Enable => autostart::enable(&util::self_exe()),
            Auto::Disable => autostart::disable(),
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
                Err(_) => Err("não achei o `schematize-gui` no PATH. Rode `schematize doctor` \
                     (ele diagnostica e conserta) ou reinstale pelo install.sh."
                    .to_string()),
            }
        }
        Cmd::Apps { sub } => match sub {
            None => crate::cli::deployer::apps_cmd(),
            Some(crate::cli::args::AppsCmd::Install { app, yes }) => {
                crate::cli::deployer::apps_instalar(app, yes)
            }
            Some(crate::cli::args::AppsCmd::Exec { app, args }) => {
                crate::cli::deployer::apps_exec(app, args)
            }
        },
        Cmd::Deployer { sub } => crate::cli::deployer::deployer_cmd(sub),
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
