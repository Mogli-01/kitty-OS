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

### Fase 6: processos, syscalls, consola e INPUT — DESENHO
Contrato completo em **`docs/FASE6.md`** (issues #46 e #50): ABI das
syscalls e códigos de erro, `UserPtr`/`copy_*_user`, handles e direitos,
`console_read`/`console_write` e `console::input_push` (driver de teclado),
arranque de `/sbin/init` a partir do initrd, e o formato `InputEvent` (24
bytes) com `input_open`/`input_read`/`input::push`. O Codex pode programar
a shell, a biblioteca de syscalls e o compositor contra esse contrato.

## Planeadas (assinaturas propostas, podem mudar)

| Interface | Fornece | Usa | Fase |
|---|---|---|---|
| `sched::tick()` — chamada pelo driver do timer a cada interrupção | CLAUDE | CODEX | 4/5 |
| `sched::spawn_kernel_thread(fn() -> !)` | CLAUDE | CODEX | 5 |
| `UserPtr<T>`, `copy_from_user`, `copy_to_user` | CLAUDE | CODEX (VFS) | 6 |
| ABI das syscalls (números, registos, códigos de erro) | CLAUDE | CODEX (userland) | 6 |
| `trait FileSystem` / `trait Inode` do VFS | CODEX | CLAUDE (syscalls) | 7 |
| Filas de espera: `WaitQueue::wait()` / `wake_one()` / `wake_all()` | CLAUDE | CODEX (drivers que bloqueiam) | 5 |
| Handles e direitos: `Handle`, `Rights { READ, WRITE, MAP, TRANSFER, … }`, `handle_close`, `handle_duplicate(h, rights_menores)` | CLAUDE | CODEX (userland) | 6 |
| `struct InputEvent` unificado (teclado + rato: tipo, código, valor, timestamp) e `input::push(InputEvent)` chamada pelos drivers | CODEX (formato) + CLAUDE (entrega) | ambos | 11 |
| Syscalls de memória partilhada: `shm_create(size) -> Handle`, `shm_map(h, CacheType) -> UserPtr`, `shm_unmap` | CLAUDE | CODEX | 12 |
| Syscalls de canais: `channel_create() -> (Handle, Handle)`, `channel_send(h, bytes, handles)`, `channel_recv(h, …)` (bloqueante e não bloqueante), tamanho máximo fixo por mensagem | CLAUDE | CODEX | 12 |
| Capacidades `DISPLAY` (`display_map() -> framebuffer WC + geometria`) e `INPUT` (`input_open() -> Handle` de canal de eventos) | CLAUDE | CODEX (compositor) | 10–11 |
| Protocolo compositor ↔ aplicações (criar janela, anexar buffer, damage, eventos, foco) | CLAUDE (spec) | CODEX (implementação) | 13 |
