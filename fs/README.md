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
e os dois blocos de terminação. Aceita padding final a zero e devolve
slices dos dados dos ficheiros. A validação de conflitos entre nomes
fica a cargo da montagem no VFS. O contrato para ligar o módulo do Limine
está em discussão na issue #22.

## Raiz inicial do VFS

`vfs/` monta o arquivo em modo só de leitura. Pesquisa caminhos como
`/bin/kitty-shell`, lista os filhos imediatos dos diretórios e lê partes
de ficheiros com um offset. Diretórios implícitos existem quando um
ficheiro tem um caminho válido sob eles. A montagem recusa nomes
duplicados e ficheiros que sejam usados como diretórios.
