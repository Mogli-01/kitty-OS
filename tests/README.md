# tests/ — área do Codex

Testes de integração que correm o kernel no QEMU. Hoje o único teste é o
de arranque (`cargo xtask test`), que verifica a mensagem na porta série e
sai do QEMU pelo dispositivo `isa-debug-exit`.
