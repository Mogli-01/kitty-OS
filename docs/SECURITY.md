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
| W^X por segmento do ELF (R / RX / R / RW) | executar dados, alterar código | ATIVA (mapeamento do Limine) | 1 |
| Requests do Limine só de leitura | redirecionar respostas do bootloader | ATIVA | 1 |
| Kernel sem SSE, sem red zone | corrupção da pilha por interrupções; fuga de estado vetorial | ATIVA | 1 |
| Panic sem endereços (só ficheiro:linha) | fuga de layout do kernel | ATIVA | 1 |
| Porta de saída do QEMU só em builds de teste | escrita em porta indevida em hardware real | ATIVA | 1 |
| Limine e GitHub Actions fixados por commit | ataques à cadeia de fornecimento | ATIVA | 1 |
| CI com `permissions: contents: read` | abuso do token da CI | ATIVA | 1 |
| Pilhas IST para double fault / NMI / #MC | triple fault por pilha corrompida | PLANEADA | 2 |
| NX (EFER.NXE) nas nossas tabelas de páginas | executar dados | PLANEADA (hoje via Limine) | 3 |
| W^X em todo o kernel com paginação própria | executar dados, alterar código | PLANEADA | 3 |
| CR0.WP (ring 0 respeita páginas só de leitura) | o kernel escrever em `.text`/`.rodata` | PLANEADA | 3 |
| Páginas de guarda em todas as pilhas | stack overflow silencioso | PLANEADA | 3 |
| Frames libertados limpos a zero | fuga de dados entre processos | PLANEADA | 3 |
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

## Regras permanentes
- Nunca imprimir endereços do kernel em output visível ao user space.
- Nunca pôr segredos no repositório (é público).
- Todo o input externo é validado; erros com `Result`, nunca `unwrap`.
- Novas proteções ou mudanças de estado: atualizar esta tabela no mesmo PR.
