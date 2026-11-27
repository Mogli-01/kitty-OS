# Roadmap

Estados: **TODO** · **EM CURSO** · **FEITO**. Dono: **CLAUDE** ou **CODEX**.
Cada um atualiza só as suas tarefas (no mesmo PR em que as faz).
As tarefas marcadas **GUI-PREP** existem para que o núcleo já nasça
preparado para a interface gráfica (Fases 10–14). As tarefas marcadas
**CAPS** fazem com que as syscalls e o IPC usem handles de capacidades
desde o início, para a Fase 15 não obrigar a refazê-los (D-058).

**Processo contínuo:** a partir da Fase 6, cada fase concluída acaba com uma
ronda de red team vs blue team (`docs/REDTEAM.md`).

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
| Descoberta ACPI (RSDP → MADT validada e copiada) + capacidades APIC filtradas (#18) | CLAUDE | FEITO |
| Driver do Local APIC + IO APIC | CODEX | TODO |
| Driver do timer (APIC timer / HPET) → chama `sched::tick()` | CODEX | TODO |
| Driver do teclado PS/2 | CODEX | TODO |
| Driver UART completo (interrupções, input) | CODEX | TODO |

## Fase 5 — Scheduler
| Tarefa | Dono | Estado |
|---|---|---|
| Threads do kernel + troca de contexto mínima em assembly | CLAUDE | FEITO |
| Scheduler preemptivo round-robin | CLAUDE | FEITO |
| Prioridades no scheduler | CLAUDE | TODO |
| DESEMPENHO: medir a troca de contexto em ciclos (rdtsc) | CLAUDE | FEITO (autoteste no arranque) |
| SEGURANÇA: pilhas de thread com página de guarda, limpas ao libertar; sem preempção com spinlock preso | CLAUDE | FEITO |
| GUI-PREP: filas de espera (bloquear/acordar threads) — base do IPC e dos eventos de input | CLAUDE | FEITO |
| Benchmarks automáticos (troca de contexto, syscalls) | CODEX | TODO |

## Fase 6 — User mode
| Tarefa | Dono | Estado |
|---|---|---|
| `syscall`/`sysret`, tabela de syscalls, validação de TODOS os argumentos | CLAUDE | TODO |
| `UserPtr<T>`, `copy_from_user` / `copy_to_user` | CLAUDE | TODO |
| Espaços de endereçamento separados por processo | CLAUDE | TODO |
| ELF loader | CLAUDE | TODO |
| GUI-PREP/CAPS: objetos do kernel + tabela de handles por processo, com direitos (D-018, D-051) | CLAUDE | TODO |
| CAPS: ABI "capability-first": toda a syscall com efeito fora do próprio processo recebe um handle; sem nomes nem caminhos globais; bits de direitos 16–31 reservados por tipo de objeto; `E_BADSIG` reservado (D-058) | CLAUDE | TODO |
| CAPS: `process_spawn(imagem, handles[])`, a única forma de dar autoridade a um processo novo (por defeito, nenhuma) | CLAUDE | TODO |
| PREP-15: campos `uid`/`gid` no processo e pontos de auditoria nas syscalls sensíveis (sem efeito até à Fase 15) | CLAUDE | TODO |
| SEGURANÇA: SMEP, SMAP, UMIP ativos | CLAUDE | TODO |
| Biblioteca de syscalls para userland | CODEX | TODO |
| Fuzzing das syscalls | CODEX | TODO |
| Ronda 1 de red team vs blue team (`docs/REDTEAM.md`) | CLAUDE + CODEX | TODO |

## Fase 7 — Sistema de ficheiros e shell
| Tarefa | Dono | Estado |
|---|---|---|
| VFS | CODEX | TODO |
| initrd (módulo do Limine) | CODEX | TODO |
| Shell em userland | CODEX | TODO |
| Entrega do módulo initrd ao VFS (`Platform::initrd`, validado, só de leitura) (#22) | CLAUDE | FEITO |
| Syscalls de ficheiros no núcleo: `open_at(dir_handle, caminho, direitos)`, read/write/close; sem caminhos absolutos globais (D-058) | CLAUDE | TODO |

## Fase 8 — Aleatoriedade e permissões
| Tarefa | Dono | Estado |
|---|---|---|
| Gerador de números aleatórios do kernel (RDRAND/RDSEED + entropia); é a base do CSPRNG da Fase 16 | CLAUDE | TODO |
| KASLR (kernel PIE + deslocamento do Limine) | CLAUDE | TODO |
| ASLR em user space | CLAUDE | TODO |
| Modelo de capacidades/permissões por processo | CLAUDE | MOVIDA (base na Fase 6, D-058; utilizadores e permissões na Fase 15) |

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
| Objeto `DISPLAY`: `display_map(h)` exige um handle com `MAP`, que o `init` dá só ao compositor; a consola do kernel larga o ecrã (D-058) | CLAUDE | TODO |
| Identidade "kitty": ecrã de arranque com o logótipo do gato e uma animação simples | CODEX | TODO |

## Fase 11 — Input
| Tarefa | Dono | Estado |
|---|---|---|
| Driver de rato PS/2 | CODEX | TODO |
| Driver USB HID (teclado e rato) | CODEX | TODO |
| Sistema de eventos de input unificado (`InputEvent`: teclado + rato) | CODEX | TODO |
| Entrega dos eventos ao user space só a quem tem o handle do objeto `INPUT`, dado pelo `init` (sem `input_open` por nome; D-019, D-058) | CLAUDE | TODO |

## Fase 12 — Memória partilhada e IPC rápido
| Tarefa | Dono | Estado |
|---|---|---|
| Syscalls de buffers partilhados: criar, mapear, partilhar por handle, com direitos (READ/WRITE/MAP/TRANSFER) | CLAUDE | TODO |
| Canais de mensagens (mensagens pequenas copiadas + transferência de handles: o handle sai do emissor, exige `TRANSFER`, e os direitos só podem diminuir; D-058) | CLAUDE | TODO |
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
| Identidade "kitty": tema de cores, ícones e cursor próprios; mascote no ambiente de trabalho; comando `meow` e outros easter eggs | CODEX | TODO |
| Jogos feitos de raiz: snake e tetris (servem também para testar gráficos, input e desempenho) | CODEX | TODO |

> **Gráficos da identidade "kitty":** todos originais, criados por nós
> (logótipo, ícones, cursor, mascote, jogos). Nada de imagens, fontes ou
> sons com direitos de autor.

## Fase 15 — Utilizadores, permissões e sandbox
| Tarefa | Dono | Estado |
|---|---|---|
| Multiutilizador no kernel: UIDs/GIDs, permissões em ficheiros e processos (decidem que handles uma sessão recebe; D-055) | CLAUDE | TODO |
| Modelo de capacidades completo: cada processo só tem os handles que lhe foram dados (ficheiros, rede, ecrã, áudio); por defeito não tem nenhum | CLAUDE | TODO |
| Registo de auditoria no kernel: abrir ficheiros protegidos, pedir rede, mudar permissões, falhas de autenticação; buffer protegido, lido só por quem tem o handle `AUDIT` (a conta de administrador) | CLAUDE | TODO |
| Ecrã de login gráfico, gestão de contas, aplicação para ver o registo de auditoria | CODEX | TODO |
| O compositor e o VFS respeitam as capacidades (uma app sem a capacidade "ecrã" não abre janelas) | CODEX | TODO |

## Fase 16 — Criptografia, arranque verificado e disco encriptado
| Tarefa | Dono | Estado |
|---|---|---|
| Biblioteca de criptografia própria (`kernel/src/crypto`, educativa e não auditada, D-056): SHA-256, ChaCha20-Poly1305, Ed25519, Argon2id, com os vetores de teste oficiais | CLAUDE | TODO |
| CSPRNG sobre RDRAND/RDSEED + entropia (evolui o RNG da Fase 8) | CLAUDE | TODO |
| Binários assinados: o ELF loader recusa programas sem uma assinatura Ed25519 válida (`E_BADSIG`), com um modo de desenvolvimento explícito para testes | CLAUDE | TODO |
| Ferramenta no xtask para gerar chaves e assinar o kernel e os programas (a chave privada nunca entra no repositório) | CODEX | TODO |
| Camada de disco encriptado no sistema de ficheiros: password pedida no arranque, chave derivada com Argon2id | CODEX | TODO |

## Fase 17 — Rede, firewall e IDS
| Tarefa | Dono | Estado |
|---|---|---|
| Driver virtio-net e stack de rede própria: Ethernet, ARP, IPv4, ICMP, UDP, TCP, DHCP e DNS | CODEX | TODO |
| Firewall no kernel: regras por processo, porta e endereço; negar tudo por defeito na entrada (D-057) | CLAUDE | TODO |
| IDS em user space, ligado ao firewall: deteção de scans de portas, floods e padrões suspeitos, com alertas na interface gráfica e no registo de auditoria | CODEX | TODO |
| Mini browser só de texto (HTTP primeiro; HTTPS só quando houver TLS) | CODEX | TODO |

## Fase 18 — Som e gestor de pacotes
| Tarefa | Dono | Estado |
|---|---|---|
| Driver Intel HDA (QEMU: `-device intel-hda`), sons do sistema e um leitor de música simples | CODEX | TODO |
| Gestor de pacotes `kpkg` (install, remove, list, update): pacotes assinados e verificados com Ed25519; as capacidades de cada pacote vêm num manifesto que o utilizador aprova na instalação | CODEX | TODO |

## Fase 19 — Hardware real
**Objetivo:** arrancar a partir de uma pen USB num portátil real (ThinkPad
T14 Gen 1 AMD: Ryzen, UEFI).

| Tarefa | Dono | Estado |
|---|---|---|
| ACPI (MADT, FADT; desligar e reiniciar), arranque UEFI pelo Limine, temporizadores HPET/TSC | CLAUDE | TODO |
| Drivers xHCI (USB), teclado e rato USB HID, NVMe, framebuffer GOP | CODEX | TODO |
| Guia em `docs/REAL_HARDWARE.md` para gravar a ISO numa pen e testar | CODEX | TODO |
