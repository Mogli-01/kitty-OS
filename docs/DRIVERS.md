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

## Transição para I/O APIC e xAPIC

A feature `xapic` liga os drivers aos blocos de registos concedidos pelo
núcleo. O controlador lê a MADT, resolve os overrides ISA e configura
rotas Fixed dirigidas ao CPU de arranque. No QEMU, IRQ0 usa GSI 2.

Antes da mudança, verifica rotas especiais do firmware, intervalos GSI,
configuração NMI e interrupções pendentes. Ativa o SVR antes de desmascarar
LINT1 e mascara o caminho ExtINT de LINT0 antes de entregar o controlador
a `arch::irq`. O núcleo regista o handler antes de desmascarar cada linha
e envia o EOI pelo LAPIC depois do handler.

A mudança é feita uma vez no arranque. Topologias recusadas conservam o
PIC. Uma falha de escrita restaura os registos locais alterados, com o
vetor espúrio reservado 0xff. Uma restauração que falhe termina o arranque.
A capacidade de hardware filtra todos os acessos e o driver usa Rust seguro.

```sh
cargo xtask test --features selftest-xapic
cargo xtask test --bios --release --features selftest-xapic
```

O teste exige o nome do controlador APIC e duas confirmações de EOI.
O marcador antigo do autoteste PIT ainda menciona o PIC no núcleo.
A feature fica opcional enquanto as integrações dos outros drivers são revistas.

A CI também corre `cargo xtask test --features selftest-xapic-no-madt`.
Só neste modo de teste, a entrada do driver recebe uma MADT ausente
simulada. O ensaio exige o marcador da simulação, a seleção do PIC e
a entrega de dois IRQ0. A descoberta ACPI do núcleo corre normalmente.

A matriz também arranca com a feature `selftest-xapic` e o ACPI
desativado no QEMU. Em BIOS usa `pc,acpi=off` e exige `NoRsdp`.
Em UEFI usa `q35,acpi=off` e aceita `NoMadt` ou `NoRsdp`. Nos dois
modos, o PIC tem de permanecer ativo e o timer tem de entregar dois IRQ0.

### Verificar o timer do Local APIC

`selftest-lapic-timer` reserva um vetor local do #51 e programa o
contador da LVT em modo one-shot. O primeiro handler rearma o contador.
O segundo mascara e pára o timer antes de libertar o vetor. A série só
confirma o resultado depois de o núcleo enviar EOI para a segunda
interrupção. O ensaio usa uma contagem curta para o QEMU.

```sh
cargo xtask test --features selftest-lapic-timer \
  --expect '[apic] two local timer deliveries acknowledged'
```

## Temporizador PIT 8254

O canal 0 do PIT divide o relógio nominal de 1 193 182 Hz e sinaliza a
linha ISA IRQ0 quando o período acaba. O driver `kitty-pit` recebe o bloco
de portas de `legacy::claim(LegacyDevice::Pit)`. Escreve `0x34` na porta
`0x43` para selecionar o canal 0, modo 2 e contagem binária. A seguir escreve
os bytes baixo e alto do divisor, por esta ordem, na porta `0x40`.
Um divisor de 65 536 é codificado como dois bytes a zero.

`Period::from_divisor` aceita de 2 a 65 536, inclusive. `Period::from_hz`
arredonda pedidos entre 19 e 596 591 Hz ao período mais próximo. Por
exemplo, um pedido de 100 Hz usa um divisor de 11 932. Um consumidor que
contabilize tempo deve usar a razão `divisor / INPUT_HZ` em segundos por
tick, com a precisão permitida pelo relógio físico.

Programe o PIT com IRQ0 mascarado e mantenha a concessão de portas
enquanto o canal estiver em uso. Uma escrita recusada pode deixar o canal
parcialmente configurado. Só registe o handler depois de concluídas as
três escritas. O controlador pode entregar um IRQ que já estava pendente
quando se desmascara a linha. O núcleo confirma cada interrupção (EOI)
depois de o handler regressar.

`Pit::stop` escreve `0x30` no registo de controlo. O modo 0 leva a
saída a zero e aguarda um novo contador, pelo que o driver pode parar
os impulsos ou voltar a programar o período. Mascare IRQ0 antes da
paragem e trate no controlador qualquer interrupção já pendente.

`cargo test -p kitty-pit --locked` verifica a ordem das escritas, o
arredondamento, os limites e as escritas recusadas através de `PortBlock`
simulado. No arranque, o PIT programa 100 Hz e o handler de IRQ0 chama
`sched::tick()`. O núcleo confirma a interrupção antes de decidir se
troca de thread. Os autotestes `selftest-irq-pit` e `selftest-preempt`
reclamam o PIT para os seus próprios cenários.

O autoteste do núcleo programa o PIT e confirma dois IRQ0 pelo PIC.
A segunda entrega verifica que o reconhecimento da primeira libertou a
linha. A CI executa o teste em BIOS e UEFI, em debug e release.

```sh
cargo xtask test --features selftest-irq-pit --expect 'hardware IRQ0 (PIT) delivered twice'
cargo xtask test --features selftest-pit-driver --expect '[pit] driver preempted a busy thread twice'
```

## Temporizador LAPIC do scheduler

A opção `lapic-scheduler-timer` seleciona o temporizador LAPIC no CPU de
arranque quando o controlador xAPIC está disponível. Programa primeiro a
entrada do temporizador local em modo one-shot, com a interrupção mascarada
e o divisor em 16. Usa o PIT a cerca de 200 Hz como referência, também com
IRQ0 mascarado. Um comando de latch (`0x00` na porta `0x43`) permite ler os
dois bytes de uma só amostra na porta `0x40`, mesmo quando o contador avança
entre as duas leituras.

O driver mede dois períodos completos do PIT com três amostras do contador
local. Limita as leituras por transição e as repetições da medição. Rejeita medições sem
progresso, instáveis ou fora do intervalo do registo. Liberta o PIT e, se
a medição for válida, carrega o período calculado para 100 Hz no LAPIC.
Regista o handler antes de desmascarar a interrupção local. O handler chama
`sched::tick()` e o núcleo confirma a interrupção antes de trocar de thread.

Se faltar um xAPIC utilizável ou a calibração falhar, o PIT conduz o
scheduler via IRQ0. Se a paragem do LAPIC falhar após um erro de configuração,
o arranque termina para evitar ativar uma segunda fonte de ticks.
O PIT também é a fonte predefinida quando a opção não está ativa.

```sh
cargo xtask test --features selftest-lapic-scheduler \
  --expect '[lapic] driver preempted a busy thread twice'
```

O teste mantém uma thread ocupada e exige duas interrupções da fonte
selecionada com trocas efetivas de thread. A CI também arranca com ACPI
desativada e verifica as mesmas trocas pelo PIT através do PIC.

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

### Ligação ao controlador

O transporte i8042 lê o estado na porta `0x64` e recolhe os dados na
porta `0x60` quando há um byte disponível. A inicialização do transporte
seleciona Set 2 e desliga a tradução do i8042. O kernel reclama as portas
e regista o handler em `devices::init`, antes de ativar as interrupções.
O handler recolhe um byte de cada vez. `Keyboard` encaminha os bytes do
teclado para o descodificador e guarda os eventos numa fila de 64 entradas.
Um erro de paridade ou timeout no teclado descarta o prefixo. A fila
sinaliza `Lost` em caso de erro ou lotação, para o consumidor limpar o
estado das teclas premidas. [Contratos do núcleo](INTERFACES.md)

O caminho do controlador até aos eventos está ligado. A entrega dos
eventos a aplicações depende da camada de input.

### Teclado português para a consola

`PortugueseKeyboard` aplica um layout ISO PT-PT aos eventos físicos.
As duas teclas Shift funcionam separadamente, o Caps Lock alterna no
primeiro evento de pressão e o Alt direito permite escrever símbolos
como `@` e `€`. As teclas mortas combinam os acentos mais comuns com a
letra seguinte. Um evento `Lost` repõe os modificadores e descarta o
acento pendente.

`ConsoleBytes` transporta de uma vez o carácter UTF-8, o resultado de
uma composição ou a sequência de edição da secção 6 do PR #52. Este
layout cobre a escrita alfanumérica. O Num Lock, os LEDs e a seleção
de layout ficam para passos seguintes. A entrega em funcionamento
normal requer `console::input_push` no núcleo.

### Testar

```sh
cargo test --locked -p kitty-ps2
rustup component add miri
cargo miri test --locked -p kitty-ps2
cargo build --locked --release -p kitty-ps2 --target x86_64-unknown-none
cargo xtask test-keyboard
```

Os testes abrangem os códigos de pressão e libertação, repetições,
teclado ISO, teclas estendidas e recuperação após sequências inválidas.
Também percorrem os 256 valores de byte em cada estado do descodificador.
`test-keyboard` envia `A`, seta para cima e a sequência física de `é`.
Verifica as pressões, as libertações e os bytes de consola produzidos
pelo layout através de IRQ1. `--bios` e `--release` mudam o firmware e
o perfil.
