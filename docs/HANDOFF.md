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

### [2026-11-25] — CODEX (descodificador MADT)

- **Feito** — parser sem alocação para a MADT xAPIC/ISA e primeira
  infraestrutura `cargo-fuzz` no host. Decisão D-106.
- **PRs abertos** — ramo `codex/acpi-madt` para `main` e issue #18
  para a descoberta da tabela e a capacidade de MMIO.
- **Testes/fuzzing** — sete testes no host e em Miri. O mesmo parser
  validou tabelas extraídas do QEMU q35 com dois vCPUs em BIOS e UEFI,
  incluindo IRQ0 → GSI 2. A primeira sessão de fuzzing com ASan correu
  mais de 1,2 milhões de casos sem falhas.
- **Benchmarks** — sem medições comparáveis nesta sessão.
- **Bloqueado por / à espera do Claude** — contrato de descoberta ACPI
  e acesso seguro a APIC (#18).
- **Próximo passo sugerido para o Claude** — definir o acesso à MADT e
  o ponto de transição de PIC para APIC na #18.
- **Avisos** — NMI e x2APIC ficam para a integração. O parser trabalha
  sobre bytes e ainda não configura hardware. Sem `unsafe` novo.

### [2026-11-26] — CODEX (rotas NMI na MADT)

- **Feito** — `kitty-acpi` passa a cruzar o ID xAPIC do CPU ativo com o
  UID ACPI para resolver NMI por pino LINT. Decisão D-122.
- **PRs abertos** — atualização do #20. Driver de registos do Local APIC
  em revisão no #37.
- **Testes/fuzzing** — 11 testes no host e em Miri. O ensaio ASan correu
  1 047 603 casos em 16 segundos. Nas duas MADT de QEMU 8.2.2 com dois
  vCPUs, o IRQ0 é encaminhado para GSI 2 e ambos os CPUs recebem NMI por
  LINT1, tanto em BIOS como em UEFI.
- **Benchmarks** — sem medições nesta sessão.
- **Proteções de segurança alteradas** — IDs repetidos, campos reservados
  e definições contraditórias de NMI bloqueiam a seleção da rota.
- **Bloqueado por / à espera do Claude** — revisão das capacidades de
  MMIO do #35 e registo do vetor do timer na #36.
- **Próximo passo sugerido para o Claude** — resolver a validação dos
  endereços antes da transição de PIC para APIC.
- **Avisos** — NMI descrita para I/O APIC ou x2APIC exige uma política
  de encaminhamento própria antes da transição.

### [2026-11-25] — CODEX (I/O APIC)

- **Feito** — driver de programação das entradas do I/O APIC por acesso
  exclusivo ao par IOREGSEL/IOWIN. Valida os limites GSI, a máscara e
  os campos de uma rota de entrega fixa. Decisão D-107.
- **PRs abertos** — ramo `codex/ioapic` sobre o #15.
- **Testes/fuzzing** — oito testes no host e em Miri. Uma ligação qtest
  ao I/O APIC de QEMU 8.2.2/q35 confirmou 24 entradas e a programação,
  desmascaramento e máscara de uma rota em GSI 2.
- **Benchmarks** — sem medições de latência de IRQ nesta sessão.
- **Bloqueado por / à espera do Claude** — concessão de MMIO e ordem
  de transição para APIC (#18).
- **Próximo passo sugerido para o Claude** — definir o contrato para o
  Local APIC e manter o handler antigo disponível até drenar o IRQ.
- **Avisos** — ensaio qtest valida os registos do dispositivo emulado.
  A entrega ao núcleo pela rota APIC será testada na integração.

### [2026-11-26] — CODEX (Local APIC e timer)

- **Feito** — driver xAPIC com leitura do ID, ativação da entrega com
  vetor espúrio reservado, EOI e timer em modo one-shot e periódico. A
  configuração verifica o divisor, vetor e máscara por leitura. Decisão D-121.
- **PRs abertos** — ramo `codex/local-apic`, baseado no #21.
- **Testes/fuzzing** — 16 testes APIC no host e em Miri, oito novos.
  Numa cópia de teste sobre o #35, QEMU 8.2.2 confirmou a expiração one-shot,
  recarga periódica e paragem do timer em BIOS e UEFI, debug e release,
  através da capacidade `LocalApicRegisters` e com o IRQ mascarado.
- **Benchmarks** — sem medições de frequência do timer ou latência de IRQ.
- **Proteções de segurança alteradas** — o driver usa exclusivamente
  `RegisterBlock`. Vetores do timer são limitados ao intervalo dinâmico.
  Uma configuração parcial impede o arranque e desmascaramento do timer.
- **Bloqueado por / à espera do Claude** — validar a identidade das
  janelas de MMIO e os limites da leitura de firmware no #35.
- **Próximo passo sugerido para o Claude** — resolver os pontos da revisão
  do #35 e definir o registo de um handler para o timer local no núcleo.
- **Avisos** — a entrega efetiva ao handler e o EOI do APIC ficam para a
  integração. O arranque usa o PIC 8259.

### [2026-11-26] — CODEX (controlador APIC)

- Feito — ligação da MADT aos controladores I/O APIC e LAPIC. D-138.
- PRs abertos — ramo `codex/ioapic-integracao` sobre #37, incluindo #20.
- Testes/fuzzing — 22 testes APIC no host. A feature selftest-xapic
  exige controlador APIC e duas confirmações de EOI no QEMU.
- Benchmarks — sem medições.
- Proteções de segurança alteradas — rotas validadas antes da mudança,
  sem unsafe novo, com verificação de interrupções pendentes.
- Bloqueado por / à espera do Claude — revisão da transição e marcador
  antigo do autoteste PIT, que ainda menciona o PIC.
- Próximo passo sugerido para o Claude — rever o controlador e tornar
  o marcador do autoteste independente do controlador ativo.
- Avisos — integração opcional no arranque, limitada ao CPU de arranque.

### [2026-11-27] — CODEX (fallback do xAPIC na CI)

- **Feito** — a CI verifica o PIC com xAPIC ativo e MADT indisponível
  simulada à entrada do driver.
- **PRs abertos** — ramo `codex/xapic-pic-fallback-test` sobre o #62.
- **Testes/fuzzing** — quatro arranques QEMU com simulação e quatro
  com APIC ativo, em BIOS e UEFI, debug e release.
- **Benchmarks** — sem novas medições.
- **Proteções de segurança alteradas** — a injeção está limitada a
  builds com a feature de teste e `qemu-exit`.
- **Bloqueado por / à espera do Claude** — nada neste ensaio.
- **Próximo passo sugerido para o Claude** — tornar o texto do
  autoteste PIT independente do controlador ativo quando for revisto.
- **Avisos** — a simulação cobre a decisão do driver após a
  descoberta de firmware.

### [2026-11-27] — CODEX (arranque BIOS sem RSDP)

- **Feito** — a matriz BIOS acrescenta um arranque QEMU com
  `pc,acpi=off` e `selftest-xapic`. Exige `NoRsdp`, o PIC
  como controlador ativo e duas entregas de IRQ0.
- **PRs abertos** — atualização do #65.
- **Testes/fuzzing** — o passo da CI passou localmente em BIOS debug
  e release com QEMU 10.0.13. Num checkout conjunto com o #71,
  teclado e rato também passaram via PIC em BIOS sem RSDP e em UEFI
  sem MADT, com ACPI desativado nas máquinas testadas.
- **Benchmarks** — sem novas medições.
- **Proteções de segurança alteradas** — cobertura adicional da
  descoberta de firmware e da entrega de interrupções.
- **Bloqueado por / à espera do Claude** — nenhuma dependência deste ensaio.
- **Próximo passo sugerido para o Claude** — atualizar o texto do
  autoteste PIT quando for revista a transição do controlador.
- **Avisos** — a matriz mantém o ensaio de ausência simulada em BIOS e UEFI.

### [2026-11-27] — CODEX (arranque UEFI sem MADT)

- **Feito** — o ensaio de descoberta com ACPI desativado cobre também
  UEFI com a máquina `q35`. Verifica a ausência do RSDP ou da MADT, o
  PIC ativo e duas entregas de IRQ0.
- **PRs abertos** — atualização do #65.
- **Testes/fuzzing** — o passo da CI passou em BIOS e UEFI, debug e
  release, com QEMU 10.0.13. A série confirmou `NoMadt` nos dois
  arranques UEFI e `NoRsdp` nos dois BIOS. Passaram formatação,
  Clippy, actionlint e a validação de sintaxe Bash.
- **Benchmarks** — sem medições nesta ronda.
- **Proteções de segurança alteradas** — cobertura da descoberta de
  firmware e do encaminhamento pelo PIC em UEFI.
- **Bloqueado por / à espera do Claude** — nenhuma dependência deste ensaio.
- **Próximo passo sugerido para o Claude** — rever o texto do autoteste PIT.
- **Avisos** — o resultado de ausência pode variar com o firmware UEFI.

### [2026-11-26] — CLAUDE (issue #36)
- **Feito:** `arch::irq::register_local`/`unregister_local` para fontes do
  local APIC sem rota ISA (timer, erro, térmica), com contrato de ciclo de
  vida em INTERFACES.md. PR `claude/irq-local-vector` (base `main`).
- **PRs abertos:** este; #39 e #40 (Fase 5) à espera de revisão.
- **Proteções de segurança alteradas:** nenhuma (o EOI por origem, D-039,
  já cobre interrupções em trânsito depois de `unregister_local`).
- **Próximo passo sugerido para o Codex:** usar `register_local` no timer
  do Local APIC (`codex/local-apic`) e verificar 2 entregas.
- **Avisos:** só o CPU de arranque (cada CPU terá o seu timer na Fase 9).

### [2026-11-26] — CLAUDE (issue #36)
- **Feito:** `arch::irq::register_local`/`unregister_local` para fontes do
  local APIC sem rota ISA (timer, erro, térmica), com contrato de ciclo de
  vida em INTERFACES.md. PR `claude/irq-local-vector` (base `main`).
- **PRs abertos:** este; #39 e #40 (Fase 5) à espera de revisão.
- **Proteções de segurança alteradas:** nenhuma (o EOI por origem, D-039,
  já cobre interrupções em trânsito depois de `unregister_local`).
- **Próximo passo sugerido para o Codex:** usar `register_local` no timer
  do Local APIC (`codex/local-apic`) e verificar 2 entregas.
- **Avisos:** só o CPU de arranque (cada CPU terá o seu timer na Fase 9).

### [2026-11-27] — CODEX (interrupções do timer Local APIC)

- **Feito** — teste do vetor local do LAPIC sobre o #51 e o #65.
  Programa um one-shot, rearma na primeira interrupção e mascara,
  pára e liberta o vetor na segunda. O marcador exige o segundo EOI.
- **PRs abertos** — ramo `codex/lapic-timer-irq` sobre o #65.
- **Testes/fuzzing** — passaram BIOS e UEFI, debug e release, em
  QEMU 10.0.13. Passaram formatação, Clippy e actionlint.
- **Benchmarks** — sem medições nesta ronda.
- **Proteções de segurança alteradas** — ensaio em Rust seguro, via
  capacidade de registos concedida pelo núcleo.
- **Bloqueado por / à espera do Claude** — revisão do contrato #51.
- **Próximo passo sugerido para o Claude** — integrar o registo de
  vetores locais após a revisão humana.
- **Avisos** — a contagem serve o ensaio em QEMU. O tick de produção
  precisa de uma frequência calibrada e da integração do scheduler.

### [2026-11-26] — CLAUDE (7.ª sessão, continuação)
- **Feito:**
  - Com aprovação do Rodrigo no chat: merge de #15, #35 e #38; #13
    fechada. Os PRs #14 e #25 do Codex foram aprovados mas têm conflitos
    com o `main` (comentei a pedir rebase). Revi os outros 12 PRs do Codex
    (comentários nos PRs).
  - **Fase 5 (este PR):** threads do kernel, troca de contexto mínima em
    assembly, scheduler round-robin preemptivo, pilhas de thread com página
    de guarda, contador de preempção nos `SpinLock`, medição em ciclos.
    Decisões D-044 (`unsafe` em `sched/`) e D-045.
- **PRs abertos:** `claude/fase5-scheduler` (este).
- **Proteções de segurança alteradas:** ATIVAS — pilhas de thread com
  guarda e limpas ao libertar; sem preempção com spinlock preso; troca só
  depois do EOI.
- **Bloqueado por / à espera do Codex:** rebase dos #14 e #25; mudanças
  pedidas em #23, #24, #27, #28, #31; adaptar o #34 a
  `devices::init(&Platform)`.
- **Próximo passo sugerido para o Codex:** no driver do timer (#19), o
  handler do IRQ0 chama `sched::tick()` (contrato em INTERFACES.md,
  "Scheduler"). Pôr `selftest-preempt` e `selftest-thread-stack-overflow`
  na CI (o primeiro sem `--expect-panic`).
- **Avisos:**
  - Ainda não há `WaitQueue` (próximo passo do Claude): threads que
    esperam fazem `yield_now()` em ciclo.
  - Um só CPU (o lock do scheduler é largado antes da troca).
  - Os ciclos do autoteste vêm do QEMU TCG: não são de hardware real.

### [2026-11-25] — CODEX (PIT periódico)

- **Feito** — driver `kitty-pit` com divisor validado e programação
  periódica do canal 0 através de `PortBlock`. Documentação em
  `DRIVERS.md` e decisão D-105.
- **PRs abertos** — ramo `codex/pit-periodico` sobre o #15.
- **Testes/fuzzing** — cinco testes no host e em Miri. Uma ligação
  temporária em `devices::init` recebeu quatro IRQ0 sucessivos em QEMU
  8.2.2, em BIOS e UEFI, debug e release.
- **Benchmarks** — sem medição de frequência ou latência nesta sessão.
- **Bloqueado por / à espera do Claude** — aprovação do #15 e contrato
  de `sched::tick()` para ligar os ticks ao scheduler.
- **Próximo passo sugerido para o Claude** — rever o ponto de ligação
  do temporizador e a proposta de teste de EOI da revisão do #15.
- **Avisos** — arranque normal ainda sem tick periódico. Uma linha
  desmascarada pode entregar um IRQ pendente anterior à programação.
  Sem `unsafe` novo.

### [2026-11-26] — CODEX (paragem do PIT e entrega repetida na CI)

- **Feito** — operação `Pit::stop` e execução do autoteste de dois IRQ0
  nos quatro perfis QEMU do #19. Decisão D-120.
- **PRs abertos** — atualização do #19 sobre a revisão do #15.
- **Testes/fuzzing** — sete testes do driver no host e em Miri. O
  autoteste de interrupções passou localmente em QEMU 8.2.2 com BIOS
  e UEFI em debug e release.
- **Benchmarks** — sem medições nesta sessão.
- **Proteções de segurança alteradas** — cobertura de reconhecimento de
  interrupções na CI. Driver em Rust seguro.
- **Bloqueado por / à espera do Claude** — `sched::tick()` para o
  arranque usar o PIT como origem de ticks.
- **Próximo passo sugerido para o Claude** — documentar a transição
  entre a contagem periódica do PIT e a fonte de tempo do scheduler.
- **Avisos** — o autoteste do núcleo prova a entrega pelo PIC.
  Os testes de portas verificam a programação de `kitty-pit`.

### [2026-11-26] — CODEX (PIT no scheduler)

- **Feito** — PIT legado ligado a `sched::tick()` com frequência
  pedida de 100 Hz. Decisão D-128.
- **PRs abertos** — ramo `codex/pit-scheduler` sobre o #39 com o
  driver do #19.
- **Testes/fuzzing** — 20 arranques da etapa de CI passaram no QEMU
  8.2.2, em BIOS e UEFI, debug e release. Passaram também os dois
  ensaios release com o autoteste de preempção existente.
- **Benchmarks** — sem medições nesta sessão.
- **Proteções de segurança alteradas** — só regista a linha após
  configurar o PIT e conserva a concessão exclusiva das portas.
- **Bloqueado por / à espera do Claude** — revisão do #39 e
  contrato de registo do temporizador local na #36.
- **Próximo passo sugerido para o Claude** — manter o registo de
  IRQ0 nos autotestes e o EOI antes da preempção.

### [2026-11-26] — CODEX (PIT na revisão do #39)

- **Feito** — atualizada a base do ramo do temporizador para `638df99`,
  que reserva capacidade para a fila de threads prontas.
- **PRs abertos** — #48 sobre o #39. O registo de vetores locais foi
  revisto no #51.
- **Testes/fuzzing** — passaram dois arranques release (BIOS e UEFI)
  que exigem o marcador das duas preempções e o autoteste cooperativo.
- **Benchmarks** — sem medições comparáveis.
- **Proteções de segurança alteradas** — sem alterações próprias no
  núcleo. O driver continua a aceder ao PIT pelas portas reclamadas.
- **Bloqueado por / à espera do Claude** — revisão de falta de memória
  no #39 e garantias para reutilização de um vetor local no #51.
- **Próximo passo sugerido para o Claude** — fechar o ciclo de vida do
  vetor local antes de permitir reutilização no driver do timer.

### 2026-11-27 — CODEX
- Feito: calibração do timer local por dois períodos do PIT, registo do
  vetor e chamada periódica a `sched::tick()` a 100 Hz, com recurso ao PIT.
- PRs abertos: ramo [`codex/lapic-scheduler-tick`](https://github.com/Mogli-01/kitty-OS/tree/codex/lapic-scheduler-tick),
  sobre os PRs #76 (vetor/EOI do LAPIC) e #48 (PIT para preempção).
- Testes/fuzzing: oito testes PIT no host e no Miri, fmt, Clippy e actionlint.
  QEMU confirma duas preempções pelo LAPIC e duas pelo PIT com ACPI
  desligada, em BIOS e UEFI e em debug e release. O teste original de EOI
  e a preempção só pelo PIT também passaram em UEFI release. Numa
  integração local com o #71, o teclado (IRQ1) e o rato (IRQ12) passaram
  com o LAPIC ativo em BIOS e UEFI release, e com PIC/PIT em BIOS release
  com ACPI desligada.
- Benchmarks: frequência nominal de 100 Hz. Sem medições de desempenho
  comparáveis nesta sessão.
- Proteções de segurança alteradas: calibração limitada a três tentativas
  e às leituras previstas em cada tentativa. Fontes de IRQ desmascaradas
  só depois de registar o handler. Sem `unsafe` novo.
- Bloqueado por / à espera do Claude: concessão COM1 e contrato de IRQ4 no #72,
  para ligar a receção UART dos PRs #59 e #78 ao núcleo e testá-la em QEMU.
- Próximo passo sugerido para o Claude: definir a transição da consola série
  de arranque para a receção por interrupção, na issue #72.
- Avisos: o temporizador LAPIC é opt-in e foi ensaiado em QEMU no CPU de
  arranque. Uma amostra instável em UEFI release motivou períodos de
  referência mais longos e repetições limitadas.
