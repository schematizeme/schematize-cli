//! Testes do núcleo sempre-on: merge do bloco, hook e instalação sob um HOME falso.

use super::*;

const CORE: &str = "<!-- schematize-core v0.29.0 -->\n# Núcleo\nregra A\n";
const CORE2: &str = "<!-- schematize-core v0.30.0 -->\n# Núcleo\nregra B\n";

/// HOME falso, exclusivo do teste.
fn home(nome: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("schematize-core-{nome}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn grava_core(h: &Path, txt: &str) {
    let p = core_path(h);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, txt).unwrap();
}

fn le(p: PathBuf) -> String {
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn marcador_de_versao() {
    assert_eq!(versao_do_marcador(CORE).as_deref(), Some("0.29.0"));
    assert_eq!(versao_do_marcador("\n<!--schematize-core 1.2.3-->\nx").as_deref(), Some("1.2.3"));
    assert_eq!(versao_do_marcador("# sem marcador"), None);
    assert_eq!(versao_do_marcador(""), None);
}

#[test]
fn arquivo_inexistente_vira_so_o_bloco() {
    let s = aplicar_bloco("", CORE).unwrap();
    assert_eq!(s, format!("{BEGIN}\n{}\n{END}\n", CORE.trim_end()));
}

#[test]
fn sem_bloco_anexa_no_fim_preservando_tudo() {
    let user = "# Minhas regras\nuse tabs\n";
    let s = aplicar_bloco(user, CORE).unwrap();
    assert!(s.starts_with(user), "o texto do usuário tem de ficar intacto no início");
    assert!(s.contains(BEGIN) && s.contains(END));
    // sem newline final também funciona, e separa com linha em branco
    let s2 = aplicar_bloco("sem fim", CORE).unwrap();
    assert!(s2.starts_with("sem fim\n\n<!-- schematize:core:begin -->"));
}

#[test]
fn bloco_antigo_e_substituido_e_o_resto_fica_byte_a_byte() {
    let antes = "# topo do usuário\n\n";
    let depois = "\n\n# rodapé do usuário\nfim";
    let velho = format!("{antes}{BEGIN}\n<!-- schematize-core v0.1.0 -->\nvelho\n{END}{depois}");
    let s = aplicar_bloco(&velho, CORE2).unwrap();
    assert!(s.starts_with(antes) && s.ends_with(depois));
    assert!(s.contains("regra B") && !s.contains("velho"));
    assert_eq!(s.matches(BEGIN).count(), 1);
}

#[test]
fn aplicar_e_idempotente() {
    let a = aplicar_bloco("# u\n", CORE).unwrap();
    assert_eq!(aplicar_bloco(&a, CORE).unwrap(), a);
    let b = aplicar_bloco(&a, CORE2).unwrap();
    assert_eq!(aplicar_bloco(&b, CORE2).unwrap(), b);
}

#[test]
fn begin_sem_end_e_erro_e_nao_adivinha() {
    let e = aplicar_bloco(&format!("user\n{BEGIN}\nlixo sem fim\nmais user"), CORE).unwrap_err();
    assert!(e.contains("sem"), "{e}");
}

#[test]
fn conteudo_do_bloco_extrai_o_core() {
    let s = aplicar_bloco("x\n", CORE).unwrap();
    assert_eq!(conteudo_do_bloco(&s).unwrap().as_deref(), Some(CORE.trim_end()));
    assert_eq!(conteudo_do_bloco("nada").unwrap(), None);
}

// ----- hook ---------------------------------------------------------------------------------

#[test]
fn hook_registra_sem_matcher_e_nao_duplica() {
    let mut v = json!({});
    assert!(registrar_hook(&mut v));
    assert!(!registrar_hook(&mut v), "segunda vez não muda nada");
    let arr = v["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert!(arr[0].get("matcher").is_none(), "sem matcher: vale em startup/resume/clear/compact");
    assert_eq!(arr[0]["hooks"][0]["type"], "command");
    assert!(hook_presente(&v));
}

#[test]
fn hook_preserva_hooks_existentes_e_o_resto_do_json() {
    let mut v = json!({
        "model": "opus",
        "hooks": {
            "Stop": [ { "hooks": [ { "type": "command", "command": "overdev check" } ] } ],
            "SessionStart": [ { "hooks": [ { "type": "command", "command": "echo do-usuario" } ] } ]
        }
    });
    assert!(registrar_hook(&mut v));
    assert_eq!(v["model"], "opus");
    assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 1);
    let ss = v["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(ss.len(), 2, "o hook do usuário fica, o nosso entra");
    assert_eq!(ss[0]["hooks"][0]["command"], "echo do-usuario");
}

#[test]
fn hook_com_comando_velho_e_atualizado_no_lugar() {
    let mut v = json!({ "hooks": { "SessionStart": [
        { "hooks": [ { "type": "command", "command": "schematize core print --velho" } ] },
        { "hooks": [ { "type": "command", "command": "echo meu" } ] },
        { "hooks": [ { "type": "command", "command": "x core print dup" } ] }
    ] } });
    assert!(registrar_hook(&mut v));
    let ss = v["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(ss.len(), 2, "duplicata antiga some, o hook alheio fica");
    assert_eq!(ss[0]["hooks"][0]["command"], hook_cmd());
    assert_eq!(ss[1]["hooks"][0]["command"], "echo meu");
}

#[test]
fn hook_tolera_json_malformado_de_hooks() {
    let mut v = json!({ "hooks": "texto" });
    assert!(registrar_hook(&mut v));
    assert!(hook_presente(&v));
}

#[test]
fn comando_do_hook_tem_fallback_e_sai_zero() {
    let c = hook_cmd();
    assert!(c.contains("core print") && c.contains("exec") && c.ends_with("exit 0"));
}

// ----- instalação sob HOME falso -------------------------------------------------------------

#[test]
fn instalar_sem_core_e_erro_claro() {
    let h = home("sem-core");
    let e = instalar_em(&h, None).unwrap_err();
    assert!(e.contains("CORE.md"), "{e}");
    assert!(!claude_md(&h).exists(), "nada pode ser criado");
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn instalar_cria_arquivos_e_segunda_vez_nao_escreve() {
    let h = home("cria");
    grava_core(&h, CORE);
    let r = instalar_em(&h, None).unwrap();
    assert!(r.bloco_alterado && r.hook_alterado && !r.backup_criado, "{r:?}");
    assert!(le(claude_md(&h)).contains("regra A"));
    assert!(estado_em(&h).em_dia());
    let r2 = instalar_em(&h, None).unwrap();
    assert_eq!(r2, Resultado::default(), "idempotente: nada a fazer");
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn instalar_preserva_claude_md_do_usuario_e_faz_backup_uma_vez() {
    let h = home("preserva");
    grava_core(&h, CORE);
    let user = "# meu\nregra minha\n";
    std::fs::create_dir_all(h.join(".claude")).unwrap();
    std::fs::write(claude_md(&h), user).unwrap();
    let r = instalar_em(&h, None).unwrap();
    assert!(r.backup_criado);
    assert_eq!(le(h.join(".claude/CLAUDE.md.bak")), user);
    assert!(le(claude_md(&h)).starts_with(user));
    // atualiza o core: o .bak NÃO é sobrescrito (guarda o original do usuário)
    grava_core(&h, CORE2);
    let r2 = instalar_em(&h, None).unwrap();
    assert!(r2.bloco_alterado && !r2.backup_criado);
    assert_eq!(le(h.join(".claude/CLAUDE.md.bak")), user);
    let c = le(claude_md(&h));
    assert!(c.starts_with(user) && c.contains("regra B") && !c.contains("regra A"));
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn estado_distingue_ausente_desatualizado_e_em_dia() {
    let h = home("estado");
    grava_core(&h, CORE);
    assert_eq!(estado_em(&h).bloco, Bloco::Ausente);
    instalar_em(&h, None).unwrap();
    assert_eq!(estado_em(&h).bloco, Bloco::EmDia);
    grava_core(&h, CORE2);
    let s = estado_em(&h);
    assert_eq!(
        s.bloco,
        Bloco::Desatualizado { do_bloco: Some("0.29.0".into()), do_core: Some("0.30.0".into()) }
    );
    assert!(!s.em_dia());
    assert!(descrever(&s).contains("0.29.0 → 0.30.0"));
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn instalar_preserva_settings_e_nao_sobrescreve_json_invalido() {
    let h = home("settings");
    grava_core(&h, CORE);
    let sp = settings(&h);
    std::fs::create_dir_all(sp.parent().unwrap()).unwrap();
    std::fs::write(&sp, r#"{"model":"opus","hooks":{"Stop":[{"hooks":[]}]}}"#).unwrap();
    instalar_em(&h, None).unwrap();
    let v: Value = serde_json::from_str(&le(sp.clone())).unwrap();
    assert_eq!(v["model"], "opus");
    assert!(v["hooks"]["Stop"].is_array() && hook_presente(&v));

    // JSON inválido: erro, arquivo intacto e CLAUDE.md não tocado.
    let h2 = home("settings-ruim");
    grava_core(&h2, CORE);
    let sp2 = settings(&h2);
    std::fs::create_dir_all(sp2.parent().unwrap()).unwrap();
    std::fs::write(&sp2, "{ quebrado").unwrap();
    assert!(instalar_em(&h2, None).is_err());
    assert_eq!(le(sp2), "{ quebrado");
    assert!(!claude_md(&h2).exists());
    let _ = std::fs::remove_dir_all(&h);
    let _ = std::fs::remove_dir_all(&h2);
}

#[test]
fn home_do_usuario_no_passwd() {
    let p = "root:x:0:0:root:/root:/bin/bash\ntom:x:1000:1000:Tom:/home/tom:/bin/bash\n";
    assert_eq!(home_de(p, "tom"), Some(PathBuf::from("/home/tom")));
    assert_eq!(home_de(p, "ninguem"), None);
}
