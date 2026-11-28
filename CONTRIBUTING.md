# Contribuir para o kitty-OS

O kitty-OS é desenvolvido por dois assistentes de IA — **Claude** (Anthropic)
e **Codex** (OpenAI) — e por duas pessoas: o **Rodrigo** (dono do
repositório, trabalha com o Claude e aprova os PRs do Codex) e o **amigo do
Rodrigo** (trabalha com o Codex e aprova os PRs do Claude). Os assistentes não
falam diretamente: comunicam só pelo repositório (ficheiros, issues, PRs e
revisões).

## Objetivos, por esta ordem
1. **Segurança**: o mínimo de `unsafe`, isolamento forte, proteções de
   hardware ativas.
2. **Desempenho**: arranque, trocas de contexto e syscalls rápidos, sem
   sacrificar o ponto 1. Só se otimiza o que foi medido.

## Regras de colaboração (obrigatórias)
- **Nunca** fazer push direto para `main`. Cada tarefa tem o seu branch:
  `claude/<tarefa>` ou `codex/<tarefa>`.
- Cada tarefa = **1 Pull Request pequeno e focado**, com descrição de:
  o que faz, como testar e impacto na segurança.
- Nenhum PR entra no `main` sem **aprovação de um humano** e **CI verde**.
- Antes de começar: pull do `main` e ler `docs/HANDOFF.md`,
  `docs/ROADMAP.md` e os PRs/issues abertos (etiquetas `para-claude` /
  `para-codex`).
- Cada um só edita a sua área (ver `CLAUDE.md` e `AGENTS.md`). Para pedir
  uma mudança na área do outro: issue com a etiqueta `para-claude` ou
  `para-codex`, a explicar o quê e porquê.
- Cada um revê os PRs do outro (comentários de revisão no GitHub):
  correção, uso de `unsafe`, race conditions, respeito por
  `docs/INTERFACES.md`, CI.
- Alterar uma interface partilhada exige atualizar `docs/INTERFACES.md`
  **no mesmo PR**.
- O código tem de passar `cargo clippy --workspace -- -D warnings` e
  `cargo fmt --all --check`.
- Nada de `unwrap()`/`expect()` em caminhos que possam falhar em runtime por
  input externo: usar `Result` e tratar os erros.
- Nunca imprimir endereços do kernel em logs visíveis ao user space (anula
  o KASLR).
- Nunca pôr tokens, passwords ou chaves no repositório (é público).
- Crates novos: entrada em `docs/DECISIONS.md` com justificação e aprovação
  de um humano no PR. Permitidos à partida: `limine`, `bitflags`.
- Se não for possível correr o QEMU localmente, dizê-lo no PR e usar a CI
  como prova de que arranca.
- Comentários de código e mensagens de commit em **inglês**; documentação
  de coordenação pode ser em **português**.

## Áreas e pastas
- **Claude (núcleo):** `kernel/src/` — `arch/`, `mm/`, `sync/`, `sched/`,
  syscalls e processos, e o `hal/` (`kitty-hal`); a partir das Fases 15–17 também `caps/`
  (capacidades), `audit/` (registo de auditoria), `crypto/` (crate `kitty-crypto` na raiz:
  criptografia educativa, não auditada) e `firewall/`.
- **Codex:** `drivers/`, `fs/`, `userland/`, `tests/`, a CI, os
  benchmarks, a futura stack de rede e a GUI, e o `AGENTS.md`.
- **Partilhadas:** `docs/` (coordenação) e `tests/security/redteam/` (só
  para acrescentar PoCs, ver abaixo). Mais tarde, `ctf/`.

## Red team vs blue team
Regras completas em `docs/REDTEAM.md` (D-059). Resumo:
- No fim de cada fase concluída, a partir da Fase 6, cada IA ataca a área
  da **outra**. Os ataques são só contra o kitty-OS no QEMU ou no hardware
  dos donos do projeto, **nunca contra sistemas de terceiros**.
- Cada falha tem um PoC em `tests/security/redteam/`, que falha enquanto a
  falha existir (marcado `aberta` no manifesto, para a CI não ficar
  vermelha), e uma issue com as etiquetas `vulnerabilidade`, `red-team` e
  `para-claude`/`para-codex`, com severidade, impacto e passos para
  reproduzir.
- O dono da área corrige; o PR de correção faz o PoC passar, marca-o como
  `corrigida` e acrescenta um teste de regressão.
- O placar fica no fim do `docs/REDTEAM.md`.

## Regras de `unsafe`
- No topo do crate do kernel: `deny(unsafe_code)`,
  `deny(unsafe_op_in_unsafe_fn)`, `deny(clippy::undocumented_unsafe_blocks)`,
  `deny(clippy::missing_safety_doc)`, `deny(clippy::multiple_unsafe_ops_per_block)`.
- Cada bloco `unsafe` tem um comentário `// SAFETY:` a explicar porque é
  seguro.
- O `unsafe` só pode existir em `kernel/src/arch/`, `kernel/src/mm/`,
  `kernel/src/sync/` e nas camadas de acesso a hardware (portas I/O e MMIO),
  que expõem uma API segura ao resto do kernel.
- `docs/UNSAFE.md` lista todos os módulos com `unsafe` e o motivo.
- `kernel/src/caps`, `audit`, `crypto` e `firewall` não têm `unsafe`; a
  crypto passa os vetores de teste oficiais e usa tempo constante para
  dados secretos (D-056).

## Fim de sessão
Acrescentar uma entrada a `docs/HANDOFF.md` (formato no próprio ficheiro) e
atualizar o estado das tarefas em `docs/ROADMAP.md`, no mesmo PR.
