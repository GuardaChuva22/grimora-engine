# Avaliação: ECS no Grimora

Novembro de 2026. Compara o modelo atual (`World` com `Vec<Object>` e índices) com `hecs` 0.11.2 e `bevy_ecs` 0.20.1.

## Recomendação

**Migrar o `World` para `hecs` antes de escrever gameplay (inimigos, itens, interação). Não usar `bevy_ecs`.**

Desempenho não é o motivo: as três opções empatam e o custo some perto do renderizador. O que importa é o modelo de dados. O `World` atual não tem como:

- dar a um objeto um identificador estável;
- remover um objeto;
- pendurar dados do jogo num objeto (vida, loot, "pode ser aberto").

Todo código de gameplay escrito agora em cima de índices vai precisar ser reescrito depois.

## Onde o modelo atual já aperta

- **Índices como identidade.** Vários lugares guardam posições no `Vec` como se fossem identidades:
  - `avatar`, `heart` e `lantern_light` no sandbox;
  - `SceneBody::object` na física;
  - `bodies` dos chunks de streaming;
  - `ObjectState::index` no save.

  Nada disso sobrevive a uma remoção, e por isso não existe `World::despawn`.
- **Objetos de streaming não têm identidade nenhuma.** O jogo só consegue achá-los por nome, a cada quadro.
- **Dados do jogo ficam de fora do objeto.** `Object` tem campos fixos. Para dar vida a um inimigo, o jogo precisa de um mapa paralelo indexado por posição no `Vec`, que quebra junto com os índices.
- **Luzes são uma lista à parte.** A luz declarada num objeto é copiada para `lights` e não acompanha o objeto. O cristal "heart" flutua e a luz dele fica parada; o sandbox move a da lanterna à mão.
- **O save só cobre objetos da cena.** Objetos criados pelo jogo ficam por conta dele, porque não há como enumerá-los com seus dados.

Um ECS resolve tudo isso de uma vez:

- `Entity` é um id geracional: um id removido nunca aponta para outro objeto.
- Componentes arbitrários: o jogo faz `insert(entidade, Vida(10))`.
- Consultas: `query::<(&Transform, &Vida)>()`.
- `despawn` existe.

## Medições

O spike reproduz o trabalho por quadro do motor:

- montar a lista de desenho de 23 mil objetos (como o `bench_forest`);
- copiar a pose de 200 corpos;
- criar e remover 1000 objetos por quadro, como o streaming.

Release, RTX 3060 / Windows; números de duas rodadas.

| | `Vec<Object>` (hoje) | `hecs` | `bevy_ecs` |
|---|---|---|---|
| Desenho + corpos (µs/quadro) | 64–76 | 82–102 | 76–90 |
| Criar e remover 1000 (µs/quadro) | 3* | 39–50 | 71–87 |
| Build limpo (release) | — | 3,6 s | 35 s |
| Crates na árvore | — | 5 | 82 |
| Rebuild após editar | — | 0,8 s | 1 s |

\* O `Vec` só "remove" truncando o fim. Remover um objeto do meio, que é o caso real, quebra os índices.

Leitura dos números:

- **Desempenho:** a diferença entre as três é de ~30 µs num quadro de ~2 ms, menos de 2%. O streaming cria poucas células por quadro, então a diferença na criação e remoção também não pesa.
- **Build:** o `bevy_ecs` traz 82 crates e 30 s a mais de build limpo. O `hecs` traz 5.

## Por que não `bevy_ecs`

- **Estilo imposto:** traz escalonador, sistemas como funções, eventos, observers e detecção de mudanças. É muito bom, mas empurra o motor para o jeito Bevy de organizar o jogo. Isso conflita com a diretriz de motor próprio e leve, e com o `Game::update` simples que facilita o trabalho com IA.
- **API instável:** quebra a cada versão, mais ou menos a cada 3–4 meses (já está na 0.20). Cada atualização vira trabalho de migração.
- **Peso no build:** 82 crates e 30 s a mais de build limpo, como mostra a tabela.

Outras opções descartadas:

- **`flecs` (Rust):** usa bindings para C, o que quebra o build simples só com Cargo.
- **`specs` e `legion`:** estão sem manutenção ativa.
- **`shipyard`:** é viável, mas tem comunidade menor e a mesma proposta do `hecs` com mais conceitos.

## Por que `hecs`

- **É só armazenamento:** não traz escalonador. O `Game::update` continua sendo um método comum que roda consultas, e não muda de estilo.
- **Leve:** 5 crates e 3,6 s de build limpo.
- **Estável:** API pequena e estável, com `serde` opcional para salvar componentes, o que serve ao save.

## Como seria a migração

1. **`World` embrulha `hecs::World`.**
   - Componentes do motor: `Transform`, `Renderable` (malha, cor, emissão, visível), `Name`, `Animator`, `Light` (luz presa à entidade, com deslocamento), `Body` (`BodyHandle`) e `SceneIndex` (para o save).
   - Os métodos atuais continuam como atalhos: `find`, `spawn`, `joint_transform`.
2. **Identidade e consultas.** `ObjectId` passa a ser `hecs::Entity`. Entram `World::despawn`, `get`, `get_mut`, `insert` e `query`.
3. **Streaming.** Células e regiões passam a guardar `Vec<Entity>`. Descarregar uma delas é dar `despawn` nessas entidades e nos corpos delas.
4. **Física, renderização e save.**
   - A física continua sem saber de entidades: o `World` guarda o par corpo ↔ entidade.
   - O runner monta os desenhos com uma consulta.
   - O save casa objetos por `SceneIndex` + nome, como hoje.
5. **Sandbox e testes.** Passam a usar `Entity` no lugar de índices. A luz da lanterna vira um componente `Light` preso ao osso.

Tamanho estimado: algumas centenas de linhas alteradas em `world.rs`, `streaming.rs`, `runner.rs`, `save.rs`, no sandbox e nos testes. A cena RON não muda.
