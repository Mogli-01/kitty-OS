# Compilar e experimentar o kitty-OS

## Preparar as ferramentas

Instale Git, Rust com `rustup` e QEMU x86_64. O ficheiro
`rust-toolchain.toml` fixa o nightly, o target e os componentes. Na
raiz do repositório, `rustup toolchain install` instala essa versão.

Em Ubuntu 24.04, instale QEMU, OVMF e xorriso com o gestor de pacotes.
OVMF fornece o firmware UEFI. xorriso cria a ISO híbrida de BIOS e UEFI.

```sh
sudo apt-get update
sudo apt-get install --no-install-recommends qemu-system-x86 ovmf xorriso
```

`KITTY_QEMU` pode indicar o executável do QEMU. `KITTY_OVMF` pode
indicar a imagem de código OVMF caso a deteção automática falhe.
`KITTY_XORRISO` permite indicar o executável usado para criar a ISO.

## Arrancar no QEMU

```sh
cargo xtask run --release         # UEFI, com janela do QEMU
cargo xtask run --bios --release  # BIOS, a partir da ISO
cargo xtask test --release        # teste automático em UEFI
cargo xtask test --bios --release # teste automático em BIOS
```

`cargo xtask iso --release` escreve `build/kitty-os.iso`. O xtask obtém
a versão de Limine fixada pelo projeto na primeira preparação da imagem.

## Empacotar um initrd

Crie uma pasta com ficheiros regulares e diretórios. Pode gerar o arquivo
no host com GNU tar e passá-lo à preparação da imagem.

```sh
mkdir -p initrd-root/etc
printf 'kitty-OS\n' > initrd-root/etc/kitty.txt
tar --format=ustar -cf initrd.tar -C initrd-root .
cargo xtask iso --release --initrd initrd.tar
cargo xtask run --release --initrd initrd.tar
```

O xtask valida a imagem com o VFS antes de a incorporar como
`/boot/initrd.tar` no meio de arranque. A entrada Limine identifica o
módulo como `kitty-initrd`. A disponibilização do módulo à raiz do VFS
no kernel depende do contrato em discussão na issue #22.
