# Roadmap

Estados: **TODO** · **EM CURSO** · **FEITO**. Dono: **CLAUDE** ou **CODEX**.
Cada um atualiza só as suas tarefas (no mesmo PR em que as faz).

## Fase 0 — Organização do projeto
| Tarefa | Dono | Estado |
|---|---|---|
| Estrutura do workspace, toolchain, perfis de compilação | CLAUDE | FEITO |
| Ficheiros de coordenação (CLAUDE.md, AGENTS.md, docs/…) | CLAUDE | FEITO |
| `cargo xtask` (build, esp, iso, run, test) | CLAUDE | FEITO |
| CI inicial (fmt, clippy, arranque UEFI/BIOS/release) | CLAUDE → CODEX | FEITO (dono passa a CODEX) |
| Etiquetas `para-claude` / `para-codex` no GitHub | CLAUDE | FEITO |

## Fase 1 — Arranque e output
| Tarefa | Dono | Estado |
|---|---|---|
| Arranque via Limine (BIOS + UEFI), W^X por segmento | CLAUDE | FEITO |
| Porta série inicial (COM1, polling) | CLAUDE | FEITO |
| Consola de texto no framebuffer | CLAUDE | FEITO |
| Panic handler com localização | CLAUDE | FEITO |
| Teste de arranque no QEMU (isa-debug-exit) | CLAUDE → CODEX | FEITO (mínimo) |
| Testes de integração mais completos (ex.: teste de panic) | CODEX | TODO |

## Fase 2 — CPU: GDT, TSS, IDT, exceções
| Tarefa | Dono | Estado |
|---|---|---|
| GDT própria + TSS com pilhas IST (double fault, NMI, machine check) | CLAUDE | TODO |
| IDT + handlers de exceções com relatório útil (registos, código de erro) | CLAUDE | TODO |
| Testes: provocar #BP, #PF, double fault e verificar o relatório | CODEX | TODO |

## Fase 3 — Memória
| Tarefa | Dono | Estado |
|---|---|---|
| PMM (bitmap/lista livre a partir do memory map do Limine) | CLAUDE | TODO |
| Paginação própria (tabelas de 4 níveis), mapear kernel e HHDM | CLAUDE | TODO |
| Heap do kernel (`#[global_allocator]`) | CLAUDE | TODO |
| SEGURANÇA: NX (EFER.NXE), W^X em todo o kernel, CR0.WP | CLAUDE | TODO |
| SEGURANÇA: páginas de guarda em todas as pilhas | CLAUDE | TODO |
| SEGURANÇA: frames libertados limpos a zero | CLAUDE | TODO |
| Registo dinâmico de consolas (`console::register`) | CLAUDE | TODO |
| Testes e fuzzing do allocator | CODEX | TODO |

## Fase 4 — Interrupções de hardware
| Tarefa | Dono | Estado |
|---|---|---|
| Spinlocks que desativam interrupções (`IrqSpinLock`) | CLAUDE | TODO |
| API de registo de handlers de IRQ | CLAUDE | TODO |
| Driver do Local APIC + IO APIC | CODEX | TODO |
| Driver do timer (APIC timer / HPET) → chama `sched::tick()` | CODEX | TODO |
| Driver do teclado PS/2 | CODEX | TODO |
| Driver UART completo (interrupções, input) | CODEX | TODO |

## Fase 5 — Scheduler
| Tarefa | Dono | Estado |
|---|---|---|
| Threads do kernel + troca de contexto mínima em assembly | CLAUDE | TODO |
| Scheduler preemptivo round-robin, depois prioridades | CLAUDE | TODO |
| DESEMPENHO: medir a troca de contexto em ciclos (rdtsc) | CLAUDE | TODO |
| Benchmarks automáticos (troca de contexto, syscalls) | CODEX | TODO |

## Fase 6 — User mode
| Tarefa | Dono | Estado |
|---|---|---|
| `syscall`/`sysret`, tabela de syscalls, validação de TODOS os argumentos | CLAUDE | TODO |
| `UserPtr<T>`, `copy_from_user` / `copy_to_user` | CLAUDE | TODO |
| Espaços de endereçamento separados por processo | CLAUDE | TODO |
| ELF loader | CLAUDE | TODO |
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
