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
Hoje só existem as duas consolas fixas do núcleo. O registo dinâmico
(`console::register`) chega na Fase 3, quando houver heap.

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

Textos estáveis na série que os testes podem verificar:
`[cpu] breakpoint at rip=` (prefixo emitido pelo handler de #BP),
`[cpu] breakpoint exception handled, execution resumed`,
`*** CPU EXCEPTION: <nome>`, `=== <nome> (vector N)`,
`non-present page on read in kernel mode`,
`(running on the IST double-fault stack)`.

---

## Planeadas (assinaturas propostas, podem mudar)

| Interface | Fornece | Usa | Fase |
|---|---|---|---|
| `mm::alloc_frame() -> Option<PhysFrame>` / `mm::free_frame(PhysFrame)` (frame limpo a zero) | CLAUDE | CODEX (drivers com DMA) | 3 |
| `mm::map_mmio(PhysAddr, len) -> Result<MmioRegion, MapError>` (uncached, NX) | CLAUDE | CODEX | 3 |
| `console::register(&'static SpinLock<dyn Console>)` | CLAUDE | CODEX | 3 |
| `arch::irq::register(vector, handler: fn(&InterruptFrame))` | CLAUDE | CODEX | 4 |
| `sync::IrqSpinLock<T>` | CLAUDE | todos | 4 |
| `sched::tick()` — chamada pelo driver do timer a cada interrupção | CLAUDE | CODEX | 4/5 |
| `sched::spawn_kernel_thread(fn() -> !)` | CLAUDE | CODEX | 5 |
| `UserPtr<T>`, `copy_from_user`, `copy_to_user` | CLAUDE | CODEX (VFS) | 6 |
| ABI das syscalls (números, registos, códigos de erro) | CLAUDE | CODEX (userland) | 6 |
| `trait FileSystem` / `trait Inode` do VFS | CODEX | CLAUDE (syscalls) | 7 |
| `enum CacheType { WriteBack, WriteCombining, Uncached }` nas APIs de mapeamento (`map_mmio`, mapeamentos de user) | CLAUDE | CODEX | 3 |
| Contagem de referências de frames: `PhysFrame` partilhado só através do VMM (os drivers nunca libertam frames de outros) | CLAUDE | CODEX | 3 |
| Filas de espera: `WaitQueue::wait()` / `wake_one()` / `wake_all()` | CLAUDE | CODEX (drivers que bloqueiam) | 5 |
| Handles e direitos: `Handle`, `Rights { READ, WRITE, MAP, TRANSFER, … }`, `handle_close`, `handle_duplicate(h, rights_menores)` | CLAUDE | CODEX (userland) | 6 |
| `struct InputEvent` unificado (teclado + rato: tipo, código, valor, timestamp) e `input::push(InputEvent)` chamada pelos drivers | CODEX (formato) + CLAUDE (entrega) | ambos | 11 |
| Syscalls de memória partilhada: `shm_create(size) -> Handle`, `shm_map(h, CacheType) -> UserPtr`, `shm_unmap` | CLAUDE | CODEX | 12 |
| Syscalls de canais: `channel_create() -> (Handle, Handle)`, `channel_send(h, bytes, handles)`, `channel_recv(h, …)` (bloqueante e não bloqueante), tamanho máximo fixo por mensagem | CLAUDE | CODEX | 12 |
| Capacidades `DISPLAY` (`display_map() -> framebuffer WC + geometria`) e `INPUT` (`input_open() -> Handle` de canal de eventos) | CLAUDE | CODEX (compositor) | 10–11 |
| Protocolo compositor ↔ aplicações (criar janela, anexar buffer, damage, eventos, foco) | CLAUDE (spec) | CODEX (implementação) | 13 |
