# tests/ — área do Codex

Os testes de integração correm o kernel no QEMU e validam a porta série e
o código de saída do dispositivo `isa-debug-exit`.

```sh
cargo xtask test                     # arranque normal
cargo xtask test-memory              # PMM, paginação e MMIO em UEFI/debug
cargo xtask test-memory --bios --release
cargo test --locked -p xtask          # validação dos relatórios no host
cargo miri test --locked -p xtask     # requer rustup component add miri
```

Os testes de memória confirmam que o PMM passa o autoteste de partilha e
limpeza de frames, que a auditoria das tabelas passa, que o kernel chega à
pilha guardada e que o teste de MMIO passa. Depois confirmam as falhas que
o kernel já exercita: a pilha de emergência IST trata o overflow da pilha,
as páginas de código e da IDT recusam escritas e o bit NX impede executar
dados. Os logs ficam em `build/memory-tests/<firmware>-<perfil>/`.
