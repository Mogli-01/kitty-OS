# Arquitetura do kitty-OS

## Visão geral

O kitty-OS é um **kernel monolítico modular** para x86_64, escrito em Rust
(`no_std`) com o mínimo de assembly. "Monolítico" porque, por agora, drivers
e sistema de ficheiros correm em ring 0 no mesmo espaço de endereçamento
(mais simples e mais rápido: uma chamada a um driver é uma chamada de
função, não uma troca de mensagens). "Modular" porque as fronteiras entre
módulos são traits e funções explícitas (`docs/INTERFACES.md`): assim, mais
tarde, um driver pode passar para user space (direção microkernel) sem
reescrever o resto.

```
            +---------------------------------------------------+
 ring 3     |  userland/ (shell, utilitários)        [Fase 6-7] |
            +--------------------- syscall ---------------------+
 ring 0     |  syscall/   sched/   security/                    |
            |  fs/ (VFS, initrd)   drivers/ (APIC, teclado, …)  |
            |  console/   mm/ (PMM, VMM, heap)   sync/          |
            |  arch/x86_64 (boot, GDT/IDT, CPU, portas I/O)     |
            +---------------------------------------------------+
                      Limine (bootloader, BIOS e UEFI)
```

### Organização do repositório

| Caminho | Conteúdo | Dono |
|---|---|---|
| `kernel/src/arch/x86_64/` | entrada do Limine, CPU, portas I/O, série inicial | Claude |
| `kernel/src/mm/` | tipos de endereço; PMM/VMM/heap (Fase 3) | Claude |
| `kernel/src/sync/` | spinlock (e mutex mais tarde) | Claude |
| `kernel/src/console/` | consola do kernel (série + framebuffer) | Claude |
| `kernel/src/sched/`, `syscall/`, `security/` | fases 5, 6, 8 | Claude |
| `drivers/`, `fs/`, `userland/`, `tests/` | ver READMEs | Codex |
| `xtask/` | build, ISO, run, test (`cargo xtask …`) | Claude (núcleo do build); Codex (testes) |
| `.github/workflows/` | CI | Codex |
| `docs/` | coordenação | ambos (cada um nas suas secções) |

## Arranque (Fase 1)

1. O firmware (BIOS ou UEFI) carrega o **Limine**.
2. O Limine lê `boot/limine/limine.conf`, carrega o ELF do kernel e mapeia
   cada segmento `PT_LOAD` com as permissões declaradas no linker script.
3. Antes de saltar para o kernel, o Limine procura os nossos *requests*
   (estruturas com números mágicos na secção `.requests`) e escreve lá os
   ponteiros para as respostas (framebuffer, informação do bootloader, …).
4. Salta para `kernel_entry` (`arch/x86_64/boot.rs`) em long mode, com
   interrupções desligadas e uma pilha válida.
5. `kernel_entry` verifica a revisão do protocolo, converte as estruturas do
   Limine em tipos seguros (`boot_info::BootInfo`) e chama `kmain`.
6. `kmain` inicializa as consolas e escreve `Hello from kitty-OS`.

## Mapa de memória virtual (x86_64, paginação de 4 níveis, 48 bits)

Um endereço virtual de 64 bits só é válido se for **canónico**: os bits
48–63 têm de ser cópias do bit 47. Isso divide o espaço em duas metades e
deixa um "buraco" enorme no meio — qualquer acesso lá dá #GP. Usamos a
metade de baixo para o user space e a de cima para o kernel.

| Início | Fim | Tamanho | Uso | Estado |
|---|---|---|---|---|
| `0x0000_0000_0000_0000` | `0x0000_0000_0000_0FFF` | 4 KiB | página nula, **nunca mapeada** (apanha null derefs) | planeado (Fase 6) |
| `0x0000_0000_0000_1000` | `0x0000_7FFF_FFFF_FFFF` | ~128 TiB | user space (um por processo) | Fase 6 |
| `0x0000_8000_0000_0000` | `0xFFFF_7FFF_FFFF_FFFF` | — | não canónico (buraco) | hardware |
| `0xFFFF_8000_0000_0000` | … | até 64 TiB | **HHDM**: toda a RAM física mapeada linearmente (offset dado pelo Limine) | Limine (hoje) |
| `0xFFFF_C000_0000_0000` | … | 1 TiB | heap do kernel | Fase 3 |
| `0xFFFF_E000_0000_0000` | … | 1 TiB | pilhas do kernel (cada uma com página de guarda) | Fase 3/5 |
| `0xFFFF_FFFF_8000_0000` | `0xFFFF_FFFF_FFFF_FFFF` | 2 GiB | **imagem do kernel** | hoje |

Porque é que o kernel está nos últimos 2 GiB: com `-C code-model=kernel` o
compilador pode endereçar qualquer símbolo do kernel com um imediato de 32
bits com extensão de sinal (`0xFFFFFFFF8xxxxxxx` cabe em 32 bits
sign-extended). O código fica mais pequeno e mais rápido do que com
endereços de 64 bits.

### Layout da imagem do kernel (linker script)

| Segmento | Secções | Permissões | Porquê |
|---|---|---|---|
| `requests` | `.requests*` | R | o Limine escreve-as *antes* de nos passar o controlo; o kernel só lê, por isso nenhum bug pode redirecionar as respostas |
| `text` | `.text` | R X | código: executável, nunca escrevível |
| `rodata` | `.rodata` | R | constantes, strings, fonte |
| `data` | `.data`, `.bss` | R W (NX) | dados: escrevíveis, nunca executáveis |

Cada segmento começa numa fronteira de 4 KiB para que nenhuma página tenha
de misturar permissões. Isto dá **W^X desde a primeira instrução** (verificado
com `llvm-readobj --program-headers`).

## Convenções

- **Tipos de endereço**: `PhysAddr` e `VirtAddr` (`mm/addr.rs`). Nunca `u64`
  solto para endereços. `VirtAddr::new` rejeita endereços não canónicos.
- **Memória de user**: só através de `UserPtr<T>` + `copy_from_user` /
  `copy_to_user` (Fase 6).
- **Erros**: `Result`/`Option`; sem `unwrap`/`expect` (proibido por lint).
- **Logs**: `kprintln!`. Nunca formatar endereços do kernel (`{:p}`) em
  output que o user space possa ler.
- **Sem SSE/AVX no kernel**: o target `x86_64-unknown-none` usa soft-float.
  Vantagem de segurança e desempenho: o kernel nunca suja os registos
  vetoriais, por isso uma syscall não precisa de os guardar.
- **Sem red zone**: o target desativa-a. Obrigatório num kernel, porque uma
  interrupção escreve na pilha atual e destruiria a red zone.
- **Línguas**: código e commits em inglês; docs de coordenação em português.
