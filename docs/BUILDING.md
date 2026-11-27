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

## Ver o ecrã de boas-vindas

```sh
cargo xtask run --release --splash         # UEFI
cargo xtask run --release --bios --splash  # BIOS
cargo xtask iso --release --splash         # build/kitty-os.iso
```

A opção inclui o ecrã animado no kernel. Durante seis segundos, o gato
pisca duas vezes, a barra avança e aparece «A iniciar o kittyOS».
A seguir, o kernel entrega o ecrã à consola. O arranque normal segue
diretamente para a consola. Se faltar o ecrã ou o temporizador necessário,
o kernel prossegue com o arranque.

A [pré-visualização](../assets/boot/kitty-welcome-preview.gif) também pode
ser vista fora do QEMU. Para atualizar a imagem embutida após editar o
PNG, usa `python3 assets/boot/encode-splash.py` com Pillow no host.

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

O xtask aceita arquivos entre 1 byte e 128 MiB e valida-os com o VFS
antes de os incorporar como `/boot/initrd.tar`. A configuração do Limine
identifica o módulo como `kitty-initrd`. O núcleo confirma a receção do
módulo com `[initrd] module validated` na consola série.

Depois de validar o arquivo USTAR e criar o índice, o VFS confirma a
montagem com `[vfs] initrd mounted read-only`. Um arquivo recusado pelo
VFS produz `[vfs] initrd archive rejected` e o kernel continua o arranque.

```sh
cargo xtask test-initrd --release        # UEFI
cargo xtask test-initrd --bios --release # BIOS
```

O teste verifica a leitura de `/etc/kitty.txt`, um arquivo com checksum
inválido e um arranque sem módulo. Os logs e meios do teste ficam em
`build/initrd-tests/<firmware>-<perfil>/`.
