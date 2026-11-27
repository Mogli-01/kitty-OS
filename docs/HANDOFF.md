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

### [2026-11-26] — CODEX (camada 16550A)

### [2026-11-25] — CODEX (teclado PS/2)

- **Feito** — configuração 8N1 e polling de TX/RX em `kitty-uart`, com
  um byte por chamada e tratamento de erros de linha. Decisão D-137.
- **PRs abertos** — ramo `codex/uart-16550` sobre `main`.
- **Testes/fuzzing** — seis testes de portas simuladas abrangem a ordem
  de configuração, falhas de acesso e preservação do estado de receção
  quando há consultas durante a transmissão. CI executa no host e em Miri.
  O arranque release no QEMU confirmou a consola inicial em UEFI e BIOS.
- **Benchmarks** — sem medições.
- **Proteções de segurança alteradas** — camada de registos em Rust seguro
  sobre `PortBlock`, com acessos limitados e sem espera ativa.
- **Bloqueado por / à espera do Claude** — contrato de posse exclusiva
  da UART e entrega de interrupções para ligar o driver às portas reais.
- **Próximo passo sugerido para o Claude** — discutir a transição da
  consola de arranque ou a concessão de uma segunda UART.
- **Avisos** — os testes de registos usam portas simuladas.
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

### [2026-11-26] — CLAUDE (desenho da Fase 6)
- **Feito:** `docs/FASE6.md` com o contrato da Fase 6 (processos, ring 3,
  ABI das syscalls, `copy_*_user`, handles, consola da shell, `/sbin/init`
  do initrd, INPUT para o compositor) e decisões D-047 a D-053. Responde às
  issues #46 e #50. Só documentação.
- **PRs abertos:** este (`claude/fase6-desenho`); #39, #40 (Fase 5) e #51
  (#36) à espera de revisão.
- **Próximo passo sugerido para o Codex:** rever o contrato; construir
  `/sbin/init` (a shell) como ELF64 estático e a biblioteca de syscalls em
  `userland/` contra a ABI; `console::input_push` no driver de teclado.
- **Próximo passo do Claude:** 6a (SMEP/SMAP/UMIP + `copy_*_user` com
  recuperação de #PF).
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

  - **`WaitQueue`** (PR seguinte, `claude/fase5-waitqueue`, base #39):
    estado `Blocked`, `wait_until`/`wake_one`/`wake_all` sem wake-ups
    perdidos (D-046), utilizável a partir de handlers de IRQ; autoteste
    produtor/consumidor no arranque.
- **PRs abertos:** #39 (`claude/fase5-scheduler`); `claude/fase5-waitqueue`
  (base #39).
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

### [2026-11-26] — CLAUDE (desenho da Fase 6)
- **Feito:** `docs/FASE6.md` com o contrato da Fase 6 (processos, ring 3,
  ABI das syscalls, `copy_*_user`, handles, consola da shell, `/sbin/init`
  do initrd, INPUT para o compositor) e decisões D-047 a D-053. Responde às
  issues #46 e #50. Só documentação.
- **PRs abertos:** este (`claude/fase6-desenho`); #39, #40 (Fase 5) e #51
  (#36) à espera de revisão.
- **Próximo passo sugerido para o Codex:** rever o contrato; construir
  `/sbin/init` (a shell) como ELF64 estático e a biblioteca de syscalls em
  `userland/` contra a ABI; `console::input_push` no driver de teclado.
- **Próximo passo do Claude:** 6a (SMEP/SMAP/UMIP + `copy_*_user` com
  recuperação de #PF).

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

  - Para esperar sem gastar CPU: `sched::WaitQueue` (ex.: o teclado pode
    acordar leitores a partir do handler do IRQ1).
  - Um só CPU (o lock do scheduler é largado antes da troca).
  - Os ciclos do autoteste vêm do QEMU TCG: não são de hardware real.

### [2026-11-27] — CLAUDE (expansão do roadmap)
- **Feito:** só documentação e configuração, nenhum código do kernel.
  - ROADMAP com as Fases 15 (utilizadores, capacidades, auditoria),
    16 (crypto, binários assinados, disco encriptado), 17 (rede, firewall,
    IDS), 18 (som, `kpkg`) e 19 (hardware real: ThinkPad T14 Gen 1 AMD),
    e a identidade "kitty" nas Fases 10 e 14 (ecrã de arranque, tema,
    mascote, `meow`, snake e tetris, tudo com gráficos originais).
  - `docs/REDTEAM.md`: rondas de red team vs blue team no fim de cada fase,
    a partir da 6; etiquetas `vulnerabilidade` e `red-team` criadas.
  - SECURITY (modelo de ameaça com o atacante local, a app maliciosa, o
    atacante na rede e o acesso físico ao disco; proteções novas),
    DECISIONS D-055…D-059, INTERFACES (esboços de capacidades, auditoria,
    crypto e hooks do firewall), CLAUDE.md e CONTRIBUTING.md.
  - **Fases 6 e 12 "capability-first" (D-058):** sem `input_open`, handles
    de DISPLAY/INPUT dados pelo `init`, `process_spawn` com lista de handles,
    `open_at` relativo a um diretório, `handle_info`, bits de direitos por
    tipo, `uid`/`gid` e auditoria preparados, `E_BADSIG` reservado.
- **PRs abertos:** este (`claude/roadmap-expansao`); #75 (6a); #39, #40,
  #51, #52 à espera de revisão. O #52 (`docs/FASE6.md`) tem de ser alinhado
  com a D-058 antes do merge.
- **Proteções de segurança alteradas:** nenhuma ativada; novas PLANEADAS
  (Fases 15–18).
- **Bloqueado por / à espera do Codex:** nada.
- **Próximo passo sugerido para o Codex:** ler o roadmap expandido,
  atualizar o AGENTS.md e preparar a CI para os testes de red team e de
  criptografia (runner de `tests/security/redteam/` com o manifesto
  `aberta`/`corrigida`, e um job no host para os vetores de teste da crypto).
- **Avisos:** a crypto é educativa e não auditada (D-056). A verificação de
  assinaturas não protege o kernel nem o Limine sem Secure Boot (ver
  SECURITY.md, fora de âmbito).

### 2026-11-27 — CODEX
- Feito: atualização do PR #59 ao `main`, preservando as decisões e o
  handoff de ambos os ramos.
- PRs abertos: #59 (camada de registos) e #78 (fontes de IRQ de receção).
- Testes/fuzzing: fmt, Clippy, seis testes UART no host e arranque em QEMU
  UEFI release.
- Benchmarks: sem novas medições.
- Bloqueado por / à espera do Claude: concessão COM1 e contrato IRQ4 (#72).
- Próximo passo sugerido para o Claude: definir a passagem da consola
  inicial para o driver que recebe por interrupção.
- Avisos: o ensaio de arranque mantém a consola série normal. A receção
  por interrupção ainda depende do contrato do núcleo.

### [2026-11-27] — CLAUDE (Fase 6a)
- **Feito:** SMEP, SMAP e UMIP ligados (com CPUID); `copy_from_user` /
  `copy_to_user` / `UserSlice` / `UserPtr` com recuperação de #PF (um ponteiro
  mau dá `Fault`, nunca panic); `clac` à entrada do dispatch. Autotestes: ida e
  volta por uma página de user real, falha recuperada (não mapeado e a cruzar
  o fim da página), recusa da metade do kernel; `selftest-smap` e
  `selftest-smep` provam que o kernel não lê nem executa páginas de user.
  D-054. PR `claude/fase6a-usercopy` (base `main`).
- **PRs abertos:** este; #39, #40, #51, #52 à espera de revisão.
- **Proteções de segurança alteradas:** ATIVAS — SMEP, SMAP, UMIP,
  `copy_*_user` com recuperação.
- **Próximo passo sugerido para o Codex:** pôr `selftest-smap` e
  `selftest-smep` na CI (com `--expect-panic` e os textos em INTERFACES.md).
- **Avisos:** a prova do UMIP precisa de ring 3 (Fase 6b). Ainda não há
  processos: a página de teste é mapeada temporariamente na metade de user
  do espaço do kernel e removida a seguir.

### [2026-11-26] — CODEX (preparação do Limine na CI)

- **Feito** — `xtask` repete o download do commit fixo e só publica
  o marcador de cache depois de o verificar por checkout. Decisão D-134.
- **PRs abertos** — ramo `codex/limine-fetch-retry` sobre `main`.
- **Testes/fuzzing** — um ensaio com Git intermediário injetou três
  falhas e confirmou que não foi criado o marcador. Outro injetou duas
  falhas e recuperou na terceira tentativa. O QEMU arrancou em UEFI e
  reutilizou depois o mesmo checkout para arrancar em BIOS.
- **Benchmarks** — sem medições. As esperas aplicam-se após falhas.
- **Proteções de segurança alteradas** — preservado o pin D-004.
  O fetch público corre sem pedidos interativos de credenciais.
- **Bloqueado por / à espera do Claude** — nada neste ramo.
- **Próximo passo sugerido para o Claude** — manter o pin D-004 nos
  ramos que acrescentam ficheiros à imagem de arranque.
- **Avisos** — uma falha persistente continua a abortar a preparação.

### [2026-11-27] — CODEX (teste das tentativas do Limine)

- **Feito** — teste determinístico das quatro sequências possíveis de
  falhas no fetch. Confirma o limite de três tentativas, o repositório
  e commit fixos, a ausência de pedidos interativos e as esperas de
  um e dois segundos.
- **PRs abertos** — atualização do #56.
- **Testes/fuzzing** — quatro testes do xtask no host e em Miri. Passaram
  formatação, Clippy e a preparação da partição EFI em release. O
  `BOOTX64.EFI` preparado coincide com o ficheiro do checkout fixo.
- **Benchmarks** — sem medições nesta ronda.
- **Proteções de segurança alteradas** — o teste verifica o pin D-004
  em cada tentativa.
- **Bloqueado por / à espera do Claude** — nenhuma dependência nesta ronda.
- **Próximo passo sugerido para o Claude** — manter o pin D-004.
- **Avisos** — o ensaio simula as falhas de rede. A matriz do GitHub
  executa os arranques BIOS e UEFI.

### [2026-11-27] — CODEX (pacotes QEMU na CI)

- **Feito** — os jobs QEMU usam apenas a lista Ubuntu do runner
  ao atualizar índices e instalar o emulador, OVMF e xorriso.
- **PRs abertos** — ramo `codex/ci-ubuntu-qemu` sobre `main`.
- **Testes/fuzzing** — Bash e actionlint passaram. Num contentor
  Ubuntu 24.04 com uma fonte externa indisponível, o APT recusou
  essa fonte e o script confirmou os três pacotes com as fontes Ubuntu.
- **Benchmarks** — sem novas medições.
- **Proteções de segurança alteradas** — mantidas as assinaturas do APT
  e a seleção do commit do Limine.
- **Bloqueado por / à espera do Claude** — nada neste passo.
- **Próximo passo sugerido para o Claude** — usar o mesmo instalador
  nos workflows que acrescentarem uma matriz QEMU.
- **Avisos** — o script espera a lista `ubuntu.sources` do runner Ubuntu 24.04.

### [2026-11-26] — CODEX (teste visual de arranque)

- **Feito** — `cargo xtask test-visual` captura a imagem de arranque
  no QEMU e compara a primeira linha com uma referência de 304 × 16.
  Decisão D-113.
- **PRs abertos** — ramo `codex/teste-visual` para `main`.
- **Testes/fuzzing** — dois testes de PPM e comparação passaram no host
  e em Miri. As quatro capturas do QEMU 8.2.2 coincidiram pixel a pixel.
  Uma referência de teste alterada foi recusada na primeira coordenada.
- **Benchmarks** — este teste ainda não recolhe tempos.
- **Bloqueado por / à espera do Claude** — nenhum bloqueio do teste
  da consola atual.
- **Próximo passo sugerido para o Claude** — indicar nas revisões
  mudanças de tipografia ou do início da consola para rever a referência.
- **Avisos** — a comparação visual cobre a primeira linha. Os relatórios
  de exceção e a proteção de memória mantêm os seus testes próprios.

### [2026-11-26] — CODEX (captura visual)

- **Feito** — O xtask espera pela imagem P6 completa. O Git trata as referências PPM como binárias.
- **PRs abertos** — #28.
- **Testes/fuzzing** — O banner coincidiu em BIOS e UEFI, debug e release, no QEMU 8.2.2.
- **Benchmarks** — sem medições.
- **Proteções de segurança alteradas** — Leitura com limite e validação do comprimento dos pixels.
- **Bloqueado por / à espera do Claude** — revisão da integração.

### [2026-11-26] — CODEX (benchmark de arranque)

- **Feito** — `cargo xtask bench-boot` mede da criação do QEMU até à
  confirmação de arranque na série. Decisão D-114 e valores iniciais
  em `BENCHMARKS.md`.
- **PRs abertos** — ramo `codex/benchmark-arranque` sobre o #28.
- **Testes/fuzzing** — dois testes novos para o marcador, no host e
  em Miri. O runner partilhado passou `test-memory --bios --release`.
- **Benchmarks** — medianas de 361,1 ms (BIOS) e 2 124,0 ms (UEFI),
  QEMU 8.2.2, release, sete amostras por firmware.
- **Bloqueado por / à espera do Claude** — scheduler e syscalls
  para medir os respetivos caminhos do núcleo.
- **Próximo passo sugerido para o Claude** — manter um ponto estável
  de arranque completo na série para medir as próximas fases.
- **Avisos** — métricas de QEMU dependem do firmware e do executor.
  A comparação de regressões entre branches será feita no mesmo runner.

### [2026-11-26] — CODEX (regressões de arranque na CI)

- **Feito** — workflow de comparação da base do PR e do candidato
  em BIOS/release, no mesmo runner, com alerta a partir de +25% e
  +50 ms. Decisão D-115.
- **PRs abertos** — ramo `codex/ci-benchmark` sobre o #29.
- **Testes/fuzzing** — teste do limiar no host e em Miri. O ensaio
  local completo com dois ELFs idênticos terminou sem aviso.
- **Benchmarks** — mediana de 357,6 ms na base e 353,1 ms no
  candidato idêntico (−4,5 ms) no ensaio local.
- **Bloqueado por / à espera do Claude** — caminhos de syscall e
  scheduler para os restantes benchmarks.
- **Próximo passo sugerido para o Claude** — usar o artefacto do
  benchmark para avaliar alterações significativas de arranque.
- **Avisos** — a CI mede o perfil BIOS/release. O UEFI continua
  disponível a partir de `cargo xtask bench-boot --release`.

### 2026-11-27 — CODEX (integração da Fase 4)
- Feito: atualizadas as bases dos drivers de IRQ a partir do `main`
  em `228bcc7`, incluindo as filas de espera e a parte 6A do núcleo.
- PRs abertos: #14, #16, #17, #19, #20, #21, #34, #37, #48, #59, #62,
  #65, #71, #76, #78 e #81.
- Testes/fuzzing: host para Set 2, i8042, PIT, MADT, APIC e UART e Miri para
  PIT e UART. QEMU UEFI release nos ramos atualizados confirma IRQ1,
  IRQ12, EOI de IRQ0 pelo APIC, fallback PIC sem MADT e EOI do timer LAPIC.
- Benchmarks: sem medições comparáveis.
- Bloqueado por / à espera do Claude: contrato de acesso à UART e IRQ
  na issue #72. A integração em `main` requer revisão humana.
- Próximo passo sugerido para o Claude: especificar se a UART do driver
  usa uma segunda porta ou recebe da consola a posse da COM1.
- Avisos: o #81 usa o #76 como base e inclui o PIT do #48. Um check
  de BIOS do #48 voltou a passar após um erro HTTP 403 no APT do runner.
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

### [2026-11-26] — CODEX (transporte AUX i8042)

- **Feito** — inicialização do rato PS/2 padrão e reposição do teclado
  no mesmo `PortBlock`. Decisão D-118.
- **PRs abertos** — ramo `codex/i8042-aux` sobre o #16.
- **Testes/fuzzing** — nove testes de transporte no host e em Miri.
  Uma ligação temporária no QEMU 8.2.2 recebeu movimento (7, 4),
  clique e libertação por IRQ12 e a tecla A por IRQ1, em BIOS e
  UEFI/release.
- **Benchmarks** — nenhuma medição de input nesta sessão.
- **Bloqueado por / à espera do Claude** — aprovação do contrato
  IRQ do #15. A fila e o ensaio automático de IRQ12 seguem noutro PR.
- **Próximo passo sugerido para o Claude** — confirmar as
  garantias de afinidade das guardas e de entrega de IRQ do #15.
- **Avisos** — a configuração corre com consumidores parados.
  O modo de três bytes exige ID zero. Sem `unsafe` novo.

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

### [2026-11-26] — CODEX (teclado e rato no mesmo controlador)

- **Feito** — ligação de IRQ1 e IRQ12 ao controlador PS/2 e fila
  comum de 64 eventos. Tratamento de perda e configuração só com
  teclado quando AUX falha. Decisão D-119.
- **PRs abertos** — ramo `codex/ps2-input` sobre o #17, com o #32
  e o #33 como dependências.
- **Testes/fuzzing** — 18 testes PS/2 no host e em Miri. Teclado e
  rato passaram em QEMU 8.2.2 nos quatro perfis. O teste de IRQ12
  verifica movimento (7, 4), pressão e libertação do botão esquerdo.
- **Benchmarks** — sem medição de latência IRQ para input.
- **Bloqueado por / à espera do Claude** — revisão do #15, contrato
  de relógio monotónico e entrega ao processo com `INPUT`.
- **Próximo passo sugerido para o Claude** — definir as capacidades
  e o relógio que vão expor os eventos ao user space.
- **Avisos** — testes de QEMU usam uma feature que regista os eventos
  na série. A fila existe no kernel e a entrega ao user space fica
  para o contrato de INPUT. Sem `unsafe` novo.

### [2026-11-26] — CODEX (PS/2 no Platform atual)

- **Feito** — adaptados o teclado e o rato à inicialização com
  `Platform`. O estado do controlador é instalado antes de registar IRQ1
  e IRQ12. Se o registo falhar, o driver pára a porta afetada.
- **PRs abertos** — #14, #16, #17, #32, #33 e #34 atualizados.
- **Testes/fuzzing** — teclado em IRQ1 e rato em IRQ12 passaram em QEMU
  8.2.2 com BIOS e UEFI, debug e release. Testes no host e clippy passaram.
- **Benchmarks** — sem medições.
- **Proteções de segurança alteradas** — os handlers encontram o estado
  inicializado assim que a linha pode entregar interrupções.
- **Bloqueado por / à espera do Claude** — timestamps e entrega por INPUT.
- **Próximo passo sugerido para o Claude** — documentar o relógio e o
  contrato de entrega para o compositor.

### [2026-11-27] — CODEX (controlador nos testes PS/2)

- **Feito** — `test-keyboard` e `test-mouse` aceitam features adicionais
  e verificam marcadores de arranque antes da injeção de eventos. A CI
  exige o controlador PIC nos ensaios PS/2 da matriz BIOS/UEFI.
- **PRs abertos** — `codex/ps2-test-controller` sobre o #34.
- **Testes/fuzzing** — passaram formatação, Clippy, actionlint e os três
  testes do xtask no host e em Miri. A execução local de QEMU ficou
  impedida pela falta de espaço para iniciar o contentor. A matriz
  do GitHub executa os testes de QEMU.
- **Benchmarks** — sem medições nesta ronda.
- **Proteções de segurança alteradas** — o ensaio confirma o controlador
  selecionado e a entrega de eventos ao driver.
- **Bloqueado por / à espera do Claude** — entrega de eventos a user
  space depende de INPUT (#46).
- **Próximo passo sugerido para o Claude** — implementar o contrato de
  INPUT quando estiver disponível a infraestrutura de processos.
- **Avisos** — o ensaio PS/2 com APIC exige integrar a feature `xapic`
  do #62 com os drivers PS/2.

### [2026-11-27] — CODEX (fuzzing das rotas ISA na MADT)

- **Feito** — o alvo gera duas rotas ISA para IRQs distintas, varia os
  GSIs e a ordem dos registos, e confirma os resultados das 16 consultas.
- **PRs abertos** — `codex/madt-fuzz-isa-routes` sobre o #20.
- **Testes/fuzzing** — 11 testes no host e os mesmos 11 em Miri.
  O AddressSanitizer completou 1 252 226 execuções em 21 s sem falhas.
  O corpus inicial inclui ambas as ordens, IRQs 0 e 15 e o maior GSI de
  32 bits. Formatação e Clippy passaram.
- **Benchmarks** — sem medições nesta ronda.
- **Proteções de segurança alteradas** — cobertura adicional do
  descodificador no host.
- **Bloqueado por / à espera do Claude** — sem bloqueio para o alvo.
- **Próximo passo sugerido para o Claude** — nenhuma dependência nova.
- **Avisos** — o ensaio verifica a interpretação da tabela em memória.
  O encaminhamento em QEMU fica coberto pelos ensaios do #62 e do #65.

### [2026-11-26] — CLAUDE (desenho da Fase 6)
- **Feito:** `docs/FASE6.md` com o contrato da Fase 6 (processos, ring 3,
  ABI das syscalls, `copy_*_user`, handles, consola da shell, `/sbin/init`
  do initrd, INPUT para o compositor) e decisões D-047 a D-053. Responde às
  issues #46 e #50. Só documentação.
- **PRs abertos:** este (`claude/fase6-desenho`); #39, #40 (Fase 5) e #51
  (#36) à espera de revisão.
- **Próximo passo sugerido para o Codex:** rever o contrato; construir
  `/sbin/init` (a shell) como ELF64 estático e a biblioteca de syscalls em
  `userland/` contra a ABI; `console::input_push` no driver de teclado.
- **Próximo passo do Claude:** 6a (SMEP/SMAP/UMIP + `copy_*_user` com
  recuperação de #PF).

### [2026-11-26] — CODEX (teclado PT-PT)

- **Feito** — layout ISO português, modificadores, acentos e sequências
  de edição para a consola. Decisão D-133.
- **PRs abertos** — ramo `codex/teclado-pt-pt` sobre o #34.
- **Testes/fuzzing** — 22 testes PS/2 no host e em Miri. O QEMU validou
  `é` e a sequência das setas em BIOS e UEFI, debug e release. Os testes
  do rato passaram em release nos dois firmwares. No host, uma sequência
  Set 2 editou `echo café` com a sessão do #53 e desenhou a saída com o #54.
- **Benchmarks** — sem medições.
- **Proteções de segurança alteradas** — saída limitada sem heap. O
  layout repõe os modificadores e a composição depois de perder eventos.
- **Bloqueado por / à espera do Claude** — publicação na consola do
  núcleo, através do contrato previsto no #52.
- **Próximo passo sugerido para o Claude** — definir a publicação
  atómica da sequência completa de uma tecla na fila da consola.
- **Avisos** — o teste QEMU usa instrumentação de teste, e não expõe
  a saída para uma shell interativa.

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
  - **`WaitQueue`** (PR seguinte, `claude/fase5-waitqueue`, base #39):
    estado `Blocked`, `wait_until`/`wake_one`/`wake_all` sem wake-ups
    perdidos (D-046), utilizável a partir de handlers de IRQ; autoteste
    produtor/consumidor no arranque.
- **PRs abertos:** #39 (`claude/fase5-scheduler`); `claude/fase5-waitqueue`
  (base #39).
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
  - Para esperar sem gastar CPU: `sched::WaitQueue` (ex.: o teclado pode
    acordar leitores a partir do handler do IRQ1).
  - Um só CPU (o lock do scheduler é largado antes da troca).
  - Os ciclos do autoteste vêm do QEMU TCG: não são de hardware real.
### [2026-11-27] — CLAUDE (expansão do roadmap)
- **Feito:** só documentação e configuração, nenhum código do kernel.
  - ROADMAP com as Fases 15 (utilizadores, capacidades, auditoria),
    16 (crypto, binários assinados, disco encriptado), 17 (rede, firewall,
    IDS), 18 (som, `kpkg`) e 19 (hardware real: ThinkPad T14 Gen 1 AMD),
    e a identidade "kitty" nas Fases 10 e 14 (ecrã de arranque, tema,
    mascote, `meow`, snake e tetris, tudo com gráficos originais).
  - `docs/REDTEAM.md`: rondas de red team vs blue team no fim de cada fase,
    a partir da 6; etiquetas `vulnerabilidade` e `red-team` criadas.
  - SECURITY (modelo de ameaça com o atacante local, a app maliciosa, o
    atacante na rede e o acesso físico ao disco; proteções novas),
    DECISIONS D-055…D-059, INTERFACES (esboços de capacidades, auditoria,
    crypto e hooks do firewall), CLAUDE.md e CONTRIBUTING.md.
  - **Fases 6 e 12 "capability-first" (D-058):** sem `input_open`, handles
    de DISPLAY/INPUT dados pelo `init`, `process_spawn` com lista de handles,
    `open_at` relativo a um diretório, `handle_info`, bits de direitos por
    tipo, `uid`/`gid` e auditoria preparados, `E_BADSIG` reservado.
- **PRs abertos:** este (`claude/roadmap-expansao`); #75 (6a); #39, #40,
  #51, #52 à espera de revisão. O #52 (`docs/FASE6.md`) tem de ser alinhado
  com a D-058 antes do merge.
- **Proteções de segurança alteradas:** nenhuma ativada; novas PLANEADAS
  (Fases 15–18).
- **Bloqueado por / à espera do Codex:** nada.
- **Próximo passo sugerido para o Codex:** ler o roadmap expandido,
  atualizar o AGENTS.md e preparar a CI para os testes de red team e de
  criptografia (runner de `tests/security/redteam/` com o manifesto
  `aberta`/`corrigida`, e um job no host para os vetores de teste da crypto).
- **Avisos:** a crypto é educativa e não auditada (D-056). A verificação de
  assinaturas não protege o kernel nem o Limine sem Secure Boot (ver
  SECURITY.md, fora de âmbito).

### 2026-11-27 — CODEX
- Feito: atualização do PR #48 ao `main` com scheduler e filas de espera.
  Os conflitos eram nas entradas de decisões e de handoff.
- PRs abertos: #48, PIT periódico ao serviço do scheduler.
- Testes/fuzzing: fmt, Clippy, sete testes PIT no host e duas preempções
  efetivas pelo PIT no QEMU UEFI release.
- Benchmarks: sem novas medições comparáveis.
- Bloqueado por / à espera do Claude: nenhuma dependência de código para o PIT.
- Próximo passo sugerido para o Claude: rever a integração do relógio de arranque
  com as filas de espera.
- Avisos: a integração final depende da CI e da aprovação humana.

### [2026-11-27] — CLAUDE (Fase 6a)
- **Feito:** SMEP, SMAP e UMIP ligados (com CPUID); `copy_from_user` /
  `copy_to_user` / `UserSlice` / `UserPtr` com recuperação de #PF (um ponteiro
  mau dá `Fault`, nunca panic); `clac` à entrada do dispatch. Autotestes: ida e
  volta por uma página de user real, falha recuperada (não mapeado e a cruzar
  o fim da página), recusa da metade do kernel; `selftest-smap` e
  `selftest-smep` provam que o kernel não lê nem executa páginas de user.
  D-054. PR `claude/fase6a-usercopy` (base `main`).
- **PRs abertos:** este; #39, #40, #51, #52 à espera de revisão.
- **Proteções de segurança alteradas:** ATIVAS — SMEP, SMAP, UMIP,
  `copy_*_user` com recuperação.
- **Próximo passo sugerido para o Codex:** pôr `selftest-smap` e
  `selftest-smep` na CI (com `--expect-panic` e os textos em INTERFACES.md).
- **Avisos:** a prova do UMIP precisa de ring 3 (Fase 6b). Ainda não há
  processos: a página de teste é mapeada temporariamente na metade de user
  do espaço do kernel e removida a seguir.

### [2026-11-26] — CODEX (descodificador PCI)

- **Feito** — `kitty-pci` descodifica os cabeçalhos convencionais de
  dispositivos e pontes. Percorre a lista de capacidades com deteção
  de repetições, offsets inválidos e truncamento. Decisão D-140.
- **PRs abertos** — ramo `codex/pci-config` sobre `main`.
- **Testes/fuzzing** — testes no host e em Miri incluem os 256 valores
  possíveis do primeiro link, o máximo de 48 entradas e os dois layouts.
- **Benchmarks** — sem medições.
- **Proteções de segurança alteradas** — validação do comprimento e
  das referências antes de ler. O código opera sobre bytes imutáveis.
- **Bloqueado por / à espera do Claude** — contrato para obter snapshots
  de configuração PCI a partir de hardware.
- **Próximo passo sugerido para o Claude** — definir o acesso seguro a
  snapshots por barramento, dispositivo e função.
- **Avisos** — os testes usam snapshots sintéticos. A enumeração de
  hardware e os efeitos de BARs e capacidades ficam por ligar.

### [2026-11-27] — CODEX (PCI BARs)

- **Feito** — o descodificador PCI interpreta os BARs convencionais
  de I/O e de memória de 32 e 64 bits a partir de um snapshot.
- **PRs abertos** — `codex/pci-bar-decode` sobre `codex/pci-config` (#61).
- **Testes/fuzzing** — 11 testes passaram no host e em Miri, incluindo
  o consumo do segundo registo de um BAR de 64 bits, o limite do
  cabeçalho de uma ponte e a recuperação após um encoding reservado.
  Formatação, Clippy e compilação release para x86_64-unknown-none passaram.
- **Benchmarks** — sem medições nesta ronda.
- **Bloqueado por / à espera do Claude** — contrato de leitura de snapshots
  de configuração PCI para enumerar dispositivos físicos.
- **Próximo passo sugerido para o Claude** — definir essa interface em
  `docs/INTERFACES.md`.
- **Avisos** — os testes usam snapshots sintéticos. O descodificador
  devolve endereços e atributos. A descoberta do tamanho, a atribuição
  e o mapeamento de recursos ficam para a integração com o hardware.

### [2026-11-26] — CODEX (testes do scheduler e das pilhas)

- **Feito** — CI verifica os marcadores de arranque do scheduler e das
  filas de espera, preempção com IRQ e transição por idle, e o relatório
  #DF na IST para a guarda da pilha de thread. Decisão D-124.
- **PRs abertos** — ramo `codex/ci-scheduler` sobre o #40.
- **Testes/fuzzing** — os 20 arranques da etapa de CI passaram em QEMU
  8.2.2 com BIOS e UEFI, debug e release, com um log série por cenário.
- **Benchmarks** — sem medições nesta sessão.
- **Proteções de segurança alteradas** — a CI exige a falha de guarda
  com o relatório esperado e o código de saída de panic.
- **Bloqueado por / à espera do Claude** — revisão sobre alocação e
  libertação de memória nos métodos de acordar a partir de IRQ no #40.
- **Próximo passo sugerido para o Claude** — preservar a capacidade das
  filas usada pelos handlers e manter os marcadores dos autotestes.

### [2026-11-26] — CODEX (CI do scheduler na revisão do #40)

- **Feito** — atualizado o ramo da CI com `ddc578a`, incluindo as
  alterações do Claude para manter capacidade nas filas.
- **PRs abertos** — #42 sobre o #40. Seguimento das revisões no #39 e #40.
- **Testes/fuzzing** — espera, arranque cooperativo, preempção e
  despertar por IRQ passaram no QEMU em release, BIOS e UEFI.
- **Benchmarks** — sem medições comparáveis.
- **Proteções de segurança alteradas** — sem alterações próprias ao
  scheduler. A CI conserva os autotestes da guarda das pilhas.
- **Bloqueado por / à espera do Claude** — revisão da ordem de locks
  em `wake_all` e da reserva de slots em caso de falta de memória.
- **Próximo passo sugerido para o Claude** — decidir as correções nos
  comentários dos PRs #39 e #40.
- **Avisos** — compilar só `selftest-preempt` gera um aviso de importação
  não usada no autoteste de CPU. Os dois arranques passaram.

### [2026-11-26] — CODEX (benchmark cooperativo do scheduler)

- **Feito** — `cargo xtask bench-scheduler` recolhe sete medições do
  autoteste depois do aquecimento e apresenta mínimo, mediana e máximo.
- **PRs abertos** — ramo `codex/scheduler-bench` sobre o #42.
- **Testes/fuzzing** — cinco testes de `xtask` passaram no host e em
  Miri. Os oito arranques passaram em cada firmware no QEMU 8.2.2 release.
- **Benchmarks** — mediana de 1 282 ciclos TSC em BIOS e 1 532 em UEFI.
  Configuração e intervalos em `docs/BENCHMARKS.md`.
- **Proteções de segurança alteradas** — a medição usa a validação do
  arranque e limita a leitura a um resultado válido por amostra.
- **Bloqueado por / à espera do Claude** — o benchmark de syscalls
  depende da entrada em user mode e da implementação das syscalls.
- **Próximo passo sugerido para o Claude** — manter o marcador do
  autoteste cooperativo enquanto a recolha do host depender dele.
- **Avisos** — a amostra mede o caminho cooperativo em TCG e inclui a
  preparação do autoteste. A CI valida o parser sem impor um limiar.

### [2026-11-26] — CODEX (comandos da shell)

- **Feito** — parser de linhas UTF-8 e comandos internos `help` e
  `echo`. Decisão D-129.
- **PRs abertos** — ramo `codex/shell-comandos` sobre o #20, para usar
  a infraestrutura de fuzzing.
- **Testes/fuzzing** — cinco testes no host e em Miri. O alvo sob
  AddressSanitizer executou 1 635 567 casos em 16 segundos.
- **Benchmarks** — sem medições.
- **Proteções de segurança alteradas** — limites de bytes e palavras
  antes de expor a linha. O parser valida UTF-8 e controlos.
- **Bloqueado por / à espera do Claude** — ABI de consola e execução
  do primeiro programa em user space.
- **Próximo passo sugerido para o Claude** — documentar o contrato
  de entrada e saída de texto para a primeira shell.

### [2026-11-26] — CODEX (editor de linha da shell)

- **Feito** — edição, navegação e remoção em fronteiras UTF-8.
  Decisão D-130.
- **PRs abertos** — ramo `codex/shell-edicao` sobre o #47.
- **Testes/fuzzing** — oito testes no host e em Miri. O alvo `shell`
  validou sequências de edição em 564 613 casos sob ASan.
- **Benchmarks** — sem medições.
- **Proteções de segurança alteradas** — armazenamento limitado e
  preservação da linha quando uma inserção é recusada.
- **Bloqueado por / à espera do Claude** — consola de user space e
  execução do primeiro programa.
- **Próximo passo sugerido para o Claude** — definir entrega dos bytes
  de texto e teclas de controlo à shell.

### [2026-11-26] — CODEX (entrada da consola da shell)

- **Feito** — sessão que conserva a edição entre leituras e executa
  um comando ao receber Enter. Ctrl+C cancela a edição. Decisão D-131.
- **PRs abertos** — ramo `codex/shell-consola` sobre o #49. Revisões
  atualizadas nos #39 e #40, e submetidas nos #51 e #52.
- **Testes/fuzzing** — 12 testes no host e em Miri. O alvo `shell`
  passou 386 970 casos sob ASan com o fluxo de entrada incluído. Um
  segundo ensaio passou 228 226 casos após alargar as inserções a todo
  o intervalo de Unicode. O teste de leituras parciais inclui um emoji.
- **Benchmarks** — sem medições.
- **Proteções de segurança alteradas** — descodificação incremental
  com estado de tamanho fixo. Uma falha de escrita não repete o comando.
- **Bloqueado por / à espera do Claude** — syscalls de consola e
  carregador de `/sbin/init` propostos no #52.
- **Próximo passo sugerido para o Claude** — fechar as perguntas no #52
  sobre perda de dados, escritas parciais e apresentação da linha.
- **Avisos** — a sessão ainda só executa através de um chamador Rust.

### [2026-11-25] — CODEX (parser do initrd)

- **Feito** — `kitty-initrd` valida arquivos USTAR e devolve entradas
  e dados de ficheiros sem copiar. Decisão D-108.
- **PRs abertos** — ramo `codex/initrd-ustar` sobre o #20 para usar o
  workflow de parsers. Contrato de arranque pedido na #22.
- **Testes/fuzzing** — nove testes passaram no host e em Miri. O parser
  leu um arquivo criado pelo GNU tar 1.35 com diretórios, caminho longo,
  UTF-8 e dados binários. Recusou um arquivo de teste com um symlink.
  Fuzzing com ASan executou 390 568 casos sem falhas.
- **Benchmarks** — sem medição de tempo de montagem.
- **Bloqueado por / à espera do Claude** — API para o módulo Limine
  com vida útil, intervalo e permissões verificados (#22).
- **Próximo passo sugerido para o Claude** — documentar o ponto em que
  o módulo é entregue em modo só de leitura.
- **Avisos** — o parser valida o formato. Conflitos de nomes ficam
  para o índice do VFS. Sem `unsafe` novo.

### [2026-11-26] — CODEX (validação dos registos USTAR)

- **Feito** — o #23 passou a exigir tamanho zero nos diretórios e
  preenchimento a zero depois do NUL nos campos de nome. A contagem de
  entradas é produzida durante a validação do arquivo.
- **PRs abertos** — atualização do #23 sobre o ramo atualizado do #20.
- **Testes/fuzzing** — 11 testes passaram no host e em Miri. O alvo
  `initrd` verifica que o número de entradas expostas coincide com a
  contagem validada.
- **Benchmarks** — sem medição de tempo de montagem.
- **Proteções de segurança alteradas** — a montagem recebe registos com
  limites e nomes canónicos. O VFS pode limitar o índice antes de alocar.
- **Bloqueado por / à espera do Claude** — a entrega do módulo do Limine
  está disponível no #38.
- **Próximo passo sugerido para o Claude** — usar a raiz só de leitura
  quando for definido o contrato de ficheiros de user space.
- **Avisos** — produtores de USTAR que escrevam capacidade no tamanho de
  diretórios precisam de gerar um arquivo com tamanho zero nesses registos.

### [2026-11-25] — CODEX (raiz inicial do VFS)

- **Feito** — montagem só de leitura, índice de caminhos, diretórios
  implícitos, listagem e leitura por offset em `fs/vfs`. Interface
  descrita em `INTERFACES.md`, decisão D-109.
- **PRs abertos** — ramo `codex/vfs-initrd` sobre o #23.
- **Testes/fuzzing** — sete testes no host e em Miri. O alvo de
  fuzzing do initrd usa a mesma imagem também para verificar a
  resolução e a leitura após a montagem. Executou 231 749 casos com
  ASan, sem falhas.
- **Benchmarks** — sem medições de tempo de montagem ou de pesquisa.
- **Bloqueado por / à espera do Claude** — passagem do módulo e
  ponto de ligação ao arranque (#22); syscalls de ficheiros na Fase 7.
- **Próximo passo sugerido para o Claude** — entregar o módulo
  imutável e usar a interface documentada para a primeira leitura
  através de um buffer validado do núcleo.
- **Avisos** — esta raiz cobre o initrd. A validação por processo e o
  acesso a disco ficam para a ligação de syscalls e outras raízes.
  Sem `unsafe` novo.

### [2026-11-26] — CODEX (limite de entradas do VFS)

- **Feito** — o VFS aceita até 65 536 cabeçalhos USTAR. Usa a contagem
  validada para uma única reserva falível do índice.
- **PRs abertos** — atualização do #24 sobre o #23.
- **Testes/fuzzing** — o teste de fronteira verifica a aceitação no
  limite e a recusa de uma entrada a mais antes da reserva.
- **Benchmarks** — sem medição de tempo de montagem.
- **Proteções de segurança alteradas** — o limite reduz a memória que
  uma imagem de arranque pode pedir para o índice.
- **Bloqueado por / à espera do Claude** — o #38 fornece o módulo ao VFS.
- **Próximo passo sugerido para o Claude** — definir os direitos e os
  buffers de leitura na interface de ficheiros de user space.
- **Avisos** — a contagem inclui o cabeçalho da raiz quando presente.

### [2026-11-27] — CODEX (fronteiras entre ficheiros USTAR)

- **Feito** — o alvo de fuzzing do initrd gera um arquivo com dois
  ficheiros. Varia o tamanho e o conteúdo do primeiro e verifica os
  nomes, a ordem e os bytes dos dois ficheiros depois do alinhamento.
- **PRs abertos** — `codex/ustar-fuzz-multifile` sobre o #23.
- **Testes/fuzzing** — 11 testes no host e os mesmos 11 em Miri.
  O AddressSanitizer completou 118 620 execuções sem falhas em 21 s,
  a partir de entradas que incluem 511, 512 e 513 bytes.
  Formatação e Clippy passaram.
- **Benchmarks** — sem medições de desempenho nesta ronda.
- **Proteções de segurança alteradas** — cobertura adicional dos limites
  entre registos no host.
- **Bloqueado por / à espera do Claude** — a primeira shell continua a
  depender do contrato em discussão na issue #50.
- **Próximo passo sugerido para o Claude** — confirmar na #50 como lançar
  `/sbin/init` e entregar a consola à shell.
- **Avisos** — esta ronda exercitou o parser no host.

### [2026-11-25] — CODEX (primitivas gráficas)

- **Feito** — superfícies BGRA8888 e desenho de píxeis, retângulos,
  linhas e blit em `userland/gfx`. Guia `GUI.md` e decisão D-110.
- **PRs abertos** — ramo `codex/graficos-base` para `main`.
- **Testes/fuzzing** — seis testes no host e em Miri. Cobrem
  coordenadas extremas, recorte, ordem dos canais, pitches ímpares
  e preservação do padding.
- **Benchmarks** — ainda sem medidas de frames.
- **Bloqueado por / à espera do Claude** — mapeamento de framebuffer
  através de DISPLAY para a apresentação futura.
- **Próximo passo sugerido para o Claude** — incluir dimensão, pitch
  e formato de píxeis no contrato de DISPLAY da fase 10.
- **Avisos** — biblioteca testada em buffers de memória. A ligação
  ao ecrã, fontes e double buffering serão feitos nas etapas seguintes.

### [2026-11-26] — CODEX (gráficos base)

- **Feito** — Integrado o main atual. A enumeração pública de erros admite novas variantes.
- **PRs abertos** — #25.
- **Testes/fuzzing** — Seis testes no host e em Miri.
- **Benchmarks** — sem medições.
- **Proteções de segurança alteradas** — A biblioteca continua em Rust seguro.
- **Bloqueado por / à espera do Claude** — revisão da integração.

### [2026-11-25] — CODEX (double buffering)

- **Feito** — `DoubleBuffer` separa desenho de apresentação e permite
  copiar um frame completo ou um retângulo alterado. Decisão D-111.
- **PRs abertos** — ramo `codex/double-buffer` sobre o #25.
- **Testes/fuzzing** — três testes novos com dimensões incompatíveis,
  confirmação de que o desenho só é visível após apresentação e
  recorte da atualização com pitches diferentes.
- **Benchmarks** — sem medições de apresentação no hardware.
- **Bloqueado por / à espera do Claude** — concessão DISPLAY
  para apresentar no dispositivo.
- **Próximo passo sugerido para o Claude** — alinhar a API de
  framebuffer com o formato e o pitch usados pela apresentação.
- **Avisos** — apresentação por cópia em memória. A sincronização
  com o ecrã será verificada quando o dispositivo estiver ligado.

### [2026-11-26] — CODEX (zonas alteradas)

- **Feito** — recorte e união de regiões alteradas com apresentação
  parcial a partir do buffer traseiro. Decisão D-125.
- **PRs abertos** — ramo `codex/zonas-alteradas` sobre o #26.
- **Testes/fuzzing** — 13 testes gráficos no host e em Miri.
- **Benchmarks** — sem medições de tempo por frame nesta sessão.
- **Proteções de segurança alteradas** — coordenadas validadas e
  lista de 16 regiões com fallback para o frame inteiro.
- **Bloqueado por / à espera do Claude** — capacidades DISPLAY,
  memória partilhada e IPC para o compositor.
- **Próximo passo sugerido para o Claude** — documentar o acesso
  ao framebuffer pelo compositor em user space.

### [2026-11-26] — CODEX (composição com alfa)

- **Feito** — composição source-over em `kitty-gfx` sobre os canais
  BGRA8888 guardados. Decisão D-126.
- **PRs abertos** — ramo `codex/composicao-alfa` sobre o #43.
- **Testes/fuzzing** — 16 testes no host e em Miri. Incluem camada
  transparente, destinos com alfa, recorte e padding.
- **Benchmarks** — sem medições de composição nesta sessão.
- **Proteções de segurança alteradas** — a mistura usa linhas
  limitadas pelas duas superfícies validadas.
- **Bloqueado por / à espera do Claude** — capacidades de framebuffer
  e buffers partilhados para ligar a composição ao ecrã.
- **Próximo passo sugerido para o Claude** — documentar os formatos
  e as permissões dos buffers partilhados entre os processos.

### [2026-11-26] — CODEX (cursor por software)

- **Feito** — ponto ativo validado, movimento absoluto e relativo,
  recorte e recomposição do cursor em user space. Decisão D-127.
- **PRs abertos** — ramo `codex/cursor-software` sobre o #44.
- **Testes/fuzzing** — 20 testes gráficos no host e em Miri. O percurso
  com buffer duplo confirma que mover o cursor restaura o fundo antigo.
- **Benchmarks** — sem medições de tempo por frame.
- **Proteções de segurança alteradas** — o recorte limita a leitura
  da imagem e o desenho à zona recomposta em Rust seguro.
- **Bloqueado por / à espera do Claude** — acesso ao framebuffer
  e entrega de eventos de rato a processos através de INPUT.
- **Próximo passo sugerido para o Claude** — documentar o contrato
  de entrega de movimento ao compositor.

### [2026-11-26] — CODEX (pilha de janelas)

- **Feito** — pilha fixa de geometrias, coordenadas locais da janela
  sob o cursor e áreas alteradas ao mover, subir ou remover. Decisão D-139.
- **PRs abertos** — ramo `codex/window-stack` sobre o #45.
- **Testes/fuzzing** — 26 testes no host e em Miri. Os seis novos abrangem
  limites, capacidade, áreas recortadas, ordem, coordenadas locais e
  recomposição após mover ou fechar uma janela com apresentação parcial.
- **Benchmarks** — sem medições. A pilha tem capacidade fixa.
- **Proteções de segurança alteradas** — dimensões validadas,
  aritmética alargada nos limites e rejeição de identificadores duplicados.
- **Bloqueado por / à espera do Claude** — IPC, memória partilhada e
  entrega de INPUT e DISPLAY ao compositor.
- **Próximo passo sugerido para o Claude** — indicar como o compositor
  associa as superfícies partilhadas às janelas pelo protocolo.
- **Avisos** — a seleção usa a geometria e conserva a ordem das camadas.
  A fila de eventos e a política de foco ficam para a integração.

### [2026-11-26] — CODEX (empacotamento do initrd)

- **Feito** — opção `--initrd` para os comandos de imagem e QEMU,
  configuração Limine e guia `BUILDING.md`. Decisão D-116.
- **PRs abertos** — ramo `codex/empacotar-initrd` sobre o #24.
- **Testes/fuzzing** — teste de configuração no host e em Miri.
  BIOS e UEFI arrancaram com o arquivo incluído. O conteúdo extraído
  da ISO coincide byte a byte com a origem. Um arquivo inválido
  foi recusado antes de alterar a imagem existente.
- **Benchmarks** — sem medição de impacto do módulo no arranque.
- **Bloqueado por / à espera do Claude** — disponibilização dos bytes
  do Limine ao núcleo e à raiz do VFS (#22).
- **Próximo passo sugerido para o Claude** — usar a marca
  `kitty-initrd` no contrato do módulo ou atualizar a #22 se mudar.

### [2026-11-26] — CODEX (limite e entrega do initrd no xtask)

- **Feito** — o xtask verifica o tamanho do arquivo antes de o carregar
  e a CI exige que o núcleo confirme a receção do módulo.
- **PRs abertos** — atualização do #31 sobre o #24.
- **Testes/fuzzing** — o ensaio de arranque com arquivo corre em BIOS
  e UEFI, com o perfil release. Compara também o conteúdo da imagem.
- **Benchmarks** — sem medições nesta sessão.
- **Proteções de segurança alteradas** — o host aplica o mesmo limite de
  128 MiB que o núcleo usa para o módulo.
- **Bloqueado por / à espera do Claude** — a entrega está disponível no
  #38. O parser e o VFS estão em revisão.
- **Próximo passo sugerido para o Claude** — usar a raiz do VFS nas
  futuras operações de ficheiros.
- **Avisos** — o ensaio do #31 confirma que o núcleo recebe o módulo.
  O PR de montagem verifica também a leitura do arquivo.

### [2026-11-26] — CODEX (montagem do initrd no arranque)

- **Feito** — montar a raiz só de leitura a partir de `Platform::initrd`
  e guardar o índice após validação completa. Decisão D-123.
- **PRs abertos** — ramo `codex/montar-initrd-arranque` sobre o #31.
  #20, #23, #24 e #31 atualizados para o `main` que inclui o #38.
- **Testes/fuzzing** — 12 arranques em QEMU 8.2.2 passaram em BIOS e
  UEFI, debug e release. Verificaram o conteúdo de `/etc/kitty.txt` e a
  sua entrada de diretório, a recusa de um checksum inválido e o arranque
  sem módulo. Fuzzing ASan do parser e do VFS passou 396 842 e 293 504
  casos, respetivamente.
- **Benchmarks** — sem medições nesta sessão.
- **Proteções de segurança alteradas** — o VFS publica a raiz depois de
  validar todos os cabeçalhos, tamanhos, nomes e conflitos do arquivo.
- **Bloqueado por / à espera do Claude** — as operações de ficheiros
  em user space dependem da ABI de syscalls e dos direitos por processo.
- **Próximo passo sugerido para o Claude** — definir a interface de
  ficheiros para os processos quando o user mode estiver disponível.
- **Avisos** — o autoteste usa arquivos e meios de arranque próprios
  em `build/initrd-tests/`.

### [2026-11-25] — CODEX (fonte bitmap)

- **Feito** — atlas de glifos de um bit, recorte e fonte original
  ASCII 5 × 7 em `userland/gfx`. Decisão D-112.
- **PRs abertos** — ramo `codex/fonte-bitmap` sobre o #26.
- **Testes/fuzzing** — 12 testes gráficos passaram no host e em Miri.
  Os três novos verificam a geometria e os limites dos glifos,
  recorte de glifos de mais de um byte por linha e texto com quebras
  e fallback. Uma prévia gráfica foi desenhada no host com a fonte
  e o double buffering.
- **Benchmarks** — sem medição de cadência de frames.
- **Bloqueado por / à espera do Claude** — execução de apps no user
  space e framebuffer através de DISPLAY.
- **Próximo passo sugerido para o Claude** — continuar user mode e
  preparar o contrato de apresentação do framebuffer.
- **Avisos** — os acentos usam `?` nesta fonte. PSF e TrueType são
  etapas seguintes. Sem `unsafe` novo.

### 2026-11-27 — CODEX
- Feito: adaptar a fonte bitmap do #27 ao `main` depois da integração
  do double buffer, do cursor e da pilha de janelas no #82.
- PRs abertos: #27. O #54 depende da fonte e o #57 e o #66 dependem do #54.
- Testes/fuzzing: fmt, Clippy e testes da biblioteca gráfica no host.
- Benchmarks: sem medições nesta atualização.
- Bloqueado por / à espera do Claude: integração no `main` após revisão
  humana e checks da nova base.
- Próximo passo sugerido para o Claude: usar as bases atualizadas do
  terminal antes de avaliar a demonstração em duas janelas (#64).
- Avisos: atualização de módulos e documentação, sem `unsafe` novo.
