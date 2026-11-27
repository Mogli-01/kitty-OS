# fs/ — área do Codex

VFS, initrd e, mais tarde, sistemas de ficheiros em disco (Fase 7 e 9).

## Initrd USTAR

`initrd/` contém o leitor só de leitura do arquivo USTAR. Para criar um
arquivo compatível com GNU tar, prepare uma pasta com ficheiros regulares
e diretórios e corra o comando abaixo.

```sh
tar --format=ustar -cf initrd.tar -C pasta-do-initrd .
```

O parser valida os cabeçalhos, os tamanhos, os caminhos relativos UTF-8
e os dois blocos de terminação. Os diretórios têm tamanho zero. Quando
há um NUL num campo de nome ou prefixo, o resto do campo contém zeros.
O parser conta as entradas e devolve slices dos dados dos ficheiros.
A montagem no VFS valida conflitos entre nomes. O núcleo entrega o
módulo do Limine através do contrato da #22.

## Raiz inicial do VFS

`vfs/` monta o arquivo em modo só de leitura. Pesquisa caminhos como
`/bin/kitty-shell`, lista os filhos imediatos dos diretórios e lê partes
de ficheiros com um offset. Diretórios implícitos existem quando um
ficheiro tem um caminho válido sob eles. A montagem recusa nomes
duplicados e ficheiros que sejam usados como diretórios.

## Volumes FAT32 só de leitura

`fat32/` monta um volume que começa no seu setor de arranque. A camada
de partições ou de blocos fornece um `ReadAt` de `kitty-storage` limitado
aos bytes do volume.
Uma imagem em memória pode ser usada diretamente com
`Fat32::mount(image.as_slice())`. A fonte tem de permanecer estável
durante a montagem e disponibilizar leituras sem alinhamento.

`read_dir("/config")` lista os filhos diretos do diretório.
`open("/config/Configuração do kitty.txt")` devolve um ficheiro cuja cadeia
de clusters foi verificada. `read_at(offset, buffer)` funciona em ficheiros
fragmentados, devolve zero no fim e reaproveita a posição para leituras
sequenciais. Um erro de I/O pode deixar parte do buffer de destino escrita.

O leitor aceita setores de 512, 1024, 2048 ou 4096 bytes e clusters até
32 KiB. Valida os limites da FAT e da região dos dados, recusa ciclos,
clusters reservados e cadeias de ficheiros incompatíveis com o tamanho.
Os nomes longos VFAT validam a ordem das entradas, o checksum do alias
e UTF-16. Se a sequência estiver danificada, fica disponível o alias 8.3
em CP437. A pesquisa ignora diferenças de maiúsculas em ASCII, compara
literalmente os restantes caracteres e recusa correspondências ambíguas.

O leitor utiliza uma das FATs indicadas pelo cabeçalho. A validação é
feita conforme se percorre o volume, incluindo a cadeia de cada ficheiro
antes da leitura. A validação de propriedade exclusiva de todos os
clusters do volume e a aplicação de permissões pertencem à futura
montagem do VFS. Esta camada ainda precisa de um driver de blocos e da
ligação ao VFS para ser usada pelo sistema operativo.

```sh
cargo test -p kitty-fat32 --locked
cargo miri test -p kitty-fat32 --locked
cargo build -p kitty-fat32 --target x86_64-unknown-none --release --locked
cargo fuzz run fat32 -- -max_total_time=15 -timeout=5 -max_len=2049
```

## Partições GPT

`storage/` lê a GPT de um disco ou imagem, com setores lógicos de 512,
1024, 2048 ou 4096 bytes. Valida o MBR protetor, o CRC dos cabeçalhos e o
CRC da tabela selecionada. Se a cópia primária falhar, verifica a cópia
de reserva. `header_copy()` identifica a cópia utilizada. O leitor rejeita
sobreposições, identificadores de partição duplicados e endereços fora da
área utilizável.

A tabela fica em memória, com um limite de 4096 entradas e 4 MiB de
metadados a ler por cópia. `partition(n)` usa o número de partição do
disco e devolve um `ReadAt` limitado aos seus setores, com offset zero
no primeiro byte da partição. Uma leitura que ultrapasse o fim falha
antes de chegar ao disco. A vista pode ser passada diretamente ao FAT32.

```rust,ignore
let disk = Gpt::read(image.as_slice(), 512)?;
let partition = disk.partition(1);
if let Some(partition) = partition {
    let fs = Fat32::mount(partition)?;
    let mut file = fs.open("/config/kitty.txt")?;
    let mut contents = [0; 512];
    let length = file.read_at(0, &mut contents)?;
    // Process contents[..length].
}
```

Os CRC detetam corrupção dos metadados. A seleção de uma partição
autorizada a partir dos identificadores e a montagem no VFS pertencem
às camadas que chamam o leitor.

```sh
cargo test -p kitty-storage -p kitty-fat32 --locked
cargo miri test -p kitty-storage -p kitty-fat32 --locked
cargo fuzz run gpt -- -max_total_time=15 -timeout=5 -max_len=1025
```
