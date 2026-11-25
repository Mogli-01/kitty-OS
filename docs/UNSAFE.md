# Inventário de código `unsafe`

O compilador recusa `unsafe` fora de `arch/`, `mm/` e `sync/`
(`#![deny(unsafe_code)]`, D-008). Esta lista tem de estar sempre igual ao
código: quem acrescenta ou remove `unsafe` atualiza-a no mesmo PR.

Verificação rápida: `git grep -n "unsafe" -- kernel/src`.

| Ficheiro | O que é unsafe | Porquê é necessário | Porquê é seguro | Dono |
|---|---|---|---|---|
| `kernel/src/arch/x86_64/port.rs` | 6 blocos `asm!` (`in`/`out` de 8/16/32 bits); `unsafe fn port_in/port_out`; `unsafe fn Port::new`; 2 blocos em `read`/`write` | o Rust não tem instruções de I/O de portas | o contrato de `Port::new` garante que a porta não pode violar a segurança de memória; os blocos só reencaminham esse contrato | CLAUDE |
| `kernel/src/arch/x86_64/cpu.rs` | 2 blocos `asm!` (`cli`, `hlt`) | instruções privilegiadas | não tocam em memória nem na pilha | CLAUDE |
| `kernel/src/arch/x86_64/serial.rs` | 1 bloco: `Port::new` para 0x3F8..0x3FD | aceder à UART COM1 | a 16550 não faz DMA; o pior caso é output corrompido | CLAUDE |
| `kernel/src/arch/x86_64/qemu.rs` | 1 bloco: `Port::new(0xF4)` | sair do QEMU nos testes | só existe com a feature `qemu-exit`; a escrita termina a VM | CLAUDE |
| `kernel/src/arch/x86_64/boot.rs` | atributos `#[unsafe(link_section)]` (5) e `#[unsafe(no_mangle)]` (1); 1 bloco `slice::from_raw_parts_mut` do framebuffer | o Limine encontra os requests pela secção; o linker encontra `kernel_entry` pelo nome; o framebuffer só nos chega como ponteiro | secções dedicadas só com estes statics; nome único; o Limine garante o mapeamento de `pitch*height` bytes, validamos ponteiro/tamanho e um `AtomicBool` garante que o `&'static mut` só é criado uma vez | CLAUDE |
| `kernel/src/sync/spinlock.rs` | `unsafe impl Sync for SpinLock<T>`; 2 blocos (`Deref`/`DerefMut` do guard) | um lock transforma acesso partilhado a `UnsafeCell` em `&mut T` | só existe um guard de cada vez (garantido pelo `AtomicBool` com Acquire/Release); `T: Send` exigido | CLAUDE |
| `kernel/src/mm/` | nenhum (só tipos de endereço) | — | — | CLAUDE |

Dependências externas com `unsafe`: crate `limine` (leitura volátil das
respostas do bootloader). Usado só em `arch/x86_64/boot.rs`.
