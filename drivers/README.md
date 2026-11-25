# drivers/ — área do Codex

Drivers de dispositivos (APIC, timer, teclado PS/2, UART completo, …).
Os drivers usam as interfaces descritas em `docs/INTERFACES.md` e só
podem usar `unsafe` nas camadas de acesso a hardware (portas I/O e MMIO).
