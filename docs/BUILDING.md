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

## Verificar o layout ELF do kernel

```sh
cargo xtask test-kernel-elf --release --expect-pie
```

O teste verifica o alinhamento ELF, a separação por página, W^X e a
localização da entrada. Também recusa imagens que declarem um
interpretador externo ou peçam uma pilha executável. Indica se a imagem
é `ET_EXEC` ou `ET_DYN`. A opção `--expect-pie` exige `ET_DYN`.
Os meios gerados pelo xtask pedem ao Limine que aleatorize a base da
imagem do kernel. A verificação no host complementa o teste de arranque
em QEMU, que verifica se a imagem arranca com essa opção.
