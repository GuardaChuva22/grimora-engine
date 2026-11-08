# Fora do combate

As regras de `SKILL.md` valem aqui: ficha, poses do chão para cima, nada para de vez, e a nota vem do usuário. O que muda é a fonte e o que conferir.

## Locomoção

Fonte: `tools/author_chartest_walk.py`, que grava direto no glTF e em `chartest_walk.bin`. Regenerar a caminhada regenera a corrida. Não crie `sword_walk.ron` nem `sword_run.ron`: eles sobrescreveriam o clipe nativo, e o legado já está em `authoring_archive/`.

- Caminhada: ciclo de 1 s a 1,2 m/s. `WALK_SPEED` no sandbox casa com isso.
- Corrida: ciclo de 2/3 s a 5,5 m/s, igual a `RUN_SPEED`.
- O passo define tudo: pé direito no chão em 0, esquerdo em 0,5. `gaits_step_at_the_same_points` confere isso e a velocidade do pé plantado. Se mudar a velocidade do clipe, mude a do jogo junto, senão o pé desliza.
- `sword_back` e os strafes são `.ron` e misturam com a caminhada (`play_mixed`), então precisam dos mesmos pontos de passo.
- Peso: o quadril desce no apoio e sobe na passagem, e o ombro gira contra o quadril. A cabeça é a parte mais estável.

## Pulo

`jump`, `jump_fall`, `jump_land*` são nativos. A queda segura a pose, em vez de repetir, e a aterrissagem vem do contato físico. Não recoloque o `jump` retargetado.

## Idle

`sword_idle` é nativo e foi o único movimento aprovado. Não crie `.ron`. Em clipes de idle próprios (`heavy_idle`), o laço fecha sem salto, a respiração move o tronco e o corpo nunca fica estático. O verificador acusa laço que não fecha.

## Rolamento e esquiva

`sword_backstep` ainda é chave à mão em `.ron`. `sword_roll` sai de `tools/gen_roll_clip.py` (rodá-lo sobrescreve o arquivo): `tuck` não é mais usado nele, porque era o que fazia a bola.

Rolamento é contato, não giro. Giro em volta do quadril enterra a cabeça ou flutua (o antigo enfiava 214 mm no chão). Por isso a altura e o avanço não são escritos: a cada passada, `tests/roll.rs` mede a malha, o quadril sobe até o tronco encostar e anda para o ponto de contato não deslizar. Escreva só o giro, a curva das costas, a inclinação que põe um ombro primeiro (`z` de `hips_rot`) e onde mãos e pés ficam em relação ao peito e ao quadril. O pé desce onde toca o chão e fica ali. A lâmina fica deitada para o lado, perto do eixo do giro, para não varrer o chão. Sem molas: mão e pé são contas sobre o corpo.

## Dano e morte

Entrada rápida, porque o golpe vem de fora: o corpo reage com o quadril empurrado primeiro, depois tronco, cabeça e braços atrasando. Saída lenta. A direção importa, então se o clipe assume um lado, diga isso no resultado. `die` mantém a entidade viva no mundo, então o clipe termina numa pose final que se sustenta.

## Feitiços

Clipes `spell_*` com eventos `stomp` e `blast`. A lança fica virada no punho, então em `spell_aim` e `spell_throw` o `blade` aponta para o pé da lança, não para a ponta. O golpe do feitiço é igual a um golpe de arma: corpo primeiro, mão por último, e o evento no ponto de maior força.

## Armas de fogo

Clipes `musket_*`, `pistol_*` e `gun_stand`, escritos por `tools/gen_gun_clips.py`. Portão: `tests/gun_clips.rs`.

- Tocam do `spine` para cima, com `gun_stand` (a base do `Moveset` `GUN`) embaixo quando parado e a marcha normal ao andar. `_hold` tem `steady` 0 e segue o quadril; `_aim`, `_fire` e `_reload` têm `steady` 1.
- `base` em cada recarga diz onde a arma fica, e tudo o que a mão esquerda faz sai de `gun.point`. Por isso a arma pode se mover em cada chave sem a mão soltar, desde que o braço direito alcance.
- Mudar um tempo da recarga muda também `reload` em `items.ron` (`ball`, `rod`, `rod_grip`, `strokes`, `cock`, `slide_in`, `slide_out`), que são eventos do jogo presos a esses instantes.
- Mudar `staff_at` muda `grounded.at` em `items.ron` e `GROUNDED_AT` no teste. O script imprime o valor.
- Cada puxada de vareta levanta o peito e cada empurrão o derruba sobre as mãos: é o que dá peso à recarga, mais do que a mão em si.

## Mão livre e objetos

`Control::HandLRot` gira a mão livre. `book_hold` depende do livro: se mudar `at`, `turn` ou `hand_l`, a página muda de lugar. O teste é `the_reading_hand_lies_on_the_page_with_both_elbows_bent`, em `tests/grip.rs`.

## Animais

Não usam `PoseClip`. Escolha a técnica pela quantidade:

| Animal | Como anima | Onde |
|---|---|---|
| Peixes, arraia, água-viva | Onda calculada (`SwimDesc::wave`, `SwimPoses`), tabela por espécie | `assets/fauna/*.ron` |
| Aves | `Animator` próprio, com chaves `Gaivota_keys.ron` | `<modelo>_keys.ron` |
| Caranguejo | Patas por IK (`step_feet`, `pose_legs`), braços em `Caranguejo_keys.ron` | `grimora/src/fauna/crab.rs` |

Em `_keys.ron`, `aim` é a direção do osso no espaço do modelo, `.L` espelha `.R`, e cada pose vira o clipe `pose:<nome>`. Aprove as poses com `animation: "pose:wings_top"` numa cena temporária. Para bichos, a mesma ideia de peso se aplica: a asa puxa para baixo com força e sobe solta, e a onda corre do corpo para a cauda com a amplitude crescendo.

`lint_clip.py` não lê esses arquivos. Use os testes em `crates/grimora/tests/fauna.rs`.
