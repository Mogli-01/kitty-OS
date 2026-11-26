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

### [2026-11-25] — CODEX

- **Feito** — CI da #4 em `codex/ci`, com build release sem features de
  teste, validação dos relatórios de exceção e um log por cenário. O
  driver COM1 e as macros `kprint!` e `kprintln!` já estavam no `main`.
- **PRs abertos** — CI da #4 em `codex/ci` para `main`. O #3 foi
  integrado com a CI de arranque verde e recebeu a revisão do Codex.
- **Testes/fuzzing** — `fmt`, Clippy, build release e `actionlint` passaram.
  Os 12 arranques da matriz passaram em QEMU 8.2.2 num contentor Ubuntu
  24.04. Os testes confirmam a entrada e o regresso de #BP, a leitura de
  uma página ausente em #PF e o relatório IST de #DF. Fuzzing nesta fase
  fica por iniciar.
- **Benchmarks** — sem medições nesta sessão.
- **Proteções de segurança alteradas** — a CI conserva o token com acesso
  só de leitura e deixa de persistir as credenciais no checkout.
- **Bloqueado por / à espera do Claude** — a #7 (`para-claude`) regista
  os ajustes aos contratos de `unsafe` do `int3` e do autoteste de #PF.
- **Próximo passo sugerido para o Claude** — ajustar o assembly em
  `arch/x86_64/cpu.rs` para refletir os acessos à pilha e à memória e
  implementar o acesso do autoteste de #PF sem depender do contrato de
  `read_volatile`. Manter os marcadores da série e atualizar `UNSAFE.md`.
- **Avisos** — os testes verificam o comportamento observado no QEMU.
  As garantias dadas ao compilador exigem revisão do código. Sem `unsafe`
  novo neste ramo.

### [2026-11-25] — CLAUDE (4.ª sessão)
- **Feito (Fase 3 completa, em 3 PRs empilhados):**
  - **3a (#5) PMM:** base de dados de frames com lista livre fora de banda,
    contagem de referências, frames limpos a zero, deteção de double free.
  - **3b (#6) paginação própria:** CR3 nosso, W^X por secção garantido pela
    API e auditado no arranque, NX/WP/PGE/PAT, HHDM com 2 MiB e alias do
    kernel só de leitura, páginas de guarda em todas as pilhas, GDT/IDT só
    de leitura, `map_mmio` + `MmioRegion`.
  - **3c (este) heap:** `#[global_allocator]` com classes de tamanho
    (links codificados, zeros ao libertar) e alocações grandes com página de
    guarda; `console::register(Box<dyn Console>)`; `xtask test --expect`.
  - 7 autotestes de falhas, todos verificados em debug e release com
    `--expect` (razão do panic): page-fault, double-fault, stack-overflow,
    write-text, exec-data, write-idt, heap-overflow.
- **PRs abertos:** #5 (base `main`), #6 (base #5), este (base #6). Fazer
  merge por esta ordem e mudar a base de cada um para `main`.
- **Proteções de segurança alteradas:** ATIVAS — NX, W^X em todo o kernel,
  CR0.WP, GDT/IDT só de leitura, páginas de guarda (pilhas e heap), frames
  e heap limpos a zero, lista livre do PMM fora de banda, safe linking no
  heap, `map_mmio` recusa RAM. Ver `docs/SECURITY.md`.
- **Bloqueado por / à espera do Codex:** nada.
- **Próximo passo sugerido para o Codex:**
  1. Issue #4: pôr na CI os 7 autotestes, **sempre com `--expect`** (lista
     e textos em `docs/INTERFACES.md`). Sem `--expect`, em release um teste
     pode passar por engano (ver D-036).
  2. Começar os drivers da Fase 4 (APIC, timer, teclado) usando
     `map_mmio`/`MmioRegion` e `Port`.
- **Avisos:**
  - **Não alocar no heap dentro de handlers de interrupção** até eu fazer o
    `IrqSpinLock` (Fase 4).
  - A memória "bootloader reclaimable" ainda não é reutilizada (~50 MiB no
    QEMU): é preciso copiar o memory map antes.
  - O segredo do safe linking vem do TSC (fraco) até haver RNG (Fase 8).
  - O espaço virtual das alocações grandes e do MMIO não é reutilizado.

### [2026-11-25] — CODEX (testes de memória)

- **Feito** — `cargo xtask test-memory` verifica o PMM, a auditoria W^X,
  a pilha guardada, o heap, o MMIO e os sete autotestes de falha. A CI
  conserva um log série por execução. Revisões entregues nos PRs #5, #6 e #9.
- **PRs abertos** — #10 (`codex/testes-memoria`) em rascunho sobre o #9.
  No #8, a decisão sobre os testes de exceções mudou para D-100 para ficar
  distinta da D-028 do PMM.
- **Testes/fuzzing** — 32 arranques passaram em QEMU 8.2.2, em BIOS e UEFI,
  em debug e release. Três testes de validação dos logs passaram no host
  e em Miri. Os testes no host e o fuzzing do allocator estão pendentes.
- **Benchmarks** — sem medições nesta sessão.
- **Proteções de segurança alteradas** — verificações adicionais na CI,
  com token `contents: read` e checkout sem persistência de credenciais.
- **Bloqueado por / à espera do Claude** — integração do #9 para mudar a
  base do #10 para `main`.
- **Próximo passo sugerido para o Claude** — rever os contratos de
  paginação e MMIO no #6, a recuperação de alocações parciais e a
  validação da lista livre no #9. Aplicar a revisão da #7 aos novos
  autotestes que geram falhas.
- **Avisos** — os testes em QEMU verificam os relatórios esperados. A
  validação dos contratos de `unsafe` continua nas revisões do núcleo.

### [2026-11-25] — CLAUDE (5.ª sessão)
- **Feito:** subagentes do Claude Code em `.claude/agents/`:
  `security-reviewer` (revê o diff antes de cada PR, só reporta),
  `test-writer` (testes só para a área do Claude) e `explorer` (resumo do
  estado no início de cada sessão). Regras de uso em `CLAUDE.md` (no máximo
  2 em paralelo, só para tarefas independentes).
- **PRs:** revi o #8 e o #10 do Codex (prontos, sem bloqueios). Com
  aprovação do Rodrigo no chat, entraram no `main` por esta ordem: #8 → #5
  → #6 → #9 → #10 → este (#12). Fechadas: #4, #7, #11.
- **Proteções de segurança alteradas:** nenhuma no kernel. Processo: cada PR
  do Claude passa a ter uma revisão de segurança automática antes de abrir.
- **Bloqueado por / à espera do Codex:** nada.
- **Próximo passo sugerido para o Codex:** drivers da Fase 4 (APIC, timer,
  teclado) com `map_mmio`/`MmioRegion` e `Port`. Opcional: juntar
  `ci.yml` e `memory.yml` num só workflow (revisão do #10).
- **Avisos:** os ficheiros em `.claude/agents/` são do Claude (não editar);
  o `test-writer` assume a infraestrutura de autotestes da Fase 3 (#9).

### [2026-11-25] — CODEX (teclado PS/2)

- **Feito** — descodificador Set 2 em `drivers/ps2`, com eventos de teclas
  físicas e documentação da ligação ao i8042. Revisão do #12 entregue.
  Os PRs #8 e #10 de CI entraram no main, incluindo a cobertura da #11.
- **PRs abertos** — #14 (`codex/ps2-set2`) para `main`.
- **Testes/fuzzing** — oito testes de descodificação passaram no host e em
  Miri. A cobertura inclui todos os 2 816 pares de estado e byte possíveis,
  com recuperação após erro. O crate compilou para `x86_64-unknown-none`.
- **Benchmarks** — sem medições nesta sessão.
- **Proteções de segurança alteradas** — o descodificador trabalha com
  estado de tamanho fixo, valida a sequência Pause e descarta códigos
  desconhecidos. O crate proíbe `unsafe`.
- **Bloqueado por / à espera do Claude** — o #15 responde à #13 com
  o contrato IRQ1 e o ponto de inicialização dos drivers.
- **Próximo passo sugerido para o Claude** — responder à #13 e rever as
  permissões de shell dos subagentes de leitura conforme a revisão do #12.
- **Avisos** — o descodificador espera Set 2 sem tradução. O transporte
  i8042 trata respostas a comandos, distingue a porta auxiliar e reinicia
  o descodificador quando perde dados.

### [2026-11-26] — CODEX (pacotes do rato PS/2)

- **Feito** — decoder de pacotes PS/2 de três bytes com deslocamento
  e estado dos botões. Decisão D-117.
- **PRs abertos** — ramo `codex/ps2-rato` sobre o #14.
- **Testes/fuzzing** — cinco testes novos no host e em Miri, incluindo
  todas as combinações dos bits de cabeçalho e os limites dos eixos.
  Os oito testes existentes do teclado continuam a passar.
- **Benchmarks** — sem medições do caminho de input.
- **Bloqueado por / à espera do Claude** — Phase 4 para a integração
  de IRQ12 e interface INPUT para a entrega futura.
- **Próximo passo sugerido para o Claude** — confirmar as regras da
  transição do PIC com linhas de teclado e rato configuradas.
- **Avisos** — modo base de três botões, ainda sem roda e sem
  ligação ao IRQ12. O transporte tem de restabelecer o alinhamento
  após perda de bytes.
