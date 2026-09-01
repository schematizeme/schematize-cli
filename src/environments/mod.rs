//! Engine de ENVIRONMENTS — instala na máquina do usuário o runtime da linguagem +
//! ferramentas comuns, por 4 métodos que o usuário escolhe (docker | mise | distro | official).
//! O quê: lógica COMPARTILHADA (o CLI usa via `schematize env`; a GUI consumirá depois).
//! Onde: `defs` traz os dados/planos puros, `detect` sonda a máquina, e aqui mora a
//! orquestração: tabela, consentimento, dry-run e execução.
//!
//! SEGURANÇA (piso): nunca executa nada sem MOSTRAR o comando exato + procedência e obter
//! CONSENTIMENTO (ou --yes). `--dry-run` imprime tudo e não executa. Idempotente.

pub mod defs;
pub mod detect;

use crate::i18n::{t, tf};
use crate::util;
use defs::{Env, Recipe, Step, Tool};
use detect::Family;
use std::io::{self, BufRead, Write};

// Submódulos (piso da casa: <=750 linhas, uma unidade lógica por arquivo).
mod acoes;
mod estado;
mod maquina;
mod path;
mod plano;
pub use acoes::*;
pub use estado::*;
use maquina::*;
pub use path::*;
pub use plano::*;

// Re-exporta `Method` no nível do módulo (traz pra escopo interno E expõe como
// `environments::Method` pra consumidores externos, ex.: a GUI).
pub use defs::Method;

// ---------------------------------------------------------------------------
// API de DADOS pública (aditiva): o status estruturado de cada environment nesta
// máquina — pra a GUI consumir SEM parsear texto. O `list()` também passa a
// consumir isto (fonte única; nada de duplicar a detecção). O egui não usa.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// FERRAMENTAS: instalação/remoção. Reusa o MESMO guardrail (mostra o comando,
// pede consentimento, respeita dry-run) e o mesmo runner das linguagens. Sem
// seletor de método: se vier `--method`, avisa e ignora.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// PRONTIDÃO PÓS-INSTALAÇÃO. Muitos instaladores oficiais (Claude Code, e qualquer
// coisa via curl|sh) jogam o binário em ~/.local/bin — que NEM sempre está no PATH
// do usuário. Sem isso, o dev instala e o comando "não abre" nem reabrindo o
// terminal. Aqui a instalação vira SELF-VERIFYING: reconfere o bin no PATH e, se
// faltar, garante ~/.local/bin no PATH (~/.bashrc + ~/.profile, idempotente) e
// orienta como recarregar. Best-effort: NUNCA quebra a instalação já concluída.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Toda combinação lang × método resolve sem panic e ou tem passos, ou traz um
    /// motivo (Todo/Na) NÃO-vazio — nada de combinação silenciosamente vazia.
    #[test]
    fn matriz_lang_metodo_completa() {
        for env in defs::ENVS {
            for method in Method::ALL {
                for fam in [Family::Debian, Family::Rpm, Family::Unknown] {
                    let r = defs::install_recipe(env, method, fam, true);
                    match r {
                        Recipe::Steps(s) => {
                            assert!(!s.is_empty(), "{} {:?}: passos vazios", env.lang, method)
                        }
                        Recipe::Todo(n) | Recipe::Na(n) => {
                            assert!(!n.trim().is_empty(), "{} {:?}: motivo vazio", env.lang, method)
                        }
                    }
                    // remoção idem
                    let _ = defs::remove_recipe(env, method, fam);
                }
            }
        }
    }

    /// Os 7 environments existem e cada um tem runtime + ao menos 1 ferramenta.
    #[test]
    fn tabela_sete_envs() {
        let langs = ["go", "rust", "elixir", "csharp", "zig", "ruby", "node"];
        assert_eq!(defs::ENVS.len(), 7);
        for l in langs {
            let e = defs::find(l).expect("env presente");
            assert!(!e.runtime.is_empty());
            assert!(!e.tools.is_empty());
            assert!(!e.bin.is_empty());
        }
    }

    /// Detecção de família por os-release falso (parse puro).
    #[test]
    fn familia_por_os_release_falso() {
        let ubuntu = "ID=ubuntu\nID_LIKE=debian\n";
        let mint = "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n";
        let suse = "ID=\"opensuse-leap\"\nID_LIKE=\"suse opensuse\"\n";
        let fedora = "ID=fedora\n";
        let arch = "ID=arch\nID_LIKE=\n";
        assert_eq!(detect::family_from(ubuntu), Family::Debian);
        assert_eq!(detect::family_from(mint), Family::Debian);
        assert_eq!(detect::family_from(suse), Family::Rpm);
        assert_eq!(detect::family_from(fedora), Family::Rpm);
        assert_eq!(detect::family_from(arch), Family::Unknown);
        assert_eq!(detect::family_from(""), Family::Unknown);
    }

    /// dry-run NUNCA chama o runner (não executa nada).
    #[test]
    fn dry_run_nao_executa() {
        let steps =
            vec![Step { cmd: "echo x".into(), source: "t".into(), sudo: false, pipe_sh: false }];
        let mut calls = 0;
        let action = run_steps(&steps, true, true, |_| {
            calls += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(action, PlanAction::DryRun);
        assert_eq!(calls, 0, "dry-run não pode executar passos");
    }

    /// Sem consentimento também não executa.
    #[test]
    fn sem_consentimento_aborta() {
        let steps =
            vec![Step { cmd: "echo x".into(), source: "t".into(), sudo: false, pipe_sh: false }];
        let mut calls = 0;
        let action = run_steps(&steps, false, false, |_| {
            calls += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(action, PlanAction::Aborted);
        assert_eq!(calls, 0);
    }

    /// Com consentimento e sem dry-run, roda todos os passos.
    #[test]
    fn consentido_executa_todos() {
        let steps = vec![
            Step { cmd: "a".into(), source: "t".into(), sudo: false, pipe_sh: false },
            Step { cmd: "b".into(), source: "t".into(), sudo: false, pipe_sh: false },
        ];
        let mut calls = 0;
        let action = run_steps(&steps, false, true, |_| {
            calls += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(action, PlanAction::Executed);
        assert_eq!(calls, 2);
    }

    /// As 3 ferramentas existem, têm bin/slug/hint e aparecem no status() (categoria "tool").
    #[test]
    fn tabela_tres_ferramentas() {
        let slugs = ["claude", "code", "codex"];
        assert_eq!(defs::TOOLS.len(), 3);
        for s in slugs {
            let t = defs::find_tool(s).expect("ferramenta presente");
            assert!(!t.display.is_empty() && !t.bin.is_empty() && !t.source_hint.is_empty());
        }
        // status() lista linguagens E ferramentas; as 3 ferramentas estão lá como "tool".
        let all = status();
        for s in slugs {
            let le = all.iter().find(|e| e.lang == s).expect("ferramenta no status");
            assert_eq!(le.category, "tool");
            assert!(le.methods_available.is_empty(), "ferramenta não usa método");
            assert!(le.installed.is_none());
        }
        // E as linguagens seguem como "language".
        assert_eq!(all.iter().find(|e| e.lang == "go").unwrap().category, "language");
    }

    /// VS Code na família Debian gera o comando do .deb oficial (com sudo).
    #[test]
    fn vscode_debian_gera_deb() {
        let tool = defs::find_tool("code").unwrap();
        let steps = match defs::tool_install_recipe(tool, Family::Debian) {
            Recipe::Steps(s) => s,
            _ => panic!("esperava Steps pra VS Code em Debian"),
        };
        assert_eq!(steps.len(), 1);
        assert!(steps[0].cmd.contains("os=linux-deb-x64"), "baixa o .deb oficial");
        assert!(steps[0].cmd.contains("apt-get install -y /tmp/schematize-vscode.deb"));
        assert!(steps[0].sudo, "instalar o .deb exige sudo");
    }

    /// VS Code no Rpm usa o repo oficial da Microsoft (3 passos: chave, repo, install).
    #[test]
    fn vscode_rpm_usa_repo_microsoft() {
        let tool = defs::find_tool("code").unwrap();
        let steps = match defs::tool_install_recipe(tool, Family::Rpm) {
            Recipe::Steps(s) => s,
            _ => panic!("esperava Steps pra VS Code em Rpm"),
        };
        assert_eq!(steps.len(), 3);
        assert!(steps[0]
            .cmd
            .contains("rpm --import https://packages.microsoft.com/keys/microsoft.asc"));
        assert!(steps[2].cmd.contains("dnf install -y code") && steps[2].cmd.contains("zypper"));
    }

    /// VS Code em família desconhecida é N/A (não chuta gerenciador de pacotes).
    #[test]
    fn vscode_familia_desconhecida_na() {
        let tool = defs::find_tool("code").unwrap();
        assert!(matches!(defs::tool_install_recipe(tool, Family::Unknown), Recipe::Na(_)));
    }

    /// Claude Code e Codex têm caminho canônico único (independe de família).
    #[test]
    fn claude_e_codex_caminho_canonico() {
        let claude = defs::find_tool("claude").unwrap();
        for fam in [Family::Debian, Family::Rpm, Family::Unknown] {
            match defs::tool_install_recipe(claude, fam) {
                Recipe::Steps(s) => {
                    assert_eq!(s.len(), 1);
                    assert!(s[0].cmd.contains("claude.ai/install.sh"));
                    assert!(s[0].pipe_sh, "curl|bash = código remoto (selo pipe)");
                }
                _ => panic!("claude sempre tem Steps"),
            }
        }
        let codex = defs::find_tool("codex").unwrap();
        match defs::tool_install_recipe(codex, Family::Debian) {
            Recipe::Steps(s) => {
                // note-step da dependência de node + o npm install.
                assert!(s.iter().any(|st| st.cmd.contains("npm install -g @openai/codex")));
                assert!(s.iter().any(|st| st.source == "nota"), "cita a dependência de Node.js");
            }
            _ => panic!("codex sempre tem Steps"),
        }
    }

    /// Remoção das ferramentas é coerente (claude remove o bin; code por família; codex npm).
    #[test]
    fn remocao_ferramentas() {
        let claude = defs::find_tool("claude").unwrap();
        match defs::tool_remove_recipe(claude, Family::Unknown) {
            Recipe::Steps(s) => assert!(s[0].cmd.contains(".local/bin/claude")),
            _ => panic!("claude remove tem Steps"),
        }
        let code = defs::find_tool("code").unwrap();
        match defs::tool_remove_recipe(code, Family::Debian) {
            Recipe::Steps(s) => assert!(s[0].cmd.contains("apt-get remove -y code")),
            _ => panic!("code debian remove tem Steps"),
        }
        assert!(matches!(defs::tool_remove_recipe(code, Family::Unknown), Recipe::Na(_)));
        let codex = defs::find_tool("codex").unwrap();
        match defs::tool_remove_recipe(codex, Family::Rpm) {
            Recipe::Steps(s) => assert!(s[0].cmd.contains("npm uninstall -g @openai/codex")),
            _ => panic!("codex remove tem Steps"),
        }
    }

    /// O recipe do Codex agora roda com SUDO (conserta o EACCES no npm global do sistema):
    /// o comando traz `sudo` literal E o selo de sudo está marcado (guardrail exibe [sudo]).
    #[test]
    fn codex_instala_com_sudo() {
        let codex = defs::find_tool("codex").unwrap();
        for fam in [Family::Debian, Family::Rpm, Family::Unknown] {
            match defs::tool_install_recipe(codex, fam) {
                Recipe::Steps(s) => {
                    let npm = s
                        .iter()
                        .find(|st| st.cmd.contains("npm install -g @openai/codex"))
                        .expect("passo de npm install do codex");
                    assert!(npm.cmd.starts_with("sudo "), "sudo literal no comando: {}", npm.cmd);
                    assert!(npm.sudo, "selo de sudo marcado (guardrail mostra [sudo])");
                }
                _ => panic!("codex sempre tem Steps"),
            }
        }
        // Remoção também com sudo (foi instalado como root).
        match defs::tool_remove_recipe(codex, Family::Debian) {
            Recipe::Steps(s) => {
                assert!(s[0].cmd.starts_with("sudo "));
                assert!(s[0].sudo);
                assert!(s[0].cmd.contains("npm uninstall -g @openai/codex"));
            }
            _ => panic!("codex remove tem Steps"),
        }
    }

    /// Decisão PURA de "precisa fixar o PATH?": só quando o bin NÃO está no PATH mas
    /// EXISTE em ~/.local/bin. Os outros 3 casos não têm PATH a consertar.
    #[test]
    fn needs_path_fix_pura() {
        assert!(needs_path_fix(false, true), "fora do PATH mas em ~/.local/bin → fixa");
        assert!(!needs_path_fix(true, true), "já no PATH → nada a fazer");
        assert!(!needs_path_fix(true, false), "já no PATH → nada a fazer");
        assert!(
            !needs_path_fix(false, false),
            "nem no PATH nem local → instalação falhou, não é PATH"
        );
    }

    /// Idempotência PURA: reconhece um rc que já garante ~/.local/bin no PATH (não duplica),
    /// e ignora comentários / linhas irrelevantes.
    #[test]
    fn rc_already_has_local_bin_pura() {
        // Já presente (várias grafias comuns).
        assert!(rc_already_has_local_bin("export PATH=\"$HOME/.local/bin:$PATH\""));
        assert!(rc_already_has_local_bin("export PATH=$PATH:$HOME/.local/bin"));
        assert!(rc_already_has_local_bin(
            "# algo\nfoo=1\nexport PATH=\"$HOME/.local/bin:$PATH\"\nbar=2"
        ));
        // Ausente / não conta.
        assert!(!rc_already_has_local_bin(""));
        assert!(!rc_already_has_local_bin("export PATH=\"$HOME/bin:$PATH\""));
        // Linha comentada NÃO conta (deny-by-default: não confia num export desativado).
        assert!(!rc_already_has_local_bin("# export PATH=\"$HOME/.local/bin:$PATH\""));
        // Menção sem PATH também não conta.
        assert!(!rc_already_has_local_bin("cd $HOME/.local/bin"));
    }

    /// A linha de export canônica satisfaz a própria checagem de idempotência
    /// (garante que, uma vez escrita, não seja re-adicionada numa 2ª execução).
    #[test]
    fn export_line_e_idempotente() {
        assert!(rc_already_has_local_bin(LOCAL_BIN_EXPORT));
    }

    /// Parse de método é deny-by-default (desconhecido = None).
    #[test]
    fn parse_metodo_deny_default() {
        assert_eq!(Method::parse("docker"), Some(Method::Docker));
        assert_eq!(Method::parse("mise"), Some(Method::Mise));
        assert_eq!(Method::parse("distro"), Some(Method::Distro));
        assert_eq!(Method::parse("official"), Some(Method::Official));
        assert_eq!(Method::parse("apt"), None);
        assert_eq!(Method::parse(""), None);
    }
}
