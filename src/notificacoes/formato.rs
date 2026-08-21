//! FORMATO e SANITIZAÇÃO das notificações — a fronteira de confiança.
//!
//! Tudo o que entra aqui vindo da rede é HOSTIL até prova em contrário: o servidor
//! pode estar comprometido, a resposta pode ter sido interceptada, ou o feed pode ter
//! sido envenenado. Este módulo é o único ponto por onde dado remoto vira `Notif`, e
//! ele é PURO — nada de I/O — pra que cada regra tenha teste.
//!
//! ## Deny-by-default
//!
//! Nada é "limpo o suficiente e passa". Cada campo tem uma forma FECHADA; o que não
//! casa é rejeitado inteiro. Notificação parcialmente válida é descartada, não
//! remendada — remendar é adivinhar a intenção de quem talvez seja um atacante.
//!
//! ## As três coisas que isto impede, concretamente
//!
//! 1. **Escalada de tipo.** `app_update` e `skill_outdated` disparam AÇÃO NO APP
//!    (o auto-update; navegar pro Mercado). São gerados LOCALMENTE. Um servidor que
//!    pudesse declarar `kind: "app_update"` faria o cliente rodar o próprio
//!    atualizador por ordem dele. Por isso [`Origem::Remota`] só pode produzir
//!    `news`/`server`, e nunca uma ação interna.
//! 2. **URI arbitrária no `xdg-open`.** A ação de um post é aberta no navegador. Um
//!    `file:///`, um `javascript:` ou um esquema registrado por outro app do sistema
//!    viraria execução fora do navegador. Só `https` passa, sem userinfo.
//! 3. **Injeção de terminal.** Título e corpo vão pro stdout do CLI. Sequências ANSI
//!    (`ESC[`) reescrevem a tela, apagam linhas e forjam saída — dá pra fabricar um
//!    "✓ tudo certo" por cima de um erro. Todo caractere de controle cai fora.

use std::fmt;

/// De onde veio o dado. Decide o que a notificação PODE declarar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origem {
    /// Montada por este binário a partir de estado local. Confiável.
    Local,
    /// Veio da rede (API do marketplace, feed do blog). Hostil por padrão.
    Remota,
}

/// Classe da notificação.
///
/// Os três primeiros ACIONAM o app (auto-update, navegação) e por isso são fechados e
/// só nascem localmente. [`Kind::Server`] carrega um rótulo livre do servidor, já
/// saneado — é INERTE no app (a UI cai no ramo `_ => {}`), então serve pra ícone e
/// texto sem virar gatilho de comportamento.
///
/// Rejeitar todo tipo desconhecido seria a casa brigando com a própria API: o servidor
/// tem tipos legítimos (`review_reply`, `follow`) e vai ganhar outros. O que não pode
/// acontecer é um deles ATIVAR algo — e é isso que a inertização garante.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    AppUpdate,
    News,
    SkillOutdated,
    /// Rótulo do servidor, saneado a `[a-z0-9_]`. Inerte.
    Server(String),
}

/// Rótulo de servidor aceitável: `[a-z0-9_]`, curto. Fora disso vira `"server"`.
///
/// O rótulo é exibido e pode virar nome de ícone; um rótulo com `../` ou caractere de
/// controle não pode chegar a nenhuma dessas duas pontas.
pub fn rotulo_seguro(bruto: &str) -> String {
    let s = bruto.trim();
    let ok = !s.is_empty()
        && s.len() <= 32
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok {
        s.to_string()
    } else {
        "server".to_string()
    }
}

impl Kind {
    pub fn como_str(&self) -> &str {
        match self {
            Kind::AppUpdate => "app_update",
            Kind::News => "news",
            Kind::SkillOutdated => "skill_outdated",
            Kind::Server(r) => r,
        }
    }

    /// Este tipo dispara AÇÃO NO APP (auto-update, navegação) em vez de só informar?
    ///
    /// É a pergunta que separa o que uma origem remota pode declarar do que não pode.
    pub fn aciona_o_app(&self) -> bool {
        matches!(self, Kind::AppUpdate | Kind::SkillOutdated)
    }

    /// Analisa um `kind` cru respeitando a origem. `None` = rejeitado.
    pub fn parse(bruto: &str, origem: Origem) -> Option<Kind> {
        let t = bruto.trim();
        match origem {
            // Local: forma fechada. Este binário não produz tipo desconhecido — se
            // produziu, é bug, e passar batido esconderia o bug.
            Origem::Local => match t {
                "app_update" => Some(Kind::AppUpdate),
                "news" => Some(Kind::News),
                "skill_outdated" => Some(Kind::SkillOutdated),
                "server" | "" => Some(Kind::Server("server".into())),
                _ => None,
            },
            // Remota: os que ACIONAM o app são recusados — é a tentativa de escalada, e
            // ela é descartada, não rebaixada em silêncio. `news` passa porque só abre
            // uma URL, que já é validada à parte. O resto vira rótulo inerte.
            Origem::Remota => match t {
                "app_update" | "skill_outdated" => None,
                "news" => Some(Kind::News),
                outro => Some(Kind::Server(rotulo_seguro(outro))),
            },
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.como_str())
    }
}

/// O que acontece ao clicar. Conjunto FECHADO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acao {
    /// Só informa.
    Nenhuma,
    /// Abre uma URL `https` no navegador do usuário.
    Abrir(String),
    /// Dispara um comando INTERNO conhecido. Nunca vem da rede.
    Interna(String),
}

impl Acao {
    /// A string que a UI recebe (vazia = sem ação).
    pub fn como_str(&self) -> &str {
        match self {
            Acao::Nenhuma => "",
            Acao::Abrir(s) | Acao::Interna(s) => s,
        }
    }
}

/// Tetos. Existem pra limitar dano, não por gosto: um corpo de 4 MB trava a UI, e
/// 10 mil notificações enchem o disco e a tela. Origem hostil não respeita bom senso.
pub const MAX_TITULO: usize = 120;
pub const MAX_CORPO: usize = 400;
pub const MAX_URL: usize = 2048;
pub const MAX_ITENS: usize = 50;

/// Limpa um texto pra exibição: remove caracteres de CONTROLE, colapsa espaço e corta
/// no teto. `None` se sobrar vazio.
///
/// O controle é o ponto: `ESC` monta sequência ANSI e reescreve o terminal; `\r` volta
/// o cursor e sobrescreve a linha; `\0` trunca em C. Nenhum deles tem uso legítimo num
/// título de notificação.
pub fn texto_limpo(bruto: &str, teto: usize) -> Option<String> {
    let mut s = String::with_capacity(bruto.len().min(teto));
    let mut espaco_pendente = false;
    for c in bruto.chars() {
        if c.is_control() {
            // Quebra de linha e tab viram espaço; o resto simplesmente some.
            if c == '\n' || c == '\t' || c == '\r' {
                espaco_pendente = !s.is_empty();
            }
            continue;
        }
        if c.is_whitespace() {
            espaco_pendente = !s.is_empty();
            continue;
        }
        if espaco_pendente {
            s.push(' ');
            espaco_pendente = false;
        }
        if s.chars().count() >= teto {
            break;
        }
        s.push(c);
    }
    (!s.is_empty()).then_some(s)
}

/// Valida uma URL de ação. Só `https`, com host, sem credenciais embutidas.
///
/// Deny-by-default de esquema: `file:`, `javascript:`, `data:` e esquemas de outros
/// apps do sistema chegariam ao `xdg-open` e virariam execução fora do navegador.
/// Sem userinfo porque `https://banco.com@evil.tld` engana quem lê a URL na tela.
pub fn url_segura(bruto: &str) -> Option<String> {
    let u = bruto.trim();
    if u.is_empty() || u.len() > MAX_URL {
        return None;
    }
    if u.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let resto = u.strip_prefix("https://")?;
    let host = resto.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains('@') {
        return None;
    }
    // Host precisa parecer host: letra/dígito/ponto/hífen/dois-pontos (porta) e nada mais.
    if !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':')) {
        return None;
    }
    Some(u.to_string())
}

/// Slug de skill aceitável numa ação interna: `[a-z0-9-]`, curto.
///
/// A ação vira um comando (`skills update <slug>`); um slug com espaço, aspas ou `;`
/// seria um vetor de injeção se alguém, um dia, montasse isso numa shell.
pub fn slug_seguro(bruto: &str) -> Option<String> {
    let s = bruto.trim();
    if s.is_empty() || s.len() > 64 {
        return None;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        .then(|| s.to_string())
}

/// Valida a ação conforme a origem.
///
/// Remota NUNCA produz [`Acao::Interna`] — é o segundo cadeado contra a escalada: mesmo
/// que um `kind` inofensivo passasse, a ação não conseguiria acionar o app.
pub fn acao_valida(bruto: Option<&str>, kind: &Kind, origem: Origem) -> Acao {
    let Some(a) = bruto.map(str::trim).filter(|s| !s.is_empty()) else {
        return Acao::Nenhuma;
    };
    if origem == Origem::Remota {
        return url_segura(a).map(Acao::Abrir).unwrap_or(Acao::Nenhuma);
    }
    match kind {
        Kind::AppUpdate if a == "upgrade" => Acao::Interna("upgrade".into()),
        Kind::SkillOutdated => match a.strip_prefix("skills update ").and_then(slug_seguro) {
            Some(slug) => Acao::Interna(format!("skills update {slug}")),
            None => Acao::Nenhuma,
        },
        _ => url_segura(a).map(Acao::Abrir).unwrap_or(Acao::Nenhuma),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ESCALADA DE TIPO: um servidor comprometido não pode declarar um tipo que
    /// dispara ação no app. `app_update` faria o cliente rodar o próprio atualizador.
    #[test]
    fn origem_remota_nao_declara_tipo_que_aciona_o_app() {
        assert_eq!(Kind::parse("app_update", Origem::Remota), None);
        assert_eq!(Kind::parse("skill_outdated", Origem::Remota), None);
        // Local pode, porque foi este binário que montou.
        assert_eq!(Kind::parse("app_update", Origem::Local), Some(Kind::AppUpdate));
        // E o que é só informativo passa dos dois lados.
        assert_eq!(Kind::parse("news", Origem::Remota), Some(Kind::News));
        assert_eq!(Kind::parse("", Origem::Remota), Some(Kind::Server("server".into())));
    }

    /// Tipo desconhecido do servidor vira rótulo INERTE — não é recusado (a API tem
    /// tipos legítimos e vai ganhar outros) nem confiado (a UI não age sobre ele).
    #[test]
    fn tipo_desconhecido_do_servidor_e_inertizado() {
        let k = Kind::parse("review_reply", Origem::Remota).unwrap();
        assert_eq!(k.como_str(), "review_reply");
        assert!(!k.aciona_o_app(), "rótulo do servidor não pode acionar o app");
        // Rótulo malformado não chega à tela nem a um nome de ícone.
        assert_eq!(Kind::parse("../../etc", Origem::Remota).unwrap().como_str(), "server");
        assert_eq!(Kind::parse("a\u{1b}[0m", Origem::Remota).unwrap().como_str(), "server");
        // Já localmente, tipo desconhecido é bug nosso — e some, em vez de passar batido.
        assert_eq!(Kind::parse("promo", Origem::Local), None);
        assert_eq!(Kind::parse("APP_UPDATE", Origem::Local), None);
    }

    /// INJEÇÃO DE TERMINAL: sequência ANSI reescreve a tela e forja saída.
    #[test]
    fn texto_mata_controle_e_ansi() {
        let ataque = "\u{1b}[2J\u{1b}[H FALSO: tudo certo\u{1b}[0m";
        let limpo = texto_limpo(ataque, MAX_TITULO).unwrap();
        assert!(!limpo.contains('\u{1b}'), "escape sobreviveu: {limpo:?}");
        assert!(!limpo.contains("[2J") || !limpo.starts_with('\u{1b}'));
        // \r sobrescreve a linha; \0 trunca em C.
        let r = texto_limpo("ok\rFALSO", MAX_TITULO).unwrap();
        assert_eq!(r, "ok FALSO");
        assert!(!texto_limpo("a\0b", MAX_TITULO).unwrap().contains('\0'));
    }

    /// Teto de tamanho, espaço colapsado, e vazio vira `None`.
    #[test]
    fn texto_respeita_teto_e_recusa_vazio() {
        let longo = "a".repeat(1000);
        assert_eq!(texto_limpo(&longo, MAX_TITULO).unwrap().chars().count(), MAX_TITULO);
        assert_eq!(texto_limpo("  a   b  ", 50).unwrap(), "a b");
        assert_eq!(texto_limpo("   ", 50), None);
        assert_eq!(texto_limpo("\u{1b}\u{1b}", 50), None, "só controle = vazio");
        // Acento e emoji são conteúdo legítimo e sobrevivem.
        assert_eq!(texto_limpo("versão nova ✓", 50).unwrap(), "versão nova ✓");
    }

    /// URI ARBITRÁRIA: só https chega ao navegador.
    #[test]
    fn url_recusa_tudo_que_nao_e_https() {
        assert!(url_segura("https://blog.schematize.org/post").is_some());
        for mau in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,<script>",
            "http://blog.schematize.org",
            "vscode://file/etc/shadow",
            "HTTPS://x.com",
            "https://",
            "https:// espaco.com",
            "https://a.com\u{1b}[0m",
        ] {
            assert_eq!(url_segura(mau), None, "devia recusar: {mau}");
        }
    }

    /// Userinfo engana quem lê a URL: `https://banco.com@evil.tld`.
    #[test]
    fn url_recusa_credencial_embutida() {
        assert_eq!(url_segura("https://blog.schematize.org@evil.tld/x"), None);
        assert_eq!(url_segura("https://user:pass@evil.tld"), None);
    }

    /// URL gigante é DoS de memória/tela.
    #[test]
    fn url_respeita_teto() {
        let g = format!("https://a.com/{}", "x".repeat(MAX_URL));
        assert_eq!(url_segura(&g), None);
    }

    /// AÇÃO INTERNA nunca vem da rede — segundo cadeado da escalada.
    #[test]
    fn acao_remota_nunca_e_interna() {
        assert_eq!(acao_valida(Some("upgrade"), &Kind::Server("server".into()), Origem::Remota), Acao::Nenhuma);
        assert_eq!(
            acao_valida(Some("skills update web"), &Kind::Server("server".into()), Origem::Remota),
            Acao::Nenhuma
        );
        assert_eq!(
            acao_valida(Some("https://x.com/p"), &Kind::News, Origem::Remota),
            Acao::Abrir("https://x.com/p".into())
        );
    }

    /// Ação interna local é validada em forma FECHADA — nada de comando arbitrário.
    #[test]
    fn acao_interna_local_e_de_forma_fechada() {
        assert_eq!(
            acao_valida(Some("upgrade"), &Kind::AppUpdate, Origem::Local),
            Acao::Interna("upgrade".into())
        );
        // Slug com metacaractere de shell não vira ação.
        assert_eq!(
            acao_valida(Some("skills update web; rm -rf /"), &Kind::SkillOutdated, Origem::Local),
            Acao::Nenhuma
        );
        assert_eq!(
            acao_valida(Some("skills update ../etc"), &Kind::SkillOutdated, Origem::Local),
            Acao::Nenhuma
        );
        assert_eq!(
            acao_valida(Some("skills update skill-web"), &Kind::SkillOutdated, Origem::Local),
            Acao::Interna("skills update skill-web".into())
        );
        // Comando que não está na forma fechada não passa nem sendo local.
        assert_eq!(acao_valida(Some("rm -rf /"), &Kind::AppUpdate, Origem::Local), Acao::Nenhuma);
    }

    #[test]
    fn slug_so_aceita_a_forma_esperada() {
        assert_eq!(slug_seguro("skill-web"), Some("skill-web".into()));
        for mau in ["Skill", "a b", "a;b", "a/b", "a$b", "", &"a".repeat(100)] {
            assert_eq!(slug_seguro(mau), None, "devia recusar: {mau:?}");
        }
    }
}
