# Transporte i8042

O controlador i8042 do PC multiplexa a saída do teclado e da porta auxiliar.
O crate `kitty-i8042` acede ao controlador pelo `kitty_hal::PortBlock`. A
porta `0x64` dá o estado quando lida e recebe comandos quando escrita. A
porta `0x60` transporta dados e respostas. [Contrato do núcleo](../../docs/INTERFACES.md)

Durante a inicialização, `I8042::initialize_keyboard` desativa as duas
portas e esvazia os dados pendentes. Desliga a tradução do controlador,
confirma Set 2 no teclado e só depois liga a interrupção do teclado no
i8042. As esperas e tentativas de reenvio têm limites. IRQ1 deve permanecer
mascarado no controlador de interrupções até existir um handler registado.

`I8042::try_read` lê no máximo um byte por chamada e indica a sua origem.
O handler pode entregar os bytes do teclado ao descodificador do #14 e
reiniciá-lo quando a porta reportar um erro de paridade ou timeout. O
núcleo trata o EOI através do contrato do #15.

`I8042::initialize_mouse` prepara a porta AUX depois do teclado, com
IRQ1 e IRQ12 ainda mascarados. O comando `0xD4` em `0x64` encaminha a
escrita seguinte de `0x60` para o rato. Cada resposta tem de trazer o
bit AUX no registo de estado. O driver repõe os valores de fábrica e
confirma o ID zero antes de aceitar relatórios de três bytes.

Se a configuração AUX falhar, o driver fecha a porta, drena os bytes
pendentes e repõe o modo de teclado. Um erro `KeyboardRestoreFailed`
significa que a ligação ao kernel deve manter ambos os IRQs mascarados.
`stop_mouse` permite restaurar o modo de teclado se o registo de IRQ12
falhar durante o arranque.

```sh
cargo test --locked -p kitty-i8042
cargo miri test --locked -p kitty-i8042
cargo build --locked --release -p kitty-i8042 --target x86_64-unknown-none
```
