# Benchmarks do kitty-OS

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
