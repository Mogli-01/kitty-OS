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

## Exceções e interrupções (Fase 2)

```
CPU deteta a exceção N
  -> lê a gate N da IDT (DPL 0, interrupt gate, IST se N for 2, 8 ou 18)
  -> se IST: muda para a pilha IST indicada na TSS
  -> empilha SS, RSP, RFLAGS, CS, RIP (+ código de erro em alguns vetores)
  -> kitty_trap_N: empilha 0 (se não houver código de erro) e N
  -> kitty_trap_common: guarda 15 registos, chama trap_dispatch(&mut TrapFrame)
  -> exceptions::handle: #BP/NMI -> regista e volta; resto -> relatório + panic
  -> (se voltar) repõe os registos, iretq
```

## Mapa de memória virtual (x86_64, paginação de 4 níveis, 48 bits)

Um endereço virtual de 64 bits só é válido se for **canónico**: os bits
48–63 têm de ser cópias do bit 47. Isso divide o espaço em duas metades e
deixa um "buraco" enorme no meio — qualquer acesso lá dá #GP. Usamos a
metade de baixo para o user space e a de cima para o kernel.

| Início | Fim | Tamanho | Uso | Estado |
|---|---|---|---|---|
| `0x0000_0000_0000_0000` | `0x0000_0000_0000_0FFF` | 4 KiB | página nula, **nunca mapeada** (apanha null derefs) | planeado (Fase 6) |
| `0x0000_0000_0000_1000` | `0x0000_7FFF_FFFF_FFFF` | ~128 TiB | user space (um por processo) | Fase 6 |
| (dentro do user space) | | | mapeamentos de buffers partilhados (janelas, framebuffer do compositor), em endereços aleatórios (ASLR) | Fase 12 |
| `0x0000_8000_0000_0000` | `0xFFFF_7FFF_FFFF_FFFF` | — | não canónico (buraco) | hardware |
| `0xFFFF_8000_0000_0000` | … | até 64 TiB | **HHDM**: toda a RAM física mapeada linearmente (offset dado pelo Limine) | Limine (hoje) |
| `0xFFFF_C000_0000_0000` | … | 1 TiB | heap do kernel | Fase 3 |
| `0xFFFF_D000_0000_0000` | … | 1 TiB | janela MMIO (`map_mmio`), uncached/WC, NX | hoje |
| `0xFFFF_E000_0000_0000` | … | 1 TiB | pilhas das threads do kernel (cada uma com página de guarda) | Fase 5 |
| `0xFFFF_FFFF_8000_0000` | `0xFFFF_FFFF_FFFF_FFFF` | 2 GiB | **imagem do kernel** (inclui a pilha principal e as IST em `.bss`, com páginas de guarda por mapear) | hoje |

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

Desde a Fase 3 estas permissões são aplicadas pelas **nossas** tabelas de
páginas (já não pelas do Limine), página a página, com três exceções em
`.data`/`.bss`: as páginas de guarda das pilhas ficam por mapear e as
páginas da GDT e da IDT ficam só de leitura.

Cada segmento começa numa fronteira de 4 KiB para que nenhuma página tenha
de misturar permissões. Isto dá **W^X desde a primeira instrução** (verificado
com `llvm-readobj --program-headers`).

## Interface gráfica (Fases 10–14) — já considerada no design

```
  app A          app B                 compositor (user space)
 [buffer A]     [buffer B]   --READ--> compõe dirty rects --> framebuffer (WC)
     ^ WRITE        ^ WRITE                ^ DISPLAY             |
     |              |                      | INPUT               v
     +---- canais (mensagens + handles) ---+                    ecrã
                                           ^
 kernel:  objetos (buffer, canal, display, input) + handles com direitos
          drivers de teclado/rato --> InputEvent --> só para quem tem INPUT
```

O que isto obriga o núcleo a ter **desde as primeiras fases** (tarefas
GUI-PREP no roadmap):

- **PMM com contagem de referências** (Fase 3): o mesmo frame físico pode
  estar mapeado no espaço da aplicação e do compositor. Só é libertado — e
  limpo a zero — quando a última referência desaparece.
- **Tipos de cache no VMM** (Fase 3, D-020): o PAT é programado no arranque
  com WB/WC/UC; os mapeamentos dizem qual querem. O framebuffer é WC.
- **Filas de espera no scheduler** (Fase 5): uma thread bloqueia à espera
  de uma mensagem ou de um evento sem gastar CPU, e é acordada pelo emissor.
- **Objetos do kernel + handles com direitos** (Fase 6, D-018): buffers,
  canais, ecrã e input são objetos; processos só os veem através de handles.
- **Compositor em user space** (D-017): o kernel não sabe o que é uma
  janela. Só fornece memória partilhada, canais e as capacidades `DISPLAY`
  e `INPUT`.

Caminho de um frame (zero cópias de píxeis): a aplicação desenha no seu
buffer → envia "damage(x, y, w, h)" pelo canal → o compositor copia só esse
retângulo para o seu back buffer → copia as zonas alteradas para o
framebuffer WC.

Caminho de uma tecla: IRQ → driver de teclado → `InputEvent` → fila do
objeto `INPUT` → compositor acorda → decide a janela com foco → mensagem
pelo canal dessa aplicação.

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
