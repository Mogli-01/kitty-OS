# kitty-OS

Um sistema operativo para x86_64 feito **completamente do zero em Rust**,
com duas prioridades, por esta ordem:

1. **Segurança** — o mínimo de `unsafe` (confinado a `arch/`, `mm/` e
   `sync/` e imposto pelo compilador), isolamento forte e proteções de
   hardware ativas (W^X, NX, SMEP, SMAP, UMIP, KASLR…).
2. **Desempenho** — arranque, trocas de contexto e syscalls rápidos, sem
   sacrificar o ponto 1.

O projeto é desenvolvido em conjunto por dois assistentes de IA, **Claude**
(núcleo do kernel) e **Codex** (drivers, userland, testes, CI), que só
comunicam através deste repositório, com revisão e aprovação humana em
todos os PRs. Ver [CONTRIBUTING.md](CONTRIBUTING.md).

**Estado:** Fase 1 — arranca via Limine (BIOS e UEFI) e escreve
`Hello from kitty-OS` na porta série e no ecrã. Plano completo em
[docs/ROADMAP.md](docs/ROADMAP.md).

## Requisitos

- [rustup](https://rustup.rs) — o toolchain nightly certo é instalado
  automaticamente a partir de `rust-toolchain.toml`
- [QEMU](https://www.qemu.org) (`qemu-system-x86_64`) com firmware UEFI
  (OVMF; já vem com o QEMU no Windows e no macOS, no Linux é o pacote `ovmf`)
- `git` (para descarregar o Limine na primeira compilação)
- Opcional, para a imagem ISO (arranque BIOS): `xorriso` e um compilador C
  (`make`, `cc`) no Linux/macOS

Ubuntu/Debian:

```sh
sudo apt install qemu-system-x86 ovmf xorriso build-essential git
```

## Compilar e correr

```sh
cargo xtask run            # compila e arranca no QEMU (UEFI)
cargo xtask run --release  # o mesmo, com otimizações completas (LTO)
cargo xtask run --bios     # arranque BIOS a partir da ISO (precisa de xorriso)
cargo xtask test           # teste de arranque automático (sem ecrã)
cargo xtask iso            # gera build/kitty-os.iso (BIOS + UEFI)
cargo xtask build          # só compila o ELF do kernel
```

A saída da porta série aparece no terminal; o ecrã do QEMU mostra a consola
do framebuffer. Se o QEMU ou o OVMF não forem encontrados automaticamente,
usa as variáveis `KITTY_QEMU`, `KITTY_OVMF` e `KITTY_XORRISO`.

Antes de abrir um PR:

```sh
cargo fmt --all --check
cargo clippy --workspace -- -D warnings
cargo xtask test
```

## Organização

| Caminho | Conteúdo |
|---|---|
| `kernel/` | o kernel (`no_std`, target `x86_64-unknown-none`) |
| `drivers/`, `fs/`, `userland/`, `tests/` | drivers, sistema de ficheiros, programas de user space, testes |
| `xtask/` | ferramenta de build (`cargo xtask …`) |
| `docs/` | arquitetura, decisões, interfaces, segurança, inventário de `unsafe`, roadmap |

## Licença

Ainda por definir pelo dono do repositório.
