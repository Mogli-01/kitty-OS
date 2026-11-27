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

## Zonas alteradas

`Damage::new` cria uma lista para as dimensões do ecrã. O compositor
regista a região antiga e a nova sempre que move ou redimensiona uma
janela. `record` recorta as coordenadas aos limites do ecrã. Regiões
sobrepostas são agrupadas no retângulo que as contém. Ao exceder 16
regiões distintas, a lista passa a cobrir o ecrã inteiro.

O compositor recompõe cada retângulo devolvido por `regions()` no
buffer traseiro. Depois, `DoubleBuffer::present_damage` copia essas
regiões para a frente e esvazia a lista. Se as dimensões forem
incompatíveis, a lista continua disponível para corrigir e repetir.

## Composição de camadas

`Canvas::blend` coloca uma camada BGRA8888 sobre o que já está no
canvas. Usa alfa direto e a operação source-over. Cada canal é
calculado a partir dos valores guardados no buffer e do alfa das
duas camadas. A composição conserva o alinhamento quando a origem ou
o destino são recortados. Para desenhar um frame, o compositor pinta
primeiro o fundo e depois percorre as camadas de trás para a frente.

## Cursor por software

`Cursor` recebe a imagem e o ponto ativo dentro dela. O ponto ativo
é a coordenada usada para localizar cliques. `move_by` recebe um
deslocamento relativo e `move_to` aceita coordenadas absolutas. Os
dois limitam o ponto ativo à área visível e registam a posição antiga
e a nova em `Damage`.

No primeiro frame ou ao reconstruir o ecrã, marca o frame inteiro.
Para cada região alterada, desenha o fundo e as janelas e chama
`Cursor::draw_in` com o mesmo retângulo. O cursor é composto sobre
píxeis acabados de redesenhar. Por fim, apresenta as regiões com
`DoubleBuffer::present_damage`.

## Ordem e seleção de janelas

`WindowStack` guarda até 32 identificadores e retângulos de janela
em memória do compositor. `windows()` devolve a ordem de desenho,
do fundo para o topo. `hit_test` recusa pontos fora do ecrã e devolve
a janela mais alta que contém o ponto, com as suas coordenadas locais.
A seleção usa os limites geométricos. O alfa é aplicado durante o
desenho.

Inserir ou remover uma janela regista a área visível. Mover regista
os limites antigos e os novos. Subir na pilha regista apenas as
interseções com as janelas ultrapassadas. `Damage` recorta estas
regiões ao ecrã e conserva as suas regras de agrupamento.

Usa `damage_mut()` para registar a posição do cursor e para apresentar
as regiões recompostas com `DoubleBuffer::present_damage`. No primeiro
frame, marca o ecrã completo. Depois, para desenhar cada janela numa
região, calcula a interseção com `Rect::intersection` e usa os limites
originais para converter a origem em coordenadas da superfície.

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

O alvo de fuzzing testa ficheiros completos e mapas Unicode gerados.
A CI usa AddressSanitizer e verifica também se desenhar um glifo
recortado preserva o padding do destino.

```sh
cargo fuzz run --fuzz-dir fuzz/graphics psf2 -- -max_total_time=15 -timeout=5 -max_len=65536
```

## Grelha de texto do terminal

`TextGrid` recebe um buffer de células `char` e a geometria em colunas e
linhas. Cada carácter Unicode ocupa uma célula. A grelha aceita texto
através de `core::fmt::Write` e conserva a posição entre escritas.
O cursor passa à linha seguinte quando chega outro carácter depois de
encher a última coluna. Uma quebra de linha explícita avança exatamente
uma linha, mesmo depois de preencher essa coluna. Ao chegar ao fundo,
a grelha reaproveita a linha mais antiga.

`\r` volta ao início da linha e `\u{8}` recua uma célula. `\t` avança
para a próxima paragem de oito colunas, até ao limite da linha. Os
restantes caracteres de controlo são ignorados. O avanço do cursor
preserva o texto que já ocupa as células atravessadas.

`draw_psf2` preenche o fundo da grelha e desenha apenas as células que
tocam a superfície. Usa as dimensões dos glifos PSF2 como tamanho da
célula. Para um carácter sem glifo, tenta `�` e depois `?`. Se a fonte
omitir ambos, essa célula mostra o fundo. O terminal pode consultar
`cursor()` para desenhar a indicação da posição de escrita.
