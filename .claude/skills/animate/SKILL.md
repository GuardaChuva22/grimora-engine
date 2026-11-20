---
name: animate
description: >-
  Processo para criar ou refazer animação de personagem 3D de alta qualidade no Grimora Engine sem depender de olhar imagens. Use SEMPRE que o pedido envolver animação, golpe, ataque, combo, espada, moveset, pose, keyframe, clipe, rolamento, esquiva, bloqueio, reação a dano, morte, feitiço, corrida, caminhada, pulo, recarga ou movimento de animal, ou arquivos em `*_clips/*.ron`, `*_keys.ron`, `pose.rs` e o animador, mesmo que a palavra "skill" não apareça e mesmo quando o pedido for só "esse movimento está feio/duro/torto". Prioridade: combate dinâmico e com peso.
---

# Animar bem no Grimora

Meta (atualizada em 2026-11-04: o usuário quer amplitude e soltura de animação conceitual, sem precisar ser lento como soulslike; veja `references/learned.md`): golpes com o peso de Dark Souls e a velocidade de anime, feitos aqui, à mão e por procedimento. Geradores por texto estão fora. Pacotes só quando o próprio usuário os trouxer: em 2026-11-11 ele trouxe os combos e o pulo do Quaternius, aplicados por `tools/import_quaternius_clips.py` (`light_*`, `medium_*`, `great_*`, `jump*`); não ofereça outros por conta própria.

## Teste verde não significa animação boa

Em novembro de 2026 a espada pesada recebeu nota **1** do usuário, e `blows`, `clearance` e `key_poses` passavam todos. Esses testes só pegam parte do que é quebrado, e nunca o que é sem graça. Por isso:

- Nunca diga que uma animação ficou boa porque os testes passaram. Diga o que passou e o que ainda depende do olho do usuário.
- A nota vem do usuário, e cada crítica vira regra (passo 7).
- Não valide por imagem. Isso gasta token e já se mostrou inútil para julgar movimento. Só capture quando um número apontar um defeito que você não consegue localizar, ou quando o usuário pedir.

## Passo 1: descobrir de onde o clipe vem

Editar a fonte errada faz o trabalho sumir ou não fazer efeito.

| Clipe | Fonte | Observação |
|---|---|---|
| `sword_1..3`, `heavy_*`, bloqueio, guarda, esquiva para trás, feitiços | `assets/models/chartest/chartest_clips/*.ron` | Assados ao carregar o modelo |
| `sword_roll` | `tools/gen_roll_clip.py` | Rodá-lo sobrescreve o `.ron`. Portão: `tests/roll.rs` (chão, deslizamento, bola) |
| `sword_walk`, `sword_run`, `jump*` | `tools/author_chartest_walk.py` | Nativos do glTF. Não crie `.ron` deles |
| `sword_idle` | `mixamo_idle.py`, na importação | Único aprovado. Não crie `.ron` nem mexa sem pedido |
| `musket_*`, `pistol_*`, `gun_stand` | `tools/gen_gun_clips.py` | Rodá-lo sobrescreve os `.ron`. Antes, confira se o script atual reproduz os arquivos, para não perder edições do animador. Ele também imprime o que `items.ron` precisa |
| `draw_*`, `stow_*`, `item_offer`, `item_eat` | `tools/gen_stow_clips.py` | Rodá-lo sobrescreve os `.ron` e `chartest_stow.ron`. São overlays do `spine` para cima; meça com `CLIP=draw_` em `gun_clips` e `CLEAR=draw_` em `clearance` |
| Animais | `<modelo>_keys.ron` e `assets/fauna/*.ron` | Veja `references/other-motion.md` |

Leia o arquivo de referência do seu caso **antes** de escrever qualquer chave:
- Golpe, bloqueio, esquiva: `references/combat.md`.
- Locomoção, feitiço, reação, recarga, animal: `references/other-motion.md`.
- Sempre: `references/body.md` (limites do corpo) e `references/learned.md` (notas e críticas do usuário).

### O clipe toca sozinho ou por cima de outro?

Veja no jogo antes de medir. Guarda, armas e `heavy_carry` são overlays do `spine` para cima (`overlay_loop`, `overlay_once`), e as pernas e o quadril vêm do clipe de baixo. Isso muda tudo:

- Quadril e pés escritos no overlay não fazem efeito. O corpo inteiro precisa do clipe de baixo.
- O balanço do clipe de baixo leva tudo junto: com o `sword_idle` embaixo, a coronha do mosquete escorregava 128 mm no chão. Se algo precisa ficar parado no mundo, o clipe de baixo tem que colaborar (`gun_stand`).
- Meça com o `Animator` real, clipe de baixo mais overlay, como faz `tests/gun_clips.rs`. Medir o clipe sozinho engana.

## Passo 2: escrever a ficha do movimento

Sem números de chave ainda. Escreva a ficha em poucas linhas e, se o movimento for novo, mostre ao usuário.

```text
clipe / arma / de onde vem / para onde vai
intenção: uma frase ("corte diagonal descendente que joga o peso para a frente")
linha de ação: a curva única que vai do pé de apoio até a ponta
peso: de qual pé para qual, e em que fase
passo: qual pé, quando, quantos metros
fases em quadros a 60 Hz: preparo | arrancada | ativo | seguimento | retomada
quadril, tronco, cabeça: ângulos no início e no fim de cada fase
lâmina: plano, início e fim do arco, para onde o fio aponta
o que atrasa: a ordem do corpo até a ponta
```

Se você não conseguir preencher, ainda não sabe o que está animando. A maioria dos movimentos ruins nasce aqui, sem decisão sobre peso e linha de ação.

## Passo 3: poses extremas, do chão para a mão

Escreva só os extremos, nesta ordem, porque cada um depende do anterior:

1. `root` e pés: onde o corpo está e onde se apoia.
2. `hips`, `hips_rot`: o peso e o giro. O quadril fica sobre os pés, entre eles em passo largo.
3. `torso`, depois `head`: a cabeça olha para o alvo antes do golpe e chega primeiro.
4. `hand_r`, `blade`, `edge`, `elbow_r`.
5. `hand_l`, `elbow_l`, `grip_l`.

### A mão não acompanha o corpo sozinha

`hand_r`, `hand_l`, `blade`, `edge` e os cotovelos ficam no espaço do `root`, não do tronco. Girar o tronco **não** leva a espada junto, e é por isso que o corpo pode parecer desligado do golpe. Ao girar o corpo em θ graus (yaw positivo é para a esquerda), gire antes esses controles em volta do quadril e só então some o movimento próprio do braço:

```text
x' = x·cos θ + z·sin θ
z' = -x·sin θ + z·cos θ
```

Ponha o quadril em (x, z) antes de girar e devolva depois. Não digite esses valores de cabeça: gere as chaves com um script numa pasta temporária, igual para arcos.

Cheque as poses antes de continuar. O limite de cada coluna está em `references/body.md`:

```bash
POSES=heavy_1 cargo test --release -p grimora_scene --test blows key_poses -- --nocapture
```

Esse teste **só imprime** e olha só as chaves. Quem julga é você. Colunas: `short`, `foot` (mm que faltam para alcançar), `wrist`, `twist`, `left <pulso> <torção>`, e depois de `edge` os mesmos valores com o melhor giro do fio. Se o `edge` sugerido for bem diferente de 0, o fio está mal escolhido.

### Ache a pose por conta, não por tentativa

No Chartest, o braço tem 0,26 m + 0,25 m, o ombro direito fica em (0,19; 1,45; 0,11) em repouso, e o cotovelo não desce mais de 0,26 m abaixo do ombro. Faça a conta antes de escrever:

- **Mão longe demais:** a distância do ombro ao pulso precisa ficar abaixo de cerca de 0,49 m, ou o cotovelo trava reto. O braço travado também tira o objeto do lugar, e a outra mão erra o que ia segurar.
- **Pulso quebrado:** com a mão abaixo do cotovelo, o antebraço aponta para baixo, e algo apontando para cima na mão dobra o pulso além de 100°. Primeiro decida a direção do antebraço, depois a do objeto.
- **Objeto na mão:** com o pulso reto, o cabo fica em ângulo reto com o antebraço. Na pistola e no mosquete, o cano sai perto da linha do antebraço, a 25-30° dela.
- **Quando restarem opções:** faça uma varredura. Um script temporário testa valores de direção e polo, roda a medição e fica com o melhor. Custa segundos e nenhuma imagem, porque os `.ron` são lidos ao rodar o teste, sem recompilar.

## Passo 4: tempo e sobreposição

Só agora preencha o movimento. Regras e motivos:

- **Dois valores iguais em chaves seguidas são uma parada real.** A curva é cúbica monótona, então a velocidade cai a zero ali, e a mola só disfarça. Em vez de repetir o valor, deixe a segunda chave avançar um pouco na mesma direção (uma "pose em movimento"). Durante o golpe, nada do corpo para.
- **Pico de canal também para.** Um canal que inverte de direção numa chave tem velocidade zero nela. Inverta no fim das fases, não no meio do corte.
- **Nada chega junto.** Cada elo pica depois do anterior, de 1 a 3 quadros: quadril, tronco, ombro/cotovelo, mão, lâmina. Na recuperação, a ordem se repete.
- **O contraste faz a velocidade.** A rapidez do corte vem do tempo lento em volta. Encurtar o corte sem alongar o preparo só deixa o golpe nervoso.
- **Arco é arco.** Entre duas chaves a mão anda quase em linha reta. Gere ao menos 3 chaves do corte em cima de um círculo, por script.
- **Mola não conserta pose.** Com `springs: true` (padrão), o clipe final atrasa e passa um pouco do alvo (`Control::spring`, em `pose.rs`). Pés e `root` não têm mola. O que você vê com o animador parado é a chave crua; tocando, é com mola.
- **`hit_stop` e os efeitos são do motor.** Não anime a parada de impacto no clipe.
- **Mão segurando algo não leva mola.** A mola da mão livre é a mais lenta (4,2 Hz), e no tiro do mosquete a mão escorregava 126 mm pelo cano. Use `springs: false` e escreva o tranco nas chaves.
- **Idle não é estátua.** Mova o peito, a arma e a cabeça com ondas de períodos diferentes, que fecham juntas no fim do laço. Como a mão fica no espaço do `root`, ela precisa respirar junto, senão fica pregada no ar.
- **Rotação da mão livre:** ângulos interpolados entre duas orientações distantes podem dobrar o pulso no meio do caminho. Gere chaves intermediárias pelo giro mais curto (`turned_between`, em `gen_gun_clips.py`).

## Passo 5: portões, nesta ordem

Rode tudo e corrija o pior antes de passar ao seguinte. Eles gastam pouco token, então prefira várias voltas aqui a abrir imagens.

```bash
python -I .claude/skills/animate/scripts/lint_clip.py heavy_1
```
Verificador das chaves: corpo congelado no golpe, partes que não entram, giro de quadril e tronco, contraste, ordem dos picos, amplitude, laço que não fecha. `FAIL` barra, e `WARN` exige um motivo, porque os limites são provisórios. Separou a espada curta (nota 6) da pesada (nota 1), mas só olha as chaves e não conhece o corpo: deu 0 `FAIL` nos idles de arma, os piores clipes medidos.

```bash
BLOW=heavy_1 cargo test --release -p grimora_scene --test blows blows_are -- --nocapture
```
Só serve para clipe com `swing`, `hit_on` e `hit_off`. Mede o clipe assado sem reprovar: ordem dos picos, ponta no preparo e no corte, plano do corte, pé deslizando, corpo sobre os pés, pulso.

```bash
cargo test --release -p grimora_scene --test blows -- --nocapture
```
Reprova de verdade os clipes em `BLOWS`, confere emendas de `COMBO`, trocas de clipe e passos. Golpe novo entra nessas listas no mesmo trabalho.

```bash
cargo test --release -p grimora_scene --test clearance -- --nocapture
```
Lâmina atravessando o corpo, braços contra cabeça e pescoço, e antebraços e mãos entrando no tronco (linha `(... in the trunk: N mm deep at most)`). Reprova nos `heavy_*`; `CLEAR=<prefixo>` mede outros sem reprovar. Rode-o em todo clipe com as mãos perto do corpo, inclusive os da espada curta e das armas.

```bash
cargo test --release -p grimora_scene --test gun_clips -- --nocapture
```
Mede cada quadro como o jogo toca: braço travado, pulsos, torção, estátua, mão escorregando, coronha no chão. Reprova nos clipes de arma; `CLIP=<prefixo>` mede qualquer outro sem reprovar, e é o portão mais confiável que temos. Use em todo overlay e idle, trocando `BASE` se o clipe de baixo for outro.

```bash
cargo test --release -p grimora_scene --test moves -- --nocapture
```
Mede cada golpe e cada laço quadro a quadro, com molas: amplitude (linha `reach`: mãos afastadas, quanto os braços cobrem, giro da lâmina, tempo à frente; reprova braços grudados), quanto do caminho da mão vem do corpo (é um piso, não maximize), o que para no meio do golpe, preparo contra corte, a ponta continuando depois do golpe, e estátuas. `TRACE=3` imprime a linha do tempo: leia isso antes de mudar qualquer número, porque quase todo defeito tem um instante.

### Não ajuste polo e giro à mão

Depois de escolher as poses, deixe a busca achar giros do fio, polos e pequenos ajustes (para os arcos de `tools/gen_heavy_clips.py`, `scripts/fit_arcs.py`, que soma o excesso de cada quadro e cobra por se afastar do arco; `fit_beats.py` e `bake_fit.py` são da versão antiga, com `Beat`s). O teste roda em menos de 0,1 s, então centenas de tentativas custam segundos, e a saída é uma linha. Limites: ela não inventa pose nova, só ajusta em volta da sua. Quando trava, a pose está errada (no `heavy_2`, a lâmina passava do antebraço) e a saída é você mudá-la.

Se mudou o `Rig` ou algo em `pose.rs`, rode também `cargo test -p grimora_scene --test grip -- --nocapture`.

## Passo 6: entregar

Não diga "ficou bom". Mande, em poucas linhas:

- o que mudou e a intenção do golpe;
- `FAIL`/`WARN` restantes e o motivo de cada um;
- os números que mudaram (antes e depois);
- o comando para ver:

```bash
cargo run --release -p animator
```

Peça uma nota de 0 a 10 e o pior momento do movimento. Mostre pedaços pequenos cedo: um golpe aprovado por vez, sem refazer um conjunto inteiro às cegas.

## Passo 7: transformar a crítica em regra

Registre em `references/learned.md`: clipe, nota, crítica nas palavras do usuário, causa encontrada. Depois vá além da correção daquele clipe:

1. Dá para medir? Coloque em `scripts/lint_clip.py` ou nos testes e confirme que o verificador reprova a versão criticada.
2. Não dá? Escreva a regra em `references/combat.md` ou `other-motion.md`, com o motivo.

Se o verificador aprova o que o usuário reprova, o defeito é do verificador: ajuste `THRESHOLDS` e cite a nota.

Se duas tentativas seguidas não melhorarem a nota, pare de ajustar números. Volte à ficha do passo 2 e pergunte o que está errado no movimento, porque o problema costuma ser a ideia, não a chave.

## O que ainda não existe

Não trate estas peças como prontas nem as cite ao usuário como se existissem. Quando alguma for construída, atualize esta skill.

- **Limites no solver:** `Rig::solve` só verifica alcance e aceita qualquer torção. Os limites hoje são conferidos por você.
- **Pulso por eixo:** `wrist` é um ângulo só e não separa dobrar (cerca de 60°) de desviar para o lado (cerca de 20°). Um pulso quebrado para o lado pode passar.
- **Colisão de arma com o corpo:** `clearance` só conhece a espada. Mosquete e pistola atravessando o corpo não são medidos.
- **Física no avaliador:** não há centro de massa com a arma, trajetória balística nem peso da espada.
- **Compilador geral de golpes e molas no lugar da curva:** `tools/gen_heavy_clips.py` é o primeiro exemplo de golpe escrito como `Beat`s, com busca (`scripts/fit_beats.py`), mas só serve às espadas grandes. O resto ainda é chave à mão. Pesquisa em `references/research.md`.
