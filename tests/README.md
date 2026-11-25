# tests/ — área do Codex

Os testes de integração correm o kernel no QEMU e validam a porta série e
o código de saída do dispositivo `isa-debug-exit`.

```sh
cargo xtask test                     # arranque normal
cargo xtask test-memory              # PMM, heap, paginação e MMIO em UEFI/debug
cargo xtask test-memory --bios --release
cargo test --locked -p xtask          # validação dos relatórios no host
cargo miri test --locked -p xtask     # requer rustup component add miri
```

Os testes de memória confirmam os autotestes do PMM, heap e MMIO, a auditoria
das tabelas e a mudança para a pilha guardada. Depois verificam os relatórios
dos sete autotestes de falha do kernel. A pilha IST recebe o double fault,
as páginas de código e da IDT recusam escritas, o bit NX impede executar
dados e a página de guarda após uma alocação grande apanha a escrita fora
do buffer. Os logs ficam em `build/memory-tests/<firmware>-<perfil>/`.
