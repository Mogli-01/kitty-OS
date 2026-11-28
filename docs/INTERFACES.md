# Interfaces partilhadas

Traits, funções e tipos que um lado usa do outro. **Qualquer alteração aqui
exige atualizar este ficheiro no mesmo PR.** Cada entrada diz quem a
fornece, quem a usa e o estado (ESTÁVEL / INSTÁVEL / PLANEADA).

---

## Disponíveis hoje

### `kprint!` / `kprintln!` — ESTÁVEL
Fornece: CLAUDE (`kernel/src/console/`). Usa: todos.
Escrevem em todas as consolas (série + framebuffer). Mesma sintaxe que
`print!`/`println!`. **Não formatar endereços do kernel** (`{:p}`, `{:#x}`
de ponteiros) em mensagens que o user space possa ver.

### `console::debug_print(format_args!(…))` — ESTÁVEL
Fornece: CLAUDE. Usa: todos, para mensagens com endereços do kernel.
Escreve **só** na porta série (nunca no ecrã) e nunca bloqueia no lock da
consola. Ver D-023.

### `trait Console` — INSTÁVEL
Fornece: CLAUDE. Implementa: CODEX (drivers de consola).
```rust
pub trait Console: Send {
    /// Writes UTF-8 text. Characters the device cannot show are replaced.
    fn write_str(&mut self, s: &str);
}
```
Registo: `console::register(Box<dyn Console>)` (precisa do heap, já
disponível depois de `mm::heap::init`). A consola registada passa a receber
todo o output do kernel.

### `sync::SpinLock<T>` — INSTÁVEL
Fornece: CLAUDE (`kernel/src/sync/`). Usa: todos.
```rust
impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self;
    pub fn lock(&self) -> SpinLockGuard<'_, T>;
    pub fn try_lock(&self) -> Option<SpinLockGuard<'_, T>>;
}
```
**Não** desativa interrupções: só para dados que nenhum handler de
interrupção toca. Para o resto, `IrqSpinLock` (abaixo).

### `mm::PhysAddr` / `mm::VirtAddr` — ESTÁVEL
Fornece: CLAUDE (`kernel/src/mm/addr.rs`). Usa: todos.
`PhysAddr::new(u64) -> Option<PhysAddr>` (rejeita bits acima de 51),
`VirtAddr::new(u64) -> Option<VirtAddr>` (rejeita endereços não canónicos),
`as_u64`, `align_down`, `VirtAddr::is_user`. Nunca passar endereços como
`u64` solto entre módulos.

### `arch::x86_64::port::Port<T>` — ESTÁVEL
Fornece: CLAUDE. Usa: CODEX (camadas de hardware dos drivers).
```rust
impl<T: PortWidth> Port<T> {          // T = u8 | u16 | u32
    pub const unsafe fn new(port: u16) -> Self;
    pub fn read(&self) -> T;
    pub fn write(&self, value: T);
}
```
Contrato de `new`: nenhum acesso a esta porta pode violar a segurança de
memória (ex.: programar DMA). Cada `Port::new` precisa de `// SAFETY:` e de
entrada em `docs/UNSAFE.md`.

### `boot_info::{BootInfo, FrameBuffer}` — INSTÁVEL
Fornece: CLAUDE. Uso interno do núcleo; os drivers não recebem estes tipos
diretamente.

### Teste de arranque — ESTÁVEL
Fornece: CLAUDE (`xtask`). Mantém: CODEX.
`cargo xtask test [--bios] [--release]` compila o kernel com a feature
`qemu-exit`, arranca o QEMU sem ecrã, guarda a série em
`build/serial-test.log` e passa se o QEMU sair com código 33 e a série
contiver `Hello from kitty-OS`. O panic handler sai com código 35.

Opções (Fase 2):
- `--features a,b` — features extra do kernel (ex.: `selftest-page-fault`,
  `selftest-double-fault`).
- `--expect-panic` — o teste passa só se o kernel entrar em panic (código
  35 e `KERNEL PANIC` na série).
- `--expect <texto>` — a série tem de conter também `<texto>` (repetível).
  **Usar sempre com os autotestes de falhas**, para verificar a razão.

Textos estáveis na série que os testes podem verificar:
`[cpu] breakpoint at rip=` (prefixo emitido pelo handler de #BP),
`[cpu] breakpoint exception handled, execution resumed`,
`*** CPU EXCEPTION: <nome>`, `=== <nome> (vector N)`,
`non-present page on read in kernel mode`,
`(running on the IST double-fault stack)`,
`[mm] W^X audit: N mappings, 0 writable+executable, 0 user pages in kernel half`,
`[mm] MMIO self-test passed`,
`protection violation on write`, `protection violation on instruction fetch`.

Autotestes de falhas (todos com `--expect-panic`): `selftest-page-fault`,
`selftest-double-fault`, `selftest-stack-overflow` (double fault),
`selftest-write-text` e `selftest-write-idt` (#PF de escrita),
`selftest-exec-data` (#PF de instruction fetch), `selftest-irq-pit` (**sem**
`--expect-panic`: `--expect "hardware IRQ0 (PIT) delivered twice"`; duas
interrupções depois da baseline provam que o EOI funciona),
`selftest-heap-overflow`
(`non-present page on write`). Arranque normal: `[mm] kernel heap ready`,
`heap self-test passed`,
`[irq] interrupts enabled (controller: 8259 PIC); dispatch, IrqSpinLock and port-filter self-tests passed`,
`[acpi] MADT validated` (ou `[acpi] no usable MADT`),
`[apic] capability self-test passed`. Em BIOS (SeaBIOS) o RSDP está na
área 0xE0000–0xFFFFF, que a allow-list de firmware aceita.

`cargo xtask test-memory [--bios] [--release]` corre o arranque normal e
uma compilação separada para cada um dos sete autotestes de falhas. Antes
das falhas, confirma `frame allocator self-test passed`, a auditoria W^X,
`[mm] running on the kernel stack (guard page below it)`, o autoteste do
heap e o do MMIO. Guarda a série em `build/memory-tests/<firmware>-<perfil>/`.
`test` e `test-memory` dão erro se o log não puder ser gravado.

`cargo xtask test-keyboard [--bios] [--release]` verifica a pressão e
libertação de `A` e da seta para cima através de IRQ1. Usa a feature
`selftest-ps2-keyboard` para a emissão dos marcadores de teste e guarda a
série em `build/serial-keyboard.log`.

### Heap do kernel (`alloc`) — ESTÁVEL
Fornece: CLAUDE (`kernel/src/mm/heap.rs`). Usa: todos.
`Box`, `Vec`, `String`, … funcionam depois de `mm::heap::init()`.
Alocações vêm sempre a zeros. O lock do heap é um `IrqSpinLock` (Fase 4),
mas continua a ser **desaconselhado alocar em handlers de interrupção**
(latência imprevisível).

---

### `mm::alloc_frame()` / `mm::PhysFrame` — INSTÁVEL
Fornece: CLAUDE (`kernel/src/mm/pmm.rs`). Usa: núcleo; CODEX (drivers com
DMA, através do VMM quando existir).
```rust
pub fn alloc_frame() -> Option<PhysFrame>;   // frame de 4 KiB, sempre a zeros
impl PhysFrame {
    pub fn addr(&self) -> PhysAddr;
    pub fn share(&self) -> Result<PhysFrame, FrameError>; // +1 referência
}
impl Drop for PhysFrame { /* -1 referência; a 0: limpo e libertado */ }
pub fn pmm::stats() -> PmmStats;              // total/free, sem endereços
```
`PhysFrame` não é `Copy`: cada valor é uma referência. Nunca guardar só o
endereço e deixar o `PhysFrame` cair, senão o frame é libertado.

### `mm::map_mmio` / `MmioRegion` / `CacheType` — INSTÁVEL
Fornece: CLAUDE (`kernel/src/mm/vmm.rs`). Usa: CODEX (drivers).
```rust
pub enum CacheType { WriteBack, WriteCombining, Uncached }
pub unsafe fn mm::vmm::map_mmio(phys: PhysAddr, len: u64, cache: CacheType)
    -> Result<MmioRegion, MapError>;
impl MmioRegion {
    pub fn read<T: MmioValue>(&self, offset: u64) -> Option<T>;   // u8/u16/u32/u64
    pub fn write<T: MmioValue>(&self, offset: u64, v: T) -> Option<()>;
    pub fn len(&self) -> u64;
}
```
Mapeia numa janela própria, nunca executável; recusa RAM/firmware. Acessos
voláteis, com limites e alinhamento verificados (`None` se inválidos).
Contrato de `map_mmio` (unsafe): nenhum acesso ao dispositivo pode violar a
segurança de memória (ex.: DMA). Para registos de dispositivos use
`CacheType::Uncached`; para framebuffers `WriteCombining`.

### `mm::MapFlags` / `mm::vmm::with_kernel_space` — INSTÁVEL
Fornece: CLAUDE. Uso interno do núcleo (pilhas de threads, heap).
`MapFlags { writable, executable, user, global, cache }`; W+X é recusado.

### Interrupções (Fase 4) — INSTÁVEL
Fornece: CLAUDE (`kernel/src/arch/x86_64/irq.rs`, reexportado como
`arch::irq`). Usa: CODEX (drivers). Responde à issue #13.

**Registar um IRQ de uma linha ISA** (ex.: teclado PS/2 = linha 1):
```rust
pub type IrqHandler = fn(Vector);
pub fn arch::irq::register_isa(line: u8, handler: IrqHandler) -> Result<Vector, IrqError>;
pub fn arch::irq::unregister_isa(line: u8) -> Result<(), IrqError>;
```
- O núcleo escolhe um vetor livre, pede ao controlador ativo para ligar a
  linha a esse vetor (o PIC usa sempre `0x20 + linha`, por isso o IRQ1 chega
  no vetor **0x21**; o I/O APIC poderá usar outro), instala o handler e **só
  depois** desmascara a linha.
- `IrqError`: `InvalidLine`, `InUse`, `NoFreeVector`, `ControllerError`,
  `NotInitialised`.

**Contexto em que o handler corre:**
- ring 0, na pilha atual do kernel, com **interrupções desligadas** (os
  handlers nunca se aninham);
- curto, sem bloquear nem dormir;
- pode usar `IrqSpinLock` (todos os globais do kernel usam); nunca um
  `SpinLock` simples que código fora de interrupções também use;
- não deve alocar (é seguro, mas torna a latência imprevisível);
- **não envia EOI**: o núcleo envia-o depois de o handler voltar, para todo o
  interrupt não espúrio num vetor 0x20–0xEF, haja ou não handler. IRQs
  espúrios (IRQ7/IRQ15 do PIC, vetor 0xFF do APIC) nunca chegam ao handler.
- `connect_isa` de um controlador só pode devolver vetores 0x20–0xEF
  (senão `ControllerError`).

**Fontes locais do APIC (issue #36):**
```rust
pub fn arch::irq::register_local(handler: IrqHandler) -> Result<Vector, IrqError>;
pub fn arch::irq::unregister_local(vector: Vector) -> Result<(), IrqError>;
```
- Reserva um vetor de dispositivo livre (0x30–0xEF) e instala o handler,
  **sem** rota ISA (timer, erro ou térmica da LVT do local APIC).
- Ordem: programar a LVT **mascarada** com o vetor devolvido → só depois
  desmascarar. Para parar ou reconfigurar: **mascarar primeiro**, depois
  `unregister_local`. Uma interrupção já a caminho que chegue depois
  encontra o vetor sem handler: conta como inesperada e **recebe na mesma o
  EOI** (regra de EOI por origem, D-039), por isso o APIC nunca fica preso.
- O EOI é do núcleo, como para todas as fontes de dispositivo.
- Pode ser chamado antes ou depois do `sti`. Só o CPU de arranque nesta
  fase (o timer local é por CPU: com SMP, Fase 9, cada CPU regista o seu).

**Controlador de interrupções** (para o driver de I/O APIC + local APIC):
```rust
pub trait InterruptController: Sync {
    fn name(&self) -> &'static str;
    fn connect_isa(&self, line: u8, preferred: Vector) -> Result<Vector, IrqError>; // deixa mascarada
    fn unmask_isa(&self, line: u8);
    fn mask_isa(&self, line: u8);
    fn eoi(&self, vector: Vector);
    fn is_spurious(&self, vector: Vector) -> bool;
    fn disable(&self);
}
pub fn arch::irq::set_controller(c: &'static dyn InterruptController);
```
Até haver driver de APIC, o controlador é o PIC 8259 do núcleo (D-039).
`set_controller` mascara o anterior e esquece as linhas ligadas por ele (os
drivers registam-se de novo). Os métodos são chamados com interrupções
desligadas e o lock da tabela preso: não podem chamar de volta `arch::irq`.

**Mapa de vetores:** 0x00–0x1F exceções · 0x20–0x2F PIC · 0x30–0xEF
dinâmicos · 0xF0–0xFE internos do kernel · 0xFF espúrio do APIC.

**Acesso a hardware sem `unsafe`** (crate `kitty-hal`, D-038):
```rust
pub trait kitty_hal::PortBlock {            // portas I/O, por offset da base
    fn read8(&self, offset: u16) -> Option<u8>;
    fn write8(&self, offset: u16, value: u8) -> Option<()>;
}
pub trait kitty_hal::RegisterBlock { /* read32/write32/read64/write64 por offset */ }
pub fn arch::legacy::claim(d: LegacyDevice) -> Option<LegacyPorts>; // impl PortBlock
// LegacyDevice::I8042 → offsets 0 (0x60, dados) e 4 (0x64, estado/comando)
// LegacyDevice::Pit   → offsets 0–3 (0x40–0x43) · LegacyDevice::Cmos → 0–1
```
Cada dispositivo só pode ser reclamado uma vez (o `Drop` liberta-o). Offsets
fora da lista devolvem `None`. **Escritas filtradas:** na porta de comando do
i8042 (offset 4) só passam `0x20, 0x60, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB, 0xAD,
0xAE, 0xD4` (`I8042_ALLOWED_COMMANDS`); `0xD1` (output port: reset/A20) e
`0xF0`–`0xFF` (pulsos/reset) devolvem `None`. No CMOS, o bit 7 do índice
(desliga NMIs) é sempre escrito a 0. Se um driver precisar de outro comando,
pede-o numa issue `para-claude`.

**ACPI e APIC (issue #18):**
```rust
pub struct devices::Platform<'a> {
    pub madt: Option<&'a [u8]>,   // cópia da MADT, validada sobre a própria cópia
}                                 // (assinatura, tamanho ≤ 64 KiB, checksum)
pub fn devices::init(platform: &Platform<'_>);

pub fn arch::apic::local_apic() -> Result<LocalApicRegisters, ApicError>;
pub fn arch::apic::io_apic(phys: u64) -> Result<IoApicRegisters, ApicError>;
// ambos: impl kitty_hal::RegisterBlock (32 bits; 64 bits → None)
// ApicError: Disabled, X2Apic, NotDeclared, Implausible, InUse, MapFailed
```
- **Local APIC:** a base vem do MSR IA32_APIC_BASE. Se o APIC estiver
  desligado ou em **x2APIC**, devolve erro: ficar no PIC. Escrevíveis: TPR
  0x080, EOI 0x0B0, SVR 0x0F0 (vetor espúrio tem de ser **0xFF**), LVT
  timer 0x320 e erro 0x370 (só Fixed), LINT0 0x350 e LINT1 0x360 (Fixed,
  NMI ou ExtINT), contagem inicial 0x380, divisor 0x3E0. O **ICR**
  (0x300/0x310) **não** é escrevível (IPIs são do núcleo). Leitura: qualquer
  registo alinhado < 0x400.
- **I/O APIC:** `phys` tem de ser um dos endereços que a MADT validada pelo
  kernel declara (entrada tipo 1), sem sobrepor a página do local APIC nem
  outro I/O APIC, e com versão plausível; janela de 0x20 bytes: IOREGSEL
  (0x00) e IOWIN (0x10). **Ao reclamar, o IOREGSEL fica em 0x01 (versão).**
  O kernel serializa seletor e janela internamente (os filtros não dependem
  disso), mas uma sequência "seleciona, lê, modifica, escreve" do driver
  continua a precisar do lock do próprio driver para ser coerente.
  Redireções permitidas: Fixed, Lowest-priority, NMI, ExtINT.
- **Vetores:** uma LVT ou redireção Fixed/Lowest-priority **não mascarada**
  tem de usar um vetor **0x20–0xEF**. Mascarada, o vetor não é verificado
  (é-o quando for desmascarada, porque cada escrita leva o registo inteiro).
- Cada APIC tem um só dono de cada vez; o `Drop` liberta-o.
- **Não usar as capacidades no handler de NMI**: usam `IrqSpinLock` e o
  `cli` não mascara NMIs (deadlock se a NMI chegar com o lock preso).

**Transição PIC → APIC** (em `devices::init`, com interrupções desligadas):
1. Estado à entrada: PIC remapeado e com **todas** as linhas mascaradas;
   MADT validada; endereços dos I/O APIC registados no kernel.
2. Sem `platform.madt`, ou `local_apic()` com erro (`X2Apic`, `Disabled`):
   **ficar no PIC** (não fazer nada).
3. Driver de APIC: `io_apic(addr)` para cada I/O APIC da MADT →
   `mask_all()` (apaga as rotas do firmware) → local APIC: SVR com software
   enable e vetor espúrio **0xFF**, TPR = 0, LVT timer/erro **mascarados**,
   LINT0 **mascarado** (o ExtINT do PIC deixa de ser usado), LINT1 = **NMI**
   segundo as entradas "Local APIC NMI" da MADT (a configuração de NMI é
   separada do encaminhamento de IRQs).
4. `arch::irq::set_controller(&APIC)`: o núcleo mascara o PIC por completo
   e esquece as linhas ligadas por ele.
5. Só depois os outros drivers fazem `register_isa`: `connect_isa` aplica os
   overrides da MADT (ex.: no QEMU q35 o IRQ0 vai para a GSI 2), deixa a rota
   **mascarada**, o núcleo instala o handler e só então desmascara.
6. O kernel liga as interrupções (`sti`) depois de `devices::init`.
O EOI do APIC é a escrita em 0x0B0 feita por `InterruptController::eoi`.

**initrd (issue #22):**
```rust
pub struct devices::Platform<'a> {
    pub madt: Option<&'a [u8]>,
    pub initrd: Option<&'static [u8]>,   // módulo `kitty-initrd`, validado
}
```
- **Marca:** `module_string: kitty-initrd` no `limine.conf` (chega no campo
  `cmdline` do ficheiro). Comparação **exata** (`kitty-initrdX` não conta).
- **Validação do núcleo** antes de criar o slice: um só módulo marcado
  (dois → recusado como ambíguo; mais de 64 módulos → recusado); tamanho
  1 B–128 MiB; o intervalo físico inteiro dentro de uma única região "kernel
  and modules" do mapa, sem tocar em nenhuma página da imagem do kernel, e
  com todas as páginas mapeadas só de leitura.
- **Garantias do slice:** imutável e válido para sempre (`'static`): as
  páginas estão mapeadas só de leitura e NX no HHDM e o PMM nunca as entrega.
  Não há cópia: o VFS pode guardar referências para os ficheiros do arquivo.
  O slice tem exatamente o tamanho do ficheiro reportado pelo Limine.
- **Falhas:** sem módulo → `None` e `[initrd] no module`; módulo recusado →
  `None` e `[initrd] module REFUSED (<razão>)` (`Ambiguous`,
  `TooManyModules`, `BadSize`, `NotInDirectMap`, `OutsideModuleMemory`,
  `OverlapsKernel`, `WritableAlias`). O arranque continua sem initrd.
- **O que fica com o VFS (Codex):** validar o formato USTAR (cabeçalhos,
  checksums, tamanhos, nomes) antes de expor qualquer entrada.
- Marcadores na série: `[initrd] module validated (N KiB, read-only)`,
  `[initrd] no module`, `[initrd] module REFUSED`.

**Onde ligar um driver:** em `kernel::devices::init()`
(`kernel/src/devices/`), que corre depois do heap e do núcleo de
interrupções e **antes** de as interrupções serem ligadas (D-040). Ex. para
o teclado:
```rust
// kernel/src/devices/keyboard.rs (Rust seguro; o lint proíbe unsafe aqui)
static KBD: IrqSpinLock<Option<kitty_ps2::I8042<LegacyPorts>>> = IrqSpinLock::new(None);
fn on_irq1(_: Vector) { if let Some(k) = KBD.lock().as_mut() { k.on_interrupt(); } }
pub fn init() {
    let Some(ports) = legacy::claim(LegacyDevice::I8042) else { return };
    // … configurar o i8042 (Set 2, sem tradução) …
    *KBD.lock() = Some(kitty_ps2::I8042::new(ports));
    let _ = irq::register_isa(1, on_irq1);
}
```

### `sync::IrqSpinLock<T>` — ESTÁVEL
Fornece: CLAUDE (`kernel/src/sync/irq_spinlock.rs`). Usa: todos.
Mesma API que `SpinLock` (`new`, `lock`, `try_lock`); desliga as
interrupções enquanto o guard existe e repõe o estado anterior ao libertar.
Usar para tudo o que um handler de interrupção possa tocar. Os guards
(`SpinLockGuard`, `IrqSpinLockGuard`) são `!Send` e só são `Sync` se
`T: Sync`. **Regra:**
guards aninhados libertam-se por ordem inversa (LIFO); um `drop` fora de
ordem pode religar as interrupções com outro lock ainda preso.

### Scheduler e threads do kernel (Fase 5) — INSTÁVEL
Fornece: CLAUDE (`kernel/src/sched/`). Usa: CODEX (timer, drivers com
threads), núcleo.
```rust
pub fn sched::spawn(name: &'static str, entry: fn()) -> Result<ThreadId, SpawnError>;
pub fn sched::yield_now();
pub fn sched::exit() -> !;          // também acontece quando `entry` volta
pub fn sched::tick();               // chamar no handler do timer
pub fn sched::current() -> ThreadId;
pub fn sched::switches() -> u64;
```
- **Timer (Codex):** o handler do IRQ do timer só tem de chamar
  `sched::tick()` (não aloca nem toma locks). Não troca de thread dentro do handler: o núcleo fá-lo no
  fim do dispatch, depois do EOI. Frequência sugerida: 100–1000 Hz.
- Cada thread tem 32 KiB de pilha com página de guarda; um overflow dá
  double fault com relatório (não corrompe outra thread).
- **Não preempta** enquanto houver um `SpinLock` simples preso; por isso,
  num laço longo, não seguras um `SpinLock` (usa `IrqSpinLock` com secções
  curtas, ou liberta o lock).
- Para esperar por algo **sem gastar CPU**, usar `sched::WaitQueue` (abaixo).
- Marcadores: `[sched] cooperative self-test passed`, e com a feature
  `selftest-preempt` `[sched] preemption self-test passed`;
  `selftest-thread-stack-overflow` → double fault na IST.

### `sched::WaitQueue` (Fase 5) — INSTÁVEL
Fornece: CLAUDE (`kernel/src/sched/wait.rs`). Usa: CODEX (drivers com
leituras bloqueantes, input), núcleo (IPC, Fase 12).
```rust
pub const fn WaitQueue::new() -> WaitQueue;            // pode ser `static`
pub fn wait_until(&self, ready: impl FnMut() -> bool); // bloqueia até ready()
pub fn wake_one(&self) -> bool;                        // também em handlers de IRQ
pub fn wake_all(&self) -> usize;                       // também em handlers de IRQ
```
- `wake_one`/`wake_all` **não usam o heap** (a fila mantém o buffer; o
  scheduler reserva espaço na fila de prontas no `spawn`), por isso são
  seguros em handlers de IRQ. O `wait_until` (só em contexto de thread)
  pode alocar ao registar-se.
- Padrão: o produtor **publica primeiro** (ex.: põe o byte num buffer
  protegido por `IrqSpinLock`, ou num atómico) e **depois** chama
  `wake_one`. O consumidor chama `wait_until(|| há_dados())`.
- `ready` tem de ser barato e não pode bloquear nem tomar um `SpinLock`
  simples (pode ler atómicos ou tomar um `IrqSpinLock` curto).
- **Só em contexto de thread**, com interrupções ligadas e sem nenhum lock
  preso: **nunca num handler de IRQ** (lá usa-se `wake_one`/`wake_all`). O
  uso errado dá panic logo à entrada. Volta sempre com as interrupções
  ligadas.
- Não há wake-ups perdidos (D-046); um wake-up espúrio só faz a condição ser
  verificada de novo.
- Marcadores: `[sched] wait queue self-test passed`; com `selftest-preempt`,
  `idle + wake from IRQ work` (a thread de arranque dorme na thread idle e é
  acordada pelo handler do timer).

### Fase 6: processos, syscalls, consola e INPUT — DESENHO
Contrato completo em **`docs/FASE6.md`** (issues #46 e #50): ABI das
syscalls e códigos de erro, `UserPtr`/`copy_*_user`, handles e direitos,
`console_read`/`console_write` e `console::input_push` (driver de teclado),
arranque de `/sbin/init` a partir do initrd, e o formato `InputEvent` (24
bytes) com `input_read`/`input::push`. Sem autoridade ambiente (D-058):
o `init` recebe os handles de CONSOLE, raiz do initrd, INPUT e DISPLAY e
passa-os com `process_spawn`; não há `input_open`. O Codex pode programar
a shell, a biblioteca de syscalls e o compositor contra esse contrato.

### Memória de user: `mm::user` (Fase 6a) — INSTÁVEL
Fornece: CLAUDE (`kernel/src/mm/user.rs`). Usa: núcleo (syscalls).
```rust
pub fn UserSlice::new(addr: u64, len: usize) -> Result<UserSlice, UserError>; // só valida o intervalo
pub fn copy_from_user(dst: &mut [u8], src: UserSlice) -> Result<(), UserError>;
pub fn copy_to_user(dst: UserSlice, src: &[u8]) -> Result<(), UserError>;
pub fn UserPtr::<T>::new(addr: u64) -> Result<UserPtr<T>, UserError>;    // T = u8..u64, i32, i64
pub fn UserPtr::<T>::read(self) -> Result<T, UserError>;
pub fn UserPtr::<T>::write(self, v: T) -> Result<(), UserError>;
// UserError::BadRange (fora da metade de user, overflow, > 1 MiB) → E_INVAL/E_FAULT
// UserError::Fault (não mapeado / sem acesso) → E_FAULT
```
- Nunca há panic por um ponteiro de user mau.
- Copiar uma vez para o kernel e só depois validar (sem TOCTOU).
- Marcador: `[mm] user-copy self-test passed`; features `selftest-smap`
  (`protection violation on read in kernel mode`) e `selftest-smep`
  (`protection violation on instruction fetch in kernel mode`).

### Eventos internos do PS/2 — INSTÁVEL

Fornece: CODEX (`kitty_ps2::Ps2Input`). A fila tem 64 entradas e entrega
`InputEvent::Key(KeyEvent)` e `InputEvent::Mouse(MouseReport)` por ordem
de descodificação. IRQ1 e IRQ12 partilham o bloco de portas. O bit AUX
do estado identifica o dispositivo que enviou o byte. Cada handler lê no
máximo um byte.

`next_input` devolve `InputEvent::Lost` antes de retomar a entrega após
perda. O consumidor limpa o estado de teclas e botões retidos. Um byte AUX
corrompido, quando a posição é conhecida, descarta o restante pacote. Um
erro que torne a posição desconhecida exige nova configuração do rato.
A entrega a processos e os timestamps dependem das futuras interfaces de
`INPUT` e de tempo monotónico.

### Raiz só de leitura do initrd (`kitty-vfs`) — INSTÁVEL

Fornece: CODEX (`fs/vfs`). O módulo de arranque vem do núcleo (#22).

A montagem aceita até 65 536 entradas USTAR, contando o cabeçalho da raiz
quando presente. Acima desse limite, devolve `Error::TooManyEntries` antes
de reservar memória para o índice.

```rust
impl<'a> InitrdFs<'a> {
    pub fn mount(bytes: &'a [u8]) -> Result<Self, Error>;
    pub fn lookup(&self, path: &str) -> Result<Inode<'a>, Error>;
    pub fn read_dir(&self, path: &str) -> Result<ReadDir<'_, 'a>, Error>;
}
impl File<'_> {
    pub fn read_at(self, offset: u64, destination: &mut [u8]) -> usize;
}
```

`lookup` e `read_dir` recebem caminhos absolutos canónicos UTF-8. A raiz
é `/`. Os restantes caminhos têm até 256 bytes depois da barra inicial.
`lookup` devolve `Inode::Directory` ou `Inode::File(File)`. `read_dir`
enumera os filhos diretos por ordem das componentes, incluindo diretórios
implícitos. `read_at` lê para um buffer Rust já validado, devolve zero no
fim do ficheiro e preserva o resto do buffer. O inode de ficheiro e as
entradas de diretório conservam o tempo de vida dos bytes do arquivo.

O caminho por syscall depende de validação de ponteiros e direitos pelo
núcleo. Esta interface é a primeira implementação da raiz do VFS.

`devices::init` cria a raiz a partir de `Platform::initrd`. O marcador
`[vfs] initrd mounted read-only` confirma que o índice foi publicado.
`[vfs] initrd archive rejected` confirma que o VFS recusou o conteúdo e
continua o arranque sem a raiz. `selftest-initrd` verifica a listagem e
os bytes de `/etc/kitty.txt` no arquivo de teste e emite
`[vfs] initrd read verified (/etc/kitty.txt)` quando ambos coincidem.

### Aleatoriedade: `rng` e `kitty-crypto` (Fase 8a) — INSTÁVEL
Fornece: CLAUDE (`kernel/src/rng.rs`, crate `crypto/` = `kitty-crypto`, D-066). Usa: núcleo (ASLR, KASLR, chaves) e user space (`random_fill`).
```rust
pub fn rng::fill(out: &mut [u8]);     // seguro em qualquer contexto
pub fn rng::u64() -> u64;
pub fn rng::below(bound: u64) -> u64; // sem enviesamento
pub fn kitty_crypto::sha256::sha256(data: &[u8]) -> [u8; 32];
pub fn kitty_crypto::chacha20::{block, xor_stream};
pub fn kitty_crypto::{wipe, ct_eq};   // apagar segredos; comparar em tempo constante
```
- **Syscall 8, `random_fill(buf, len)`:** até 512 bytes por chamada, sem handle (não dá autoridade nenhuma).
- **Arranque:** `[rng] seeded (RDSEED n words, RDRAND n words, r rejected, j jitter samples)`, com `-- WARNING ... weak entropy` se não houver fonte de hardware de confiança.
- **Testes no host:** `cargo test -p kitty-crypto` corre os vetores oficiais (FIPS 180-4, RFC 8439), o "um milhão de a" e as fronteiras do padding. Isto responde ao ponto 4 da revisão do #80: o adaptador de host deixa de ser preciso, porque o crate já é testável no host.

### Syscalls (Fase 6c) — INSTÁVEL (ABI de user)
Fornece: CLAUDE (`kernel/src/syscall`, D-063). Usa: CODEX (biblioteca de syscalls em `userland/`, shell).
- **Entrada:** `syscall`. O número vai em `rax`, os argumentos em `rdi, rsi, rdx, r10, r8, r9` e o resultado em `rax` (`< 0` = erro, D-049). O `rcx`/`r11` são destruídos; todos os outros registos são preservados.
- **Números e erros:** ver a tabela em `docs/FASE6.md` §3. A ABI é estável.
- O processo começa com o handle `0` = CONSOLE (`READ|WRITE|DUP|TRANSFER`).
- Um handle é um `u32` (índice + geração). Depois de `handle_close`, o número **nunca mais** é válido.
- **`console_write`:** até 4096 B. Os caracteres de controlo (exceto `\n` e `\t`) e os invisíveis aparecem como `?`; o texto de user tem `[u] ` na série. **`console_read`:** devolve no máximo 512 B por chamada.
- **`console_read`:** bloqueia até haver pelo menos 1 byte. Um processo morto enquanto espera acaba na mesma.
- **Do lado do kernel (driver de teclado):** `console::input_push(&[u8])`, que é seguro em IRQ e não usa heap. O buffer tem 4 KiB e, quando está cheio, descarta os bytes novos (`console::input_dropped()`).
- **Marcador:** `[syscall] self-test passed`. Também aparece `[user] hello from ring 3 via syscall`, escrito pelo próprio programa.

### INPUT: `input::push` e `input_read` (Fase 6e) — INSTÁVEL
Fornece: CLAUDE (`kernel/src/input.rs`, D-065). Usa: CODEX (driver PS/2 e, mais tarde, USB HID e o compositor).
```rust
#[repr(C)] pub struct input::InputEvent { pub time_ns: u64, pub kind: u16, pub code: u16,
                                          pub value: u32, pub dx: i32, pub dy: i32 } // 24 B
pub const KIND_KEY: u16 = 1; pub const KIND_MOUSE: u16 = 2; pub const KIND_LOST: u16 = 3;
pub fn input::push(event: InputEvent);                 // rato (e teclas sem texto): seguro em IRQ, sem heap
pub fn input::push_key(event: InputEvent, text: &[u8]); // tecla + o texto do layout; o kernel decide o destino
pub fn input::dropped() -> u64;          // eventos descartados (diagnóstico)
```
- **Driver (Codex):** converte cada `kitty_ps2::InputEvent` e chama `input::push_key`, para as teclas, ou `input::push`, para o rato. **Já não chama `console::input_push` diretamente.** O kernel encaminha o texto para a consola só enquanto ninguém estiver a ler o INPUT; depois da primeira `input_read` do dono, o texto deixa de chegar à consola (anti-keylogger).
- **Conversão:** Uma tecla vira `kind=1`, `code=KeyCode` e `value` 0/1/2; o rato vira `kind=2`, com os botões em `value` e `dx`/`dy`. O `time_ns` fica a 0 por agora; o kernel passa a preenchê-lo quando o relógio do #94 entrar.
- **Leitor:** o handle INPUT tem `READ|TRANSFER`, **sem `DUP`**. O `/sbin/init` recebe-o como **handle 2** e passa-o ao compositor com `SPAWN_MOVE`. A syscall 13, `input_read(h, buf, max ≤ 32)`, bloqueia até haver pelo menos 1 evento.
- **Perda:** a fila tem 256 eventos. Quando está cheia, os eventos novos são descartados. Depois de o leitor ter lido todos os eventos que estavam na fila, recebe um `KIND_LOST`, que serve para repor o estado das teclas e dos botões.
- **Troca de dono:** a fila é esvaziada sempre que o handle é fechado, o dono morre ou o handle é movido. O dono seguinte nunca vê eventos do anterior.
- **Marcador:** `[input] self-test passed`.

### Ficheiros, ELF e `process_spawn` (Fase 6d) — INSTÁVEL (ABI de user)
Fornece: CLAUDE (`kernel/src/proc/{elf,fs,spawn}.rs`, D-064). Usa: CODEX (`/sbin/init`, shell, biblioteca de syscalls).
- **O `/sbin/init`** é lido do initrd e arranca com o handle `0` = CONSOLE (`READ|WRITE|DUP|TRANSFER`), o handle `1` = DIR da raiz (`READ|EXEC|DUP|TRANSFER`) e o handle `2` = INPUT (`READ|TRANSFER`).
  - Para lançar a shell: `open_at(1, "bin/sh", READ|EXEC)` → `process_spawn(h, grants, n)`.
  - Linhas de arranque: `[init] /sbin/init started (pid N)` ou `[init] /sbin/init missing or invalid (<razão>)`.
- **Formato do executável:** ELF64 estático `ET_EXEC` x86_64, sem intérprete, sem `PT_DYNAMIC` e sem TLS. Até 16 `PT_LOAD`, nunca W+X, sem dois segmentos na mesma página, todos entre 1 MiB e `0x7FFF_FFFF_F000`. A entrada fica num segmento executável. Até 64 MiB de memória e até 16 MiB de ficheiro.
- **Pilha:** 64 KiB com topo em `0x0000_7FFF_FFFF_0000` e página de guarda por baixo. RSP alinhado a 16 na entrada.
- **No arranque do processo:** todos os registos a zero; ainda não há `argv`/`envp` (virão num contrato próprio).
- **Números das syscalls:** 9–12, ver a tabela em `docs/FASE6.md` §3.
- **Marcador:** `[proc] ELF loader and process_spawn self-test passed`.
- **Para a CI do Codex:** um caso com `--initrd` que contenha um `/sbin/init` mínimo, e as linhas `[init] /sbin/init started` e o que esse init escrever.

### Processos e user mode: `proc`, `mm::uspace`, `sched::spawn_user` (Fase 6b) — INSTÁVEL
Fornece: CLAUDE. Usa: núcleo (6c/6d: syscalls, ELF loader).
```rust
pub fn UserSpace::new() -> Result<UserSpace, MapError>;               // metade do kernel partilhada
pub fn UserSpace::map_page(&mut self, virt: u64, perm: UserPerm, contents: &[u8]) -> Result<(), MapError>;
pub fn UserSpace::audit(&self) -> UserAudit;                          // U em tudo, sem W+X, nada abaixo de 1 MiB
pub fn sched::spawn_user(name, process: Arc<Process>, rip: u64, rsp: u64) -> Result<ThreadId, SpawnError>;
pub fn Process::kill(&self);                                          // morre na próxima volta a ring 3
pub fn Process::status(&self) -> Status;                              // Running | Killed | Fault(vector)
```
- Layout: nada abaixo de `USER_MIN = 1 MiB`; código a partir de 4 MiB; pilha
  com topo em `0x0000_7FFF_FFFF_0000`.
- Uma exceção causada por código de user mata o processo (relatório só na
  série, linha `[proc] user fault: …`).
- Marcador de arranque: `[proc] user-mode self-test passed`.
- Um programa acaba por `exit` (6c), por falha ou por `kill`.

## Planeadas (assinaturas propostas, podem mudar)

| Interface | Fornece | Usa | Fase |
|---|---|---|---|
| ABI das syscalls (números, registos, códigos de erro) | CLAUDE | CODEX (userland) | 6 |
| `trait FileSystem` / `trait Inode` do VFS | CODEX | CLAUDE (syscalls) | 7 |
| Handles e direitos: `Handle`, `Rights` (bits 0–15 genéricos `READ, WRITE, MAP, TRANSFER, DUP, EXEC`; 16–31 por tipo), `handle_close`, `handle_dup(h, rights_menores)`, `handle_info(h)` (D-058) | CLAUDE | CODEX (userland) | 6 |
| `process_spawn(imagem: Handle, handles: UserSlice<(Handle, Rights)>) -> Handle` — a única forma de dar autoridade a um processo novo (D-058) | CLAUDE | CODEX (init, shell) | 6 |
| `open_at(dir: Handle, caminho, rights) -> Handle`, sem caminhos globais; o VFS recusa sair do diretório do handle (D-058) | CLAUDE (syscall) + CODEX (VFS) | ambos | 7 |
| `struct InputEvent` unificado (teclado + rato: tipo, código, valor, timestamp) e `input::push(InputEvent)` chamada pelos drivers | CODEX (formato) + CLAUDE (entrega) | ambos | 11 |
| Syscalls de memória partilhada: `shm_create(size) -> Handle`, `shm_map(h, CacheType) -> UserPtr`, `shm_unmap` | CLAUDE | CODEX | 12 |
| Syscalls de canais: `channel_create() -> (Handle, Handle)`, `channel_send(h, bytes, handles)`, `channel_recv(h, …)` (bloqueante e não bloqueante), tamanho máximo fixo por mensagem | CLAUDE | CODEX | 12 |
| Objetos `DISPLAY` (`display_map(h) -> framebuffer WC + geometria`, exige `MAP`) e `INPUT` (`input_read(h, …)`); os handles são dados pelo kernel ao `init`, sem syscall de "abrir" (D-058) | CLAUDE | CODEX (compositor) | 10–11 |
| Protocolo compositor ↔ aplicações (criar janela, anexar buffer, damage, eventos, foco) | CLAUDE (spec) | CODEX (implementação) | 13 |

## Esboços das Fases 15–17 (só desenho, vão mudar)

Assinaturas para o Codex poder planear. Nenhuma está implementada; cada
uma passa a INSTÁVEL na secção "Disponíveis hoje" quando entrar.

### Capacidades (Fase 6 → 15) — CLAUDE
```rust
// syscalls (rax = número; erros < 0 como na D-049, + E_BADSIG = -11)
handle_close(h) -> 0
handle_dup(h, rights) -> h2          // rights ⊆ rights(h), exige DUP
handle_info(h) -> (kind: u16, rights: u32)
process_spawn(image: Handle /*EXEC*/, handles: *const (u32, u32), n) -> Handle
// Fase 15: identidade e sessões
process_identity(h_proc) -> (uid, gid)         // exige READ no processo
session_create(h_accounts, uid, auth_token) -> h_session   // só o login
```
Tipos de objetos reservados: `CONSOLE`, `FILE`, `DIR`, `PROCESS`,
`CHANNEL`, `SHM`, `INPUT`, `DISPLAY`, `AUDIO`, `NET`, `SOCKET`,
`FIREWALL`, `AUDIT`, `ACCOUNTS`.

### Auditoria (Fase 15) — CLAUDE
```rust
// dentro do kernel (núcleo, drivers, VFS e stack de rede podem chamar)
pub fn audit::record(event: AuditEvent);   // nunca bloqueia, nunca aloca no caminho de IRQ
pub struct AuditEvent {
    pub time_ns: u64, pub uid: u32, pub pid: u32,
    pub kind: AuditKind,       // OpenProtected, NetRequest, PermChange, AuthFail, CapDenied, …
    pub object: u32,           // id estável do objeto, nunca um endereço do kernel
    pub result: i32,           // 0 ou código de erro
}
// user space: só quem tem um handle AUDIT com AUDIT_READ (a sessão de admin)
audit_read(h_audit, buf, max) -> n        // devolve eventos inteiros; o primeiro pode ser "perdidos: N"
```

### Criptografia (Fase 16) — CLAUDE, `kernel/src/crypto` (educativa, não auditada)
```rust
pub fn sha256(data: &[u8]) -> [u8; 32];
pub fn chacha20poly1305_seal(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], buf: &mut [u8]) -> [u8; 16];
pub fn chacha20poly1305_open(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], buf: &mut [u8], tag: &[u8; 16]) -> Result<(), CryptoError>; // tag comparada em tempo constante
pub fn ed25519_verify(public: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> Result<(), CryptoError>;
pub fn argon2id(password: &[u8], salt: &[u8], params: Argon2Params, out: &mut [u8]) -> Result<(), CryptoError>;
pub fn rng::fill(out: &mut [u8]);           // CSPRNG do kernel
// user space
random_fill(buf, len) -> n                  // sem handle (D-058)
```
A assinatura (`ed25519_sign`) vive só na ferramenta do xtask, no host: a
chave privada nunca entra no kernel nem no repositório.

### Hooks do firewall (Fase 17) — CLAUDE fornece, CODEX (stack de rede) chama
```rust
pub enum Verdict { Accept, Drop, Reject }
pub struct PacketMeta { pub dir: Direction, pub proto: Proto, pub src: IpPort, pub dst: IpPort,
                        pub socket: Option<SocketId>, pub len: u16 }
pub fn firewall::check_inbound(meta: &PacketMeta) -> Verdict;   // antes de entregar a um socket
pub fn firewall::check_outbound(meta: &PacketMeta) -> Verdict;  // antes de enviar
pub fn firewall::check_bind(owner: Pid, proto: Proto, port: u16) -> Verdict;
pub fn firewall::check_connect(owner: Pid, dst: IpPort) -> Verdict;
// user space
fw_add_rule(h_fw, rule) -> id          // admin: exige FW_ADMIN
fw_add_block(h_fw, match, ttl_s) -> id // IDS: exige só FW_ADD_BLOCK (nunca abre)
fw_events(h_fw, buf, max) -> n         // metadados para o IDS, fila limitada
```
Regras: os hooks são chamados em contexto de IRQ ou de thread, nunca
alocam e nunca bloqueiam. Sem regra que aceite, a entrada é `Drop`.
