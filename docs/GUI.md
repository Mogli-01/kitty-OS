# Interface gráfica do kitty-OS

## Superfícies em memória

`kitty-gfx` é a primeira biblioteca gráfica de user space. Desenha em
buffers emprestados pelo chamador. Cada píxel tem quatro bytes na ordem
azul, verde, vermelho e alfa (BGRA8888). A largura e altura medem-se em
píxeis, o pitch em bytes. O pitch é a distância entre os inícios de
duas linhas e pode incluir padding. A biblioteca valida até ao último
píxel visível e preserva os bytes de padding.

`Canvas` empresta o buffer para escrita com acesso exclusivo. `Surface`
é uma vista de leitura. `put_pixel`, `fill_rect` e `line` recortam o
desenho aos limites do destino. `blit` recorta a origem e o destino em
conjunto e copia os quatro bytes de cada píxel. O alfa é guardado para
uma fase de composição posterior. A linha percorre apenas a parte visível
do seu eixo principal, mesmo quando os extremos estão muito longe.

```sh
cargo test -p kitty-gfx --locked
cargo miri test -p kitty-gfx --locked
cargo build -p kitty-gfx --target x86_64-unknown-none --release --locked
```

O compositor usará estas superfícies para desenhar em RAM antes de
apresentar uma imagem. A concessão `DISPLAY` e a descrição do formato
do framebuffer entram pela interface do núcleo prevista no roadmap.
