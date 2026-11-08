# Pesquisa para evoluir o processo

Levantado em novembro de 2026, lendo só resumos e descrições: nada foi rodado, e os artigos não foram lidos por inteiro. Não há skill ou repositório pronto que faça animação de combate boa sem mocap. Para construir as peças que faltam, leia a fonte primeiro.

## Para construir

| Peça | Técnica | Fonte |
|---|---|---|
| Física no avaliador | Restrições de momento geram movimento dinâmico a partir de animação simples. Usar versão simplificada: massas por parte do corpo e da arma, centro de massa, balística | [Liu & Popović, 2002](https://grail.cs.washington.edu/projects/charanim/phylter.html) |
| Otimizador de tempo | Resolver tempo e velocidade em cada chave, sem mover a pose: problema bem menor | [Keyframe Motion Optimization by Relaxing Speed and Timing](https://www.microsoft.com/en-us/research/publication/keyframe-motion-optimization-by-relaxing-speed-and-timing/) |
| Interpolação por molas | Chaves como metas de posição e velocidade, sem paradas | [Spring Roll Call](https://theorangeduck.com/page/spring-roll-call), [Spring-It-On](https://github.com/orangeduck/Spring-It-On) (licença não informada no artigo: reescrever a partir da técnica) |
| Troca de clipe | Inertialization: guarda a diferença na troca e a faz sumir com mola | Mesmas fontes |
| Sobreposição | Splines com oscilação controlável | [Wiggly Splines](https://graphics.pixar.com/library/WigglySplines/index.html) |
| CMA-ES em Rust | [`cmaes`](https://docs.rs/crate/cmaes/latest) (a doc menciona Fortran: conferir) ou [`fcmaes-core`](https://docs.rs/crate/fcmaes-core/latest/source/Cargo.toml) | |

## Para orientar o movimento

- [Estudo de esgrima](https://ojs.ub.uni-konstanz.de/cpa/article/view/6606): especialistas mostram sequência do centro para a ponta mais clara que iniciantes.
- [Nem todo golpe segue essa sequência](https://jssm.org/jssm-15-92.xml-Fulltext). Por isso a regra é por tipo de golpe. Não achei estudo específico de kendo ou espada longa.
- [David Rosen, GDC 2014](https://gdcvault.com/play/1020049/Animation-Bootcamp-An-Indie-Approach): animação fluida com poucas chaves. Li só a descrição.
- [Skullgirls, GDC 2014](https://gdcvault.com/play/1020575/): chaves fortes, antecipação, tempo. Li só a descrição.
- [Cancelamentos em God of War](https://www.gamedeveloper.com/pc/combat-canceled-i-god-of-war-i-action-game-design).
- Não achei palestra da GDC sobre Souls.

## Avaliados e deixados de lado

- Skills de Blender: a visível ensina o programa, não qualidade de movimento.
- `rebirth-ik` e crates parecidos: limite de torção ainda no plano, e o solver já é nosso.
- [MuJoCo MPC](https://github.com/google-deepmind/mujoco_mpc): simulador externo e nenhuma indicação de que sirva para golpes de espada.
- Treino por reforço: pesado para o ganho.
- Amplitude articular em fonte primária: ainda falta a tabela AAOS, principalmente quadril e coluna.
