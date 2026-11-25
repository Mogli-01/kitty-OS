# Registo de passagem de turno

Cada sessão (do Claude ou do Codex) acaba com uma entrada nova **no fim**
deste ficheiro, no mesmo PR do trabalho. Ler as últimas entradas antes de
começar qualquer sessão.

Formato:

```
### [AAAA-MM-DD] — CLAUDE | CODEX
- Feito: ...
- PRs abertos: ...
- Proteções de segurança alteradas: ...
- Bloqueado por / à espera do <outro>: ...
- Próximo passo sugerido para o <outro>: ...
- Avisos (bugs conhecidos, unsafe novo, coisas frágeis): ...
```

---

### [2026-11-25] — CLAUDE
- **Feito:**
  - Estrutura do repositório: workspace Cargo (`kernel`, `xtask`),
    `rust-toolchain.toml` (nightly-2026-11-20), `.cargo/config.toml`,
    perfis de release (`opt-level=3`, `lto="fat"`, `codegen-units=1`,
    `panic="abort"`), pastas `drivers/`, `fs/`, `userland/`, `tests/` com
    README.
  - Ficheiros de coordenação: `CLAUDE.md`, `AGENTS.md` (versão inicial — a
    partir de agora é do Codex), `CONTRIBUTING.md`, e em `docs/`:
    ARCHITECTURE, ROADMAP, HANDOFF, DECISIONS (D-001…D-016), INTERFACES,
    SECURITY, UNSAFE.
  - **Fase 1 completa:** o kernel arranca via Limine 11.4.1 e escreve
    `Hello from kitty-OS` na COM1 e no framebuffer (fonte 8x8 a 2x). Panic
    handler mostra `ficheiro:linha:coluna` + mensagem.
  - `cargo xtask build | esp | iso | run | test` (só std).
  - CI inicial: fmt, clippy (debug, release, feature `qemu-exit`) e teste de
    arranque UEFI, BIOS (ISO) e release.
- **PRs abertos:** `claude/estrutura-inicial` (este).
- **Proteções de segurança alteradas:** ativadas W^X por segmento ELF,
  requests do Limine só de leitura, confinamento de `unsafe` por lint,
  proibição de `unwrap`/`expect` no kernel, Limine e actions fixados por
  commit, CI só com `contents: read` (ver `docs/SECURITY.md`).
- **Bloqueado por / à espera do Codex:** nada.
- **Próximo passo sugerido para o Codex:**
  1. Rever este PR (sobretudo `xtask` e a CI, que passam a ser teus).
  2. Acrescentar um teste de panic ao `cargo xtask test` (kernel compilado
     com uma feature que provoca um panic; esperar código 35 e
     `KERNEL PANIC` na série).
  3. Preparar o esqueleto de `drivers/` como crate(s) `no_std` que dependam
     só das interfaces de `docs/INTERFACES.md`.
- **Avisos:**
  - `SpinLock` ainda **não** desativa interrupções (não há interrupções
    até à Fase 2/4). Não usar em handlers até existir `IrqSpinLock`.
  - A consola do framebuffer faz scroll copiando o ecrã inteiro (lento, mas
    só afeta o log de arranque; otimizar só se for medido).
  - `unsafe` novo: ver `docs/UNSAFE.md` (port, cpu, serial, qemu, boot,
    spinlock).
  - No Windows não há `xorriso`, por isso `--bios` só foi testado na CI.

### [2026-11-25] — CLAUDE (2.ª sessão)
- **Feito:**
  - Roadmap: Fases 10–14 (gráficos base, input, memória partilhada e IPC,
    compositor, toolkit e aplicações) e tarefas **GUI-PREP** nas Fases 3,
    5 e 6 para o núcleo já nascer preparado (contagem de referências de
    frames, PAT/tipos de cache, filas de espera, handles com direitos).
  - Decisões D-017 (compositor em user space), D-018 (objetos + handles com
    direitos desde a Fase 6), D-019 (input só para quem tem `INPUT`), D-020
    (PAT gerido pelo VMM), D-021 (desenho do IPC).
  - `docs/SECURITY.md`: secção "Isolamento entre janelas" e novas proteções.
  - `docs/ARCHITECTURE.md`: arquitetura gráfica e o que ela exige ao núcleo.
  - `docs/INTERFACES.md`: interfaces planeadas para gráficos, input e IPC.
- **PRs abertos:** `claude/estrutura-inicial` (#1); `claude/roadmap-interface-grafica`
  (este, empilhado sobre o #1).
- **Proteções de segurança alteradas:** nenhuma ativada; planeadas as de
  isolamento entre janelas (ver SECURITY.md).
- **Bloqueado por / à espera do Codex:** nada.
- **Próximo passo sugerido para o Codex:** rever o formato proposto para
  `InputEvent` (é teu na Fase 11) e dizer numa issue `para-claude` se
  precisas de algo diferente nas interfaces de gráficos/input. Nota: na
  Fase 10, programar o PAT é do núcleo (D-020); tu usas `CacheType`.
- **Avisos:** só documentação, sem código. As assinaturas das interfaces
  planeadas podem mudar quando forem implementadas.

### [2026-11-25] — CLAUDE (3.ª sessão)
- **Feito:**
  - **Fase 2 completa.** GDT própria (layout pronto para `sysret`), TSS com
    3 pilhas IST (double fault, NMI, machine check) e sem bitmap de I/O;
    IDT com os 32 vetores de exceção (DPL 0, interrupt gates); stubs em
    assembly que guardam todos os registos num `TrapFrame`.
  - Relatório de exceção: nome, vetor, código de erro descodificado (#PF:
    CR2 e tipo de acesso; #GP/#NP/#SS/#TS: tabela e índice do seletor),
    RIP/CS/RFLAGS/RSP/SS e todos os registos — **só na série** (D-023).
  - Autoteste no arranque: `int3` tem de chegar ao handler e voltar.
  - Features `selftest-page-fault` e `selftest-double-fault`;
    `cargo xtask test --features … --expect-panic`.
  - Decisões D-022…D-027; SECURITY, UNSAFE, INTERFACES e ARCHITECTURE
    atualizados.
- **PRs abertos:** `claude/fase2-gdt-idt` (este).
- **Proteções de segurança alteradas:** ATIVAS — pilhas IST, IDT com DPL 0
  e vetores não usados não presentes, TSS sem bitmap de I/O, endereços só
  na série. PLANEADA — GDT/IDT só de leitura (Fase 3).
- **Bloqueado por / à espera do Codex:** nada.
- **Próximo passo sugerido para o Codex:** pôr na CI os dois autotestes
  (`cargo xtask test --features selftest-page-fault --expect-panic` e o mesmo
  com `selftest-double-fault`) e verificar no `build/serial-test.log` os
  textos estáveis listados em `docs/INTERFACES.md`. Há uma issue
  `para-codex` com isto.
- **Avisos:**
  - As interrupções continuam desligadas (IF=0); IRQs só na Fase 4.
  - As pilhas IST ainda não têm página de guarda (Fase 3).
  - `swapgs` ainda não é tratado nos stubs: obrigatório antes de haver
    ring 3 (Fase 6); está marcado com TODO em `trap.rs`.
  - GDT/TSS só existem para o CPU de arranque; SMP (Fase 9) precisa de uma
    por CPU.
