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
**Aviso:** ainda NÃO desativa interrupções. Não usar dentro de handlers de
interrupção até à Fase 4 (`IrqSpinLock`).

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
`selftest-exec-data` (#PF de instruction fetch), `selftest-heap-overflow`
(`non-present page on write`). Arranque normal: `[mm] kernel heap ready`,
`heap self-test passed`.

`cargo xtask test-memory [--bios] [--release]` corre o arranque normal e
uma compilação separada para cada um dos sete autotestes de falhas. Antes
das falhas, confirma `frame allocator self-test passed`, a auditoria W^X,
`[mm] running on the kernel stack (guard page below it)`, o autoteste do
heap e o do MMIO. Guarda a série em `build/memory-tests/<firmware>-<perfil>/`.
`test` e `test-memory` dão erro se o log não puder ser gravado.

### Heap do kernel (`alloc`) — ESTÁVEL
Fornece: CLAUDE (`kernel/src/mm/heap.rs`). Usa: todos.
`Box`, `Vec`, `String`, … funcionam depois de `mm::heap::init()`.
Alocações vêm sempre a zeros. **Não alocar em handlers de interrupção** até
à Fase 4 (o lock ainda não desliga interrupções).

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

## Planeadas (assinaturas propostas, podem mudar)

| Interface | Fornece | Usa | Fase |
|---|---|---|---|
| `arch::irq::register(vector, handler: fn(&InterruptFrame))` | CLAUDE | CODEX | 4 |
| `sync::IrqSpinLock<T>` | CLAUDE | todos | 4 |
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
