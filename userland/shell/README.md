# Núcleo da shell

`CommandLine::parse` recebe uma linha UTF-8 com até 1 024 bytes.
Espaços e tabs separam palavras fora de aspas. Aspas simples preservam
o conteúdo literal. Fora de aspas simples, a barra invertida preserva
o carácter seguinte, incluindo dentro de aspas duplas. O comando e
os argumentos ocupam até 16 palavras.
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
diagnóstico e liberta a linha para o próximo comando. `run_console`
mantém uma única sessão entre leituras, apresenta o prompt e repete as
escritas parciais até enviar todos os bytes. Uma escrita sem progresso
termina com erro. O transporte implementa o trait Rust `Console`.
A apresentação visual da linha em edição e do cursor pertence ao
consumidor de consola.

## Experimentar no host

```sh
cargo run -p kitty-shell --example host_shell
printf 'help\necho "olá mundo"\n' | cargo run -q -p kitty-shell --example host_shell
```

O exemplo usa a entrada e a saída padrão do host. Para correr dentro
do kitty-OS, o adaptador deverá usar `console_read` e `console_write`
através do handle CONSOLE. Falta o núcleo publicar a tabela de números
das syscalls e carregar `/sbin/init`.
