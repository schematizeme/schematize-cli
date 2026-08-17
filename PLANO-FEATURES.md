# Plano — features que faltam (2026-08-16)

Batch do usuário. Estado real descoberto: SSH/environments/settings **já têm backend**
(`sshkeys.rs`, `environments/`, `settings.rs`); a GUI já tem aba Environments. Foco: GUI +
módulos novos + skill.

## Onda 1 (paralela)
### A — lib backend novo (`schematize_cli_rs`)
- [ ] `overdevdb.rs` — **DB local SQLite** (rusqlite bundled) em `~/.schematize/overdev.db`.
  Snapshot dos arquivos de `.overdev/` (+ PERGUNTAS) por projeto (content+hash+ts). Como o
  Claude pode editar/apagar esses arquivos, o DB garante que nada se perca. API: `snapshot`,
  `history`, `get`, `restore`. Auto-snapshot nos ops do overdev. CLI `overdev snapshot|history|restore`.
- [ ] `githist.rs` — histórico de **commits + pushs**: `git log` parseado (hash/autor/data/assunto)
  + estado vs upstream (ahead/behind → marca commit "pushado" vs "local"). API pra GUI. CLI `git-log`.
- [ ] overdev **load/index** — `overdev::load_cmd()`/`index_cmd()` (strings `/eng-load`,`/eng-index`)
  + `Over::Load`/`Over::Index` (dispara one-shot ou injeta na sessão acoplada).

### B — skill nova (repo próprio, paralela)
- [ ] `schematize-overdev-context` — skill que **cria o contexto geral do overdev** (Fase 0:
  decisões acordadas + grafo/index + escopo → documento de contexto coeso). Scaffold + install local.

## Onda 2 (depois de A) — GUI Slint (`schematize_gui_slint`)
- [ ] Tela **SSH** (liga `sshkeys`: gerar/listar/exportar/copiar/remover).
- [ ] Tela **Configurações** (liga config/lang/autostart/settings: idioma, autostart do agente,
  hooks do overdev, dev_dirs, atalho SSH).
- [ ] Environments: revisar/completar a aba existente.
- [ ] Overdev: botões **Load** e **Index** + **histórico do DB** (ver/restaurar snapshots) +
  painel **histórico de commits/pushs**.
- [ ] **Paginação** nas listas longas (mercado, projetos, histórico) + Home → páginas reais.

## Onda 3 — release (eu)
- [ ] bump, build CLI+Slint, release, reinstalar nesta máquina.
