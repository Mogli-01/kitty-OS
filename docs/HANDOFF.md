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

### [2026-11-25] — CLAUDE (6.ª sessão)
- **Feito (Fase 4, parte do núcleo):**
  - `IrqSpinLock`; todos os locks globais convertidos. `cli`/`sti`/`hlt`
    deixaram de ter `nomem` (D-037); idle com `sti; hlt` atómico.
  - Núcleo de interrupções: stubs e gates para os 256 vetores,
    `irq::register_isa`/`unregister_isa`, dispatch com **EOI pelo núcleo**,
    filtragem de IRQs espúrios, trait `InterruptController` +
    `set_controller` (D-039).
  - PIC 8259 como controlador de arranque (remapeado para 0x20–0x2F, tudo
    mascarado até haver registo).
  - Crate `kitty-hal` (`PortBlock`, `RegisterBlock`) e
    `arch::legacy::claim` (i8042, PIT, CMOS) para drivers sem `unsafe`
    (D-038); `devices::init()` antes do `sti` (D-040).
  - Autotestes: dispatch por `int 0xF0`, `IrqSpinLock` e um IRQ0 real do
    PIT através do PIC (`selftest-irq-pit`).
  - Respondi à #13 em `docs/INTERFACES.md` ("Interrupções (Fase 4)").
  - Primeiro uso do `security-reviewer`: encontrou 4 pontos, todos
    corrigidos — comandos perigosos do i8042 (reset/A20) e bit NMI do CMOS
    filtrados; vetores de controladores limitados a 0x20–0xEF; EOI decidido
    pela origem do vetor; regra LIFO dos `IrqSpinLock` documentada.
  - Revisão do Codex ao #15 (2 pontos, corrigidos): os guards dos locks
    passaram a `!Send` e só `Sync` com `T: Sync` (antes, um guard de
    `Cell` era `Sync`: data race em código seguro), com asserções em tempo
    de compilação; o autoteste do PIT programa o timer antes de
    desmascarar e exige 2 interrupções depois de uma baseline (prova o
    EOI).
- **PRs abertos:** `claude/fase4-irq` (este).
- **Proteções de segurança alteradas:** ATIVAS — locks que desligam
  interrupções, PIC remapeado/mascarado, EOI só pelo núcleo, portas legadas
  por allow-list com dono único, drivers sem `unsafe`.
- **Bloqueado por / à espera do Codex:** nada.
- **Próximo passo sugerido para o Codex:**
  1. Transporte i8042 + teclado: `kernel/src/devices/keyboard.rs` com
     `legacy::claim(LegacyDevice::I8042)` e `irq::register_isa(1, …)` (exemplo
     em INTERFACES.md). Atualizar o `AGENTS.md` para permitir ficheiros em
     `kernel/src/devices/` (Rust seguro).
  2. Driver de I/O APIC + local APIC como `InterruptController` e timer.
  3. Pôr `selftest-irq-pit` no `test-memory`/CI (sem `--expect-panic`).
- **Avisos:**
  - Os handlers correm com interrupções desligadas e **não** enviam EOI.
  - O transporte i8042 só pode enviar os comandos de
    `I8042_ALLOWED_COMMANDS`; se faltar algum, pedir numa issue.
  - `set_controller` esquece as linhas ligadas pelo controlador anterior:
    os drivers têm de se registar de novo depois de o APIC tomar o lugar.
  - Ainda não há timer periódico nem scheduler (Fase 5).

### [2026-11-26] — CLAUDE (7.ª sessão)
- **Feito:**
  - PR #15 (Fase 4): respondi à revisão do Codex. Os guards dos locks
    passaram a `!Send` e a `Sync` só com `T: Sync` (havia uma data race
    possível em código seguro), com asserções em tempo de compilação; o
    autoteste do PIT exige 2 IRQs depois de uma baseline (prova o EOI) e
    pára o PIT sem deixar IRQ pendente. `security-reviewer`: OK.
  - Revi #16, #17, #19 e #21 do Codex (prontos; notas não bloqueantes).
  - **Issue #18** (este PR, empilhado sobre o #15): descoberta ACPI no
    núcleo (RSDP físico → XSDT/RSDT → MADT, tudo validado, MADT copiada),
    `vmm::copy_from_firmware`, `RegisterBlock` para `MmioRegion`,
    `devices::init(&Platform)` com a MADT, e capacidades
    `arch::apic::local_apic()` / `io_apic(phys)` com filtro de valores
    (ICR fechado, sem SMI/INIT). Transição PIC → APIC documentada em
    INTERFACES.md. Decisões D-041 e D-042.
  - O `security-reviewer` encontrou 7 problemas na 1.ª versão, todos
    corrigidos antes de abrir o PR: contorno **confirmado** do filtro do
    I/O APIC (seletor herdado de outro dono), TOCTOU seletor/janela,
    vetores não filtrados (0x10–0x1F desalinhariam a pilha), I/O APIC
    declarado por cima do ICR, firmware lido por deny-list (aceitava
    MMIO), `&'static [u8]` sobre memória de firmware e dupla leitura.
  - **Issue #22** (PR seguinte, `claude/initrd-modulo`, base #35): o
    módulo `kitty-initrd` do Limine é validado pelo núcleo (marca exata,
    único, tamanho, todo em memória de módulos) e entregue em
    `Platform::initrd` como `&'static [u8]` só de leitura e NX (D-043).
    Testado à mão no QEMU: 1 módulo → validado; 2 → recusado (ambíguo);
    marca parecida → ignorado. O `security-reviewer` confirmou que um
    "módulo" por cima da imagem do kernel passava a validação: agora é
    recusado (verificação por página), tal como páginas com alias RW e
    mais de 64 módulos.
- **PRs abertos:** #15 (Fase 4, pronto); #35 (`claude/fase4-acpi-apic`,
  base #15); `claude/initrd-modulo` (base #35).
- **Proteções de segurança alteradas:** ATIVAS — validação ACPI e cópia da
  MADT; capacidades APIC com dono único, UC/NX, ICR fechado e sem SMI/INIT.
- **Bloqueado por / à espera do Codex:** nada.
- **Próximo passo sugerido para o Codex:** ligar o `kitty-acpi` (#20) e o
  driver de I/O APIC (#21) em `kernel/src/devices/` pela ordem descrita em
  INTERFACES.md ("Transição PIC → APIC"), com o local APIC como
  `InterruptController`. Testar com o IRQ0 repetido e uma tecla, como a
  #18 propõe. **Atenção ao #20:** o `libfuzzer-sys` é um crate externo novo
  (workspace de fuzzing só no host) e precisa de entrada em DECISIONS e de
  aprovação humana, mesmo não entrando no kernel.
- **Avisos:**
  - x2APIC não é suportado: nessas máquinas o kernel fica no PIC.
  - Os mapeamentos de firmware e MMIO não são libertados (espaço virtual
    da janela MMIO não é reutilizado).
  - A sequência IOREGSEL/IOWIN tem de ser serializada pelo driver.

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

### [2026-11-25] — CODEX (transporte i8042)

- **Feito** — transporte i8042 com `PortBlock`, inicialização Set 2
  limitada e leitura de um byte por chamada. Resolvido o conflito do #14
  com o `main`. Revisão do #15 publicada.
- **PRs abertos** — #14 (`codex/ps2-set2`) e `codex/i8042` sobre o #15.
- **Testes/fuzzing** — cinco testes do transporte passaram no host e em
  Miri. Num kernel de teste com ligação temporária a IRQ1, QEMU 8.2.2 em
  BIOS e UEFI entregou `1c f0 1c` para a pressão e libertação da tecla
  `A`. A CI do #14 passou.
- **Benchmarks** — sem medições nesta sessão.
- **Proteções de segurança alteradas** — o transporte proíbe `unsafe`,
  valida a origem e o estado dos bytes e limita as esperas e reenvios.
- **Bloqueado por / à espera do Claude** — a ligação permanente do
  teclado precisa das APIs do #15 e do descodificador do #14.
- **Próximo passo sugerido para o Claude** — rever a afinidade dos
  guards e o teste de entrega e EOI do PIT conforme a revisão do #15.
- **Avisos** — as verificações QEMU foram feitas com integração de
  teste. O ponto de inicialização do teclado ainda aguarda o PR de ligação.

### [2026-11-25] — CODEX (integração do teclado)

- **Feito** — `devices::init` reclama e configura o i8042, liga IRQ1 e
  encaminha eventos Set 2 para uma fila de 64 entradas. A fila sinaliza
  `Lost` quando perde dados. `cargo xtask test-keyboard` entrou na CI.
- **PRs abertos** — `codex/ps2-integracao` sobre o #16, com o descodificador
  do #14 e a API de interrupções do #15.
- **Testes/fuzzing** — pressão e libertação de `A` e da seta para cima
  confirmadas em QEMU 8.2.2 em BIOS e UEFI, debug e release. Os três
  testes novos de fila, overflow e recuperação após perda passaram no
  host e em Miri. Também passou o arranque BIOS/release sem a feature.
- **Benchmarks** — sem medições nesta etapa.
- **Proteções de segurança alteradas** — um byte por IRQ e memória fixa;
  logs de tecla na série só com a feature de teste. Sem `unsafe` novo.
- **Bloqueado por / à espera do Claude** — aprovação e integração dos
  PRs de base. A entrega de eventos à GUI pertence à Fase 11.
- **Próximo passo sugerido para o Claude** — concluir a revisão da API
  de interrupções do #15 para ligar os drivers sobre o `main`.
- **Avisos** — a fila guarda eventos físicos. A camada de input aplica o
  layout e o timestamp depois desta etapa.
