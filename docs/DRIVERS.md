# Drivers do kitty-OS

## Teclado PS/2

O crate `kitty-ps2` descodifica os bytes do teclado em Set 2. O prefixo
`E0` identifica teclas estendidas, como as setas e o Alt direito. `F0`
indica uma libertação. A tecla Pause sem modificadores tem uma sequência
própria e gera um evento momentâneo. Os Shift de preenchimento que podem
acompanhar Print Screen ficam de fora dos eventos. [Referência QEMU](https://github.com/qemu/qemu/blob/v8.2.2/hw/input/ps2.c)

`Decoder::push` recebe um byte e devolve um `KeyEvent` quando completa
uma tecla reconhecida. `KeyCode` identifica a posição física, incluindo
a tecla ISO adicional dos teclados portugueses. A camada de input pode
aplicar o layout e associar o timestamp depois da descodificação.

### Ligação prevista ao controlador

O transporte i8042 lê o estado na porta `0x64` e recolhe os dados na
porta `0x60` quando há um byte disponível. A inicialização do transporte
tem de selecionar Set 2 e desligar a tradução do i8042. Respostas a
comandos e dados da porta auxiliar são encaminhados pelo transporte.
Em caso de paridade inválida, timeout ou perda de bytes, `Decoder::reset`
descarta o prefixo pendente. A recolha por IRQ1 depende da API de IRQ da
Fase 4. [Contratos do núcleo](INTERFACES.md)

### Testar

```sh
cargo test --locked -p kitty-ps2
rustup component add miri
cargo miri test --locked -p kitty-ps2
cargo build --locked --release -p kitty-ps2 --target x86_64-unknown-none
```

Os testes abrangem os códigos de pressão e libertação, repetições,
teclado ISO, teclas estendidas e recuperação após sequências inválidas.
Também percorrem os 256 valores de byte em cada estado do descodificador.
