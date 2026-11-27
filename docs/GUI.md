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
