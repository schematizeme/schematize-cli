#!/usr/bin/env python3
"""Mutation testing dirigido: quebra UMA invariante por vez e exige que a suíte pegue.

Não é mutação aleatória. Cada mutante desliga deliberadamente uma DEFESA do sistema —
se a suíte continuar verde, aquela defesa não está sendo testada de verdade, e o teste
que a "cobre" é decorativo."""
import subprocess, sys, os, shutil, atexit, signal

R = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))  # a raiz do crate

# (arquivo, trecho original, trecho mutado, o que a mutação desliga)
MUTANTES = [
 ("src/vps/registro.rs", '_ => Ambiente::Prd,', '_ => Ambiente::Dev,',
  "falha fechada do ambiente (desconhecido deixa de virar Prd)"),
 ("src/vps/registro.rs", '_ => ModoPolitica::ReadOnly,', '_ => ModoPolitica::Livre,',
  "falha fechada do modo (desconhecido deixa de virar ReadOnly)"),
 ("src/vps/capacidade.rs", '_ => Fronteira::Sem,', '_ => Fronteira::OpsShellRoot,',
  "falha fechada da fronteira (desconhecido vira o nivel MAIS forte)"),
 ("src/vps/politica.rs", 'if let Some(motivo) = padrao_catastrofico(cmd) {',
  'if let Some(motivo) = None::<&str> {', "denylist catastrofica inteira"),
 ("src/vps/politica.rs", '(Ambiente::Prd, Veredito::Allow) => Veredito::Confirm(',
  '(Ambiente::Prd, Veredito::Allow) => Veredito::Allow.pass_through(',
  "gate de producao (Prd deixa de pedir confirmacao)"),
 ("src/vps/conexao.rs", 'a.push("StrictHostKeyChecking=yes".into());',
  'a.push("StrictHostKeyChecking=accept-new".into());', "pinning da host key (volta ao TOFU cego)"),
 ("src/vps/conexao.rs", 'a.push("none".into());', 'a.push("/dev/null".into());',
  "o -F none (o ~/.ssh/config do usuario volta a entrar)"),
 ("src/vps/conexao.rs", 'if !esta_confiado(p) {', 'if false {',
  "exigencia de host confiado"),
 ("src/vps/auditoria.rs", 'let limpo = crate::debugreport::scrub(transcript_bruto);',
  'let limpo = transcript_bruto.to_string();', "redacao do transcript na escrita"),
 ("src/vps/hook.rs", 'if let Some(h) = CABECALHOS_DE_CHAVE.iter().find(|h| bruto.contains(**h)) {',
  'if let Some(h) = CABECALHOS_DE_CHAVE.iter().find(|h| bruto.contains(**h) && false) {',
  "bloqueio de chave privada no input de tool"),
 ("src/vps/hook.rs", 'for bin in binarios_invocados(cmd) {', 'for bin in Vec::<String>::new() {',
  "bloqueio de ssh cru no Bash do agente"),
 ("src/vps/registro.rs", 'valid_opcao(o)?;', 'let _ = valid_opcao(o);',
  "allowlist de extra_opts (a vulnerabilidade critica P3)"),
 ("src/vps/politica.rs", 'if !ascii_imprimivel(cmd) {', 'if false {',
  "defesa contra homoglifo/bidi (ataque ao gate humano)"),
 ("src/mcp/tools.rs", 'vps::Confirmacao::Ausente', 'vps::Confirmacao::HumanoConfirmou',
  "impossibilidade do agente se autoconfirmar"),
 ("src/vps/politica.rs", 'if let Some(m) = catastrofico_por_estrutura(&analisar(cmd)) {',
  'if let Some(m) = None::<&str> {', "a camada ESTRUTURAL da denylist (D1)"),
 ("src/vps/registro.rs", 'pub fn resumir(v: &str) -> String {\n    const TETO: usize = 120;',
  'pub fn resumir(v: &str) -> String {\n    const TETO: usize = usize::MAX;',
  "truncagem da mensagem de erro (amplificacao de DoS, D2)"),
 ("src/mcp/mod.rs", 'pub const MAX_LINHA: u64 = 1024 * 1024;',
  'pub const MAX_LINHA: u64 = u64::MAX / 4;', "teto de linha do MCP (D2)"),
 ("src/vps/db.rs", 'if md.file_type().is_symlink() {', 'if false {',
  "recusa de escrita atraves de symlink (D3)"),
 ("src/vps/bootstrap.rs", 'while ! mkdir "$TRAVA" 2>/dev/null; do',
  'while false; do', "trava do bootstrap concorrente (D4)"),
 ("src/vps/verbos.rs", 'if VERBOS_RESERVADOS.contains(&nome) {', 'if false {',
  "recusa de verbo com nome reservado (D5)"),
 ("src/vps/registro.rs", 'if p.port == 0 {', 'if false {', "recusa de porta 0 (D6)"),
 ("src/vps/registro.rs", 'const MAX: usize = 253;', 'const MAX: usize = usize::MAX;',
  "teto de tamanho do host (D9)"),
 ("packaging/ops-shell/schematize-ops-shell",
  """    *' '*|*';'*|*'&'*|*'|'*|*'`'*|*'$'*|*'>'*|*'<'*|*'('*|*')'*|*'""",
  """    *'\\x00NUNCA'*|*';;;;'*|*'&&&&'*|*'||||'*|*'````'*|*'$$$$'*|*'>>>>'*|*'<<<<'*|*'(((('*|*'""",
  "recusa de metacaractere no shim (o servidor)"),
]

def rodar_suite():
    r = subprocess.run(["cargo","test","--quiet"], cwd=R, capture_output=True, text=True, timeout=600)
    return r.returncode == 0

# RESTAURACAO A PROVA DE MORTE SUBITA.
#
# A primeira versao deste script deixou uma mutacao no fonte: foi morto pelo timeout do shell
# DEPOIS de mutar e ANTES de restaurar, e o codigo ficou com a defesa desligada. Um mutante
# esquecido e pior que nao rodar mutation testing nenhum.
#
# Agora todo backup e restaurado por atexit E pelos sinais de termino.
_PENDENTES = {}

def _restaurar_tudo(*_):
    for caminho, backup in list(_PENDENTES.items()):
        if os.path.exists(backup):
            shutil.move(backup, caminho)
            os.utime(caminho, None)  # mtime AGORA: forca o cargo a recompilar
            print(f"  [restaurado no encerramento] {caminho}")
        _PENDENTES.pop(caminho, None)

atexit.register(_restaurar_tudo)
for _s in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
    signal.signal(_s, lambda *_: (_restaurar_tudo(), sys.exit(130)))

print("=== baseline: a suite tem que estar VERDE antes de mutar ===")
if not rodar_suite():
    print("baseline VERMELHO — abortando"); sys.exit(1)
print("baseline verde\n")

pegos, escaparam = 0, []
for arq, orig, mut, alvo in MUTANTES:
    caminho = os.path.join(R, arq)
    backup = caminho + ".mutbak"
    # `copy` e nao `copy2`: o copy2 PRESERVA o mtime, e ao restaurar o arquivo ficava mais
    # VELHO que o artefato ja compilado — o cargo considerava tudo atualizado e seguia usando
    # o binario COM A MUTACAO DENTRO. O sintoma: a suite falhava com o fonte visivelmente
    # correto na tela. Restaurar tem que reabrir a janela de recompilacao.
    shutil.copy(caminho, backup)
    _PENDENTES[caminho] = backup
    src = open(caminho).read()
    if orig not in src:
        print(f"  ?? ALVO NAO ENCONTRADO  {alvo}  ({arq})")
        os.remove(backup); _PENDENTES.pop(caminho, None); continue
    open(caminho, "w").write(src.replace(orig, mut, 1))
    try:
        verde = rodar_suite()
    except subprocess.TimeoutExpired:
        verde = False
    shutil.move(backup, caminho)
    os.utime(caminho, None)  # ver a nota no `copy` acima
    _PENDENTES.pop(caminho, None)
    if verde:
        print(f"  !! ESCAPOU  {alvo}")
        escaparam.append(alvo)
    else:
        print(f"  ok pego     {alvo}")
        pegos += 1

print(f"\n=== {pegos} pegos, {len(escaparam)} escaparam ===")
for e in escaparam: print(f"  NAO TESTADO: {e}")
sys.exit(1 if escaparam else 0)
