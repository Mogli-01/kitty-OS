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
