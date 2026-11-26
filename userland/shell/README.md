# Núcleo da shell

`CommandLine::parse` recebe uma linha UTF-8 com até 1 024 bytes.
Espaços e tabs separam palavras fora de aspas. Aspas simples preservam
o conteúdo literal. Fora de aspas simples, a barra invertida preserva
o carácter seguinte. O comando e os argumentos ocupam até 16 palavras.
A validação recusa sequências UTF-8 inválidas e caracteres de controlo
fora da tabulação.

`execute` recebe um destino que implementa `core::fmt::Write`. Inclui
`help` e `echo`, que operam dentro do processo. O futuro programa
interativo liga esta biblioteca à leitura de linhas e à consola em
user space.

```sh
cargo test -p kitty-shell --locked
cargo miri test -p kitty-shell --locked
cargo fuzz run shell -- -max_total_time=15 -timeout=5 -max_len=2048
```

## Editar antes de executar

`LineBuffer` insere e apaga texto até ao mesmo limite de 1 024 bytes.
O cursor é um offset em bytes numa fronteira UTF-8. Esquerda e direita
percorrem caracteres Unicode, e início e fim saltam para as extremidades
da linha. Backspace remove o carácter anterior e Delete remove o
seguinte. O resultado de `as_bytes()` pode ser passado a
`CommandLine::parse` quando o utilizador submete a linha.
