# Núcleo da shell

`CommandLine::parse` recebe uma linha UTF-8 com até 1 024 bytes.
Espaços e tabs separam palavras fora de aspas. Aspas simples preservam
o conteúdo literal. Fora de aspas simples, a barra invertida preserva
o carácter seguinte. O comando e os argumentos ocupam até 16 palavras.
A validação recusa sequências UTF-8 inválidas e caracteres de controlo
fora da tabulação.

`execute` recebe um destino que implementa `core::fmt::Write`. Inclui
`help` e `echo`, que operam dentro do processo.

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

## Ler da consola

O programa mantém uma `ShellSession` entre leituras e passa os bytes
recebidos a `push` pela ordem em que chegaram. O descodificador retém
sequências UTF-8 e de teclas ainda incompletas. Interpreta Enter, Tab,
Backspace, Delete, Esquerda, Direita, Home e End. Ctrl+C descarta a
linha. As outras teclas de controlo são ignoradas. A codificação foi
proposta na secção 6 do PR #52.

Ao premir Enter, a sessão executa a linha, escreve o resultado ou
diagnóstico e liberta a linha para o próximo comando. Quem chama
`push` trata a apresentação da linha e do prompt, bem como erros ou
escritas parciais da futura syscall. O kernel ainda precisa de carregar
`/sbin/init` e disponibilizar `console_read` e `console_write`.
