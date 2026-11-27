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
