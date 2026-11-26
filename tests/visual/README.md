# Capturas do arranque em QEMU

`cargo xtask test-visual` espera pela mensagem de arranque completo na
porta série e pede ao QEMU uma captura do framebuffer. O teste compara
os 304 × 16 píxeis do canto superior esquerdo com `boot-banner.ppm`.
A região contém a primeira linha, `Hello from kitty-OS`. A CI corre o
teste em BIOS e UEFI, em debug e release.

Os ficheiros produzidos ficam em `build/visual/`. O relatório indica a
primeira coordenada diferente. A captura completa e os logs ficam
disponíveis como artefactos no workflow de CI.

Se um PR alterar intencionalmente a primeira linha ou a tipografia da
consola, verifique a captura antes de substituir a referência. Para a
regenerar a partir da imagem revista, pode usar ImageMagick.

```sh
magick build/visual/frame.ppm -crop 304x16+0+0 +repage -depth 8 -strip PPM:tests/visual/boot-banner.ppm
```
