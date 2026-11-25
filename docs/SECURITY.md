# Segurança do kitty-OS

## Modelo de ameaça

**O que protegemos:** a integridade e a confidencialidade do kernel, e o
isolamento entre processos.

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

**Fora de âmbito (por agora):** ataques físicos (DMA por Thunderbolt,
cold boot), firmware comprometido, canais laterais micro-arquiteturais
(Spectre/Meltdown — revisitar depois da Fase 6), Secure Boot.

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
| Locks que desativam interrupções | deadlocks em handlers | PLANEADA | 4 |
| SMEP | o kernel executar código de user | PLANEADA | 6 |
| SMAP | o kernel ler/escrever memória de user sem querer | PLANEADA | 6 |
| UMIP | user space ler GDT/IDT (`sgdt`/`sidt`) e descobrir endereços | PLANEADA | 6 |
| Validação de todos os argumentos das syscalls | ataques via syscall | PLANEADA | 6 |
| `UserPtr<T>` + `copy_from_user`/`copy_to_user` | TOCTOU e ponteiros para o kernel | PLANEADA | 6 |
| Espaços de endereçamento separados por processo | um processo ler outro | PLANEADA | 6 |
| Página nula nunca mapeada | null deref explorável | PLANEADA | 6 |
| RNG do kernel (RDRAND/RDSEED + entropia) | aleatoriedade previsível | PLANEADA | 8 |
| KASLR | saber onde está o kernel | PLANEADA | 8 |
| ASLR em user space | ROP com endereços fixos | PLANEADA | 8 |
| Capacidades / permissões por processo | escalada de privilégios | PLANEADA | 8 |
| Objetos do kernel só por handle com direitos (D-018) | acesso a buffers/canais que ninguém nos deu | PLANEADA | 6/12 |
| Filas de IPC limitadas | DoS da memória do kernel por user space | PLANEADA | 12 |
| Compositor em user space (D-017) | bug no compositor comprometer o kernel | PLANEADA | 13 |
| Isolamento entre janelas (ver secção abaixo) | ler o ecrã, o input ou os buffers de outra aplicação | PLANEADA | 10–13 |

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
