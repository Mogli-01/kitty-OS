# Segurança do kitty-OS

## Modelo de ameaça

**O que protegemos:** a integridade e a confidencialidade do kernel; o
isolamento entre processos, janelas e utilizadores; os dados guardados no
disco; e a confiança no que é executado (programas e pacotes assinados).

**Atacantes considerados:**
1. **Programa malicioso em user space** (o principal). Pode chamar qualquer
   syscall com quaisquer argumentos, passar ponteiros inválidos ou para
   memória do kernel, tentar corridas (TOCTOU) entre threads, e ler tudo o
   que o kernel lhe deixe ver (para descobrir endereços e anular o KASLR).
2. **Dados externos malformados**: ficheiros ELF, imagens de initrd,
   sistemas de ficheiros, input de dispositivos. Tratados sempre como não
   confiáveis: validação completa, sem `unwrap`, sem confiar em tamanhos.
3. **Bugs no próprio kernel**: reduzimos o impacto com W^X, páginas de
   guarda, SMEP/SMAP e o mínimo de `unsafe`.
4. **Atacante local sem privilégios** (Fase 15): uma conta normal com
   sessão aberta. Quer ler ficheiros, ecrã ou input de outro utilizador,
   tornar-se admin, apagar os seus rastos do registo de auditoria, ou
   adivinhar passwords à força (o login limita tentativas e regista falhas).
5. **App maliciosa instalada** (Fases 15 e 18): instalada pelo próprio
   utilizador com `kpkg`. Só tem as capacidades que o manifesto declarou e o
   utilizador aprovou; tenta usar objetos que não lhe foram dados, enganar o
   utilizador com janelas falsas, ou fazer DoS. Um pacote alterado depois de
   assinado é recusado.
6. **Atacante na rede** (Fase 17): controla os pacotes que chegam (scans,
   floods, pacotes malformados, respostas DHCP/DNS falsas). A entrada nega
   tudo por defeito; a stack trata cada pacote como não confiável; o IDS
   deteta padrões e só pode **apertar** regras. Sem TLS, o tráfego HTTP é
   legível e alterável: o mini browser deve dizê-lo ao utilizador.
7. **Acesso físico ao disco** (Fase 16): alguém tira o disco ou a pen e lê-a
   noutra máquina. O disco encriptado (ChaCha20-Poly1305, chave derivada com
   Argon2id) protege os dados **em repouso**, com a máquina desligada.
   Atenção: a crypto é educativa e não auditada (D-056), por isso não se
   deve guardar no kitty-OS nada que precise mesmo de proteção.

**Fora de âmbito (por agora):** outros ataques físicos (DMA por
Thunderbolt, cold boot, "evil maid" que altere o kernel ou o Limine numa
máquina sem Secure Boot: a verificação de assinaturas só protege os
programas carregados **pelo** kernel), firmware comprometido, canais
laterais micro-arquiteturais (Spectre/Meltdown — revisitar depois da
Fase 6), Secure Boot (revisitar na Fase 19, com hardware real).

**Testes ofensivos:** a partir da Fase 6, cada fase acaba com uma ronda de
red team vs blue team (`docs/REDTEAM.md`). As falhas encontradas são
provadas por PoCs que correm só no QEMU e ficam como testes de regressão.

**Confiamos em:** firmware, Limine (fixado por commit, D-004), CPU e o
compilador Rust fixado.

## Proteções

Estados: **ATIVA** · **PARCIAL** · **PLANEADA**.

| Proteção | O que impede | Estado | Fase |
|---|---|---|---|
| Rust + `deny(unsafe_code)` fora de arch/mm/sync | a maioria dos bugs de memória por construção | ATIVA | 1 |
| `// SAFETY:` obrigatório, 1 operação unsafe por bloco | unsafe sem justificação | ATIVA | 1 |
| Sem `unwrap`/`expect` no kernel (lint) | panics provocados por input | ATIVA | 1 |
| W^X por segmento do ELF (R / RX / R / RW) | executar dados, alterar código | ATIVA (nas nossas tabelas desde a Fase 3) | 1 |
| Requests do Limine só de leitura | redirecionar respostas do bootloader | ATIVA | 1 |
| Kernel sem SSE, sem red zone | corrupção da pilha por interrupções; fuga de estado vetorial | ATIVA | 1 |
| Panic sem endereços (só ficheiro:linha) | fuga de layout do kernel | ATIVA | 1 |
| Porta de saída do QEMU só em builds de teste | escrita em porta indevida em hardware real | ATIVA | 1 |
| Limine e GitHub Actions fixados por commit | ataques à cadeia de fornecimento | ATIVA | 1 |
| CI com `permissions: contents: read` | abuso do token da CI | ATIVA | 1 |
| Pilhas IST para double fault / NMI / #MC | triple fault por pilha corrompida | ATIVA (com página de guarda) | 2 |
| IDT: DPL 0 em todos os vetores, interrupt gates, vetores sem handler não presentes (D-026) | user space disparar interrupções com `int n` | ATIVA | 2 |
| TSS sem bitmap de I/O (`iomap_base` = fim da TSS) | user space usar `in`/`out` | ATIVA | 2 |
| Relatórios de crash com endereços só na série (D-023) | fuga de endereços do kernel pelo ecrã | ATIVA | 2 |
| GDT/IDT só de leitura depois do arranque (D-031) | sobrescrever a IDT para desviar exceções | ATIVA | 3 |
| NX (EFER.NXE) nas nossas tabelas de páginas | executar dados | ATIVA | 3 |
| W^X em todo o kernel com paginação própria, garantido na API e auditado no arranque (D-032) | executar dados, alterar código | ATIVA | 3 |
| Alias da imagem do kernel no HHDM só de leitura (D-029) | reescrever `.text` pelo mapa direto | ATIVA | 3 |
| Sem remapeamentos silenciosos; bit user recusado na metade do kernel | trocar um mapeamento existente por engano | ATIVA | 3 |
| `map_mmio` recusa RAM/firmware (D-033) | alias MMIO de memória do kernel | ATIVA | 3 |
| CR0.WP (ring 0 respeita páginas só de leitura) | o kernel escrever em `.text`/`.rodata` | ATIVA | 3 |
| Páginas de guarda em todas as pilhas (D-030) | stack overflow silencioso | ATIVA | 3 |
| Frames libertados limpos a zero; frames entregues sempre a zeros | fuga de dados entre processos | ATIVA | 3 |
| Lista livre do PMM fora de banda (D-028) | use-after-free corromper o alocador de frames | ATIVA | 3 |
| Deteção de double free e overflow de referências no PMM | dois donos para a mesma memória | ATIVA | 3 |
| Heap: pedaços limpos ao libertar, links codificados e validados, deteção de double free (D-035) | fuga de dados antigos; use-after-free a corromper o alocador | ATIVA | 3 |
| Heap: página de guarda depois de cada alocação grande | overflow de um buffer grande a corromper outro | ATIVA | 3 |
| Pilhas das threads do kernel com página de guarda e espaço por mapear por cima; frames limpos ao libertar (D-045) | stack overflow silencioso entre threads; fuga de dados de uma thread morta | ATIVA | 5 |
| Sem preempção com `SpinLock` preso; troca só depois do EOI (D-045) | deadlocks e IRQs presos por preempção no sítio errado | ATIVA | 5 |
| Locks que desativam interrupções em todos os globais (D-037) | deadlocks em handlers de interrupção | ATIVA | 4 |
| `cli`/`sti`/`hlt` como barreiras de memória do compilador (D-037) | acessos movidos para fora de secções críticas | ATIVA | 4 |
| PIC remapeado para 0x20–0x2F e mascarado por omissão (D-039) | IRQs confundidos com exceções (0x08 = double fault); interrupções de dispositivos não pedidas | ATIVA | 4 |
| EOI enviado só pelo núcleo; IRQs espúrios filtrados (D-039) | controlador bloqueado por EOI esquecido / EOI a mais | ATIVA | 4 |
| Drivers `forbid(unsafe_code)`; portas legadas por allow-list e reclamadas uma vez (D-038) | um driver tocar em portas/memória que não são suas | ATIVA | 4 |
| Tabelas ACPI copiadas uma vez e validadas na cópia; só memória de firmware por allow-list (D-041) | firmware malformado, TOCTOU, leituras de MMIO | ATIVA | 4 |
| Capacidades APIC: base do MSR, I/O APIC só se declarado na MADT, dono único, UC/NX (D-042) | driver mapear/aceder a MMIO arbitrário | ATIVA | 4 |
| ICR do local APIC fechado; allow-list de modos de entrega e vetores 0x20–0xEF em LVTs e redireções; seletor do I/O APIC sob lock e reposto (D-042) | driver "seguro" a acordar outros CPUs com código arbitrário, resetá-los, ou entregar IRQs em vetores de exceção | ATIVA | 4 |
| Módulo initrd: marca exata, único, tamanho limitado, todo dentro de memória de módulos, só de leitura e NX (D-043) | initrd falso/ambíguo, slice para fora do módulo, escrita no initrd | ATIVA | 7 |
| Comandos do i8042 por allow-list; bit NMI-disable do CMOS sempre limpo (D-038) | reset da máquina, A20 desligado (corrupção de memória) ou NMIs desligados por um driver "seguro" | ATIVA | 4 |
| SMEP (CR4.20, se suportado) | o kernel executar código de user ("ret2user") | ATIVA (provado por `selftest-smep`) | 6 |
| SMAP (CR4.21, se suportado); janela `stac`/`clac` de uma instrução; `clac` à entrada do dispatch | o kernel ler/escrever memória de user sem querer | ATIVA (provado por `selftest-smap`) | 6 |
| UMIP (CR4.11, se suportado) | user space ler GDT/IDT (`sgdt`/`sidt`) e descobrir endereços | ATIVA (prova em ring 3 virá com a Fase 6b) | 6 |
| Validação de todos os argumentos das syscalls | ataques via syscall | PLANEADA | 6 |
| `UserSlice`/`UserPtr` + `copy_from_user`/`copy_to_user`; #PF na cópia → `Fault` (nunca panic), só se o CR2 for de user e sem `RSVD` | TOCTOU, ponteiros para o kernel, crash por ponteiro mau, corrupção mascarada | ATIVA | 6 |
| Invariante: a metade de baixo só tem páginas U (D-054) | `copy_*_user` alcançar páginas de supervisor mapeadas em baixo | REGRA (auditoria na 6b) | 6 |
| Espaços de endereçamento separados por processo | um processo ler outro | PLANEADA | 6 |
| Página nula nunca mapeada | null deref explorável | PLANEADA | 6 |
| RNG do kernel (RDRAND/RDSEED + entropia) | aleatoriedade previsível | PARCIAL (palavra de hardware no heap quando disponível) | 8 |
| KASLR | saber onde está o kernel | PLANEADA | 8 |
| ASLR em user space | ROP com endereços fixos | PLANEADA | 8 |
| Objetos do kernel só por handle com direitos (D-018, D-051) | acesso a buffers/canais que ninguém nos deu | PLANEADA | 6/12 |
| Sem autoridade ambiente: syscalls "capability-first", sem nomes/caminhos globais, processo novo começa sem handles (D-055, D-058) | apps que acedem a tudo o que o utilizador pode; "confused deputy" | PLANEADA | 6 |
| Direitos só diminuem (`dup`, transferência por canal); handles com geração | aumentar direitos; usar um handle reciclado | PLANEADA | 6/12 |
| Filas de IPC limitadas | DoS da memória do kernel por user space | PLANEADA | 12 |
| Compositor em user space (D-017) | bug no compositor comprometer o kernel | PLANEADA | 13 |
| Isolamento entre janelas (ver secção abaixo) | ler o ecrã, o input ou os buffers de outra aplicação | PLANEADA | 10–13 |
| UIDs/GIDs e permissões em ficheiros e processos; sem "root" que salte as verificações (o admin é um conjunto de handles, D-055) | um utilizador ler ou alterar dados de outro; utilizador → admin | PLANEADA | 15 |
| Registo de auditoria no kernel: só acrescentar, lido só com o handle `AUDIT`, limite por processo, contador de eventos perdidos, sem endereços do kernel | apagar rastos; encher o registo para esconder um ataque | PLANEADA | 15 |
| Login: passwords guardadas só como Argon2id com sal; tentativas limitadas e auditadas | roubo de passwords; força bruta | PLANEADA | 15 |
| Crypto própria com vetores oficiais (RFC 8439, RFC 8032, FIPS 180-4, RFC 9106) e tempo constante para segredos (D-056) | erros de implementação; ataques de timing | PLANEADA (educativa, não auditada) | 16 |
| CSPRNG (ChaCha20) semeado por RDSEED/RDRAND + entropia; recusa arrancar sem entropia mínima | chaves, nonces e ASLR previsíveis | PLANEADA | 16 |
| ELF loader só aceita programas com assinatura Ed25519 válida; o modo de desenvolvimento é explícito e anunciado no arranque | executar programas alterados ou não confiáveis | PLANEADA | 16 |
| Disco encriptado (ChaCha20-Poly1305, nonce único por bloco, chave por Argon2id) | ler ou alterar o disco com a máquina desligada | PLANEADA | 16 |
| Firewall no kernel, entrada nega tudo por defeito, regras por processo/porta/endereço (D-057) | serviços expostos sem querer; apps a falar com a rede sem a capacidade | PLANEADA | 17 |
| IDS em user space: só pode apertar regras (nunca abrir portas) | scans e floods não detetados; IDS comprometido a abrir o firewall | PLANEADA | 17 |
| Stack de rede `forbid(unsafe_code)`, pacotes tratados como não confiáveis, filas limitadas | corrupção de memória por pacotes; DoS por flood | PLANEADA | 17 |
| Pacotes `kpkg` assinados (Ed25519) com manifesto de capacidades aprovado pelo utilizador | instalar pacotes alterados; apps com mais acesso do que o declarado | PLANEADA | 18 |

## Isolamento entre janelas (Fases 10–13)

Objetivo: uma aplicação gráfica maliciosa não consegue ver nem influenciar
outra aplicação, nem enganar o utilizador.

| Regra | Como é garantida |
|---|---|
| Cada aplicação desenha só no **seu** buffer | o buffer é um objeto do kernel; a aplicação tem o handle com `WRITE`, o compositor recebe uma cópia do handle só com `READ`/`MAP`. Mais ninguém tem handle. |
| Nenhuma aplicação lê o ecrã nem buffers de outras | só o compositor tem a capacidade `DISPLAY` (mapear o framebuffer); não existe syscall de "ler o ecrã". Capturas de ecrã só pelo compositor, com ação explícita do utilizador. |
| Input só chega à janela com foco | só o compositor tem `INPUT` (D-019) e encaminha cada evento para uma única janela. Não há input global para aplicações (anti-keylogger). |
| O foco não pode ser roubado | só o compositor muda o foco, como resposta a ações do utilizador (clique, atalho), nunca a pedido livre de uma aplicação. |
| Secure attention key | uma combinação de teclas é sempre tratada pelo compositor e nunca entregue a aplicações: o utilizador tem sempre forma de chegar a UI confiável. |
| Sem janelas impostoras / clickjacking | decorações (título, botões) desenhadas pelo compositor, não pela aplicação; janelas transparentes/sobrepostas não recebem cliques destinados a outras. |
| O compositor trata buffers como não confiáveis | os píxeis são só copiados/compostos, nunca interpretados; estruturas lidas de memória partilhada são copiadas primeiro e validadas (contra double fetch/TOCTOU); tamanhos vêm do kernel, não da aplicação. |
| Recursos libertados ao morrer | quando um processo morre o kernel fecha os seus handles; os frames só são reutilizados (e limpos a zero) quando a última referência desaparece. |
| Clipboard e arrastar/largar | passam pelo compositor e só por ação explícita do utilizador; a aplicação de destino recebe uma cópia. |

## Regras permanentes
- Nunca imprimir endereços do kernel em output visível ao user space.
- Nunca pôr segredos no repositório (é público).
- Todo o input externo é validado; erros com `Result`, nunca `unwrap`.
- Novas proteções ou mudanças de estado: atualizar esta tabela no mesmo PR.
