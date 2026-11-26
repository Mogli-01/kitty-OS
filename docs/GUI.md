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

## Desenho e apresentação em dois buffers

`DoubleBuffer` recebe duas superfícies de escrita com a mesma largura e
altura. Cada uma pode ter um pitch próprio. `draw()` empresta apenas a
superfície onde se prepara o frame. `present()` copia toda a área visível.
`present_region(rect)` copia a interseção dessa área com um retângulo
alterado, preservando o resto da imagem apresentada.

A apresentação é uma cópia de memória. A sincronização com a leitura
do ecrã depende da futura interface de apresentação do dispositivo.

## Texto em bitmap

O atlas guarda glifos de dimensões fixas, com um bit por píxel e a
coluna esquerda no bit mais significativo de cada byte. `BitmapFont`
valida se o tamanho é um múltiplo do comprimento de um glifo e só
empresta glifos completos. `draw_glyph` recorta os dois eixos antes de
escrever os píxeis ativos. O fundo e o padding mantêm os seus valores.

`draw_ascii` usa a fonte original do kitty-OS, com glifos de 5 × 7 e
células de 6 × 8. A fonte inclui os 95 caracteres ASCII imprimíveis,
usa `?` para outros caracteres e começa uma nova linha com `\n`.

## Carregar fontes PSF2

`Psf2Font::parse` recebe os bytes de um ficheiro PSF2 v0 já descomprimido.
Valida o cabeçalho, o tamanho do atlas e o mapa Unicode opcional. A
biblioteca empresta os bytes do ficheiro e aceita glifos de 1 a 255
píxeis por eixo. Cada linha usa `ceil(largura / 8)` bytes.

`bitmap().glyph(indice)` procura por índice. `glyph_for_char` percorre
as correspondências Unicode e devolve a primeira que inclui o carácter.
O mapa pode conter sequências de vários caracteres. São validadas
durante a leitura e reservadas para uma futura composição de texto.
O glifo devolvido usa o mesmo `Canvas::draw_glyph` da fonte original.
