# Drivers do kitty-OS

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

`cargo test -p kitty-pit --locked` verifica a ordem das escritas, o
arredondamento, os limites e as escritas recusadas através de `PortBlock`
simulado. O PIT periódico será ligado ao scheduler quando a interface
`sched::tick()` estiver disponível.
