# Arsenal de modelos e texturas

Catálogo do que já existe em `assets/`, para o plano da vida na vila. Antes de pedir um modelo novo, procure aqui. Os modelos são `.glb`/`.gltf` e as texturas `.png`, todos com o nome do arquivo.

## Modelos (`assets/models/`)

### Para a casa

- `objects/`: Porta, Porta-de-Pedra, Janela-Rustica, Pilar, Poste, Grade-de-Ferro.
- **Cercas** (`objects/`): Cerca-Falha-Central, Cerca-Miolo-Baixo, Cerca-Quebrada, Cerca-Tabuas-Inclinadas, Cerca-Travessa-Partida.

### Móveis

- **`objects/mobiliario/`**
  - Camas: Cama-Casal-Manta-Azul, Cama-Casal-Sem-Manta, Cama-Simples-com-Manta, Cama-Solteiro-Manta-Dobrada, Cama-Solteiro-Sem-Manta, Beliche-Manta-Verde, Beliche-Sem-Manta, Enxergao-Sobre-Estrado.
  - Assentos: Banco-de-Madeira, Banqueta-de-Madeira.
  - Guardar: Cesto-de-Armazenamento, Prateleira-Avulsa.
- **`objects/`**: Mesa, Cadeira, Armario, Guarda-Roupa, Bau, Barril, Caixote.

### Cozinha e comida

- **`objects/cozinha/`**
  - Fogo e barro: Fogao-de-Barro, Pote-de-Barro, Jarra-de-Barro, Tigela-de-Barro.
  - Utensílios: Tabua-de-Cortar, Faca-de-Cozinha, Cutelo-de-Acougueiro, Colher-de-Madeira, Concha-de-Madeira, Garfo-de-Madeira.
- **`objects/`**
  - Utensílios: Panela-de-Ferro, Prato, Caneca, Balde-de-Madeira.
  - Comida: Pao, Queijo, Banana, Maca.

### Fogo e luz

- **`objects/fogo-e-iluminacao/`**
  - Lareiras: Lareira-Com-Lenha, Lareira-Lenha-Queimada, Lareira-Limpa.
  - Luz: Tocha-de-Parede, Tocha-de-Pe, Vela-com-Castical.
  - Lenha: Lenha-Avulsa.
- **`objects/`**: Lanterna, Fogueira.

### Higiene

`objects/higiene-e-limpeza/`: Bacia-de-Lavar, Penico-de-Ceramica, Vassoura-de-Palha.

### Ofícios e outros

- **Pesca e mar:**
  - `objects/`: Vara_Pesca, Ancora-Grande, Ancora-Media, Ancora-Pequena.
  - `boats/`: Bote, Chalupa.
- **Leitura e culto** (`objects/`): Livro-Articulado, Altar, Cristais.
- **Bandeiras** (`banner/`): Estandarte, banner.

### Ofícios da vila

- **Ferraria** (`objects/ferraria/`): Bigorna, Forja, Martelo-de-Ferreiro.
- **Taverna** (`objects/mobiliario/`): Balcao-de-Taverna.
- **Pesca** (`objects/`): Rede-de-Pesca.
- **Água** (`objects/`): Poco-de-Pedra.
- **Roupa** (`objects/higiene-e-limpeza/`): Varal-de-Madeira.

### Natureza

- **`savana/`** (com LOD e colisor; a ilha já usa)
  - Árvores: acácias (11), baobás (7), palmeiras (7).
  - Arbustos: espinhosos (2), verdes (2).
  - Mortos: árvores mortas (2), tronco caído.
  - Cupinzeiros (3).
  - Laterita (6).
- **`objects/`** (cópias sem LOD de algumas árvores da `savana/`, mais o que só existe aqui)
  - Bananeiras (7).
  - Rochas (5).
  - Cogumelos (5).
  - Trombetas de cobre (5).
  - Tronco-Oco.
- **Mar:**
  - `objects/algas/`: 6 algas.
  - `animals/corais/`: 12 corais.

### Gente, animais e armas

- **Gente:** `chartest/`, o corpo-base com as armaduras em `chartest/armor/`.
- **Animais** (`animals/`): Agua-Viva, Arraia, Caranguejo, Gaivota, Sardinha. Ainda não há nenhum animal de terra.
- **Armas** (`weapons/`, 24 modelos)
  - Lâminas curtas: adaga, faca de caça, main gauche.
  - Espadas leves: espada reta, rapieira, florete, katana, wakizashi.
  - Sabres e curvas: sabre, sabre de abordagem, cimitarra, alfange, khopesh, machete.
  - Espadas grandes: espada larga, flamberge, zweihänder, espada colossal.
  - Machados: machadinha, machado de abordagem, machado grande.
  - Hastes e arco: lança rústica, arco curto.
  - Armas de fogo: mosquete, pistola de pederneira.

## Texturas (`assets/textures/`, 213 na raiz)

### Casa

- **Paredes:** parede_taipa, parede_de_barro, reboco_envelhecido, gesso_branco_gasto, parede_madeira_escura, parede_de_pedra_umida, papel_de_parede1.
- **Tijolo:** tijolos_de_argila, tijolo_aparente_claro, tijolos_com_fuligem, tijolos_de_pedra, tijolos_de_pedra_quebrados.
- **Pisos:**
  - terra e madeira: piso_terra_batida, piso_madeira_desgastada, tabuas_de_madeira, tabuas_quebradas;
  - pedra e cerâmica: piso_de_pedra_gasto, piso_ceramica_antiga, piso_mosaico_gasto, piso_rachado_com_raizes.
- **Telhado:** telhado_de_palha, palha_de_telhas_faixa, palha_de_telhas_canto, palha_de_telhas_tufo (estas três para a beira e as quinas), telhado_de_telhas.
- **Tecido e tapete:** carpete_gasto, tecido_tapecaria, tecido_grosso, tecido_rasgado, couro1, couro_claro.
- **Madeira:** madeira_lisa, madeira_apodrecida, madeira_queimada, caixa_de_madeira, tampa_de_barril.
- **Metal e louça:** metal, metal_escurecido, ferro_oxidado, porcelana_pratos.

### Decalques de sujeira e uso

Todos começam com `decal_`:

- **Sujeira:** sujeira_base_parede, sujeira_poeira, sujeira_pontos, sujeira_lama.
- **Fuligem:** fuligem.
- **Mofo e musgo:** mofo_*, musgo_*.
- **Lama e lodo:** lama_*, lodo_*.
- **Rachaduras:** rachadura_fina, rachadura_larga, fissuras_ramalho.
- **Papel:** papel_descolando, papel_rasgado_grande, papel_rasgado_pequeno.
- **Sangue:** sangue_*.
- **Terra:** terra_seca, terra_arrastada.

### Teias e plantas de parede

- **Teias:** teia_*.
- **Plantas:** hera_pendente, trepadeira_vertical, trepadeira_enrolada, musgo_pendente.

### Chão aberto, pedra e natureza

A ilha já usa parte destas.

- **Terra:** terra_vermelha_savana, terra_seca_rachada, terra_clara_ressecada, terra_pedregosa.
- **Areia e cascalho:** areia, areia_de_praia, cascalho, leito_seixos.
- **Lama:** lama_estuario, lama_umida.
- **Rocha:** rocha_laterita, rocha_avermelhada, pedra_arenito, mais muitas pedras e rochas.
- **Plantas de chão:** capins, mato1 a mato4, juncos, taboas, samambaia.

### Outras pastas

- `algas/`: 4 texturas.
- `corais/`: 5 texturas.
- `moons/`: kurel, maris.
- `to review/` (79): quase todas são cópias de arquivos da raiz. Os decalques, as teias, `mato*`, `palha_de_telhas_*`, `sol` e `lua` já estão lá. As luas `kurel` e `maris` também existem em `moons/`.

## O que ainda falta

Casca, interior e cozinha: nada falta.

- **Etapa 2:** bancadas de ofício, como bigorna e forja do ferreiro, rede de pesca pendurada e balcão da taberna. Tapete e esteira podem sair das texturas, como peça plana.
- **Etapa 3:** poço, varal e carroça.
- **Etapa 4:** roupas no esqueleto do chartest (camisa, colete, saia, calça, avental, chapéu, lenço) e 2 ou 3 cabelos e barbas.
- **Etapa 7:** os animais de terra da savana, depois de escolher as espécies.
