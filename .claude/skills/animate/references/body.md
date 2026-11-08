# Limites do corpo

O solver não impõe nada disto (`Rig::solve` só verifica alcance). Até haver limites no solver, a checagem é sua.

## Medidas do projeto

Valores do guia do projeto e dos testes, os que você compara com `key_poses`:

| Coluna | Limite | Origem |
|---|---|---|
| `short`, `foot` | 0 mm. Algo acima disso significa membro esticado sem alcançar | Solver |
| `wrist` | 55°, até 65° em estocada; os testes barram em 70° (uma mão) e 75° (duas) | CLAUDE.md, `blows.rs` |
| `twist` | Cerca de 100°; os testes barram em 105° nos clipes de `BLOWS` | CLAUDE.md, `blows.rs` |
| `left <pulso> <torção>` | Pulso até 75°. Torção até 145° com a palma para cima sob um cano, até uns 115° nos outros casos | `gun_clips.rs`, estimativa |
| Ponta da lâmina | Até uns 40 m/s; testes barram em 45 | CLAUDE.md, `blows.rs` |
| Pé deslizando | Abaixo de 0,3 m/s | `blows.rs` |
| Corpo fora dos pés | Abaixo de 200 mm | `blows.rs` |

**Atenção:** `blows.rs` só olha a mão direita, então a coluna `left` passa sem julgamento: em `heavy_1` e `heavy_2` ela chega a 131° e 135° de torção. Uma torção alta aqui é sinal de `edge` ruim ou de mão esquerda em posição ruim. Rode com `POSES_TURNS=1` para ver a tabela por giro do fio.

## Amplitude humana de referência

Ponto de partida vindo de **fontes secundárias e às vezes divergentes**, não de medição do personagem. Personagem de jogo pode ter folga para estilo, mas folga consciente. Confirme antes de colocar em código.

| Articulação | Movimento | Aproximado |
|---|---|---|
| Pulso | Dobrar para cima ou para baixo | 60° |
| Pulso | Desviar para o lado | 20° |
| Antebraço | Girar a palma, cada lado | 80° |
| Cotovelo | Dobrar | 140 a 150°, sem passar de 0° |
| Ombro | Levantar à frente | 180° |
| Ombro | Rotação interna / externa | 70° / 90° |

Sem número confirmado por fonte: quadril, joelho, tornozelo, coluna e pescoço. Os limites de `torso` e `head` em `lint_clip.py` são estimativa e geram só `WARN`.

## Regras que evitam a maior parte dos ossos tortos

- Limites protegem, mas não dão vida. Um golpe dentro de todos eles ainda pode ser rígido: braços e mãos têm que ir longe do peito (`moves`, linha `reach`), e o corpo conduzir não significa prender o braço a ele.

- O pulso reto põe a lâmina em ângulo reto com o antebraço, e o fio `edge` aponta de volta pelo antebraço.
- O cotovelo fica do lado oposto ao da lâmina.
- `edge` nunca inverte entre duas chaves: o caminho do giro fica ambíguo e o braço torce.
- A lâmina nunca ultrapassa o antebraço nas espadas grandes. As mãos vão na frente; no fim do corte, o antebraço cruza o corpo e a lâmina ainda está à frente dele. Se a lâmina passa, um dos pulsos dobra além de 100°.
- Se a mão não precisar virar entre dois golpes, eles emendam mais rápido.
- Não oriente a arma separada da mão, e não mexa na pegada dentro de um clipe: quem manda nela é `chartest_grip.ron`.
- Controles de joelho e cotovelo são direções de polo, não posições. Um polo que cruza o plano do membro entre chaves faz o joelho ou cotovelo virar de repente.
- Pés plantados não recebem chave nova, e o peso precisa caber entre eles.
