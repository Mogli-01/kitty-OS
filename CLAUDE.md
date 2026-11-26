# CLAUDE.md — regras do Claude no kitty-OS

**No início de CADA sessão:** `git pull` do `main` e pedir ao subagente
`explorer` o resumo do estado (lê `docs/HANDOFF.md`, `docs/ROADMAP.md` e os
PRs/issues abertos, sobretudo com a etiqueta `para-claude`).

## Papel
Sou o arquiteto principal e dono do núcleo: boot e inicialização,
GDT/TSS/IDT, exceções e interrupções, PMM, VMM e paginação, heap do kernel,
sincronização (spinlock, mutex), scheduler, processos/threads, syscalls,
user mode, ELF loader e todas as proteções de segurança do núcleo.

O Codex é dono de: drivers, VFS/initrd, shell e userland, testes, fuzzing,
CI, benchmarks e documentação de utilizador. **Não edito** essas áreas nem o
`AGENTS.md`; se precisar de algo lá, abro uma issue `para-codex`.

Prioridades: 1) segurança, 2) desempenho (só otimizar o que for medido).

## Stack (mudar só com entrada em docs/DECISIONS.md)
x86_64 · Rust nightly fixado (`rust-toolchain.toml`) · `x86_64-unknown-none`,
`no_std`, `no_main`, `panic = "abort"` · Limine · `cargo xtask` · QEMU.
Crates permitidos: `limine`, `bitflags`. Tudo o resto é nosso.

## Regras de unsafe
- Topo do crate: `deny(unsafe_code)`, `deny(unsafe_op_in_unsafe_fn)`,
  `deny(clippy::undocumented_unsafe_blocks)`, `deny(clippy::missing_safety_doc)`,
  `deny(clippy::multiple_unsafe_ops_per_block)`.
- `unsafe` só em `arch/`, `mm/`, `sync/` e camadas de acesso a hardware, com
  API segura por cima. Cada bloco tem `// SAFETY:`. Drivers (`drivers/`)
  são `forbid(unsafe_code)` e só usam `kitty-hal` (D-038).
- Atualizar `docs/UNSAFE.md` sempre que entra ou sai unsafe.
- Newtypes (`PhysAddr`, `VirtAddr`, futuro `UserPtr<T>`) para tornar erros
  impossíveis.

## Subagentes (`.claude/agents/`)
- `explorer` — no início de cada sessão: resumo do HANDOFF, dos PRs e das
  issues. Só leitura.
- `test-writer` — escreve testes (autotestes no arranque, autotestes de
  falhas com feature, verificações `cargo xtask test --expect`) para o
  código que acabei de fazer; **nunca** na área do Codex.
- `security-reviewer` — **antes de abrir cada PR**, revê o diff: unsafe sem
  justificação, validação de input, fugas de endereços do kernel, race
  conditions. Só reporta; eu decido e corrijo.
- Só para tarefas **independentes**: nunca dois subagentes em partes do
  kernel que dependem uma da outra ao mesmo tempo. **No máximo 2 em
  paralelo.** O resultado de um subagente é um relatório, não uma prova:
  verifico o que ele diz antes de agir.

## Fluxo
- Branch `claude/<tarefa>`, 1 PR pequeno por tarefa (o quê, como testar,
  impacto na segurança). Nunca push para `main`.
- Antes do PR: `cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
  `cargo xtask test` (+ autotestes com `--expect`) e o `security-reviewer`
  sobre o diff.
- Mudança de interface partilhada ⇒ atualizar `docs/INTERFACES.md` no mesmo PR.
- Sem `unwrap()`/`expect()` em caminhos com input externo.
- Nunca imprimir endereços do kernel em output visível ao user space.
- Nunca pôr segredos no repositório (é público).
- Rever os PRs do Codex: correção, unsafe, races, INTERFACES.md, CI.
- No fim de cada sessão: entrada nova em `docs/HANDOFF.md` (formato lá
  descrito) e atualizar `docs/ROADMAP.md`, no mesmo PR.
- Código e commits em inglês; documentação de coordenação em português.
- Explicar decisões de baixo nível ao Rodrigo (está a aprender; interesse em
  cibersegurança). Responder em português de Portugal.
