# O que o usuário ensinou

Registro de notas e críticas. Acrescente no topo de cada seção. Esta é a calibragem do processo: o verificador precisa concordar com estas notas.

## Notas

| Data | Clipes | Nota | Palavras do usuário | Causa encontrada | Virou regra? |
|---|---|---|---|---|---|
| 2026-11-04 | `sword_1..3` | 6 | "espada curta está nota 6" | Ainda sem análise detalhada; preparo curto para o corte | Parcial: contraste é `WARN` |
| 2026-11-04 | `heavy_1..3` | 1 | "espada pesada está nota 1"; "ossos tortos, posições não naturais" | Corpo congelado no golpe e torção alta na mão esquerda | Sim: corpo congelado é `FAIL` |

## Primeiro teste da skill: armas de fogo (2026-11-04)

O usuário apontou os idles e as recargas como os piores, sem números. A medição por quadro concordou, e essa concordância é o que dá confiança nela:

| Clipe | Antes | Depois |
|---|---|---|
| `musket_hold` | braço 64 mm curto, pulsos 119° e 146° | alcança, pulsos 67° e 38° |
| `pistol_hold` | braço 46 mm curto, os dois cotovelos travados | alcança, pulsos 9° e 25° |
| `musket_reload` | braço 79 mm curto, pulsos 119° e 147° | alcança, pulsos 65° e 64° |
| `pistol_reload` | braço 88 mm curto, pulsos 110° e 111° | alcança, pulsos 61° e 72° |

Causas: mãos fora do alcance, por poses escritas sem conta; o `sword_idle` balançando o quadril por baixo; idle de 0,8° de tronco com a mão parada; molas na mão que segura o cano; ângulos interpolados na mão livre.

Lições para a skill:

- `lint_clip.py` deu 0 `FAIL` nos idles. Olhar só as chaves não basta: o portão real é medir o corpo, quadro a quadro, como o jogo toca.
- Mostrar o erro numa conta (ombro, braço, antebraço) resolveu mais rápido que qualquer tentativa.
- Avaliação do usuário, sem nota numérica: "ficou excelente, muito bom mesmo, melhorou muito". O método funcionou: conta do corpo, medição quadro a quadro como o jogo toca, varredura por script, e nenhuma imagem.

## Segundo teste: espadas grandes (2026-11-04)

Pedido depois da aprovação das armas de fogo. A primeira versão foi **recusada** (seção seguinte); a tabela é dela e mostra como os números podiam melhorar com o resultado piorando.

| Clipe | Antes | Depois |
|---|---|---|
| `heavy_1` | corpo 9% do caminho da mão, torção 126, mão esquerda 72/149 | corpo 64%, torção 98, esquerda 67/100 |
| `heavy_2` | braço 49 mm curto, perna 27 mm curta, 2 cotovelos travados, esquerda 79/136 | alcança, cotovelos 79/82, esquerda 67/99 |
| `heavy_3` | braço 71 mm curto, perna 35 mm curta, quadril parado 7 quadros, esquerda 92 | alcança, parado 1 quadro, esquerda 74 |
| `heavy_idle`, `_carry`, `_guard` | estátuas, 0 a 3 mm | peito e mão respirando, 10-32 mm |

Lições para a skill:

- Antes de virar número, eu estava escrevendo chaves à mão. A virada foi medir que um teste custa menos de 0,1 s: uma busca faz em 30 s o que eu fazia em 10 rodadas, e sem tela.
- A busca não conserta pose errada. No `heavy_2`, ela travou com o pulso em 86: a lâmina passava do antebraço no fim do corte. O que resolveu foi eu deixar o corpo levar o fim do golpe, com a lâmina menos girada.
- Chaves escritas à mão ficam para trás: mudei de onde o pé parte e esqueci três chaves com o valor antigo, e o pé escorregou. Faça tudo sair de uma variável.
- Evento vem depois do atraso: com a lâmina 0,05 s atrás do quadril, `combo` e `hit_off` ficam em relação a ela, senão o combo salta e o golpe termina antes de acertar.
- Teste que parecia regressão era outra coisa: em `bag.rs`, o alvo andava enquanto a espada, agora mais lenta, descia. Olhe o instante antes de culpar o clipe.

## Espadas grandes, recusa: "os bracinhos juntos demais" (2026-11-04)

Crítica do usuário: falta amplitude, braços e mãos grudados e para a frente, espada sempre na frente, rígido. Pediu algo dinâmico, como animações conceituais de personagem do Twitter, sem precisar ser lento e pesado como soulslike. Tronco e pés foram elogiados.

| Medida (`moves`, linha `reach`) | Recusado | Refeito | Espada leve |
|---|---|---|---|
| Mãos afastadas | 0,12 m | 1,16 a 1,27 m | 0,87 a 1,27 m |
| Braços cobrem (dir./esq.) | 0,30-0,44 m | 1,02-1,28 / 0,74-1,09 m | 0,69-0,88 m |
| Giro da lâmina | até 128° no `heavy_2` | 172 a 179° | |
| Lâmina à frente | até 65% do clipe | 21 a 46% | |

Nota do refeito: "melhor no geral", mas o idle "não tem sentido nenhum" (uma mão, espada pendurada para trás) e **não gostou de uma mão só: as pesadas são justamente para as duas mãos**.

Terceira versão: duas mãos o tempo todo, mantendo os arcos. As mãos vão do ombro direito ao quadril esquerdo enquanto o peito gira; braços cobrem 0,72-0,75 / 0,62-0,70 m (a recusada, 0,30-0,44), a lâmina gira 176-179° e fica à frente 39-49% do clipe. O idle segura baixo, com as duas mãos na barriga, o peito virado para a direita e a espada saindo para esse lado. Nota: **pendente**.

- Amplitude não é tirar uma mão. Com as duas no cabo, ela vem de levar as mãos de um lado do corpo ao outro e de deixar a lâmina fora da frente; o peito girando faz o resto.
- Com duas mãos, a torção ficou alta em quase todo quadro: era o polo dos cotovelos. Uma varredura de fórmulas de polo em todos os clipes baixou tudo de uma vez, antes de mexer em chave.
- Busca pelo pior quadro empaca; somando o excesso de cada quadro (`TRACE=2`) ela anda. `scripts/fit_arcs.py` faz isso e cobra por se afastar do arco escrito.

Lições:

- **Eu otimizei a medida errada.** "Quanto da mão vem do corpo" subiu e o golpe piorou: quem maximiza isso prende o braço ao peito. É um piso, nunca uma meta.
- **Mãos coladas à frente do peito prendem tudo.** O defeito era as mãos não saírem da frente do peito, não as duas mãos. (Na segunda versão tirei a esquerda do cabo, e o usuário recusou: veja abaixo.)
- **Meça o que o olho reclama.** Nenhum teste via amplitude. Agora `moves` reprova, e confirmei que os clipes recusados caem nesses portões.
- **Escreva o arco do braço, não a mão.** Cada chave diz azimute e elevação de braço e lâmina no espaço do peito; fio e cotovelos são derivados. Some o giro do peito ao escolher os ângulos, senão a ponta chega a 90 m/s.
- **A lâmina se arrasta de um lado só do antebraço.** Atravessar a linha do braço vira a mão (torção 170). Para trocar de lado, passe por cima.
- **Duas mãos, mãos no meio do peito.** Com a direita à frente do ombro direito, a esquerda fica 11 cm curta e o pulso passa de 100.
- **`roll` tira a torção sem mudar o arco.** Uma varredura de 12 valores, em segundos, resolveu pulso e torção que chaves à mão não resolviam.

## Braços atravessando o corpo, guardas com duas mãos (2026-11-04)

Crítica: no idle pesado de duas mãos, "os braços atravessando o corpo". O `clearance` só olhava braço contra cabeça e pescoço, então passava. Agora mede quanto cada ponto de antebraço e mão entra no tronco (barriga, quadril e coxas, `TRUNK`), e reprova acima de `SINK`. Os números mostraram 70 a 110 mm em **todos** os clipes pesados, não só no idle.

Pedido seguinte, já **aprovado** ("ficou ótimo"):

- **Guarda da espada pesada:** lâmina deitada e cruzada à frente, ponta para a direita, mãos no cabo na frente do peito.
- **Guarda da espada curta:** a pose cruzada que já existia, agora com a esquerda espalmada atrás da lâmina (meia-espada). Ficaram só as guardas 1 e 2, e `sword_block_1` agora começa da guarda 2.
- **Idle pesado:** guarda média, mãos na frente da barriga e lâmina para a frente e para cima.

Lições:

- **Uma busca que só olha pulso e torção põe o cotovelo dentro do corpo.** Os polos que eu escolhi por torção dobravam os cotovelos para dentro. Toda busca precisa pontuar também a profundidade no tronco (`fit_arcs.py` já faz isso).
- **E ela encolhe o movimento se puder.** Na primeira rodada com a profundidade, o giro da lâmina caiu para 135 a 139°. `fit_arcs.py` agora cobra por perder amplitude e por se afastar do arco escrito.
- **Duas mãos no cabo só servem se o pomo ficar do lado da mão esquerda.** Com a lâmina apontando para a esquerda, a esquerda iria para a direita da direita, e os braços se cruzariam. Nesse caso, a esquerda vai espalmada na lâmina.
- **Antes de pôr a segunda mão, meça o alcance.** Na guarda 3 da espada curta, a lâmina ficava a uns 80 cm do ombro esquerdo: nenhuma mão chega, e a pose precisa ser outra.
- **Idle de duas mãos é guarda.** As mãos baixas e coladas na barriga fazem os antebraços entrarem no quadril, e a espada saindo de lado "não tem sentido".

## Pistola: idle solto e mira esticada (2026-11-04), **aprovado** ("ficou ótimo")

- Idle: braço pendurado ao lado da coxa, cotovelo um pouco para trás, cano para o chão. O braço livre sai do overlay (`Animator::overlay_leave`) e vem do clipe de baixo, então balança ao andar como na espada curta. Overlay da coluna para cima congela o braço que não segura nada: evite.
- Mira no estilo Red Dead: braço quase travado (97 no `moves`), peito girado 40° para levar o ombro à frente, cabeça girando de volta. Ache o ponto da mão variando a distância em passos de 1 a 2 cm até `arm` dar 97 a 98 sem `short`.

## Rolamento (2026-11-05)

Crítica: "ele entra em um cannonball estranho muito feio, tem que ser mais natural de combate tipo elden ring/dark souls". Causa medida: `tuck` juntava mãos e pés no peito, e o corpo só girava em volta do quadril, com 214 mm de cabeça no chão, 5 quadros de cabeça para baixo no ar e 2,5 m/s de deslizamento. Refeito por ombro, costas e quadril, de braços e pernas abertos (`tools/gen_roll_clip.py`, `tests/roll.rs`). A rolagem foi aprovada ("a rolagem em si ficou boa"), mas o braço "se inverte e quebra durante a rolagem voltando ao normal no final". Causa: o polo do cotovelo cruzou a linha do ombro ao punho, e o braço virou 149° em um quadro. Pulso e torção não pegam isso, porque só olham o valor em cada quadro. Agora `the_arms_do_not_flip_in_a_roll` mede quanto cada osso do braço gira por quadro e reprova acima de `MOST_FLIP`.

- **Toda busca de polo pontua giro por quadro.** Sem isso, ela acha um valor bom em cada ponta e um salto no meio.
- Em giro de corpo inteiro, escolha a pose da mão relativa ao peito e deixe o fio e o polo para a varredura, por fase e com uma chave no meio da troca. A torção caiu de 175° para uns 90° assim.
- Blend direto entre dois frames de mão pode torcer o antebraço. Mova a lâmina e deixe o giro em volta dela crescer à parte.

## Calibragem de `lint_clip.py`

Medido em 2026-11-04:

| Clipe | FAIL | WARN |
|---|---|---|
| `sword_1` | 0 | 1 |
| `sword_2` | 0 | 3 |
| `sword_3` | 0 | 2 |
| `heavy_1` | 6 | 1 |
| `heavy_2` | 3 | 2 |
| `heavy_3` | 4 | 1 |

O verificador separa os dois grupos, mas só há duas notas. Ele sabe apontar defeitos, não dar nota.

## Preferências permanentes

- Peso de Dark Souls com velocidade de anime; fluido e interessante.
- O corpo inteiro se move, principalmente em ataques. Eles precisam ser bem dinâmicos.
- Mão no cabo é olhada de perto, e poses duras e retas são rejeitadas.
- Espadas pesadas: duas mãos, arcos largos. Guardas: lâmina cruzada à frente do corpo.
- Sem mocap, Mixamo, pacotes ou geradores.
- O único movimento aprovado até agora é o `sword_idle`, e mesmo nele a pegada foi criticada na época.
- Em novembro de 2026, o usuário deixou claro que validar por imagem não está funcionando e gasta token.
- Não basta "o teste passou": dizer isso sem a nota do usuário não vale nada.

## 2026-11-05: corrida, mosquete parado, sacar do bolso
- `sword_run` antigo: "esquisitíssima". Causas: quadril subia 14,5 cm no voo, cotovelos quase retos (32-48°), braços com pouca amplitude (20-30°), e o pé do balanço subia no meio do caminho, sem chutar o calcanhar para trás. A nova versão sobe 7 cm, tem cotovelos perto de 90° (mais dobrados na ida), braços a 36-44°, giro de quadril de 10° e de peito de 12° contra ele, a cabeça nivelada e o calcanhar subindo atrás antes de o joelho levar o pé à frente. Ainda sem nota.
- `musket_hold`: "mão direita torta, mosquete podia estar mais pra cima, como se segura um rifle". Agora fica em posição de porta-arma, achada por varredura medida. A mão esquerda vai ao lado da coronha, com a palma nela: com a palma por baixo, a torção do antebraço chegava a 112-129°.
- `draw_pocket`: "inverte a mão de forma desnecessária". O `best_hand` escolhia livremente o giro da mão. Quando não há o que pegar com um giro próprio, mantenha a rotação da mão relativa ao antebraço, que é a de quando ela está pendurada.
- Mesma data, segunda volta. `draw_pocket` aprovado. Corrida: "tecnicamente boa, mas ainda não me pega". Agora os pés pousam quase numa linha só (x ±0,055, antes ±0,115, que parecia dois trilhos), o apoio ficou um pouco mais longo (0,26 do ciclo) e o braço cruza o peito ao ir à frente. O braço da espada balança menos (30°), senão a ponta salta a mais de 45 m/s ao pular de uma corrida. Mosquete: "mais junto do corpo", com a empunhadura 8 cm mais perto da barriga. O pulso direito fica em 64° em qualquer giro: é o ângulo da coronha.
- Terceira volta da corrida: "talvez a coluna ereta demais, falta movimento no corpo todo". O `spine` era fixo em -8°. Agora ele é a onda entre o quadril e o peito: torce no meio do caminho, dobra para o lado e dobra mais um tempo depois de cada pouso. O peito inclina 14° e dobra 3,5°, os ombros vão e voltam com o braço, e a cabeça fica quase parada no mundo. Em `author_chartest_walk.py`, `desired` são giros no mundo, não relativos ao pai.
- Corrida: nota 7,5, mantida assim por enquanto. `draw_sling_r`: "entorta todo o braço". Com a boca para cima, a mão pegava o cano e o mosquete precisava girar na mão. Agora ele vai de boca para baixo, quase em pé no lado direito das costas, e a mão o pega pela empunhadura atrás do ombro com o mesmo giro que terá na mão (`musket_frame`), achado por varredura (pulso 21°, torção 19° ao pegar). Regra: quando algo sai das costas, a mão deve pegar onde vai segurar, não noutro ponto.

## draw_sling_r (saque do mosquete) — 2026-11-06
- Rejeitado duas vezes: "o braço que puxa continua se quebrando todo", "parece a original mas do lado invertido". Pulso esquerdo chegava a 107° e torção a 116° na subida.
- Causas: (1) a orientação da mão era interpolada no espaço do corpo (ângulos ou slerp), não em relação ao antebraço; (2) a mão escorregava pelo cano no clipe enquanto o jogo prendia o mosquete rígido na mão, então o que o jogo mostrava não era o que o script calculava.
- Regra: mão que carrega um objeto preso a ela não escorrega nele; e a orientação da mão se interpola relativa ao antebraço (`left_forearm`, espelho do `solve_arm`), assim o pulso só dobra o que dobra nas pontas.
- Ferramenta: `scripts/plan_sling.py` (rodar da raiz) mede cada quadro em Python, igual ao `gun_clips` do Rust (conferido), com braço contra tronco, pescoço e cabeça e a arma contra o corpo, e busca o plano. Sem as regras de "pêndulo" (coronha baixa, boca para cima) a busca fazia o mosquete dar cambalhota sobre a cabeça; sem o braço contra o pescoço, enrolava o braço na nuca.

## draw_sling_r (saque do mosquete das costas), nota 3
Crítica: "o braço ainda entorta, ainda mais na hora de desequipar, o mosquete atravessa o corpo/braço; quem pega é a mão esquerda e nem é na coronha, é no cano". Causas: (1) overlay sem `set_overlay_steady`, então o tronco seguia o balanço do quadril do clipe de baixo e a coronha entrava no quadril; (2) mão errada para o lado do mosquete. Refeito: a direita pega a coronha atrás do quadril. A geometria decide o pulso: com a pegada na altura do quadril e o mosquete quase em pé, o antebraço desce paralelo à coronha e o pulso dobra 100°; em diagonal, com a pegada no fim das costas (y 1,05), caiu para uns 87° só no quadro da pegada. Métricas de esforço não pegam cotovelo debatendo: a busca cobra velocidade do cotovelo. Busca a 10 Hz deixa passar penetração entre quadros: refine a 20-30 Hz.

## Arremetida (2026-11-11): nota implícita muito baixa, "horrendo"

Clipes `leap_*` escritos à mão por pose (`gen_leap_clips.py`, apagado): carga, subida, encolher, quebra, voo deitado, pouso de joelho. O usuário: o agachamento e o pulo originais (`jump_land` segurado, `jump`) eram melhores, e o voo "péssimo" (corpo deitado reto, braços colados para trás, cabeça virada, parece boneco de pano). Os portões passavam todos (pulso, `clearance`), e não diziam nada sobre isso.

Regras: não troque um clipe nativo aprovado por um escrito à mão sem pedido. Para um movimento de corpo inteiro novo (voo, acrobacia), procure antes um clipe de captura nos pacotes que o usuário trouxe (`assets/animations/quaternius/UAL2_Standard.glb`, escolhido pelo nome em `LEAP`, `import_quaternius_clips.py`) e só escreva à mão o que não existir. Deitar o corpo inteiro pelo controlador (`Flight::lean` perto de 1,5) esconde a pose: mantenha a inclinação baixa (0,6) e deixe o clipe mostrar o movimento.

## Katana (2026-11-11, `tools/gen_katana_clips.py`)
- Duas mãos juntas no cabo pedem outro polo do cotovelo esquerdo: `POLE_L 0.02,0.56,0.04,-0.21` (com o das grandes, pulso esquerdo 104).
- Chave por chave os limites passavam, mas os antebraços entravam na barriga (até 105 mm): ponha a profundidade do `clearance` na varredura de pose (deep + short + foot + excesso de pulso). Mãos baixas à esquerda no fim do corte (elevação -59) afundam; -26 com alcance 0,98 fica limpo.
- Na volta do corte para a guarda, o antebraço desfaz a torção do tenouchi (twist passa por 0): o pico de pulso fica no meio da transição, não nas chaves. Meça os quadros entre chaves.
- Resultado ainda sem nota do usuário.

## draw_holster_r0 / l0 (saque das pistolas do coldre), 2026-11-19

- Crítica: "simplesmente horríveis, totalmente bugadas". No jogo, logo depois de pegar, o pulso ia a 114° e a torção a 154°: a mão virava do avesso. Causas: (1) o polo do cotovelo era interpolado em linha reta entre chaves muito diferentes e passava rente à linha do braço, e aí o solver do jogo virava o cotovelo; o gerador, com outro ombro, não via; (2) girar a mão em linha reta no mundo, do coldre até a pegada, torce o antebraço até 175°.
- Agora: `holster_frames` em `gen_stow_clips.py` escreve um quadro por chave, com o polo escolhido a cada quadro (perto do anterior) e gravado perpendicular ao braço; a pistola sai reta pelo cano e só tomba no fim (`tip`), e o giro até a pegada é interpolado em relação ao antebraço. Plano achado por busca em `HOLSTER_PLANS`.
- Busca sem limites trapaceia: achou tronco a 60-87° e a pistola descendo. Prenda tronco (até ~15/30/15°) e direção do caminho antes de buscar.
- Meça no jogo (`CLIP=draw_holster` em `gun_clips`): o gerador dizia braço alcançando, o jogo dava 65 mm curto ao pegar com tronco (15, 30, 15). Inclinar para o outro lado (z -10) resolveu.
