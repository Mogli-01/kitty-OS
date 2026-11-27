# Decisões técnicas

Formato: cada decisão tem um número fixo (`D-NNN`), data, autor, a decisão e
o porquê. Decisões não se apagam: se mudarem, acrescenta-se uma nova que
substitui a antiga ("Substitui D-xxx").

---

### D-001 — Stack base (2026-11-25, CLAUDE)
x86_64; Rust nightly + o mínimo de assembly (`asm!`/`global_asm!`); target
`x86_64-unknown-none`, `#![no_std]`, `#![no_main]`, `panic = "abort"`;
bootloader Limine; build com workspace Cargo + `cargo xtask`; QEMU para
testes; kernel monolítico modular com fronteiras claras (direção
microkernel). Definido pelos humanos no arranque do projeto.

### D-002 — Nightly fixado: `nightly-2026-11-20` (2026-11-25, CLAUDE)
**Porquê:** precisamos de funcionalidades instáveis — `per-package-target`
(D-003) no Cargo e `ptr_metadata`, que o crate `limine` usa. Fixar a data
garante builds reprodutíveis e que a CI usa exatamente o mesmo compilador.
Atualizar o nightly é um PR próprio.

### D-003 — `per-package-target` + `forced-target` no kernel (2026-11-25, CLAUDE)
`kernel/Cargo.toml` tem `forced-target = "x86_64-unknown-none"`; o `xtask`
compila para o host. **Porquê:** assim `cargo build`, `cargo clippy` e
`cargo fmt` na raiz do workspace fazem a coisa certa sem flags extra, e a CI
corre exatamente o comando pedido nas regras (`cargo clippy -- -D warnings`).
Alternativa rejeitada: `--target` em todos os comandos (fácil de esquecer e
clippy passaria a verificar o kernel para o host).

### D-004 — Limine v11, binários fixados por commit (2026-11-25, CLAUDE)
O `xtask` descarrega o branch oficial `v11.x-binary` **no commit
`5be26a73d7b7b4d4477d18be94e1d16e615adf56`** (Limine 11.4.1) para
`build/limine` (não é commitado). **Porquê:** fixar o commit impede que uma
alteração (ou comprometimento) do branch mude o bootloader sem revisão
(segurança da cadeia de fornecimento). Atualizar = PR que muda a constante
`LIMINE_COMMIT` em `xtask/src/main.rs`.

### D-005 — Crate `limine` 0.6.5; `bitflags` ainda não usado (2026-11-25, CLAUDE)
A série 0.6 tem a API nova (mantida por Julian Scheffers) com suporte até à
base revision 6. Só o módulo `arch/x86_64/boot.rs` usa o crate. `bitflags`
continua permitido mas só entra quando houver flags (Fase 2/3).

### D-006 — Base revision do protocolo Limine fixada em 3 (2026-11-25, CLAUDE)
Pedimos explicitamente a revisão 3 em vez de "a mais alta que o crate
conhece". **Porquê:** a revisão define a semântica do arranque (o que está
mapeado, em que estado fica a máquina). Fixá-la evita que um update do
crate mude isso em silêncio. Se o Limine não suportar a revisão, o kernel
escreve um aviso na série e pára.

### D-007 — Imagem do kernel estática no topo de 2 GiB, W^X por segmento (2026-11-25, CLAUDE)
`-C relocation-model=static -C code-model=kernel`, kernel ligado em
`0xFFFFFFFF80000000`, um `PT_LOAD` por tipo de permissão (R / RX / R / RW),
alinhados a 4 KiB, e `.requests` em segmento só de leitura.
**Porquê:** W^X logo desde o arranque, sem esperar pela nossa paginação
(Fase 3). **Consequência:** sem KASLR por agora; na Fase 8 o kernel passa a
PIE (relocatable) e o Limine aplica o deslocamento aleatório.

### D-008 — Confinamento do `unsafe` imposto pelo compilador (2026-11-25, CLAUDE)
`#![deny(unsafe_code)]` no crate e `#[allow(unsafe_code)]` apenas em
`mod arch`, `mod mm` e `mod sync`. **Porquê:** a regra "unsafe só em
arch/mm/hw" deixa de depender de revisão humana — o compilador recusa
`unsafe` noutro sítio. **Ambiguidade resolvida:** `sync/` entra na lista
porque um lock é, por definição, o código que transforma acesso partilhado a
`UnsafeCell` num `&mut T` seguro; não é possível escrevê-lo sem `unsafe`.
Também adicionámos `deny(clippy::multiple_unsafe_ops_per_block)`: cada
operação unsafe tem o seu bloco e a sua justificação.

### D-009 — Consola inicial dentro do kernel (2026-11-25, CLAUDE)
Uma UART 16550 mínima em modo polling (`arch/x86_64/serial.rs`) e uma
consola de texto no framebuffer (`console/`) fazem parte do núcleo, não de
`drivers/`. **Porquê:** são precisas antes de existir qualquer
infraestrutura de drivers (e para o panic handler). O driver de UART
completo (interrupções, input) continua a ser do Codex em `drivers/`.

### D-010 — `Port::new` é unsafe, `read`/`write` são seguros; sem `nomem` (2026-11-25, CLAUDE)
**Porquê:** o perigo de uma porta I/O não está em cada acesso, mas em *qual*
porta é (algumas programam DMA para memória arbitrária). Quem cria o `Port`
garante que é inofensivo; o resto do código usa-o sem `unsafe`. Os blocos
`asm!` não usam `options(nomem)` para servirem de barreira de memória do
compilador (uma escrita numa porta pode iniciar DMA que lê memória escrita
antes).

### D-011 — Arranque local por UEFI a partir de uma pasta; ISO para BIOS (2026-11-25, CLAUDE)
`cargo xtask run`/`test` usam UEFI (OVMF) com uma pasta `build/esp` exposta
pelo QEMU como disco FAT (vvfat). `--bios` gera uma ISO híbrida
BIOS+UEFI com `xorriso`. **Porquê:** no Windows não há `xorriso`; o caminho
UEFI não precisa de nenhuma ferramenta de imagens. A CI testa os dois.

### D-012 — Fonte 8x8 de domínio público (2026-11-25, CLAUDE)
Os glifos seguem a tabela `font8x8_basic` (domínio público), desenhados a
2x (células 16x16). Sem licenças a gerir.

### D-013 — QEMU com `-machine q35 -cpu max` (2026-11-25, CLAUDE)
`q35` é um chipset moderno (PCIe, AHCI). `-cpu max` expõe todas as
funcionalidades que o QEMU consegue emular, incluindo NX, SMEP, SMAP e UMIP,
que vamos ativar nas Fases 3 e 6.

### D-014 — Saída do QEMU nos testes só com a feature `qemu-exit` (2026-11-25, CLAUDE)
O kernel só escreve na porta `0xF4` (isa-debug-exit) se for compilado com
`--features qemu-exit` (o `cargo xtask test` faz isso). Códigos: `0x10`
sucesso → QEMU sai com 33; `0x11` falha → 35. **Porquê:** kernels normais
nunca tocam numa porta de teste em hardware real; e evitamos o código 1, que
o QEMU também usa para os seus próprios erros.

### D-015 — CI inicial criada pelo Claude, passa para o Codex (2026-11-25, CLAUDE)
A CI é área do Codex, mas o PR de arranque precisa dela para provar que o
kernel arranca. Criei a versão inicial (`.github/workflows/ci.yml`:
fmt, clippy, teste de arranque UEFI/BIOS/release). A partir daqui é do
Codex. As actions estão fixadas por SHA de commit.

### D-100 — Validar os relatórios de exceção na CI (2026-11-25, CODEX)
A CI corre o arranque normal e os autotestes de #PF e #DF em QEMU com BIOS
e UEFI, em debug e release. Confirma na porta série a entrada no handler de
#BP e o regresso ao arranque. Para #PF e #DF, confirma o panic e os campos
esperados no relatório. Cada cenário tem um log próprio.

Um código de saída de panic também pode ser produzido por uma falha
diferente da esperada. Os marcadores do relatório distinguem os cenários.
A matriz cobre os dois firmwares e verifica o efeito das otimizações do
compilador nos autotestes.

### D-016 — `xtask` só com a biblioteca padrão (2026-11-25, CLAUDE)
A política de dependências aplica-se também ao `xtask`: sem `clap`,
`anyhow`, etc. O parsing de argumentos é manual e simples.

### D-017 — O compositor corre em user space (2026-11-25, CLAUDE)
O compositor / gestor de janelas (Fase 13) é um **processo normal em ring
3**, não código do kernel. **Porquê (segurança):**
- O compositor processa input complexo e não confiável: conteúdo de buffers
  de todas as aplicações, mensagens do protocolo de janelas, eventos de rato
  e teclado. Um bug aí, no kernel, daria controlo total da máquina; em user
  space, fica contido num processo com só as capacidades `DISPLAY` e
  `INPUT`.
- Mantém o kernel pequeno (menos código privilegiado = menos superfície de
  ataque) e segue a direção microkernel do projeto (D-001).
- Se o compositor falhar, o kernel continua de pé e pode reiniciá-lo.

**Custo:** cada frame implica IPC e trocas de contexto. Mitigação: zero
cópias de píxeis (memória partilhada, Fase 12), mensagens pequenas, só se
redesenham as zonas alteradas. Medimos antes de otimizar.

### D-018 — Objetos do kernel com handles e direitos, desde a Fase 6 (2026-11-25, CLAUDE)
Buffers partilhados, canais, o ecrã e o input são **objetos do kernel**. Um
processo só lhes chega através de um *handle* (índice na sua tabela de
handles) com **direitos** explícitos (ex.: `READ`, `WRITE`, `MAP`,
`TRANSFER`). Não há nomes globais nem IDs adivinháveis.
**Porquê:** é o modelo de capacidades da Fase 8 introduzido cedo. Partilhar
um buffer passa a ser "enviar um handle com menos direitos por um canal" —
o kernel verifica tudo e é impossível um processo aceder a um buffer que
ninguém lhe deu. A tabela de handles nasce com os processos (Fase 6) para
não ter de ser retrofitted mais tarde.

### D-019 — Input só é entregue ao processo com a capacidade `INPUT` (2026-11-25, CLAUDE)
Os drivers (Codex) produzem `InputEvent`s unificados; o kernel entrega-os
**só** ao processo que tem a capacidade `INPUT` (o compositor), que os
encaminha para a janela com foco. As aplicações nunca leem input global.
**Porquê:** impede keyloggers e que uma aplicação veja cliques/teclas
destinados a outra.

### D-102 — Descodificar o teclado PS/2 em Set 2 (2026-11-25, CODEX)
O crate interno `kitty-ps2` processa um byte de cada vez e produz eventos
de teclas físicas. Compila em `no_std`, sem alocação, `unsafe` ou crates
externos. O transporte do i8042 fornece o fluxo Set 2 sem tradução e o
layout e o timestamp são aplicados pela camada de input.

A separação permite exercitar todas as transições com bytes arbitrários
no host e em Miri. A tabela de teclas preserva posições que têm o mesmo
código base, como a seta para cima e o 8 do teclado numérico, bem como
a tecla adicional ISO. [Referência de códigos](https://github.com/qemu/keycodemapdb/blob/master/data/keymaps.csv)

### D-020 — Tipos de cache (PAT) geridos pelo VMM (2026-11-25, CLAUDE)
O roadmap pede o framebuffer com write-combining. Programar o MSR `IA32_PAT`
e escolher os bits PAT/PCD/PWT das entradas de página é paginação, logo
núcleo (Claude, Fase 3). O VMM expõe `CacheType { WriteBack,
WriteCombining, Uncached }` nas APIs de mapeamento; o Codex usa-o na Fase 10.
**Porquê WC:** escritas no framebuffer são agrupadas em rajadas em vez de
irem uma a uma para o barramento — tipicamente várias vezes mais rápido a
desenhar; ler do framebuffer continua lento, daí o double buffering em RAM.

### D-021 — IPC: canais com mensagens pequenas + dados grandes por memória partilhada (2026-11-25, CLAUDE)
Canais bidirecionais com fila limitada (sem alocação sem limite pedida pelo
user space → sem DoS de memória do kernel). Cada mensagem tem bytes
copiados (tamanho máximo fixo, pequeno) e pode transportar handles. Píxeis
e outros dados grandes vão sempre em buffers partilhados: zero cópias.
**Porquê:** copiar mensagens pequenas é mais rápido e mais seguro do que
partilhar memória para tudo (o recetor recebe uma cópia que o emissor já não
pode alterar — sem double fetch / TOCTOU).

### D-022 — Stubs de interrupção próprios em assembly, não `extern "x86-interrupt"` (2026-11-25, CLAUDE)
Cada vetor tem um stub de 3–4 instruções (`global_asm!` em `trap.rs`) que
põe a pilha sempre no mesmo formato e salta para uma rotina comum, que
guarda os 15 registos gerais e chama `trap_dispatch(&mut TrapFrame)`.
**Porquê:** temos todos os registos numa só struct (relatórios de crash
completos agora; na Fase 5 o scheduler troca de thread trocando o frame),
funciona sem features instáveis e são ~40 linhas de assembly auditáveis. O
ABI `x86-interrupt` é instável e só dá RIP/CS/RFLAGS/RSP/SS.

### D-023 — Relatórios com endereços só na porta série (2026-11-25, CLAUDE)
`console::debug_print` escreve **só** na série. O relatório completo de
uma exceção (registos, CR2, RIP) vai para lá; o ecrã mostra só o nome da
exceção. **Porquê:** o ecrã vai ser visível ao user space e os endereços
do kernel anulariam o KASLR (Fase 8). A série é o canal de debug do
programador e nunca será exposta a processos de user.

### D-024 — Layout da GDT preparado para `syscall`/`sysret` (2026-11-25, CLAUDE)
`null, kernel code (0x08), kernel data (0x10), user data (0x18), user code
(0x20), TSS (0x28)`. **Porquê:** o `sysret` calcula os seletores de user a
partir do MSR STAR (SS = base+8, CS = base+16), o que obriga "user data"
a vir imediatamente antes de "user code". Já fica certo para a Fase 6.

### D-025 — Pilhas IST estáticas de 16 KiB, sem página de guarda até à Fase 3 (2026-11-25, CLAUDE)
Três pilhas IST (double fault, NMI, machine check) em `.bss`. Ainda não há
página de guarda porque a paginação é do Limine; na Fase 3 passam a ter
uma página não mapeada por baixo. 16 KiB chegam com folga para o relatório
(que não aloca e usa pouca pilha).

### D-026 — IDT com DPL 0 em todos os vetores e só interrupt gates (2026-11-25, CLAUDE)
**Porquê:** com DPL 0, o user space (Fase 6) não consegue disparar nenhum
vetor com `int n` (dá #GP). Interrupt gates (e não trap gates) desligam
o IF na entrada, por isso um handler nunca é interrompido antes de guardar
o estado. Vetores sem handler ficam "não presentes" (#NP em vez de saltar
para lixo).

### D-027 — Autotestes de falhas como features do kernel (2026-11-25, CLAUDE)
`selftest-page-fault` e `selftest-double-fault` provocam a falha logo após
o arranque; `cargo xtask test --features <f> --expect-panic` verifica que
o QEMU sai com 35 e que a série tem `KERNEL PANIC`. As features nunca
estão ativas em builds normais. Integrá-los na CI e verificar o conteúdo
do relatório é do Codex.

### D-028 — PMM: base de dados de frames com lista livre fora de banda (2026-11-25, CLAUDE)
Cada frame físico de 4 KiB tem uma entrada `FrameInfo { refcount, next_free,
dirty }` (12 bytes, ~0,3 % da RAM) guardada num bloco de RAM reservado no
arranque. Alocar e libertar são O(1) (pilha de frames livres).
**Porquê fora de banda:** muitos kernels guardam o ponteiro "próximo livre"
*dentro* do frame livre. Um bug de use-after-free que escreva nesse frame
corrompe o alocador e pode fazê-lo devolver um endereço escolhido pelo
atacante. Aqui o link vive na base de dados, onde nenhum dono de frame chega.
**Contagem de referências** (GUI-PREP): o mesmo frame pode estar mapeado em
vários espaços de endereçamento; só volta à lista livre quando a última
referência desaparece. `PhysFrame` é um tipo com dono (não `Copy`): o
compilador impede libertar duas vezes a mesma referência, e o `Drop` liberta.
**Zeros:** um frame é limpo quando a última referência sai, e os frames que
vêm do bootloader são limpos na primeira alocação (arranque rápido). Todo o
frame entregue está a zeros. Double free e overflow da contagem são bugs
fatais (panic) em vez de corrupção silenciosa.
**Limitações:** a memória "bootloader reclaimable" ainda não é reutilizada
(tem a pilha de arranque e o memory map); índices de 32 bits limitam a 16 TiB.

### D-029 — Tabelas de páginas próprias, construídas a partir das do Limine (2026-11-25, CLAUDE)
O kernel cria o seu PML4 e muda o CR3 no arranque. Os endereços físicos de
cada página do kernel são lidos **das tabelas do Limine** (e não calculados
com uma fórmula), por isso não dependemos de como o Limine dispôs a imagem
na RAM. Antes de carregar o CR3 verificamos que o código e a pilha em uso
estão mapeados (senão seria um triple fault sem diagnóstico).
- As 256 entradas da metade do kernel no PML4 são **pré-alocadas** (1 MiB):
  na Fase 6 cada processo copia estas entradas e vê automaticamente tudo o
  que o kernel mapear depois.
- O HHDM usa **páginas de 2 MiB** sempre que possível (~512× menos entradas
  e menos falhas de TLB) e todas as entradas do kernel são **globais**
  (CR4.PGE): sobrevivem às trocas de CR3.
- Permissões do HHDM por tipo de região: RAM e ACPI RW+NX; a imagem do
  kernel **só de leitura** (senão o `.text` podia ser reescrito pelo alias
  do HHDM, contornando o W^X); o framebuffer em write-combining; regiões
  "reserved"/"bad" não são mapeadas.

### D-030 — Páginas de guarda: memória em `.bss` que fica por mapear (2026-11-25, CLAUDE)
A pilha principal (64 KiB) e as 3 pilhas IST (16 KiB) vivem em `.bss`,
cada uma com uma página de 4 KiB por baixo que **não** é mapeada nas nossas
tabelas. Um stack overflow bate na página de guarda → #PF → (a entrega do
#PF também falha) → double fault na pilha IST. Assim não é preciso reescrever
a TSS depois do arranque (evita escritas não atómicas a entradas IST que um
NMI podia ler a meio). Depois de mudar o CR3, o kernel salta para a pilha
principal com `run_on_main_stack` e abandona a pilha do Limine.

### D-031 — GDT e IDT em páginas próprias, só de leitura (2026-11-25, CLAUDE)
A GDT e a IDT ocupam cada uma uma página alinhada e são mapeadas só de
leitura. Um atacante com uma primitiva de escrita no kernel já não consegue
desviar exceções reescrevendo a IDT. **Detalhe:** o CPU escreve o bit
"accessed" de um descritor na primeira vez que o segmento é carregado; esse
bit é pré-ativado em todos os descritores para o CPU nunca precisar de
escrever na GDT. A TSS continua RW (o RSP0 muda a cada troca de thread na
Fase 6).

### D-032 — W^X garantido na construção e auditado no arranque (2026-11-25, CLAUDE)
`MapFlags` com `writable && executable` é recusado por `map_4k`/`map_2m`
(`MapError::WriteAndExecute`), mapear por cima de um mapeamento existente é
um erro, e o bit "user" é recusado na metade do kernel. Depois de construir
as tabelas, uma auditoria percorre toda a metade do kernel e **o arranque
pára** se encontrar alguma página W+X ou acessível ao user.

### D-033 — `map_mmio` é unsafe e recusa RAM (2026-11-25, CLAUDE)
Mesmo raciocínio do `Port::new` (D-010): o perigo está em *que* dispositivo
se mapeia (DMA). `map_mmio` mapeia numa janela própria (1 TiB a partir de
`0xFFFF_D000_0000_0000`), nunca executável, e **recusa intervalos que
coincidam com RAM ou firmware** do memory map (um alias MMIO de RAM
contornaria todas as outras proteções). O `MmioRegion` devolvido só faz
acessos voláteis, com limites e alinhamento verificados.

### D-034 — PAT programado no arranque (2026-11-25, CLAUDE)
`IA32_PAT = 0x0007_0106_0007_0106`: índice 0 = WB, 1 = WC, 2 = UC-, 3 = UC
(4–7 repetem). `CacheType` escolhe os bits PWT/PCD das entradas. É feito com
`wbinvd` antes, imediatamente antes de carregar as tabelas novas.

### D-035 — Heap do kernel: classes de tamanho + páginas com guarda (2026-11-25, CLAUDE)
**Pequenas** (≤ 2048 bytes): 8 listas livres (16…2048 bytes); quando uma
classe fica vazia recebe uma página nova cortada em pedaços iguais. O(1),
pedaços naturalmente alinhados ao tamanho. **Grandes**: páginas mapeadas a
pedido na região do heap (`0xFFFF_C000_0000_0000`, 1 TiB), seguidas de uma
**página de guarda por mapear**; ao libertar, as páginas são desmapeadas e
devolvidas ao PMM (que as limpa).
**Endurecimento:** pedaços pequenos são limpos a zero ao libertar (toda a
alocação já vem a zeros); o link da lista livre guardado no pedaço é
**codificado** (`next ^ (pedaço >> 12) ^ segredo`, "safe linking") e
validado ao tirar da lista — um use-after-free que o reescreva é detetado
em vez de devolver um endereço escolhido pelo atacante; libertar o pedaço
que já está à cabeça da lista (double free comum) é detetado.
**Limitações conhecidas:** o segredo vem do TSC (fraco) até haver RNG na
Fase 8; o espaço virtual das alocações grandes não é reutilizado (1 TiB
chega por agora); o heap usa `SpinLock` sem desligar interrupções, por isso
**não se pode alocar em handlers de interrupção** até à Fase 4.
Ordem de locks: heap → espaço de endereçamento do kernel → PMM.

### D-036 — `xtask test --expect <texto>` (2026-11-25, CLAUDE)
Em release, o LLVM pode eliminar pares alloc/dealloc e outros testes; um
autoteste de falha que entrasse em panic por outra razão passava por
engano. Com `--expect` o teste exige o texto do relatório (ex.:
`"protection violation on write"`). Os autotestes do núcleo devem ser
sempre corridos com `--expect`.

### D-101 — Testes QEMU das proteções de memória (2026-11-25, CODEX)
`cargo xtask test-memory` reutiliza o teste de arranque e as expectativas
da D-036. Ativa uma única feature de falha de cada vez e guarda a saída
série de cada execução. Os testes exigem que o PMM, a auditoria W^X, a
pilha guardada, o heap e o MMIO passem antes de confirmar o relatório de
falha. A CI corre os testes em BIOS e UEFI, em debug e release.

Isolar os autotestes e verificar a ordem dos relatórios distingue o panic
esperado de uma falha prematura. Os testes de validação dos logs correm
no host e em Miri.

### D-037 — `IrqSpinLock` em todos os locks globais; `cli`/`sti`/`hlt` sem `nomem` (2026-11-25, CLAUDE)
Um handler que tente apanhar um lock que o código interrompido já tem fica
a girar para sempre (no mesmo CPU o dono nunca volta a correr). O
`IrqSpinLock` desliga as interrupções **antes** de apanhar o lock e repõe o
estado anterior **depois** de o libertar (por esta ordem; ao contrário
haveria uma janela para o deadlock). Aninhar funciona: só o guard exterior
volta a ligar as interrupções. Consola, PMM, espaço do kernel, memory map,
heap e tabela de IRQs passaram todos a `IrqSpinLock`.
**Mesma lição da #7:** os `asm!` de `cli`, `sti` e `hlt` tinham `nomem`, o
que deixava o compilador mover acessos à memória para dentro ou para fora
da secção crítica. Agora são barreiras de memória do compilador. O idle usa
`sti; hlt` numa só instrução `asm!`: o `sti` só tem efeito depois da
instrução seguinte, por isso nenhuma interrupção se "perde" entre os dois.

### D-038 — Drivers sem `unsafe`: `kitty-hal` + portas reclamáveis (2026-11-25, CLAUDE)
Os crates de `drivers/` (Codex) são `#![forbid(unsafe_code)]` e não
dependem do kernel. Falam com o hardware só através dos traits do crate
`kitty-hal` (`PortBlock` para portas I/O, `RegisterBlock` para MMIO). O
kernel implementa-os sobre as suas camadas `unsafe` auditadas e entrega
**objetos-capacidade** só para os registos permitidos:
`arch::legacy::claim(LegacyDevice::I8042)` devolve um `LegacyPorts` que só
deixa tocar em 0x60 e 0x64 (a 0x61, controlo do altifalante/NMI, fica de
fora). Cada dispositivo só pode ser reclamado uma vez. A cola entre um driver
e o kernel vai em `kernel/src/devices/<dispositivo>.rs`, em Rust seguro (o
lint do crate proíbe `unsafe` aí). **Porquê:** todo o `unsafe` de hardware
fica num sítio pequeno e revisto, e um bug num driver não consegue tocar em
portas ou memória que não lhe foram dadas — o primeiro passo para mais tarde
mover drivers para user space (D-001).
**Valores também filtrados** (encontrado pelo `security-reviewer`): mesmo sem
DMA, alguns registos afetam a máquina inteira. Na porta de comando do i8042,
`0xFE` reinicia o CPU e `0xD1` escreve o output port (reset e **A20 gate**:
desligar o A20 sobrepõe endereços físicos, ou seja, corrupção de memória
provocada por código "seguro"). Por isso só passam os comandos de uma lista
(`I8042_ALLOWED_COMMANDS`), e no CMOS o bit "desligar NMIs" é sempre limpo.

### D-039 — PIC 8259 como controlador de arranque; EOI pelo núcleo (2026-11-25, CLAUDE)
O núcleo traz um driver mínimo do 8259 (só portas I/O): remapeia as linhas
para 0x20–0x2F (de origem iam para 0x08–0x0F, em cima das exceções — 0x08 é
o double fault!) e deixa-as todas mascaradas até alguém registar. Assim as
IRQs funcionam já, antes de existir driver de APIC. O driver de I/O APIC do
Codex implementa o trait `InterruptController` e toma o lugar do PIC com
`irq::set_controller`, que mascara o PIC por completo.
**EOI:** é o núcleo que o envia, depois de o handler voltar, para todos os
vetores 0x20–0xEF (exceto IRQs espúrios), **haja ou não handler**: decidir
por "está registado?" deixaria o bit "em serviço" preso se a linha fosse
desregistada com a interrupção a caminho (SMP), bloqueando essa linha e as de
menor prioridade. Um driver nunca envia EOI.
**Mapa de vetores:** 0x20–0x2F PIC, 0x30–0xEF dinâmicos (`register_isa`),
0xF0–0xFE internos do kernel, 0xFF espúrio do APIC.

### D-040 — `devices::init()`: o ponto de ligação dos drivers (2026-11-25, CLAUDE)
Corre uma vez no CPU de arranque, depois do heap e do núcleo de interrupções
e **antes** do `sti`. Um driver pode alocar, reclamar o hardware e registar
o seu IRQ sem correr o risco de receber a primeira interrupção a meio da
inicialização. O kernel passou a terminar o arranque em idle com as
interrupções ligadas (em vez de `cli; hlt`).

### D-041 — Descoberta ACPI no núcleo: validar e copiar a MADT (2026-11-26, CLAUDE)
O kernel segue RSDP → XSDT/RSDT → MADT (o RSDP do Limine rev. 3 é um
endereço **físico**) e valida cada passo: assinaturas, checksums (incluindo
o estendido do RSDP v2), comprimentos (cabeçalho ≥ 36, tabela ≤ 64 KiB).
A memória de firmware é lida com `vmm::copy_from_firmware`: cada tabela
é **copiada** (leitura volátil, byte a byte, por um mapeamento temporário
só de leitura e NX que é removido a seguir) e **validada sobre a cópia**.
Só aceita memória de firmware por *allow-list*: regiões ACPI/reservadas do
mapa de memória ou a área da BIOS (0x80000–0xFFFFF); recusa sempre RAM do
kernel (também dentro da área da BIOS, onde abaixo de 640 KiB costuma haver
RAM normal), a gama APIC/HPET 0xFEC00000–0xFEFFFFFF e a página do LAPIC (o
UEFI reporta MMIO como "reserved") e **buracos do mapa** (normalmente MMIO:
ler aí pode ter efeitos laterais, e um alias WB de uma página UC é
comportamento indefinido). Máximo 64 KiB por
tabela e 256 entradas da XSDT/RSDT. Não fica nenhuma referência longa a
memória de firmware (que tem alias RW no HHDM e pode ser alterada por
SMM/AML). Sem MADT válida, o kernel fica no PIC 8259.
*Revisto depois do `security-reviewer`:* a 1.ª versão devolvia
`&'static [u8]` sobre a memória de firmware (promessa de imutabilidade
falsa), usava uma deny-list (aceitava MMIO) e validava o checksum na
memória de firmware antes de copiar (dupla leitura).

### D-042 — Capacidades APIC com filtro de valores (2026-11-26, CLAUDE)
`arch::apic::local_apic()` e `arch::apic::io_apic(phys)` devolvem objetos
que implementam `kitty_hal::RegisterBlock`, mapeados UC e NX, com dono único.
- A base do local APIC vem **só** do MSR IA32_APIC_BASE (nunca do driver);
  em modo x2APIC (registos por MSR) devolve `X2Apic` e o kernel fica no PIC.
- Um I/O APIC só é concedido num endereço que a **cópia da MADT validada
  pelo kernel** declara.
- Acessos de 32 bits, alinhados, dentro da janela (LAPIC 0x400, I/O APIC
  0x20); 64 bits recusados.
- **Valores filtrados por allow-list** (mesma lição do A20, D-038): o
  **ICR** do local APIC não é escrevível (enviaria INIT/SIPI a outros CPUs,
  que passariam a executar código num endereço físico baixo: tomada total
  da máquina); modos de entrega permitidos: LVT timer/erro só Fixed,
  LINT0/1 Fixed/NMI/ExtINT, redireções Fixed/Lowest-priority/NMI/ExtINT
  (SMI, INIT e reservados recusados); entradas Fixed/Lowest-priority não
  mascaradas só com **vetores 0x20–0xEF** (0x10–0x1F cairiam em stubs de
  exceção que esperam um código de erro e desalinhariam a pilha); SVR só
  com o vetor espúrio 0xFF.
- **I/O APIC:** seletor e janela sob um `IrqSpinLock` interno da
  capacidade (sem TOCTOU entre verificar e escrever, nem de um handler nem
  de outro CPU) e o IOREGSEL é **reposto em hardware a cada novo dono**
  (antes, um seletor deixado por outro dono permitia contornar o filtro).
  Endereços de I/O APIC cuja janela toque na página do local APIC ou noutro
  I/O APIC são descartados (uma MADT hostil podia apontar um "I/O APIC" para
  o ICR); a versão lida tem de ser plausível.

### D-043 — Módulo initrd: validado pelo núcleo, entregue só de leitura (2026-11-26, CLAUDE)
O initrd é o módulo do Limine com a string `kitty-initrd`
(`module_string: kitty-initrd` no `limine.conf`). No arranque, o kernel lê
diretamente o endereço, o tamanho e a string de cada módulo (o
`File::cmdline()` do crate `limine` faz `unwrap()` a UTF-8 inválido e daria
panic; o `File::data()` cria um slice a partir de valores não verificados),
compara a string **exatamente** (no máximo 64 bytes lidos), e recusa se
houver mais do que um módulo marcado (ambíguo). Depois das nossas tabelas
de páginas estarem ativas, `vmm::initrd_slice` exige tamanho 1 B–128 MiB e
que **todo** o intervalo físico esteja numa única região "kernel and
modules" do mapa, que **não se sobreponha a nenhuma página física da imagem
do kernel** (verificado página a página nas nossas tabelas: a imagem tem o
mesmo tipo no mapa e páginas `.data`/`.bss` escrevíveis — um "módulo" por
cima dela seria um alias imutável de memória que o kernel escreve e uma
fuga de dados do kernel para o VFS), e que todas as suas páginas do HHDM
estejam mapeadas **só de leitura** (uma página partilhada com uma região
ACPI podia ter ficado RW). Mais de 64 módulos → recusado (não se sabe se
há outro `kitty-initrd` escondido). O PMM nunca entrega estas páginas, por
isso o `&'static [u8]` é sólido durante toda a vida do kernel.
*Revisto depois do `security-reviewer`*, que confirmou o caso da
sobreposição com a imagem do kernel. Sem módulo, ou módulo recusado: o arranque
continua sem initrd (diagnóstico), com a razão escrita no log.

### D-044 — `unsafe` permitido em `sched/` (2026-11-26, CLAUDE)
Trocar de pilha (chamar `arch::x86_64::context::switch`) e voltar a fazer
de um endereço um `fn()` não se consegue em Rust seguro, e o scheduler é o
único dono dessas operações. Tal como o `sync/` (D-008), o `sched/` entra na
lista de módulos com `#[allow(unsafe_code)]`. Cada bloco tem `// SAFETY:` e
está em `docs/UNSAFE.md`.

### D-045 — Threads do kernel e scheduler preemptivo round-robin (2026-11-26, CLAUDE)
- **Troca de contexto mínima** (`context.rs`): guarda os 6 registos
  callee-saved e o RSP, carrega o RSP da outra thread e faz `ret` para
  dentro dela (14 instruções). Os caller-saved já são guardados pelo
  compilador à volta da chamada; o kernel não usa SSE (sem estado de FPU);
  a troca é sempre feita com interrupções desligadas. Uma thread parada é
  só o seu RSP.
- **Pilhas** (`mm/stack.rs`): slots de 64 KiB na região
  `0xFFFF_E000_0000_0000`, com 32 KiB de pilha RW+NX, **página de guarda**
  por baixo e espaço por mapear por cima; slots reutilizados; frames limpos
  pelo PMM ao libertar.
- **Preempção:** o timer chama `sched::tick()`, que só marca um pedido
  (atómico, sem lock). A troca acontece no fim de `irq::dispatch`, **depois
  do EOI** e sem nenhum lock da tabela preso. **Nunca preempta** com um
  `SpinLock` simples preso (contador em `sync::preempt`): no mesmo CPU, a
  thread seguinte podia girar para sempre num lock cujo dono não corre. Com
  `IrqSpinLock` o problema nem se põe (as interrupções estão desligadas).
- **Saída e recolha:** uma thread que volta da função de entrada morre; a
  pilha dela é libertada pela próxima decisão do scheduler, fora do lock e
  nunca enquanto ainda se está a correr nela.
- **Sem heap em contexto de interrupção** (revisão do Codex ao #39/#40): a
  fila `ready` e a lista de slots livres das pilhas reservam capacidade em
  contexto de thread (`spawn`, `KernelStack::new`) para todas as threads e
  slots existentes; por isso o caminho de preempção, a recolha de threads
  mortas no fim de uma IRQ e o `wake` chamado por um handler nunca alocam.
- **Ordem dos locks** (do exterior para o interior): `SCHED` → heap →
  espaço do kernel → PMM, e `stack::SLOTS` → heap. `SCHED` nunca é pedido
  com um destes presos. O caminho da troca não aloca (a thread que morre
  vai para um `Option`, não para um `Vec`).
- **Revisto depois do `security-reviewer`:** o contador de preempção passou
  a incrementar *antes* do CAS do `SpinLock`; uma troca cooperativa com um
  lock preso é um `assert!`; o panic handler desliga as interrupções logo à
  entrada (sem preempção durante um panic); `exit` da última thread
  executável é um panic explícito; e acabou o `transmute` do endereço de
  entrada (o trampolim passa o `fn()` diretamente).
- **Um CPU:** o lock do scheduler é largado *antes* da troca, com as
  interrupções ainda desligadas, o que chega num só CPU. O SMP (Fase 9)
  exige filas por CPU e o lock preso durante a troca.
- **Medição:** autoteste no arranque com 2 threads em ping-pong
  (`yield_now`), que mostra os ciclos (TSC) por yield+troca. No QEMU TCG
  os "ciclos" não são de hardware real: o número serve para comparar
  versões, não como valor absoluto.

### D-046 — Filas de espera (`WaitQueue`) sem wake-ups perdidos (2026-11-26, CLAUDE)
`WaitQueue::wait_until(|| condição)` verifica a condição, regista a thread
e bloqueia-a (estado `Blocked`, fora da fila de prontas) **com as
interrupções desligadas**. Num só CPU nada corre entre esses passos, por
isso quem acorda ou corre antes da verificação (a condição já é verdadeira)
ou depois do registo (a thread é acordada): nunca se perde um wake-up. A
condição é sempre verificada de novo depois de acordar (wake-ups espúrios
não fazem mal). `wake_one`/`wake_all` podem ser chamados de handlers de
interrupção e ignoram entradas obsoletas. Se nenhuma outra thread puder
correr, o waiter dorme com `sti; hlt` em vez de girar. SMP (Fase 9): a
verificação e o registo passam a ser feitos sob o lock da fila.
**Revisto depois do `security-reviewer`:** (1) uma thread acordada voltava
com as interrupções **desligadas** (a troca não transporta o RFLAGS): agora
`wait_until` exige IF=1 e nenhum lock à entrada (panic sempre que for mal
usado, não só quando a condição é falsa) e liga sempre as interrupções ao
sair; o autoteste verifica-o. (2) Há uma **thread idle dedicada**, que nunca
está na fila de prontas, nunca bloqueia nem sai: é escolhida quando a thread
atual bloqueia ou sai sem mais nada pronto. Antes, se a thread de arranque
estivesse bloqueada, o `exit` da última thread dava panic.

### D-047 a D-053 — Desenho da Fase 6 (2026-11-26, CLAUDE)
O contrato completo está em `docs/FASE6.md`. Resumo das decisões:
- **D-047** Processos com espaço de endereçamento próprio; metade do kernel
  partilhada (PML4 256–511 copiadas); primeiro 1 MiB de user nunca mapeado;
  `TSS.RSP0` atualizado em cada troca.
- **D-048** Entrada em ring 3 por `iretq` com registos a zero; `swapgs`
  nos stubs vindos de ring 3; exceções de user matam o processo; SMEP,
  SMAP e UMIP ligados antes do primeiro processo.
- **D-049** ABI: `syscall`, número em `rax`, argumentos `rdi rsi rdx r10 r8
  r9`, erros negativos (`E_INVAL` a `E_TOOBIG`); todos os argumentos
  validados antes de qualquer efeito; `sysret` só com RIP canónico de user.
- **D-050** `UserPtr`/`copy_*_user`: intervalo todo na metade de user, sem
  overflow, SMAP aberto só durante a cópia, #PF na cópia dá `E_FAULT` por
  tabela de exceções (nunca panic), copiar uma vez antes de validar.
- **D-051** Handles = índice + geração; direitos `READ WRITE MAP TRANSFER
  DUP`; `handle_dup` só reduz direitos; sem nomes globais.
- **D-052** `/sbin/init` do initrd, ELF64 validado e **copiado** para
  frames novos; recebe só o handle 0 = CONSOLE (`READ|WRITE`); consola de
  bytes (UTF-8 + sequências ESC), escrita parcial, leitura bloqueante.
- **D-053** INPUT: eventos de 24 bytes com `time_ns` monotónico; objeto
  exclusivo (`E_BUSY`); fila de 256 com evento "perdidos"; **fila esvaziada
  na troca de consumidor** (o seguinte não vê teclas do anterior); o handler
  de IRQ publica sem heap e acorda com `WaitQueue::wake_one`.
- **Alinhadas com a D-058** (PR #77): sem `input_open`; o `init` recebe os
  handles e passa-os com `process_spawn`; `handle_info`; direitos com bits
  por tipo; `uid`/`gid` e auditoria preparados; `E_BADSIG` reservado.

### D-054 — Implementação da 6a: SMEP/SMAP/UMIP e `copy_*_user` (2026-11-27, CLAUDE)
- SMEP, SMAP e UMIP são ligados no CR4 **só se o CPUID os reportar**
  (escrever um bit reservado dá #GP); o arranque escreve na consola quais
  ficaram ativos, para um CPU antigo sem SMAP não passar despercebido.
- A cópia é um único `rep movsb` numa rotina em assembly com uma "tabela de
  exceções" de uma entrada: um #PF em ring 0 **exatamente** nessa instrução
  é recuperado saltando para um fixup que devolve erro. Qualquer outro #PF
  do kernel continua a ser fatal.
- `stac`/`clac` rodeiam só a cópia. **Detalhe importante:** o CPU não limpa
  a flag AC ao entrar numa interrupção, por isso `trap_dispatch` faz `clac`
  à entrada; sem isso, um handler que interrompesse uma cópia correria com
  o SMAP desligado. O `iretq` repõe a AC de quem foi interrompido.
- **Revisto depois do `security-reviewer`:** o fixup só se aplica se o
  endereço da falha (CR2) estiver na metade de user e o código de erro não
  tiver `RSVD` (tabelas corrompidas); uma falha do lado do kernel é
  corrupção e continua fatal, em vez de virar um `E_FAULT` silencioso.
- **Invariante (a manter nas fases seguintes):** a metade de baixo de
  qualquer espaço de endereçamento **só tem páginas de user (U=1)**. O SMAP
  só protege páginas com U; uma página de supervisor ali (trampolim de APs,
  mapeamento de identidade, estruturas por processo) ficaria acessível a
  `copy_*_user`. Na 6b, uma auditoria da metade de user verifica isto.
- **Para a 6c:** o `IA32_FMASK` do `syscall` tem de limpar também a **AC**
  (bit 18), além de IF/DF/TF: o ring 3 pode pôr AC=1 com `popf`, e a entrada
  por `syscall` não passa pelo `clac` do dispatch.

### D-055 — Capacidades em vez de só permissões estilo Unix (2026-11-27, CLAUDE)
- **Decisão:** a autoridade no kitty-OS são **handles com direitos**
  (D-018/D-051). Um processo só pode atuar sobre os objetos para os quais
  tem um handle (ficheiro, diretório, rede, ecrã, input, áudio, auditoria),
  e começa **sem nenhum**: quem o cria passa-lhe explicitamente os que deve
  ter. Os UIDs/GIDs e as permissões de ficheiros (Fase 15) continuam a
  existir, mas **só decidem que handles uma sessão recebe** (por exemplo, o
  VFS recusa `open_at` num ficheiro de outro utilizador). Não há um UID 0
  que salte as verificações: "admin" é uma sessão que recebeu certos
  handles (`AUDIT`, gestão de contas) depois de autenticada.
- **Porquê:**
  - **Autoridade ambiente:** no Unix, qualquer programa do utilizador pode
    abrir todos os ficheiros desse utilizador e falar com a rede. Um jogo
    comprometido lê as chaves SSH. Com capacidades, o jogo só tem o ecrã,
    o input e a sua pasta.
  - **Confused deputy:** um serviço com privilégios que abre um caminho a
    pedido de outro pode ser enganado a usar os seus próprios direitos. Com
    handles, quem pede passa o handle, e o serviço usa só esse.
  - **Menor privilégio por defeito**, que é o que precisam o sandbox das
    apps (Fase 15) e os manifestos do `kpkg` (Fase 18).
  - **Auditável:** a autoridade de um processo é a sua tabela de handles,
    uma lista finita que se pode inspecionar e registar.
- **Custo:** as syscalls e o IPC têm de nascer assim (D-058); adaptar
  programas pensados para POSIX dá mais trabalho.

### D-056 — Criptografia feita de raiz, educativa e não auditada (2026-11-27, CLAUDE)
- **Decisão:** SHA-256, ChaCha20-Poly1305, Ed25519, Argon2id e o CSPRNG
  são implementados por nós em `kernel/src/crypto` (Fase 16), sem crates
  externos (mantém-se a regra dos crates permitidos). O módulo e a
  documentação dizem sempre **"educativa, não auditada"**.
- **Regras obrigatórias:**
  - passar os **vetores de teste oficiais**: RFC 8439 (ChaCha20-Poly1305),
    RFC 8032 (Ed25519, incluindo as verificações de maleabilidade: `S < L`
    e pontos inválidos recusados), NIST FIPS 180-4 / CAVP (SHA-256) e
    RFC 9106 (Argon2id). Os testes correm na CI no host e no arranque;
  - **tempo constante** para tudo o que toca em segredos: comparação de
    tags e hashes sem saída antecipada, sem ramos nem índices de tabelas
    que dependam de dados secretos, aritmética de campo do Ed25519 sem
    ramos. A revisão do red team procura isto de propósito;
  - **nunca reutilizar um nonce** com a mesma chave (o disco usa nonces
    derivados de forma única por bloco e versão);
  - segredos apagados da memória quando deixam de ser precisos, com
    escritas que o compilador não pode eliminar;
  - sem `unsafe` em `crypto/` (o acesso a RDRAND/RDSEED fica em `arch/`).
- **Porquê:** o objetivo do projeto é aprender, e implementar estas
  primitivas ensina exatamente onde costumam falhar. **Mas** crypto caseira
  não substitui bibliotecas auditadas: por isso o rótulo é obrigatório e o
  modelo de ameaça (SECURITY.md) avisa para não guardar dados que precisem
  mesmo de proteção.

### D-057 — Firewall no kernel, IDS em user space (2026-11-27, CLAUDE)
- **Decisão:** o **firewall** (Fase 17) fica no kernel, em
  `kernel/src/firewall`, e a stack de rede chama-o em pontos fixos
  (entrada, saída, novo socket, nova ligação). O **IDS** é um processo
  de user space que recebe metadados e cópias limitadas dos pacotes, e
  fala com o firewall por um handle `FIREWALL` com direitos limitados.
- **Porquê:**
  - **Aplicação** (enforcement) tem de estar no caminho de cada pacote e
    fora do alcance dos processos: se o firewall estivesse em user space,
    matar ou comprometer esse processo desligava a proteção. As regras são
    simples (tuplos por processo, porta e endereço), por isso o código no
    kernel é pequeno e rápido.
  - **Deteção** é o contrário: muito parsing de dados hostis e heurísticas
    que mudam. Em user space, um bug no IDS faz crashar só o IDS, não o
    kernel, e pode ser atualizado e testado como qualquer programa.
  - O handle do IDS só tem o direito de **acrescentar regras de bloqueio
    temporárias**; nunca abre portas nem apaga regras do administrador.
    Assim, um IDS comprometido não consegue abrir o firewall.
- Regras por processo funcionam porque um socket é um objeto do kernel
  criado a partir de um handle `NET` com direitos (`CONNECT`, `LISTEN`,
  `BIND_LOW_PORT`…): o firewall sabe sempre a que processo pertence.

### D-058 — Fases 6 e 12 "capability-first" (2026-11-27, CLAUDE)
Ajusta o contrato da Fase 6 (`docs/FASE6.md`, D-049, D-051, D-053) e o
desenho da Fase 12, para a Fase 15 não obrigar a refazer as syscalls.
- **Regra da ABI:** toda a syscall com efeito fora do próprio processo
  recebe um handle como primeiro argumento. Só não recebem handle:
  `exit`, `yield`, `clock_now`, `random_fill` e as operações sobre a
  própria tabela de handles (`handle_close`, `handle_dup`, `handle_info`).
  Não há nomes nem caminhos globais.
- **Direitos (`u32`):** bits 0–15 genéricos (`READ`, `WRITE`, `MAP`,
  `TRANSFER`, `DUP`, `EXEC`, reservados os restantes); bits 16–31
  **específicos de cada tipo** de objeto (por exemplo `NET_CONNECT`,
  `NET_LISTEN`, `FW_ADD_BLOCK`, `AUDIT_READ`). Novos tipos não mudam a ABI.
- **`handle_info(h) -> (tipo, direitos)`** desde a 6c, para um processo
  saber o que tem sem tentar e falhar.
- **`process_spawn(imagem, handles, n)`** é a única forma de criar um
  processo e de lhe dar autoridade: a imagem é um handle de ficheiro com
  `EXEC`; `handles` é uma lista de pares (handle, direitos), copiados com
  direitos iguais ou menores. Um processo novo não herda nada por omissão.
- **INPUT e DISPLAY deixam de ter syscalls de "abrir":** os objetos são
  criados no arranque e o kernel dá os handles ao `init` (PID 1), que os
  passa ao compositor com `process_spawn`. Substitui o `input_open()` da
  D-053: a exclusividade vem de existir um só handle sem `DUP`; o resto da
  D-053 (fila, perda, esvaziar ao fechar) mantém-se. `display_map(h)`
  exige `MAP`.
- **Ficheiros:** `open_at(dir_handle, caminho, direitos) -> h`, sempre
  relativo a um diretório que o processo tem; `..` não sai do diretório do
  handle (o VFS do Codex recusa). O `init` recebe um handle `READ` para a
  raiz do initrd.
- **Canais (Fase 12):** transferir um handle numa mensagem exige
  `TRANSFER`, retira-o ao emissor (só fica com uma cópia se tiver `DUP` e
  a pedir), e os direitos só podem diminuir.
- **Preparação da Fase 15:** o processo tem `uid`/`gid` (0 = "sistema" até
  lá) e as syscalls sensíveis chamam `audit::record` (sem efeito até à
  Fase 15). **Preparação da Fase 16:** o código de erro `-11 E_BADSIG`
  fica reservado já, para o ELF loader recusar programas não assinados.
- **O `docs/FASE6.md` está no PR #52, ainda sem merge:** é alinhado com esta
  decisão no próprio #52.

### D-059 — Red team vs blue team (2026-11-27, CLAUDE)
- **Decisão:** a partir da Fase 6, cada fase concluída acaba com uma ronda
  de ataques cruzados (o Claude ataca a área do Codex e vice-versa), com as
  regras em `docs/REDTEAM.md`. Cada falha tem um PoC que corre só no QEMU,
  em `tests/security/redteam/`, e uma issue `vulnerabilidade`.
- **Exceção à regra das áreas:** `tests/security/redteam/` é partilhada,
  mas só para acrescentar o PoC das falhas que o próprio encontrou. O
  runner e a CI continuam a ser do Codex.
- **CI verde com falhas conhecidas:** cada PoC tem o estado `aberta` ou
  `corrigida` num manifesto; com `aberta`, a CI espera que o PoC falhe
  (xfail), e a correção muda-o para `corrigida`.
- **Porquê:** quem escreve o código tem os mesmos pontos cegos quando o
  revê. Um atacante diferente, com incentivo para encontrar falhas, vê
  outras coisas. Os PoCs transformam cada falha num teste permanente.

### D-137 — Camada de registos 16550A sem espera ativa (2026-11-26, CODEX)

`kitty-uart` usa exclusivamente `kitty-hal::PortBlock`. Configura 8N1
com um divisor diferente de zero. Limpa DLAB antes de escrever IER e
volta ao banco de dados após programar o divisor. Se a configuração
falhar, as operações de dados ficam indisponíveis até nova configuração
completa. Cada operação de dados consulta o LSR uma vez e transfere no
máximo um byte. As flags da receção são preservadas quando a transmissão
consulta o LSR. Um *overrun* isolado preserva o byte mais antigo na FIFO.

A D-009 conserva a função da consola de arranque. A integração com
portas reais exige uma concessão exclusiva e um contrato de interrupções.

### D-134 — Repetir a preparação do Limine quando o transporte falha (2026-11-26, CODEX)

O `xtask` tenta o `git fetch` até três vezes, com esperas de um e dois
segundos entre tentativas. O Git pode abandonar ligações HTTP que
transferem menos de um byte por segundo durante vinte segundos.
O checkout é sempre feito pelo commit fixo da D-004. O marcador
de cache só é escrito depois de o checkout passar.

### D-113 — Comparação visual da primeira linha de arranque (2026-11-26, CODEX)

`cargo xtask test-visual` espera pela confirmação de arranque na porta
série e obtém o framebuffer com `screendump` do QEMU. A referência
contém a primeira linha de 304 × 16 píxeis. A comparação ignora o
conteúdo fora dessa região, onde aparecem valores dependentes do
firmware e do build. O leitor PPM verifica dimensões e comprimento
usando aritmética verificada. Em caso de diferença, o relatório
identifica a primeira coordenada e preserva a captura.

### D-114 — Medir o arranque pelo marcador da porta série (2026-11-26, CODEX)

`cargo xtask bench-boot` usa o relógio monotónico do host de antes do
`spawn` até o leitor receber a linha de arranque completa. O leitor
em separado mede a entrega da linha. A preparação do ELF e da imagem e
o intervalo de polling da saída do QEMU ficam fora da medição.
Cada amostra exige o código de sucesso do kernel. O relatório usa a
mediana de sete amostras depois de um arranque de aquecimento.

### D-115 — Comparar benchmarks com a base do PR no mesmo runner (2026-11-26, CODEX)

O job de benchmark compila o ELF da base do PR com `qemu-exit`. O
mesmo xtask mede esse ELF e o candidato em BIOS/release, com as mesmas
versões de QEMU e Limine. Uma subida de pelo menos 25% e 50 ms na
mediana produz um aviso no GitHub. A condição dupla separa variações
pequenas de uma regressão a investigar. Erros de preparação ou de
arranque fazem o job falhar.
### D-106 — Descodificação MADT sobre bytes imutáveis (2026-11-25, CODEX)

`kitty-acpi` verifica a assinatura, o comprimento declarado (limite de
64 KiB) e a soma de verificação antes de percorrer a MADT. Descodifica
registos I/O APIC, overrides de IRQ ISA e o override de endereço do Local
APIC. Rejeita campos reservados nos registos usados e definições repetidas
do mesmo ID de I/O APIC, fonte ISA ou endereço alternativo do Local APIC.
Avança sobre os outros tipos apenas depois de validar os limites do
registo. Sem alocação no parser.

A descoberta e o mapeamento de firmware pertencem ao núcleo (#18).
Endereços físicos descodificados precisam da capacidade de MMIO do
núcleo. O driver deve determinar o número de entradas de cada I/O APIC e
validar os intervalos GSI antes de programar qualquer rota.

O workspace separado `fuzz/` usa `libfuzzer-sys` e `cargo-fuzz`
só no host. A CI verifica a lógica com Miri e corre AddressSanitizer
tanto sobre bytes brutos como sobre registos dentro de um cabeçalho
com soma válida, para exercitar a lógica após essa verificação.

### D-122 — Resolver as rotas NMI do Local APIC por CPU (2026-11-26, CODEX)

Os registos MADT tipo 0 ligam o ID de um xAPIC ativo ao UID ACPI do seu
processador. Os registos tipo 4 usam esse UID ou `0xff` para indicar todos
os processadores. `local_nmis` resolve separadamente LINT0 e LINT1, valida
a polaridade e o modo de disparo e rejeita configurações contraditórias.
As flags conformes são interpretadas como ativo alto, disparo por flanco.
UIDs ou IDs repetidos entre processadores ativos tornam a tabela ambígua.

Uma entrada de NMI do I/O APIC ou x2APIC exige uma política própria. O
pedido de rotas xAPIC devolve erro nestes casos para que a inicialização
possa preservar o encaminhamento existente. [ACPI 6.6, §§5.2.12.2,
5.2.12.6–7 e 5.2.12.13](https://uefi.org/specs/ACPI/6.6/05_ACPI_Software_Programming_Model.html#multiple-apic-description-table-madt)

### D-107 — Programar rotas I/O APIC com a entrada mascarada (2026-11-25, CODEX)

O driver recebe um `RegisterBlock` exclusivo. Escreve o índice de 8 bits
em IOREGSEL (offset `0x00`) e acede ao registo de 32 bits em IOWIN (offset
`0x10`). A versão define quantas entradas cabem na tabela. Cada entrada
ocupa dois índices consecutivos a partir de `0x10`. O par seletor/janela
requer exclusão mútua por dispositivo.

Antes de escrever a metade alta (destino) e depois a baixa (vetor,
polaridade e trigger), o driver confirma a máscara da rota anterior.
Se houver entrega em curso ou Remote IRR, mantém a máscara e espera
que o dono do handler trate a interrupção antiga antes de voltar a
tentar. O driver confirma por leitura os campos da rota nova e só
permite desmascará-la após uma configuração completa.

O núcleo fornece o mapeamento UC e valida a topologia antes da mudança de
controlador (#18). A ligação futura confirma que o destino é um APIC ID
válido, que os intervalos GSI não se sobrepõem e que o handler está
instalado antes de desmascarar.

### D-121 — Programar o timer do Local APIC com a entrada mascarada (2026-11-26, CODEX)

O driver acede a registos de 32 bits por um `RegisterBlock` xAPIC UC do
CPU em execução. Lê a revisão e o limite da tabela LVT em `0x030` e obtém
o destino físico unicast em `0x020`. Ativa a entrega em `0x0f0` com o vetor
espúrio `0xff` e difusão de EOI. Antes de ativar, verifica as entradas LVT
presentes na revisão. Aceita entradas mascaradas e LINT já configurado
para NMI. O controlador confirma as fontes e o estado do PIC antes da
transição. Para vetores de dispositivo, escreve zero em `0x0b0` depois
de o handler devolver. Um vetor espúrio dispensa EOI.

O timer mascara a sua entrada LVT (`0x320`) e escreve zero na contagem
inicial (`0x380`) antes de mudar o divisor (`0x3e0`) e o modo. O divisor
usa os bits 0, 1 e 3. O driver confirma por leitura o divisor, o modo e
o vetor. A contagem inicial de 32 bits é positiva e `0x390` devolve os
ticks restantes após divisão. A calibração pode observar a contagem
enquanto a entrada está mascarada. O modo periódico recarrega a contagem
ao expirar. O chamador instala o handler antes de desmascarar e trata
eventuais interrupções já aceites pelo CPU quando pára o timer.

[Intel SDM, vol. 3A, §§12.4.8, 12.5.4 e 12.8.5](https://cdrdv2-public.intel.com/835754/253668-sdm-vol-3a.pdf)

### D-138 — Ligar o controlador APIC no arranque (2026-11-26, CODEX)

A feature `xapic` prepara o I/O APIC e o LAPIC pelas capacidades do
núcleo e instala `InterruptController` antes dos restantes dispositivos.
Os overrides ISA precedem as rotas por identidade. Uma GSI tem um único
dono. São recusadas rotas especiais ativas, intervalos sobrepostos,
NMI em LINT0 e NMI de nível em LINT1. O LAPIC é ativado antes de
configurar NMI e os bancos ISR e IRR têm de estar vazios.
### D-044 — `unsafe` permitido em `sched/` (2026-11-26, CLAUDE)
Trocar de pilha (chamar `arch::x86_64::context::switch`) e voltar a fazer
de um endereço um `fn()` não se consegue em Rust seguro, e o scheduler é o
único dono dessas operações. Tal como o `sync/` (D-008), o `sched/` entra na
lista de módulos com `#[allow(unsafe_code)]`. Cada bloco tem `// SAFETY:` e
está em `docs/UNSAFE.md`.

### D-045 — Threads do kernel e scheduler preemptivo round-robin (2026-11-26, CLAUDE)
- **Troca de contexto mínima** (`context.rs`): guarda os 6 registos
  callee-saved e o RSP, carrega o RSP da outra thread e faz `ret` para
  dentro dela (14 instruções). Os caller-saved já são guardados pelo
  compilador à volta da chamada; o kernel não usa SSE (sem estado de FPU);
  a troca é sempre feita com interrupções desligadas. Uma thread parada é
  só o seu RSP.
- **Pilhas** (`mm/stack.rs`): slots de 64 KiB na região
  `0xFFFF_E000_0000_0000`, com 32 KiB de pilha RW+NX, **página de guarda**
  por baixo e espaço por mapear por cima; slots reutilizados; frames limpos
  pelo PMM ao libertar.
- **Preempção:** o timer chama `sched::tick()`, que só marca um pedido
  (atómico, sem lock). A troca acontece no fim de `irq::dispatch`, **depois
  do EOI** e sem nenhum lock da tabela preso. **Nunca preempta** com um
  `SpinLock` simples preso (contador em `sync::preempt`): no mesmo CPU, a
  thread seguinte podia girar para sempre num lock cujo dono não corre. Com
  `IrqSpinLock` o problema nem se põe (as interrupções estão desligadas).
- **Saída e recolha:** uma thread que volta da função de entrada morre; a
  pilha dela é libertada pela próxima decisão do scheduler, fora do lock e
  nunca enquanto ainda se está a correr nela.
- **Sem heap em contexto de interrupção** (revisão do Codex ao #39/#40): a
  fila `ready` e a lista de slots livres das pilhas reservam capacidade em
  contexto de thread (`spawn`, `KernelStack::new`) para todas as threads e
  slots existentes; por isso o caminho de preempção, a recolha de threads
  mortas no fim de uma IRQ e o `wake` chamado por um handler nunca alocam.
- **Ordem dos locks** (do exterior para o interior): `SCHED` → heap →
  espaço do kernel → PMM, e `stack::SLOTS` → heap. `SCHED` nunca é pedido
  com um destes presos. O caminho da troca não aloca (a thread que morre
  vai para um `Option`, não para um `Vec`).
- **Revisto depois do `security-reviewer`:** o contador de preempção passou
  a incrementar *antes* do CAS do `SpinLock`; uma troca cooperativa com um
  lock preso é um `assert!`; o panic handler desliga as interrupções logo à
  entrada (sem preempção durante um panic); `exit` da última thread
  executável é um panic explícito; e acabou o `transmute` do endereço de
  entrada (o trampolim passa o `fn()` diretamente).
- **Um CPU:** o lock do scheduler é largado *antes* da troca, com as
  interrupções ainda desligadas, o que chega num só CPU. O SMP (Fase 9)
  exige filas por CPU e o lock preso durante a troca.
- **Medição:** autoteste no arranque com 2 threads em ping-pong
  (`yield_now`), que mostra os ciclos (TSC) por yield+troca. No QEMU TCG
  os "ciclos" não são de hardware real: o número serve para comparar
  versões, não como valor absoluto.
### D-105 — PIT legado em modo periódico (2026-11-25, CODEX)

O crate `kitty-pit` programa o canal 0 do PIT 8254 em modo 2 através de
`PortBlock`. Os divisores válidos são 2–65 536. O valor máximo usa a
codificação de 16 bits a zero prevista pelo hardware. Um tipo validado
permite calcular o período antes de escrever nas portas e preservar
a razão exata divisor/frequência nominal para contabilizar tempo.

A programação faz-se com IRQ0 mascarado. Se uma escrita falhar mantém-se a
linha mascarada até uma configuração completa. O driver fica disponível
para integração quando houver um consumidor de ticks no kernel.

### D-120 — Parar o PIT antes da transferência do IRQ0 (2026-11-26, CODEX)

`Pit::stop` escreve o controlo do canal 0 em modo 0 e deixa-o à espera
de um novo contador. Antes da paragem, o núcleo mascara IRQ0 e trata
interrupções que o PIC já tenha registado. A CI verifica a cadeia de
entrega através do autoteste do #15, que exige dois IRQ0 depois de
programar o modo periódico.

### D-128 — Ligar o PIT legado ao scheduler (2026-11-26, CODEX)

O driver programa o canal 0 com uma frequência pedida de 100 Hz e
regista a linha ISA 0 depois de obter as portas e configurar o período.
O handler pede preempção através de `sched::tick()`. O núcleo decide
a troca de thread após o EOI.

Os autotestes do núcleo que programam o PIT têm o seu próprio registo
de IRQ0. O novo autoteste usa o driver do arranque e verifica duas
trocas de contexto durante a execução de uma thread ocupada.

### D-151 — Tic do scheduler pelo LAPIC com referência PIT (2026-11-27, CODEX)

A opção `lapic-scheduler-timer` calibra o contador local com IRQ0 e a entrada
LAPIC mascarados. Mede dois períodos do PIT a cerca de 200 Hz, limita o
número de leituras e repete medições inconsistentes até três tentativas.
Com uma medição válida, usa o temporizador LAPIC em modo periódico a 100 Hz
no CPU de arranque. O PIT continua disponível para o scheduler quando o
LAPIC está indisponível ou a calibração não é fiável.
### D-117 — Descodificar o modo base do rato PS/2 (2026-11-26, CODEX)

O rato em modo base envia um cabeçalho e dois bytes de deslocamento.
O bit 3 do cabeçalho indica a posição esperada do início. Os bits de
sinal completam valores de movimento com nove bits, e o eixo Y é
invertido para coordenadas de ecrã. Um overflow deixa apenas esse
eixo sem movimento válido e preserva os botões do relatório.

O decoder espera exclusivamente bytes AUX do modo de três bytes,
depois do tratamento das respostas de comandos. Um erro de
transporte descarta o pacote parcial. Para retomar, o transporte
estabelece a fronteira do pacote seguinte.

### D-103 — Transporte i8042 através de `PortBlock` (2026-11-25, CODEX)
O crate interno `kitty-i8042` usa `kitty-hal` para configurar o teclado
em Set 2 sem tradução. Mantém as IRQs do controlador desligadas até
confirmar o modo do teclado e limita a espera e os reenvios. A leitura
em interrupção recolhe um byte por chamada, com a origem e o resultado
da validação de paridade e timeout.

O transporte é partilhável com a futura porta auxiliar. O descodificador
do teclado recebe só bytes validados da sua porta e pode limpar o estado
quando o controlador sinaliza uma perda.

### D-104 — Fila limitada para o teclado PS/2 em IRQ1 (2026-11-25, CODEX)
O handler recolhe um byte por IRQ e guarda as teclas descodificadas numa
fila de 64 eventos. Um erro do teclado ou uma fila cheia gera `Lost` antes
dos eventos seguintes, para que o consumidor limpe teclas que julgava
premidas. A porta auxiliar segue um percurso distinto.

A fila mantém o trabalho em interrupção limitado e usa memória fixa. O
teste QEMU envia uma tecla normal e uma estendida e valida as pressões e
libertações pela saída série de um build de teste.

### D-118 — Respostas AUX e recuperação do teclado no i8042 (2026-11-26, CODEX)

O mesmo bloco de portas serve o teclado e o rato. Durante a preparação,
o driver fecha as portas e drena a saída. Reencaminha cada comando do
rato com `0xD4` e aceita apenas respostas com o bit AUX. Confirma o
ID padrão zero antes de habilitar a produção de relatórios e o IRQ12.

O modo de teclado preparado antes da configuração AUX serve de
fallback. O driver desativa a porta AUX, drena dados e repõe a
configuração anterior. Uma falha na reposição é comunicada a quem
regista as linhas ISA, para que as mantenha mascaradas.

### D-119 — Fila PS/2 partilhada e validação em IRQ12 (2026-11-26, CODEX)

IRQ1 e IRQ12 partilham o controlador. O estado do i8042 decide se cada
byte vai para o Set 2 ou para o descodificador do rato. A fila de
64 entradas conserva a ordem de conclusão dos eventos. Uma perda
descarta entradas anteriores e publica `Lost` para o consumidor
limpar o estado de teclas e botões.

Se a origem AUX e a posição do byte corrompido forem conhecidas,
o descodificador conta e descarta os restantes bytes desse pacote.
Quando a posição deixa de ser conhecida, a leitura de relatórios
fica suspensa até nova configuração. O teste QEMU verifica que
o deslocamento e ambos os estados do botão chegam pelo handler de IRQ12.
### D-133 — Traduzir teclas ISO PT-PT para a consola (2026-11-26, CODEX)

`PortugueseKeyboard` aplica modificadores e teclas mortas às posições
físicas PS/2. Produz sequências completas de até oito bytes, sem heap.
O código de edição usa o formato da secção 6 do PR #52. Um evento de
perda repõe todo o estado do layout para evitar manter modificadores
ou combinações de acentos depois de uma sequência incompleta.

### D-140 — Descodificar snapshots de configuração PCI (2026-11-26, CODEX)

`kitty-pci` exige um cabeçalho completo de 64 bytes para os layouts 0 e
1. Interpreta os campos little-endian e conserva os BARs em bruto. A
lista de capacidades convencional só é seguida quando o estado a
anuncia. Os links ficam limitados a `0x40`–`0xfc`, alinhados a quatro
bytes e dentro do slice. Um mapa de bits impede voltar ao mesmo offset.
O descodificador usa Rust seguro, sem alocação nem dependências. A
obtenção do snapshot e a validação de recursos ficam na futura camada
de acesso à configuração PCI.

### D-124 — Verificar preempção, espera e pilhas de threads na CI (2026-11-26, CODEX)

O arranque normal verifica os autotestes do scheduler cooperativo e da
fila de espera. O cenário `selftest-preempt` exige que threads ocupadas
progridam com ticks de hardware e que uma interrupção acorde a thread
que espera enquanto o CPU está em idle.

O cenário `selftest-thread-stack-overflow` exige o código de saída de
panic e o relatório #DF na pilha IST depois dos autotestes do scheduler.
Cada cenário guarda um log série em BIOS e UEFI, em debug e release.

### D-136 — Amostrar os ciclos do scheduler em arranques QEMU (2026-11-26, CODEX)

`bench-scheduler` reutiliza a captura série e o código de saída do teste
de arranque. Recolhe o valor `rdtsc` já medido pelo autoteste cooperativo
do kernel, descarta o primeiro arranque e resume as sete amostras
seguintes. O teste do parser corre no host e em Miri. A medição em TCG
fica disponível para recolha explícita, sem limiar de regressão na CI.

### D-129 — Limitar e validar as linhas da shell (2026-11-26, CODEX)

O parser recebe até 1 024 bytes de UTF-8 e guarda até 16 palavras num
buffer de capacidade fixa. As aspas e escapes preservam espaços e
argumentos vazios. A validação recusa controlos fora da tabulação e
linhas com delimitadores incompletos antes de executar um comando.
`help` e `echo` escrevem num destino `fmt::Write` concedido pelo
chamador. A biblioteca pode ser testada no host e em Miri.

### D-130 — Editar linhas em fronteiras UTF-8 (2026-11-26, CODEX)

`LineBuffer` conserva até 1 024 bytes de texto e um cursor num
offset de carácter válido. Inserção e remoção preservam os bytes
dos restantes caracteres. Uma inserção acima do limite ou com um
controlo recusado deixa a edição disponível. O parser recebe o
slice utilizado da linha quando o comando é submetido.

### D-131 — Descodificar o fluxo de entrada da consola (2026-11-26, CODEX)

`ShellSession` mantém o estado da descodificação e da edição entre
leituras. Usa o formato proposto na secção 6 do PR #52. Lê UTF-8 e as
sequências de edição byte a byte num buffer de até quatro bytes. A
apresentação recebe o estado do editor e os eventos de edição, envio
e cancelamento. Enter consome a linha antes de escrever o resultado,
para que uma falha de escrita deixe a sessão pronta para outra linha.

### D-108 — Formato USTAR só de leitura para o initrd (2026-11-25, CODEX)

O initrd usa registos de 512 bytes, cabeçalhos USTAR com soma de
verificação e campos de tamanho em octal. O parser aceita ficheiros
regulares, incluindo o tipo 7 com a semântica de ficheiro regular,
e diretórios. Os dados de um ficheiro são emprestados ao consumidor com
o comprimento exato. Os metadados de proprietário e modo ficam fora
da interpretação do formato inicial.

Todos os cabeçalhos passam pela validação antes de devolver o arquivo.
A política do initrd exige diretórios com tamanho zero, campos de nome e
prefixo preenchidos com zeros após um NUL, dois blocos de fim a zero e
padding final a zero. A validação conta as entradas para dimensionar o
índice do VFS.
Caminhos UTF-8 podem começar com `./`. O resultado canónico exclui
componentes vazias, `.` interiores e `..`. Tipos especiais, links e
cabeçalhos de extensão são recusados. A montagem no VFS valida conflitos
entre nomes. O acesso às páginas do módulo é tratado na #22.

[POSIX.1-2024, formato USTAR](https://pubs.opengroup.org/onlinepubs/9799919799/utilities/pax.html#tag_20_94_13_06)

### D-109 — Índice inicial do VFS sobre o initrd (2026-11-25, CODEX)

A primeira raiz só de leitura mantém uma entrada de índice por cabeçalho
USTAR e empresta os dados do módulo. A montagem valida o arquivo completo
e ordena as entradas pela sequência de componentes do caminho. Entradas
com o mesmo caminho ficam adjacentes. Um ficheiro que seja prefixo de
outro caminho fica imediatamente antes do primeiro descendente. A
montagem recusa ambos os conflitos antes de disponibilizar a raiz.

A pesquisa usa busca binária. Os nomes com um prefixo comum formam um
intervalo pelo qual se listam os filhos de um diretório, incluindo os
implícitos. O índice aceita até 65 536 cabeçalhos. O VFS usa a contagem
da validação USTAR para fazer uma reserva de memória falível antes de
preencher o índice. A leitura por offset copia apenas bytes do ficheiro
para um buffer já validado.

### D-110 — Superfícies BGRA8888 seguras em user space (2026-11-25, CODEX)

`kitty-gfx` recebe slices separados para leitura e escrita exclusiva.
O layout BGRA8888 usa quatro bytes por píxel, com pitch explícito por
linha. A validação da geometria limita todos os acessos até ao último
píxel visível. Primitivas aceitam coordenadas com sinal e recortam
antes de escrever. O blit preserva a posição relativa dos píxeis ao
recortar as duas superfícies e copia o alfa sem o misturar.

As linhas avançam pelo eixo principal, com o passo inicial calculado
no recorte para manter o arredondamento dos mesmos píxeis. O buffer
permanece em memória de user space. O formato do framebuffer do
dispositivo será tratado na apresentação depois de concedido DISPLAY.

### D-111 — Apresentação explícita a partir do buffer de desenho (2026-11-25, CODEX)

`DoubleBuffer` mantém duas superfícies independentes com as mesmas
dimensões e pitches próprios. O desenho só altera o buffer traseiro.
A apresentação copia toda a área visível ou apenas o retângulo pedido.
A biblioteca reutiliza o recorte do blit e preserva o padding. O par
de empréstimos mutáveis garante acesso exclusivo a cada buffer.

### D-125 — Limitar as regiões alteradas de um frame (2026-11-26, CODEX)

`Damage` recorta as regiões às dimensões da superfície e agrupa as
sobreposições. Guarda até 16 retângulos numa lista de capacidade fixa.
Se faltar espaço, marca todo o frame. Assim, a memória fica limitada
e o compositor conserva todas as zonas que precisa de redesenhar.

O compositor recompõe cada região antes de chamar `present_damage`.
A apresentação rejeita dimensões diferentes e limpa a lista só depois
de copiar todas as regiões.

### D-126 — Compor camadas BGRA8888 com alfa direto (2026-11-26, CODEX)

`Canvas::blend` usa a mesma geometria de recorte que `blit`. A mistura
source-over usa aritmética inteira e divide os canais pela soma dos
pesos antes de arredondar o alfa final. A operação interpreta diretamente
os canais guardados em BGRA8888. `blit` continua a fazer a cópia integral
dos bytes para as operações de apresentação.

### D-127 — Recompor o cursor dentro das regiões alteradas (2026-11-26, CODEX)

O cursor empresta uma imagem BGRA8888 e valida o ponto ativo dentro
dela. O movimento limita esse ponto ao ecrã e regista os dois limites
da imagem em `Damage`. O desenho aplica source-over dentro da
interseção do cursor com cada região recomposta. A região antiga
volta a mostrar o fundo ou a janela que estava por baixo.

### D-139 — Manter a geometria das janelas numa pilha limitada (2026-11-26, CODEX)

A pilha do compositor guarda até 32 identificadores únicos e a geometria
correspondente. A seleção começa no topo e converte a posição absoluta
em coordenadas locais. O desenho percorre a ordem inversa da seleção.
A pilha usa o mesmo `Damage` que o cursor e a apresentação. Ao subir uma
janela, regista apenas as interseções com as camadas ultrapassadas.
Mover e remover exigem recompor os limites visíveis afetados, incluindo
as camadas subjacentes visíveis através de regiões transparentes.

### D-116 — Empacotar initrd opcional na ESP e na ISO (2026-11-26, CODEX)

`xtask --initrd` lê um arquivo regular entre 1 byte e 128 MiB, o limite
que o núcleo usa para receber o módulo em `docs/INTERFACES.md`.
Valida-o com o VFS antes de regenerar o meio de arranque e usa os mesmos
bytes para escrever `/boot/initrd.tar`. A configuração do Limine associa
a marca `kitty-initrd` ao módulo.

Em complemento à D-016, o xtask pode usar crates por caminho do próprio
workspace para partilhar a validação com o núcleo. O carregador do
initrd depende de `kitty-vfs` e a política sobre crates externos da
D-016 aplica-se à ferramenta.

### D-123 — Publicar a raiz do initrd após validar o arquivo (2026-11-26, CODEX)

`devices::init` monta `Platform::initrd` depois de o heap estar pronto.
O VFS valida todo o USTAR e o índice antes de guardar a raiz. Os dados
dos ficheiros permanecem nas páginas só de leitura reservadas pelo
núcleo. Um arquivo recusado deixa a raiz indisponível e o arranque
continua com a razão na consola série.

O `xtask test-initrd` verifica um ficheiro conhecido, uma imagem em que
o checksum do cabeçalho é inválido e a ausência de módulo. Os ensaios
usam meios de arranque próprios em `build/initrd-tests/`. O comando
normal `--initrd` valida o arquivo no host antes de preparar o meio.

### D-152 — Arte do sistema num crate próprio, em índices de paleta (2026-11-27, CLAUDE)

A identidade visual (paleta de 16 cores, Pantufa, animação de arranque)
fica em `art/pantufa` (`kitty-pantufa`): `no_std`, `forbid(unsafe_code)`,
sem dependências nem alocação. A arte é desenhada em índices de paleta
numa cena de 320×180 e só no fim é convertida para BGRA8888 com um fator
de escala inteiro (`present_bgra`).

**Porquê:** o mesmo código serve o ecrã de arranque (que corre antes do
user space), o futuro compositor e os testes no host, por isso não pode
viver no kernel nem em `userland/`. Índices de paleta tornam os fades e o
escurecer da sala uma troca de cores, sem redesenhar; a escala inteira
mantém os píxeis nítidos. Os sprites são formas (elipses, triângulos)
com contorno automático em vez de bitmaps, o que dá poses e
squash-and-stretch sem multiplicar dados. A matemática em `f32` é pequena
(`floor`, `sqrt`, `sin` próprios) porque o alvo bare-metal usa soft-float.

A ligação ao framebuffer e à ordem real do arranque é do Codex (ROADMAP,
Fase 10). O ecrã de arranque nunca espera pela animação: é o gato que
espera pelo sistema.

### D-156 — Apresentação opcional do Pantufa no arranque

O núcleo empresta o framebuffer validado à apresentação até ao fim da
animação. O PIT ou o LAPIC fornece os ticks do relógio do scheduler.
O desenho é feito no contexto da thread de arranque, com um mínimo de
quatro ticks entre apresentações (25 fps pretendidos a 100 Hz). Se
um frame se atrasar, o seguinte usa o tempo atual e pode saltar poses.

O estado e a cena ficam em armazenamento estático, fora da pilha do
núcleo. A consola série continua a receber os diagnósticos e a consola
gráfica recebe o framebuffer no fim. As pegadas de memória,
interrupções, drivers e ficheiros refletem marcos reais. As restantes
dependem dos respetivos subsistemas.
