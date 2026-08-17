# Plano — marketplace vivo + login + notificações + fork (2026-08-16)

Batch grande, multi-sistema. Faseado por dependência.

## FASE A — app-local (não depende de servidor/deploy) — EM ANDAMENTO
Entrega já, com release + reinstall.
- [ ] **Versão + botão de atualizar o app** no Slint (hoje não mostra versão nem tem update visível).
  Backend: `upgrade::app_version()` + `app_update_available()` + `selfupdate::run()`.
- [ ] **Sininho de notificações** (fontes LOCAIS): 
  - Global = nova versão do app + posts do news.
  - Pessoal = "sua skill X está desatualizada" (installed < latest).
  Backend: novo `notifications.rs` (`collect`/`count`/`Notif{scope,...}`). GUI: sino + badge + dropdown.
- [ ] **Modelo de FORK de skill**: skill OFICIAL não é editada — ao editar, forka (cópia ativa +
  base oficial guardada em `~/.schematize/skill-base/<slug>/`). Update de forkada = COMPARAR, não
  sobrescrever. Backend: `skills::fork/compare_update/is_official` + gancho em `skilledit::write_file`.
  GUI: fluxo editar→fork + tela de comparar (fork vs oficial novo).

## DECISÕES (usuário, 2026-08-16)
- **Login**: device flow — adicionar `/device_authorization` ao IdP; app mostra código, aprova no browser.
- **Metadados**: fonte única = a própria skill (skill.toml/SKILL.md). Um gerador produz o `content/skills/*.json`
  do site E popula/espelha na API. Sem duplicar à mão.
- **Ordem Fase B**: TUDO — site do marketplace + API (ranking+notificações) + login/notif no app.

## Estado real (Explore)
- API `schematizeskills_api_rs`: ratings/reviews/comments/follows/feed PRONTOS. FALTA ranking (só alfabético),
  notificações (do zero), metadados ricos (hoje no front).
- IdP `schematize_auth_rs`: authorization_code+PKCE+public client OK. FALTA device flow, registro de client,
  validação de redirect.
- Front: SSG lê `content/skills/*.json` (tem comandos/floors). SEM UI de nota/comentário/ranking/login.

## FASE B — plataforma (login + servidor) — planejada
Depende dos backends (`schematizeskills_api_rs` :8080 + `schematize_auth_rs` :8787, já no ar) e do
deploy do front (gated). Mapeando o que já existe pra planejar:
- [ ] **Login na plataforma pelo app** (OIDC — device flow ou loopback+PKCE contra o auth).
- [ ] **Notificações servidor→pessoa** (skill desatualizada/news via API, não só local) + globais que
  eu publico no news.
- [ ] **Marketplace rico no site** (skill.schematize.org): página da skill com resumo, utilidade,
  comandos, etc.; contas podem **comentar, dar nota, ranking**. Refletir nota/comentário/ranking no app.

## Ordem
Fase A agora (LIB2 → GUI2 → release). Fase B: planejar a partir do Explore + provável input do usuário
(escopo do site + deploy gated).
