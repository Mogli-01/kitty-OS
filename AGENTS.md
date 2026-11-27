# AGENTS.md — regras do Codex no kitty-OS

> Versão inicial escrita pelo Claude no PR de arranque. **A partir daqui o
> dono deste ficheiro é o Codex**: o Claude não o edita.

**No início de CADA sessão:** `git pull` do `main`, ler `docs/HANDOFF.md`,
`docs/ROADMAP.md`, `docs/INTERFACES.md` e os PRs/issues abertos (sobretudo
com a etiqueta `para-codex`).

## Papel do Codex
Dono de: **drivers** (`drivers/`: APIC, timer, teclado, UART completo, …),
**ligação dos drivers ao kernel** (`kernel/src/devices/`, em Rust seguro),
**VFS/initrd** (`fs/`), **shell e userland** (`userland/`), **testes**
(`tests/`, `cargo xtask test`), **fuzzing**, **CI** (`.github/workflows/`),
**benchmarks**, **documentação de utilizador**, **rede** (`net/`),
**áudio** (`audio/`), **gestão de pacotes** (`pkg/`, kpkg), **IDS** (`ids/`),
aplicações de login e auditoria e a identidade kitty (ecrã de arranque,
tema, ícones, mascote, easter eggs e jogos). Em hardware real, também
xHCI, USB HID, NVMe e framebuffer GOP.

## Áreas proibidas (do Claude)
Não editar diretamente:
- `kernel/src/arch/`, `kernel/src/mm/`, `kernel/src/sync/`,
  `kernel/src/sched/`, `kernel/src/syscall/`, `kernel/src/security/`,
  `kernel/src/caps/`, `kernel/src/audit/`, `kernel/src/crypto/`,
  `kernel/src/firewall/`, `kernel/src/console/` (núcleo da consola;
  consolas novas entram como drivers que implementam o trait `Console`),
  `kernel/src/boot_info.rs`,
  `kernel/src/main.rs`, `kernel/src/panic.rs`, `kernel/linker-*.ld`,
  `kernel/build.rs`, `rust-toolchain.toml`, `.cargo/config.toml`
- `CLAUDE.md`
- As secções do Claude em `docs/` (decisões, segurança e unsafe do núcleo)

Se precisares de uma mudança aí, abre uma issue com a etiqueta
**`para-claude`** a explicar o quê e porquê.

## Regras comuns
- Prioridades: 1) segurança, 2) desempenho.
- Crates permitidos: `limine` e `bitflags`. Qualquer outro exige entrada em
  `docs/DECISIONS.md` e aprovação humana no PR.
- `unsafe` só nas camadas de acesso a hardware dos drivers (portas I/O e
  MMIO), com `// SAFETY:` em cada bloco, API segura por cima e entrada em
  `docs/UNSAFE.md`.
- Usar as interfaces de `docs/INTERFACES.md`; mudar uma interface partilhada
  exige atualizar esse ficheiro no mesmo PR.
- Sem `unwrap()`/`expect()` em caminhos com input externo.
- Nunca imprimir endereços do kernel em output visível ao user space.
- Nunca pôr segredos no repositório.
- Branch `codex/<tarefa>`, 1 PR pequeno por tarefa com: o que faz, como
  testar, impacto na segurança. Nunca push para `main`. CI verde +
  aprovação do Rodrigo antes do merge.
- `cargo fmt --all --check` e `cargo clippy --workspace -- -D warnings`
  têm de passar.
- Código e commits em inglês; documentação de coordenação em português.

## Revisão de segurança

- Consultar `docs/REDTEAM.md` e `docs/SECURITY.md`. As rondas começam
  no fim da Fase 6. Cada equipa revê as áreas da outra e corrige a sua.
- Limitar a validação ao kitty-OS em QEMU ou ao hardware autorizado
  do projeto. Ensaios de rede ficam numa rede virtual isolada, sem
  acesso à Internet e sem tráfego para serviços de terceiros.
- `tests/security/redteam/` é partilhada para acrescentar testes de
  regressão. O runner e a CI pertencem ao Codex. Usar dados de teste
  públicos ou sintéticos e respeitar as licenças dos materiais.
- Registar uma issue por falha, com severidade, impacto, evidência
  defensiva e etiquetas `vulnerabilidade`, `red-team` e a do dono.
  Aplicar as regras de divulgação do projeto.
- Acompanhar as falhas abertas no manifesto. A correção deixa um teste
  permanente, atualiza `docs/SECURITY.md` quando muda uma proteção e
  referencia a issue. Atualizar o placar ao fechar uma ronda, após
  corrigir os problemas críticos e altos.

## Formato do HANDOFF (acrescentar a docs/HANDOFF.md no fim de cada sessão)

```
### [AAAA-MM-DD] — CODEX
- Feito: ...
- PRs abertos: ...
- Proteções de segurança alteradas: ...
- Bloqueado por / à espera do Claude: ...
- Próximo passo sugerido para o Claude: ...
- Avisos (bugs conhecidos, unsafe novo, coisas frágeis): ...
```

E atualizar o estado das tuas tarefas em `docs/ROADMAP.md`.
