# Água: o que falta fazer de assets

Lista do que o sistema de água precisa para ficar completo. Os itens estão separados entre o que **já funciona assim que o arquivo existir** e o que **também precisa de código** (fauna, flora e o resto do que vem depois).

Prioridade: ★★★ muda muito o resultado, ★★ ajuda bastante, ★ acabamento.

## Regras gerais

- **Texturas**: PNG em pixel art, sem emenda nas bordas quando for para repetir, 256 px (128 px em peças pequenas). Mesmo processo de sempre: `tools/import_textures.py`.
- **Modelos**: `.glb`, low poly, com textura de pixel art. Medidas em metros reais. Em `assets/models/<grupo>/`.
- **Sons**: WAV 44,1 kHz. Loops sem clique na emenda, com pelo menos 10 s. Efeitos curtos em mono.
- **Animações**: FBX do Mixamo, *Without Skin*, 30 FPS. Marque *In Place* em tudo o que for ciclo. Em `assets/animations/mixamo/`.

## 1. Funciona assim que o arquivo existir

### Animações ★★★

Hoje o personagem nada com a caminhada, inclinado. É o que mais se nota de perto.

| Arquivo | Busca no Mixamo | Para quê |
|---|---|---|
| `swimming.fbx` | Swimming | Nado na superfície (clipe `swim`) |
| `treading_water.fbx` | Treading Water | Parado, boiando (clipe `swim_idle`) |
| `swimming_under.fbx` | Swimming, a variante mais horizontal | Nado embaixo d'água |
| `swim_to_edge.fbx` | Swimming To Edge | Sair da água em um cais (pede código) |

Os nomes `swim` e `swim_idle` já são procurados pelo sandbox. A conversão para o esqueleto do chartest precisa de um passo novo no importador, que eu faço quando os arquivos chegarem.

### Texturas da água ★★★

Quanto mais clara e sem cor for a imagem, mais a cor de `look` manda: o detalhe vem da imagem, e a cor, da cena.

| Arquivo | Como deve ser |
|---|---|
| `agua_mar.png` | Ondinhas e veios em tons médios, sem direção forte |
| `espuma_mar.png` | Só o canal vermelho conta: branco é espuma, preto é vazio. Renda com buracos, em manchas (`foam_texture`) |
| `agua_rio.png` | Linhas alongadas em uma direção. O eixo **V** é o do fluxo |
| `espuma_rio.png` | Como a do mar, em fiapos esticados ao longo do V |
| `agua_lamacenta.png` | Rio barrento, em tons quentes |
| `agua_cristalina.png` | Quase vazia: poucas linhas finas e claras |

Já existem e servem: `agua.png`, `agua_parada_pantano.png`, `agua_negra_pelicula_esverdeada.png`.

### Fundo do mar e do rio ★★

Sem cáusticas ou brilhos pintados na imagem: o motor já desenha os dois por cima, e pintados eles ficariam parados.

| Arquivo | Como deve ser |
|---|---|
| `areia_fundo_mar.png` | Areia limpa com ondulações. A `areia_submersa_com_causticas.png` atual tem as cáusticas pintadas |
| `areia_fundo_escuro.png` | Areia e lodo de mar profundo |
| `rocha_com_algas.png` | Rocha com manchas verdes e roxas |
| `lodo_de_rio.png` | Lama escura, com raízes finas |
| `seixos_de_rio.png` | Pedras redondas para o leito claro |

### Sons ★★

Gerei versões provisórias por código, e elas já tocam: `sea`, `river`, `underwater`, `splash_1`, `splash_2` e `swim`. Basta substituir o arquivo com o mesmo nome.

| Arquivo | O que é |
|---|---|
| `sea.wav` | Ondas chegando na praia (loop) |
| `river.wav` | Rio correndo (loop) |
| `underwater.wav` | Pressão grave com bolhas distantes (loop) |
| `splash_1.wav`, `splash_2.wav` | Algo caindo na água |
| `swim.wav` | Uma braçada |

## 2. Precisa de código, mas já vale fazer

### Sons novos ★★

| Arquivo | O que é |
|---|---|
| `sea_storm.wav` | Mar pesado, para tempestade (loop) |
| `hull_slap_1..3.wav` | Onda batendo no casco |
| `hull_creak_1..3.wav` | Madeira do navio rangendo |
| `pier_water.wav` | Água batendo nos pilares (loop) |
| `surface_gasp.wav` | Respiração ao voltar à tona |
| `bubbles_1..3.wav` | Bolhas perto do rosto |
| `splash_big.wav` | Navio ou objeto grande caindo |

### Cais e píer ★★★

São o que mais mostra a espuma em volta dos objetos. Faça em peças que encaixem, com a origem no centro da base:

- Pilar de madeira, em 2 ou 3 alturas, com a parte de baixo escura e com cracas
- Trecho de deque de 4 m, reto e quebrado
- Escada de sair da água
- Enfeites: corda, boia, rede, caixotes, lampião

### Navios e barcos ★★

Bote e Chalupa já flutuam, medidos em teste. Para os próximos:

- Casco fechado por fora, sem furos abaixo da linha d'água
- Marque `hollow: true` no `float` da cena: é o que tira o mar de dentro do barco ou do porão
- Nada pendurado para fora abaixo da linha d'água, como âncora ou corrente, na mesma malha: a máscara usa o ponto mais baixo de cada lugar visto de cima, então essas peças abririam um buraco no mar. Deixe-as em objetos separados
- Proa em +X, como nos dois atuais
- Velas e bandeiras como peças separadas, para o vento mexer nelas depois

### Água doce ★★

- Cachoeiras já saem sozinhas de qualquer trecho íngreme do rio, sem textura. Um som gravado pode substituir `waterfall.wav`
- Pedras de corredeira em 3 ou 4 formas, arredondadas
- Tronco caído e galhos boiando
- Ponte de madeira ou de pedra
- Sprites com recorte: junco, taboa, vitória-régia, lentilha-d'água

## 3. Futuro: povoar o fundo

A ordem é a que eu faria, do que dá mais vida com menos trabalho. Nada disso precisa existir agora.

### Flora ★★★

Sprites em cartões com recorte, como os galhos dos pinheiros, ou modelos simples. Cada um com uma imagem de 256 px e 2 ou 3 variações.

- **Kelp**: fita alta de 4 a 12 m, desenhada de baixo para cima. É a mais importante
- Algas baixas em tufos: verde, vermelha e roxa
- Capim marinho, para espalhar em grande quantidade
- Corais: galho, cérebro, leque, mesa e tubo, com versões com `_emission.png` para o lado onírico
- Anêmonas e esponjas

### Fauna que nada ★★★

Modelos sem esqueleto, com 100 a 400 triângulos, frente em +X e origem no centro. Um shader dobra o corpo, então não precisa de rig.

- Peixes pequenos, em 3 ou 4 formas, para cardumes
- Peixes médios, em 2 ou 3 formas
- **Água-viva** com `_emission.png`, o sino separado dos tentáculos
- Arraia e tartaruga
- Tubarão e uma criatura enorme ao longe, com rig
- Peixes de rio: 2 formas, mais sapo e libélula

### Fauna do chão ★★

- Caranguejo, com rig simples
- Estrela-do-mar, ouriço, conchas em 4 ou 5 formas, ostra
- Lula ou polvo escondido em uma toca

### Lugares ★★★

É o que dá motivo para mergulhar:

- Naufrágio inteiro e pedaços de casco, com canhões e âncora
- Ruínas submersas: colunas, arcos, estátua
- Baú, ossadas e caveira gigante
- Arcos e torres de rocha, e entrada de caverna
- Algo que só faça sentido em sonho, como uma escadaria que desce para o nada ou uma lua caída no fundo

### Texturas de apoio ★

- `plancton.png`: 4 pontos de luz para partículas em água profunda
- `raios_de_luz.png`: só se os raios atuais não bastarem
