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
`--expect-panic`: `--expect "hardware IRQ0 (PIT) delivered"`),
`selftest-heap-overflow`
(`non-present page on write`). Arranque normal: `[mm] kernel heap ready`,
`heap self-test passed`,
`[irq] interrupts enabled (controller: 8259 PIC); dispatch, IrqSpinLock and port-filter self-tests passed`.

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
Usar para tudo o que um handler de interrupção possa tocar. **Regra:**
guards aninhados libertam-se por ordem inversa (LIFO); um `drop` fora de
ordem pode religar as interrupções com outro lock ainda preso.

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
