# Combate

## Fases de um golpe e eventos

| Fase | O que acontece | Evento |
|---|---|---|
| Preparo | O corpo carrega: o quadril recua e gira para trás, o peso vai ao pé de trás, a lâmina fica lenta | |
| Arrancada | O pé empurra, o quadril vira, o resto ainda atrasa. A lâmina até recua um pouco | `swing` |
| Ativo | A lâmina passa na velocidade máxima, com o peso caindo à frente | `hit_on` a `hit_off` |
| Seguimento | A inércia leva o corpo além do alvo, a lâmina freia por último | |
| Retomada | O peso volta. Aqui a pose pode fluir para o próximo golpe | `combo`, `cancel` |

Sem `hit_on`/`hit_off`, a arma não acerta nada. Os eventos abrem janelas de jogo: mudar seus tempos muda o combate, então confira `Weapon` em `assets/items/items.ron`. Se `impact` existir, ele dispara `update_ground_hit`.

## O corpo inteiro faz o golpe

Um corte convincente vem do chão. O pé empurra, o quadril gira, o tronco é puxado, o ombro avança, o cotovelo abre e o pulso vira por último. O teste mede essa ordem pelos picos de velocidade. Os estudos de esgrima apoiam isso, mas nem todo golpe segue o padrão, então adapte:

- **Corte horizontal ou diagonal:** a cadeia completa, com muito giro de quadril.
- **Corte descendente:** menos giro, mais queda do quadril e flexão do tronco para a frente. O peso desce junto com a lâmina.
- **Estocada:** o quadril vai em linha reta, a ponta quase não gira. A velocidade é da translação, e o passo é longo.
- **Golpe no chão:** o corpo inteiro cai, joelhos dobram e o seguimento é curto, porque o chão para a arma.
- **Giro completo:** a cabeça vai à frente e o pé de apoio pivota, em vez de ficar plantado e torcer o joelho.

Em todos eles, cheque o movimento de cada parte que não seja o braço. Se esconder o braço e o resto do corpo não indicar para onde vai o golpe, o corpo não está participando.

## Leve e pesado são movimentos diferentes

Peso não é velocidade multiplicada: a arma pesada comanda o corpo, enquanto a leve é comandada por ele.

| | Espada leve (`sword_*`) | Espada pesada (`heavy_*`) |
|---|---|---|
| Mãos | Uma; a outra equilibra e atrasa | Duas (`grip_l` 1) |
| Preparo | Curto, com o pulso participando | Longo, com o corpo inteiro carregando |
| Quem inicia | Ombro e mão junto do quadril | Quadril e pernas, bem antes dos braços |
| Arrancada | Rápida | Atrasada: a lâmina resiste antes de sair |
| Seguimento | Curto, freia e emenda | Longo: a arma leva o corpo, com passo de recuperação |
| Retomada | Rápida, pronta para combo | Lenta, a arma é trazida de volta |
| Passo | Curto | Longo e com peso real no pé da frente |

### Como os clipes atuais estão (não são meta)

| Clipe | Nota | Preparo até `swing` | `swing` até `hit_off` | Verificador |
|---|---|---|---|---|
| `sword_1` | parte do 6 | 0,23 s | 0,14 s | 0 `FAIL` |
| `heavy_1` | parte do 1 | 0,38 s | 0,30 s | 6 `FAIL` |

Os defeitos do `heavy_1` são quadril e tronco congelados durante o golpe, de 0,45 a 0,55 s e de 0,63 a 0,90 s. O corte em si é longo demais para o preparo. Provável caminho: preparo maior e mais carregado, janela ativa mais curta, e seguimento longo, sem paradas.

Meta de tempo pedida pelo usuário ainda não existe. Se o usuário trouxer contagens de quadro de jogos que admira, guarde em `learned.md`; é só tempo, nunca copie o movimento.

## Regras das espadas grandes

Pesado quer dizer duas mãos, não lento. As mãos ficam no cabo e viajam de um ombro ao quadril do outro lado enquanto o peito gira; a lâmina passa a maior parte do clipe fora da frente. Regras mais abaixo que falem em mãos presas à frente do peito são da versão recusada: `learned.md` manda.


- Moveset: `heavy_1..3`, idle, guarda, bloqueio e `heavy_carry`. `Moveset::of` olha o prefixo `heavy` do clipe da arma.
- `grip_l` vai a 1 e a mão esquerda fica a `TWO_HANDS_APART` da direita; com duas mãos, a esquerda é definida pelo cabo, e a posição de `hand_l` perde efeito.
- A zweihänder é a mais longa, e é com ela que o `clearance` mede. Passou com ela, passa com as outras.
- O evento `impact` em `heavy_3` chama `update_ground_hit`.

## Combos e trocas de clipe

- O golpe seguinte começa onde o anterior está no evento `combo`. O teste barra salto de mais de 100 mm na mão, 15° na lâmina e 80 mm nos pés (`COMBO`, em `blows.rs`).
- Emende energia, não só pose: o final de um golpe deve já carregar o preparo do próximo. Se a mão não precisar virar, a emenda é mais rápida.
- Entrar num golpe tem mistura de 0,12 s, então o começo do clipe é parcialmente engolido quando se vem andando ou correndo. Antecipação curta demais não aparece.
- Sair de um golpe tem mistura de 0,3 s, e bloqueio de 0,05 s. As durações estão em `blows.rs`; se mudar, mude lá e no sandbox.
- Bloqueio: cada `sword_block_N` começa na pose da guarda anterior e termina na `N`.
- Reação e esquiva cortam o golpe (`cancel`). O teste `changing_clips_does_not_jump` mede a mão e a ponta em cada troca.

## Sobre o que o jogo já faz

Não anime isto no clipe:

- `hit_stop` (clipes dos dois lados a 0,05 de velocidade);
- deslocamento: use `root` no clipe (`root_delta`), e o controlador move a cápsula;
- rotação para o alvo: `ThirdPersonController::turn_to`;
- rastros, faíscas e som: eventos e efeitos.

Pense no golpe com tudo isso somado. O clipe precisa deixar espaço para a parada de impacto no momento certo.

## Referências

Quando precisar de direção, nunca copiando movimento: ver `research.md`. As palestras de David Rosen (poucas chaves e técnica procedural) e de Skullgirls (chaves fortes, antecipação e tempo) são as mais próximas do que buscamos.
