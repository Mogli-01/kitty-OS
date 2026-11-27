# Drivers do kitty-OS

## UART 16550A

`kitty-uart` recebe um `PortBlock` exclusivo que começa na base da UART.
O acesso usa offsets a partir dessa base, de acordo com o contrato do
HAL. O controlador tem de estar identificado antes da configuração.

| Offset | Registo ou função |
|---|---|
| 0 | RBR para receber e THR para transmitir, ou byte baixo do divisor com DLAB ativo |
| 1 | IER para as fontes de interrupção, ou byte alto do divisor com DLAB ativo |
| 2 | FCR para ativar e limpar as FIFOs |
| 3 | LCR para escolher o formato da linha e alternar DLAB |
| 4 | MCR para as linhas de controlo, incluindo DTR e RTS |
| 5 | LSR para consultar o estado da transmissão e da receção |

`configure` seleciona 8 bits de dados, sem paridade e um bit de paragem
(8N1). Desativa as fontes de IRQ no IER antes de ativar DLAB para escrever
o divisor. A taxa em baud é a frequência do relógio a dividir por 16 e
pelo divisor. Numa UART com relógio de 1,8432 MHz, o divisor 1 seleciona
115 200 baud. A configuração limpa as FIFOs e ativa DTR e RTS, com OUT2
desligado. O chamador mantém a linha de IRQ mascarada durante a operação.

`try_write` escreve um byte se o THR estiver vazio e devolve `false` se
estiver ocupado. `try_read` devolve `None` quando não há dados. Cada
chamada consulta o LSR uma vez e transfere no máximo um byte. Uma leitura
do LSR pode limpar flags de erro, por isso o driver guarda as que observa
durante a transmissão para a operação seguinte de receção. Um erro de
paridade, de trama ou um sinal de *break* faz descartar o byte afetado. O
*overrun* indica que se perdeu um byte novo e preserva os bytes em fila.

O kernel usa a consola série de arranque para COM1. A ligação de
`kitty-uart` ao hardware depende de um contrato de posse das portas e
entrega de IRQs.

```sh
cargo test -p kitty-uart --locked
cargo miri test -p kitty-uart --locked
cargo build -p kitty-uart --target x86_64-unknown-none --release --locked
```

Os testes simulam as respostas do `PortBlock` e confirmam a ordem dos
acessos, o tratamento das falhas e a preservação dos bytes ainda em fila.
[Referência dos registos TL16C550C](https://www.ti.com/lit/ds/symlink/tl16c550c.pdf)

### Interrupções de receção da UART

O offset 2 devolve o registo IIR numa leitura e continua a selecionar
FCR numa escrita. O IIR indica se há uma interrupção e qual a fonte
de maior prioridade. O driver distingue dados, timeout e erros de linha.

`set_receive_interrupts(true)` ativa no IER apenas dados de receção e
estado de linha. Depois ativa OUT2 no MCR, que liga a saída de IRQ na
UART de um PC. Para desligar, fecha esse caminho e limpa o IER. Se
uma escrita falhar ao ativar, tenta as duas operações de desativação.
O chamador mantém a linha do controlador mascarada em caso de erro.

O futuro handler pode consultar `interrupt_cause()` e chamar
`try_read()` para tratar cada byte ou erro de linha. O handler precisa
de um limite de trabalho e de uma estratégia para continuar a drenar
a FIFO. O núcleo é responsável pelo EOI. A ligação às portas reais
e a escolha da linha IRQ estão descritas na issue #72.
