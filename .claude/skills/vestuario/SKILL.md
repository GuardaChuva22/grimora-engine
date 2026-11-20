---
name: vestuario
description: Criar roupas, acessórios, cabelos e vestimentas para o personagem chartest do Grimora/Nyraldar (camisa, calça, saia, túnica, avental, chapéu, lenço, cinto, bota, capa, cabelo, barba, armadura de pano ou couro) com o kit Blender de tools/blender/clothes, prontos para as shape keys do corpo e para vestir moradores. Use SEMPRE que o pedido envolver roupa, vestimenta, acessório, peça de vestir, cabelo, barba ou figurino de NPC ou do jogador, ou arquivos em assets/models/chartest/clothes, accessories ou hairs, mesmo que a palavra "skill" não apareça.
---

# Vestuário do chartest

Toda peça nasce de um script Python que roda no Blender em segundo plano, a partir do `chartest.blend`. Assim ela já sai com:

- os pesos dos 42 ossos;
- as 24 shape keys do corpo (feminino, músculo, gordura, magreza, postura de idoso, rosto e outras);
- folga contra a pele;
- textura PSX de 128 px;
- prévia em 5 tipos de corpo.

O motor assa as mesmas shape keys do morador na peça. Por isso a roupa acompanha qualquer corpo, desde que o kit tenha transferido as keys.

## Onde fica cada coisa

- **Kit:** `tools/blender/clothes/`.
  - `grimora_cloth_kit.py` é a fachada: `open_clean_chartest`, `clone_clean_body_bmesh` e `finalize_garment_pipeline`.
  - O coração está em `build_linen_shirt.py`, que traz `autofit_garment_to_chartest`, `enforce_mesh_clearance_over_bvh`, `extrude_closed_rim_inward`, `ordered_boundary_loop`, `verify_shirt_and_export_glb`, `export_uv_overlay` e as prévias. Não renomeie esse arquivo: o kit importa dele.
- **Exemplos prontos,** um por tipo de peça. Copie o mais parecido:

  | Tipo de peça | Exemplo |
  |---|---|
  | Calça | `build_knee_pants.py` (o menor, use como molde), `build_linen_pants.py` |
  | Tronco | `build_linen_shirt.py`, `build_short_sleeve_shirt.py`, `build_female_blouse.py` |
  | Peça longa | `build_long_tunic.py`, `build_long_skirt.py` |
  | Por cima de outra roupa | `build_aprons.py`: importa as roupas já aprovadas para não atravessá-las e gera os dois aventais |
  | Cabelo | `build_hair_short_male.py`, `build_hair_long_loose.py`, `build_hair_samurai_bun.py`, `build_hair_loose_female_bun.py`: casca sobre o crânio por raycast (`build_head_bvh`, `hairline_z`), presa à cabeça |
- **`inspect_*.py`, `find_shirt_boundary_loops.py` e `test_neck_loop.py`:** sondas para achar os anéis de vértices do corpo (pescoço, punho, cintura), que é onde uma peça nova começa e termina.
- **Saída de trabalho:** `tools/blender/clothes/out/<peça>/` (ignorada pelo git), com o `.glb`, a textura, o mapa de UV, o `.blend` e as prévias.
- **Destino aprovado:**
  - `assets/models/chartest/clothes/`: tronco, pernas, peças longas;
  - `assets/models/chartest/accessories/`: avental, coldre, cinto, chapéu;
  - `assets/models/chartest/hairs/`: cabelo e barba.

## Fazer uma peça nova

1. **Molde.** Copie o exemplo mais próximo para `build_<peca>.py` e troque o nome do `.glb`, o `OUT_DIR` (dentro de `out/`) e a textura.
   - A textura parte de uma imagem do projeto (`assets/textures/tecido_grosso.png`, `couro1.png`, `couro_claro.png`, `tecido_rasgado.png`) e é reduzida para 128×128, com tecido visível e cores terrosas.
   - Uma cor clara e neutra é melhor, porque o jogo tinge cada morador.
2. **Malha.** Escreva só a forma, em `build_<peca>_mesh`:
   - parta do corpo base (`clone_clean_body_bmesh`), recorte a região e empurre para fora;
   - feche as bordas com `extrude_closed_rim_inward`;
   - devolva os índices internos (`interior_indices`).

   Low poly PSX: poucas centenas de vértices, silhueta firme e nada de dobras finas.
3. **Pipeline.** Chame `finalize_garment_pipeline` (veja o `main()` do molde). Ele roda, nesta ordem:
   1. exporta o UV;
   2. faz o auto-fit (pesos e shape keys);
   3. exporta o `.glb`;
   4. confere a folga em 5 corpos;
   5. salva o `.blend`;
   6. gera as prévias.
4. **Rodar,** uma passada de uns 10 s:

   ```bash
   blender --background --python tools/blender/clothes/build_<peca>.py
   ```

   Leia o relatório de folga que o script imprime. Se algum corpo atravessa a peça, aumente a folga (`min_vert_gap`) ou a margem ali e rode de novo.
5. **Aprovação.** Mostre as prévias de `out/<peça>/` ao usuário, porque ele aprova visualmente. Só depois de aprovada, copie o `.glb` para a pasta de destino.
6. **No jogo.** Ponha a peça na escolha dos moradores (onde o sandbox escolhe roupa por sexo e ofício; procure por `clothes` e `accessories` no código e no `CLAUDE.md`). Pelo mesmo caminho ela fica disponível como `ItemKind::Armor` no corpo do jogador. Compile e abra o jogo; o usuário confere.

## Regras

- **Esqueleto e pose.** Sempre o mesmo esqueleto, na mesma pose de ligação do chartest. Nunca mexa em `chartest.blend` nem em `chartest.gltf`: o kit só lê.
- **Camadas.** Uma peça por cima de outra (avental, colete, capa) importa as peças de baixo como referência de folga, como faz `build_aprons.py`.
- **Recolorir.** Prefira variar a cor no jogo a criar outra peça. Só faça modelo novo quando a forma mudar.
- **Economia de tokens.** Os exemplos têm de 17 a 75 KB. Leia com grep e trechos (as funções `def` e o `main`), nunca inteiros.
- **Estilo.** PSX onírico de época, séculos XVII–XVIII: linho, couro, lã crua. Nada de visual moderno nem limpo demais.
