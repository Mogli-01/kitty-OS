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
