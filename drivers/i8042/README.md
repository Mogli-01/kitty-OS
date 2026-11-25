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

```sh
cargo test --locked -p kitty-i8042
cargo miri test --locked -p kitty-i8042
cargo build --locked --release -p kitty-i8042 --target x86_64-unknown-none
```
