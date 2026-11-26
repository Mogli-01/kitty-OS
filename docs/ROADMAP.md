# Roadmap

Estados: **TODO** · **EM CURSO** · **FEITO**. Dono: **CLAUDE** ou **CODEX**.
Cada um atualiza só as suas tarefas (no mesmo PR em que as faz).
As tarefas marcadas **GUI-PREP** existem para que o núcleo já nasça
preparado para a interface gráfica (Fases 10–14).

## Fase 0 — Organização do projeto
| Tarefa | Dono | Estado |
|---|---|---|
| Estrutura do workspace, toolchain, perfis de compilação | CLAUDE | FEITO |
| Ficheiros de coordenação (CLAUDE.md, AGENTS.md, docs/…) | CLAUDE | FEITO |
| `cargo xtask` (build, esp, iso, run, test) | CLAUDE | FEITO |
| CI inicial (fmt, clippy, arranque UEFI/BIOS/release) | CLAUDE → CODEX | FEITO (dono passa a CODEX) |
| Etiquetas `para-claude` / `para-codex` no GitHub | CLAUDE | FEITO |
| Subagentes do Claude Code (`security-reviewer`, `test-writer`, `explorer`) | CLAUDE | FEITO |

## Fase 1 — Arranque e output
| Tarefa | Dono | Estado |
|---|---|---|
| Arranque via Limine (BIOS + UEFI), W^X por segmento | CLAUDE | FEITO |
| Porta série inicial (COM1, polling) | CLAUDE | FEITO |
| Consola de texto no framebuffer | CLAUDE | FEITO |
| Panic handler com localização | CLAUDE | FEITO |
| Teste de arranque no QEMU (isa-debug-exit) | CLAUDE → CODEX | FEITO (mínimo) |
| Teste de integração de panic no QEMU (código de saída e série) | CODEX | FEITO (CI da Fase 2) |

## Fase 2 — CPU: GDT, TSS, IDT, exceções
| Tarefa | Dono | Estado |
|---|---|---|
| GDT própria + TSS com pilhas IST (double fault, NMI, machine check) | CLAUDE | FEITO |
| IDT + handlers de exceções com relatório útil (registos, código de erro) | CLAUDE | FEITO |
| Autotestes no kernel (`selftest-page-fault`, `selftest-double-fault`) + `xtask --features/--expect-panic` | CLAUDE | FEITO |
| Testes no QEMU: #BP, #PF e #DF com verificação do relatório (UEFI/BIOS, debug/release) | CODEX | FEITO |

## Fase 3 — Memória
| Tarefa | Dono | Estado |
|---|---|---|
| PMM (base de dados de frames + lista livre fora de banda, a partir do memory map do Limine) | CLAUDE | FEITO |
| Paginação própria (tabelas de 4 níveis), mapear kernel e HHDM | CLAUDE | FEITO |
| Heap do kernel (`#[global_allocator]`): classes de tamanho + páginas com guarda | CLAUDE | FEITO |
| SEGURANÇA: NX (EFER.NXE), W^X em todo o kernel, CR0.WP | CLAUDE | FEITO |
| SEGURANÇA: GDT/IDT só de leitura, alias do kernel no HHDM só de leitura, auditoria W^X no arranque | CLAUDE | FEITO |
| SEGURANÇA: páginas de guarda em todas as pilhas | CLAUDE | FEITO |
| SEGURANÇA: frames libertados limpos a zero | CLAUDE | FEITO |
| Registo dinâmico de consolas (`console::register`) | CLAUDE | FEITO |
| `xtask test --expect <texto>` e autotestes de falhas de memória | CLAUDE | FEITO |
| GUI-PREP: PMM com contagem de referências por frame (frames partilhados entre espaços de endereçamento; limpo a zero só quando a última referência sai) | CLAUDE | FEITO |
| GUI-PREP: tipos de cache no VMM via PAT (WB, WC, UC) — `map_mmio(…, CacheType::WriteCombining)` | CLAUDE | FEITO |
| Testes QEMU do PMM, heap, W^X, NX, WP, páginas de guarda, IDT e MMIO | CODEX | FEITO |
| Testes e fuzzing do allocator no host | CODEX | TODO |

## Fase 4 — Interrupções de hardware
| Tarefa | Dono | Estado |
|---|---|---|
| Spinlocks que desativam interrupções (`IrqSpinLock`); todos os locks globais convertidos | CLAUDE | FEITO |
| API de registo de handlers de IRQ (`irq::register_isa`, EOI pelo núcleo, trait `InterruptController`) | CLAUDE | FEITO |
| PIC 8259 como controlador de arranque (remapeado, tudo mascarado) | CLAUDE | FEITO |
| `kitty-hal` + portas legadas reclamáveis (i8042, PIT, CMOS) + `devices::init` | CLAUDE | FEITO |
| Driver do Local APIC + IO APIC | CODEX | Drivers de registos em revisão (#21 e `codex/local-apic`). Transição do PIC depende das capacidades e da validação de MMIO (#18, #35) |
| Driver do timer (APIC timer / HPET) → chama `sched::tick()` | CODEX | Timer local one-shot e periódico validado com o IRQ mascarado em QEMU. Faltam calibração, entrega ao handler e ligação ao scheduler |
| Driver do teclado PS/2 | CODEX | TODO |
| Driver UART completo (interrupções, input) | CODEX | TODO |

## Fase 5 — Scheduler
| Tarefa | Dono | Estado |
|---|---|---|
| Threads do kernel + troca de contexto mínima em assembly | CLAUDE | TODO |
| Scheduler preemptivo round-robin, depois prioridades | CLAUDE | TODO |
| DESEMPENHO: medir a troca de contexto em ciclos (rdtsc) | CLAUDE | TODO |
| GUI-PREP: filas de espera (bloquear/acordar threads) — base do IPC e dos eventos de input | CLAUDE | TODO |
| Benchmarks automáticos (troca de contexto, syscalls) | CODEX | TODO |

## Fase 6 — User mode
| Tarefa | Dono | Estado |
|---|---|---|
| `syscall`/`sysret`, tabela de syscalls, validação de TODOS os argumentos | CLAUDE | TODO |
| `UserPtr<T>`, `copy_from_user` / `copy_to_user` | CLAUDE | TODO |
| Espaços de endereçamento separados por processo | CLAUDE | TODO |
| ELF loader | CLAUDE | TODO |
| GUI-PREP: objetos do kernel + tabela de handles por processo, com direitos (D-018) | CLAUDE | TODO |
| SEGURANÇA: SMEP, SMAP, UMIP ativos | CLAUDE | TODO |
| Biblioteca de syscalls para userland | CODEX | TODO |
| Fuzzing das syscalls | CODEX | TODO |

## Fase 7 — Sistema de ficheiros e shell
| Tarefa | Dono | Estado |
|---|---|---|
| VFS | CODEX | TODO |
| initrd (módulo do Limine) | CODEX | TODO |
| Shell em userland | CODEX | TODO |
| Syscalls de ficheiros (open/read/write/close) no núcleo | CLAUDE | TODO |

## Fase 8 — Aleatoriedade e permissões
| Tarefa | Dono | Estado |
|---|---|---|
| Gerador de números aleatórios do kernel (RDRAND/RDSEED + entropia) | CLAUDE | TODO |
| KASLR (kernel PIE + deslocamento do Limine) | CLAUDE | TODO |
| ASLR em user space | CLAUDE | TODO |
| Modelo de capacidades/permissões por processo | CLAUDE | TODO |

## Fase 9 — Escala
| Tarefa | Dono | Estado |
|---|---|---|
| SMP (vários núcleos), locks por CPU | CLAUDE | TODO |
| Sistema de ficheiros em disco | CODEX | TODO |
| Mais drivers (AHCI/NVMe, rede) | CODEX | TODO |

## Fase 10 — Gráficos base
| Tarefa | Dono | Estado |
|---|---|---|
| Biblioteca gráfica em user space: píxeis, retângulos, linhas, blit | CODEX | TODO |
| Fontes bitmap, depois PSF e TrueType simples | CODEX | TODO |
| Double buffering | CODEX | TODO |
| Framebuffer mapeado com write-combining (usa a API de PAT da Fase 3) | CODEX | TODO |
| Capacidade `DISPLAY`: syscall para mapear o framebuffer só no processo que a tem; a consola do kernel larga o ecrã | CLAUDE | TODO |

## Fase 11 — Input
| Tarefa | Dono | Estado |
|---|---|---|
| Driver de rato PS/2 | CODEX | TODO |
| Driver USB HID (teclado e rato) | CODEX | TODO |
| Sistema de eventos de input unificado (`InputEvent`: teclado + rato) | CODEX | TODO |
| Entrega dos eventos ao user space só ao processo com a capacidade `INPUT` (D-019) | CLAUDE | TODO |

## Fase 12 — Memória partilhada e IPC rápido
| Tarefa | Dono | Estado |
|---|---|---|
| Syscalls de buffers partilhados: criar, mapear, partilhar por handle, com direitos (READ/WRITE/MAP/TRANSFER) | CLAUDE | TODO |
| Canais de mensagens (mensagens pequenas copiadas + transferência de handles) | CLAUDE | TODO |
| DESEMPENHO: medir a latência de ida e volta de uma mensagem em ciclos | CLAUDE | TODO |
| Fuzzing das syscalls de memória partilhada e canais | CODEX | TODO |

## Fase 13 — Compositor / gestor de janelas (user space)
| Tarefa | Dono | Estado |
|---|---|---|
| Protocolo compositor ↔ aplicações (mensagens, buffers, eventos) em `docs/INTERFACES.md` | CLAUDE | TODO |
| Foco, encaminhamento do input e "secure attention key" (combinação de teclas que só o compositor vê) | CLAUDE | TODO |
| Composição das janelas, cursor, cliques | CODEX | TODO |
| Redesenhar só as zonas alteradas (dirty rectangles) | CODEX | TODO |
| Revisão de segurança do compositor (isolamento entre janelas, ver SECURITY.md) | CLAUDE | TODO |

## Fase 14 — Toolkit e aplicações
| Tarefa | Dono | Estado |
|---|---|---|
| Toolkit: botões, caixas de texto, menus | CODEX | TODO |
| Terminal gráfico | CODEX | TODO |
| Explorador de ficheiros | CODEX | TODO |
| Ambiente de trabalho com barra de tarefas e tema "kitty" | CODEX | TODO |
