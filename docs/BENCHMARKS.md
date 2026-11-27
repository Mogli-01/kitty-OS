# Benchmarks do kitty-OS

## Tempo de arranque no QEMU

```sh
cargo xtask bench-boot --bios --release
cargo xtask bench-boot --release
```

O xtask compila o ELF e prepara a imagem uma vez por comando. Faz um
arranque de aquecimento, depois sete amostras. Mede com o relógio
monotónico do host, desde imediatamente antes de iniciar o QEMU até
receber a linha `[boot] boot complete, …` na porta série. Cada amostra
exige a saída de sucesso do kernel. O relatório dá o mínimo, a mediana
e o máximo em milissegundos. A linha `boot_bench` pode ser usada pela CI.

A medição inclui QEMU, firmware, Limine e kernel. Os valores abaixo
foram recolhidos a 26 de novembro de 2026 num contentor Ubuntu 24.04
com QEMU 8.2.2 (TCG, `q35`, `-cpu max`, 256 MiB, 1 vCPU) e kernel de
`4b0223a` em release. Use a mesma configuração e o mesmo executor para
comparar alterações. O firmware e a carga do host afetam os tempos.

| Firmware | Amostras | Mínimo | Mediana | Máximo |
|---|---:|---:|---:|---:|
| BIOS/ISO | 7 | 354,0 ms | 361,1 ms | 387,2 ms |
| UEFI/OVMF | 7 | 2 002,8 ms | 2 124,0 ms | 2 753,0 ms |

## Comparação na CI

O job BIOS/release mede o ELF da base do PR e o candidato no mesmo
runner, com o mesmo QEMU e Limine. `KITTY_BENCH_AGAINST` aponta para
um ELF de referência compilado com `qemu-exit`. Cada lado tem o seu
aquecimento e sete amostras. A CI emite um aviso se a mediana subir
pelo menos 25% e 50 ms. O artefacto guarda amostras e logs de ambos.

Num ensaio local com o mesmo ELF dos dois lados, as medianas foram
357,6 ms e 353,1 ms. A variação foi de −4,5 ms e o aviso ficou inativo.

Os benchmarks de ciclos por syscall e de troca de contexto dependem do
scheduler e da entrada em user mode. A medição dos frames do compositor
entra quando houver apresentação no framebuffer.

## `yield` cooperativo e troca de contexto no QEMU

```sh
cargo xtask bench-scheduler --bios --release
cargo xtask bench-scheduler --release
```

O `xtask` compila o kernel uma vez e prepara o meio de arranque de cada
amostra. Faz oito arranques e descarta o primeiro como aquecimento.
Cada amostra exige que o kernel termine o teste de arranque com sucesso
e publique uma única leitura do autoteste cooperativo na porta série.
O comando imprime as sete leituras e a linha `scheduler_bench` com o
mínimo, a mediana e o máximo. O último log fica em
`build/serial-bench-scheduler.log`.

Cada leitura é uma estimativa em ciclos do TSC do convidado por
`yield_now` mais troca de contexto. Inclui a preparação das duas
threads e o trabalho de agendamento do autoteste. O QEMU usa TCG e a
carga do host afeta a medição. Para comparar versões, manter o mesmo
host, firmware, perfil, versão do QEMU e configuração de CPU e memória.

Medição de 26 de novembro de 2026 com o kernel de `9ea6af6` em release,
QEMU 8.2.2 (TCG, `q35`, `-cpu max`, 1 vCPU e 256 MiB), num contentor
Ubuntu 24.04 num Intel Xeon Platinum 8573C. Todos os valores da tabela
são ciclos TSC do convidado por `yield` mais troca de contexto.

| Firmware | Amostras | Mínimo | Mediana | Máximo |
|---|---:|---:|---:|---:|
| BIOS/ISO | 7 | 1 196 | 1 282 | 1 668 |
| UEFI/OVMF | 7 | 1 310 | 1 532 | 3 076 |
