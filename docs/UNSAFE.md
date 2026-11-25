# Inventário de código `unsafe`

O compilador recusa `unsafe` fora de `arch/`, `mm/` e `sync/`
(`#![deny(unsafe_code)]`, D-008). Esta lista tem de estar sempre igual ao
código: quem acrescenta ou remove `unsafe` atualiza-a no mesmo PR.

Verificação rápida: `git grep -n "unsafe" -- kernel/src`.

| Ficheiro | O que é unsafe | Porquê é necessário | Porquê é seguro | Dono |
|---|---|---|---|---|
| `kernel/src/arch/x86_64/port.rs` | 6 blocos `asm!` (`in`/`out` de 8/16/32 bits); `unsafe fn port_in/port_out`; `unsafe fn Port::new`; 2 blocos em `read`/`write` | o Rust não tem instruções de I/O de portas | o contrato de `Port::new` garante que a porta não pode violar a segurança de memória; os blocos só reencaminham esse contrato | CLAUDE |
| `kernel/src/arch/x86_64/cpu.rs` | 4 blocos `asm!` (`cli`, `hlt`, `mov cr2`, `int3`) | instruções privilegiadas | `cli`/`hlt`/ler CR2 não tocam em memória; `int3` só depois de a IDT estar carregada, e o handler devolve todos os registos intactos | CLAUDE |
| `kernel/src/arch/x86_64/gdt.rs` | `unsafe impl Sync for CpuTables`; 2 escritas (TSS, GDT); 1 bloco `asm!` (`lgdt`, far return para recarregar CS, `mov` de segmentos, `ltr`) | o CPU lê (e escreve o bit busy da TSS) por trás do compilador; recarregar CS só é possível em assembly | escrito uma vez por quem ganha o `AtomicBool`, antes de o CPU o conhecer; os descritores novos têm o mesmo privilégio e modo que os do Limine | CLAUDE |
| `kernel/src/arch/x86_64/idt.rs` | `unsafe impl Sync for Idt`; 1 escrita; 1 bloco `asm!` (`lidt`) | carregar a IDT | escrita única antes do `lidt`; todas as entradas presentes apontam para stubs válidos com o seletor de código do kernel | CLAUDE |
| `kernel/src/arch/x86_64/trap.rs` | `global_asm!` (stubs e rotina comum); leitura do `extern static` `kitty_trap_stubs` | a entrada de uma interrupção não pode ser escrita em Rust | os stubs guardam e repõem todos os registos e mantêm a pilha alinhada a 16 (ver comentário no ficheiro); a tabela está em `.rodata` e nunca é escrita | CLAUDE |
| `kernel/src/arch/x86_64/selftest.rs` | 2 blocos: leitura de endereço não mapeado; `mov rsp` inválido + `int3` | provocar falhas de propósito | só existe com as features `selftest-*`; o handler nunca volta | CLAUDE |
| `kernel/src/arch/x86_64/serial.rs` | 1 bloco: `Port::new` para 0x3F8..0x3FD | aceder à UART COM1 | a 16550 não faz DMA; o pior caso é output corrompido | CLAUDE |
| `kernel/src/arch/x86_64/qemu.rs` | 1 bloco: `Port::new(0xF4)` | sair do QEMU nos testes | só existe com a feature `qemu-exit`; a escrita termina a VM | CLAUDE |
| `kernel/src/arch/x86_64/boot.rs` | atributos `#[unsafe(link_section)]` (5) e `#[unsafe(no_mangle)]` (1); 1 bloco `slice::from_raw_parts_mut` do framebuffer | o Limine encontra os requests pela secção; o linker encontra `kernel_entry` pelo nome; o framebuffer só nos chega como ponteiro | secções dedicadas só com estes statics; nome único; o Limine garante o mapeamento de `pitch*height` bytes, validamos ponteiro/tamanho e um `AtomicBool` garante que o `&'static mut` só é criado uma vez | CLAUDE |
| `kernel/src/sync/spinlock.rs` | `unsafe impl Sync for SpinLock<T>`; 2 blocos (`Deref`/`DerefMut` do guard) | um lock transforma acesso partilhado a `UnsafeCell` em `&mut T` | só existe um guard de cada vez (garantido pelo `AtomicBool` com Acquire/Release); `T: Send` exigido | CLAUDE |
| `kernel/src/mm/pmm.rs` | `unsafe impl Send for Pmm`; inicializar e criar o slice da base de dados de frames; `write_bytes` para limpar frames; `unsafe fn PhysFrame::from_raw`; 2 acessos voláteis no autoteste | a memória física só é acessível por ponteiros (via HHDM) | a base de dados fica numa região usable reservada só para ela e inicializada antes de criar o slice; só se limpa um frame sem dono (refcount 0) ou acabado de tirar da lista, com o lock do PMM; `from_raw` documenta que cada referência só pode ser recriada uma vez | CLAUDE |
| `kernel/src/mm/hhdm.rs` | nenhum (só aritmética de endereços) | — | — | CLAUDE |

Dependências externas com `unsafe`: crate `limine` (leitura volátil das
respostas do bootloader). Usado só em `arch/x86_64/boot.rs`.
