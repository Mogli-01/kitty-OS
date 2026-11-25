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
