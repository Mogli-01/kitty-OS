# Drivers do kitty-OS

## Cabeçalhos de configuração PCI

`kitty-pci` descodifica um slice imutável de bytes little-endian e exige
os primeiros 64 bytes do cabeçalho convencional. Os primeiros dois bytes
contêm o identificador do fabricante. `0xffff` indica função ausente.
O byte em `0x0e` distingue um dispositivo de tipo 0 de uma ponte de
tipo 1 e guarda ainda a indicação de multifunção no bit 7.

Os dois tipos expõem identificadores, classe, revisão, estado e valores
de interrupção. O layout do dispositivo inclui seis BARs e os
identificadores de subsistema. O da ponte inclui dois BARs e os números
de barramento primário, secundário e subordinado. Os BARs são valores
brutos. Qualquer acesso a recursos depende de validação e de concessão
pelo núcleo. [Mapa de registos PCI](https://github.com/torvalds/linux/blob/v6.12/include/uapi/linux/pci_regs.h)

Se o estado anunciar a lista de capacidades convencional, o descodificador
segue os links a partir do offset guardado em `0x34`. Lê o identificador
e o link seguinte de cada entrada. O iterador expõe o identificador e a
posição atual. Valida o alinhamento de quatro bytes, a presença dos dois
bytes no slice e a ausência de repetições. Pode visitar até 48 offsets
distintos. Um link 0 termina a lista normalmente. Um offset inválido,
uma repetição ou uma entrada incompleta terminam-na com erro.

A enumeração em hardware depende de um contrato de acesso à configuração.
O tipo 2 CardBus, a interpretação de cada capacidade e o espaço PCIe
estendido ficam para etapas seguintes.

```sh
cargo test -p kitty-pci --locked
cargo miri test -p kitty-pci --locked
cargo build -p kitty-pci --target x86_64-unknown-none --release --locked
```

### Interpretar os BARs

`ConfigSpace::bars()` percorre os seis registos de um dispositivo ou os
dois de uma ponte. Distingue endereços de I/O, memória de 32 bits e
memória de 64 bits. Neste último caso, consome o registo seguinte
como metade alta. Uma palavra zero é omitida, pois o snapshot não
distingue um BAR ausente de um BAR de memória sem endereço atribuído.

Encodings reservados ou antigos e pares de 64 bits incompletos
produzem um erro com a posição. O resultado descreve o endereço lido
e o atributo de prefetch quando presente. O tamanho e o acesso ao
recurso dependem de descoberta e validação próprias.
