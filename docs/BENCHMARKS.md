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

Os benchmarks de ciclos por syscall e de troca de contexto dependem do
scheduler e da entrada em user mode. A medição dos frames do compositor
entra quando houver apresentação no framebuffer.
