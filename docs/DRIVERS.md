# Drivers do kitty-OS

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
