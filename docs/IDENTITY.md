# Identidade kitty

A identidade entra por etapas. A Fase 10 apresenta o gato no arranque.
A Fase 14 usa a mesma paleta e o mesmo desenho no ambiente de trabalho.
Os recursos pertencem ao projeto e usam os formatos simples já suportados
pela biblioteca gráfica.

## Paleta

| Papel | Cor |
|---|---|
| Fundo | `#161B27` |
| Janela | `#25384F` |
| Texto | `#F4EDE7` |
| Destaque e foco | `#F3B477` |
| Detalhes do gato | `#D87F94` |
| Estado positivo | `#92D1BA` |
| Erro | `#FF9696` |

O foco também tem contorno. Estados de erro e sucesso incluem texto ou
uma forma distinta para continuarem legíveis sem distinguir as cores.

## Fonte, ícones e logótipo

A primeira fonte é a bitmap original de 5 × 7 píxeis do projeto, com
células de 6 × 8 e escala inteira. A extensão para caracteres acentuados
usa glifos desenhados para o projeto. Ver D-112 no #27 e D-150.

Os ícones usam uma grelha de 16 × 16, silhueta simples e no máximo três
cores. O cursor preserva o ponto ativo ao escalar. A mascote usa a mesma
silhueta do arranque, com olhos e orelhas como elementos de animação.
O logótipo original está em [kitty-logo.txt](identity/kitty-logo.txt).
São 32 linhas de 32 símbolos em ASCII.

| Símbolo | Píxel |
|---|---|
| `.` | Transparente |
| `F` | Destaque |
| `I` | Fundo |
| `P` | Detalhe |
| `W` | Texto |

A animação de arranque usa dois quadros com um piscar de olhos. O logótipo
fica estático quando o temporizador ou a animação estiver indisponível.
A animação decorativa não deve atrasar o arranque.

## Easter eggs e jogos

| Ideia | O que exercita |
|---|---|
| `meow` | Execução na shell, escrita na consola e glifos do terminal |
| Mascote reage a um clique | Hit testing e entrega de input à janela com foco |
| Mascote pisca após inatividade | Temporizador, redesenho parcial e repouso da GUI |
| Jogo de serpente | Input direcional, movimento por ticks e regiões alteradas |
| Jogo de blocos descendentes | Rotação, repetição de teclas, desenho da grelha e consistência do temporizador |

Os jogos terão regras, formas, cores, sons e níveis originais do kitty-OS.
