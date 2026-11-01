# Grimora Engine — guia para agentes de IA

Motor 3D em Rust (wgpu 30, winit 0.30, glam 0.34, rapier3d 0.36) para jogos low poly dark fantasy. Visão e roteiro: [README.md](README.md). O usuário fala português; código e comentários ficam em inglês.

## Comandos

```bash
cargo build -p sandbox
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
```

Conferência visual (sempre depois de mudar renderização ou cenas). Abra a imagem gerada e avalie:

```bash
cargo run -p sandbox -- --screenshot screenshots/check.png --size 1280x720
```

Outros ângulos: `--camera x,y,z --look-at x,y,z`. A última linha impressa (`[grimora] frames=... avg_ms=...`) traz métricas de desempenho; compare antes e depois de mudanças pesadas, de preferência com `--release`.

## Mapa do código

- `crates/grimora_core` — tipos compartilhados: `Time`, `Input`, `Transform`, `Camera`, `MeshData`/`Vertex`/`Material`/`Submesh`/`TextureData`, `Environment`/`PointLight`, `Skeleton`/`AnimationClip`/`Animator` (`animation.rs`, só CPU: amostra clipes, mistura o crossfade e gera as matrizes de skinning).
- `crates/grimora_render` — `Renderer`. Shaders em `src/shaders/*.wgsl`. Os structs em `uniforms.rs` e `material.rs` espelham os structs WGSL campo a campo: altere os dois juntos. `material.rs` sobe texturas (com mipmaps, cache por `Arc`) e cria o bind group 2 de cada submalha (0 parâmetros, 1-4 cor/normal/metal-rugosidade/oclusão, 5 sampler, 6 emissão). Há 4 pipelines de malha (`mesh_pipeline_index`: culling × alpha-to-coverage) e um pipeline de sombra recortada (`fs_shadow_masked`) para materiais com alfa. Derivadas (`fwidth`) e `textureSample` ficam no topo de `fs_main`, antes de qualquer `discard`. `Vertex` usa as locations 0-2, 11-14 (13-14 são ossos `Uint8x4` e pesos `Unorm8x4`); a instância usa 3-10. Skinning: as matrizes de todos os objetos animados vão num storage buffer (grupo 0, binding 1, `Frame::joint_matrices`); o `w` livre de `normal_0` da instância guarda o índice da primeira matriz (-1 = sem skinning). `skin_matrix` é usado em `vs_main` e nos dois vertex shaders de sombra.
- `crates/grimora_scene/src/model.rs` — importação glTF (`MeshDesc::Model`): achata a cena do arquivo numa `MeshData` com uma submalha por primitiva; cache global por caminho + mtime. Caminhos são relativos ao `.ron` (`Scene::load` → `Scene::build_in`). Com skin, a primeira vira o `MeshData::skeleton`: primitivas com skin ficam no espaço do modelo, malhas filhas de ossos entram com peso 1 no osso (`Binding::Rigid`), e os canais que animam ossos viram `AnimationClip`s.
- `crates/grimora_scene` — formato RON (`SceneDesc`), `Scene::build`, terreno (`terrain.rs`), geradores de malha (`shapes.rs`), RNG/ruído determinísticos (`rng.rs`).
- `crates/grimora_scene/src/collider.rs` — `ColliderDesc` (no arquivo de cena) → `ColliderShape` (forma resolvida, só dados); `BodyDesc` (`Static` ou `Dynamic { density, mass, ... }`).
- `crates/grimora_physics` — `Physics` (envolve o `PhysicsWorld` do Rapier: colisores estáticos da cena, corpos dinâmicos, raycast, passo fixo de 60 Hz) e `CharacterBody` (cápsula com `KinematicCharacterController`; `move_and_slide` recebe `&mut Physics` porque empurra os corpos dinâmicos em que esbarra). O corpo dinâmico fica na origem do objeto, então a pose dele já é a posição/rotação de render.
- `crates/grimora` — laço principal (`runner.rs`), trait `Game`, `World`, `FlyCamera`, `ThirdPersonController` (`third_person.rs`), CLI (`config.rs`). O runner reconstrói `ctx.physics` a cada (re)carga de cena e chama `physics.step` e depois `World::sync_bodies` antes de `Game::update`: `Physics::scene_bodies` liga cada corpo dinâmico ao índice do objeto (mesma ordem em `Scene::objects` e `World::objects`) e a sincronização copia posição e rotação para o `Transform`, mantendo a escala.
- `crates/grimora/tests/character.rs` e `dynamic.rs` — testes que dirigem o personagem pela cena real com teclas simuladas (`Input::key_down`), sem janela nem GPU. Prefira testar jogabilidade assim.
- `World::update_animations` roda depois de `Game::update`; `World::joint_transform(objeto, "osso")` dá a transformação no mundo. `ObjectDesc.animation` toca um clipe em laço ao carregar a cena.
- `examples/sandbox` + `assets/scenes/sandbox.ron` — cena de teste. O personagem é `assets/models/wanderer` (gerado por `tools/gen_wanderer.py`; rode de novo depois de editar o script): clipes `idle`/`walk`/`run`/`jump` escolhidos por `animate` em `main.rs`; a luz da lanterna segue o osso `lantern`.

## Convenções

- Mão direita, Y para cima, -Z à frente. Projeções vêm de `glam::camera::rh` com convenção `directx` (é a do wgpu: Y para cima, profundidade 0..1). Os antigos `Mat4::perspective_rh`/`look_to_rh` não existem mais no glam 0.34.
- Cores na API e nos arquivos são sRGB 0..1; a conversão para linear acontece no renderizador/shader.
- Props gerados têm a origem no centro da base (y = 0). Em cenas com terreno, `position.y` de objetos é relativa ao chão.
- Faces em sentido anti-horário vistas de fora (back-face culling ativo). `MeshData::push_triangle` gera normais planas — é o visual low poly.
- Tudo procedural recebe semente: a mesma semente gera sempre o mesmo mundo.
- Iluminação: o difuso é Lambert sem 1/π (escala original do motor) e o especular GGX é multiplicado por π para combinar. Malhas procedurais usam o `Material` padrão (branco, rugosidade 1, não metálico) e tiram a cor dos vértices; não mude esse padrão sem conferir as capturas.
- Recursos de terceiros vão em `assets/` com o arquivo de licença ao lado e uma entrada em `CREDITS.md` (crie o arquivo se não existir, com link na seção de créditos do README).
- Ao adicionar campos a `Environment` ou ao formato de cena, use `#[serde(default)]` para não quebrar cenas existentes e acrescente exemplo/comentário em `sandbox.ron`.

## Armadilhas do Rapier 0.36

O Rapier usa sua própria cópia do glam (0.31, via `glamx`), incompatível com o glam 0.34 do motor: converta nos limites com `to_rapier`/`from_rapier` em `grimora_physics`. Não exponha tipos do Rapier na API pública além de `ColliderHandle`. Colisores adicionados só aparecem nas consultas depois de `step`/`refresh`.

## Armadilhas da API wgpu 30

`PipelineLayoutDescriptor` usa `bind_group_layouts: &[Option<&_>]` e `immediate_size`; `VertexState.buffers` é `&[Option<VertexBufferLayout>]`; `DepthStencilState` recebe `Option` em `depth_write_enabled`/`depth_compare`; apresentação é `queue.present(surface_texture)`; `get_current_texture()` retorna o enum `CurrentSurfaceTexture`. Na dúvida, consulte o código-fonte em `~/.cargo/registry/src/*/wgpu-30.*`.
