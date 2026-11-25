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
