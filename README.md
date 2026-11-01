# Grimora Engine

Motor 3D próprio, escrito em **Rust**, para jogos low poly com visual dark fantasy estilizado. Construído para ser desenvolvido junto com IA: cenas em arquivos legíveis, recarga automática, capturas de tela e métricas pela linha de comando.

![Sandbox: círculo de pedras ao entardecer](docs/sandbox.png)

## Começando

Requisitos: Windows 10/11, GPU com Vulkan ou DirectX 12, [Rust](https://rustup.rs) estável.

```bash
cargo run -p sandbox
```

Você controla um andarilho encapuzado com uma lanterna:

| Tecla | Modo personagem (padrão) | Modo voo (**Tab** alterna) |
| --- | --- | --- |
| Mouse | gira a câmera | segure o botão direito para olhar |
| WASD | anda relativo à câmera | voa |
| Shift | corre | acelera |
| Espaço | pula | sobe (E/Q também) |
| Roda | aproxima/afasta a câmera | ajusta a velocidade |

**F5** recarrega a cena, **F12** salva uma captura em `screenshots/`, **Esc** sai.

![Andarilho com lanterna](docs/wanderer.png)

Com o jogo aberto, edite [assets/scenes/sandbox.ron](assets/scenes/sandbox.ron) e salve: a cena recarrega na hora. Se o arquivo tiver um erro, o motor mostra linha e coluna no terminal e continua com a versão anterior.

### Linha de comando (para automação e IA)

```bash
cargo run -p sandbox -- --screenshot screenshots/teste.png
```

```bash
cargo run -p sandbox -- --screenshot shots/vista.png --camera -13,5,-15 --look-at 0,1,0 --size 1280x720
```

```bash
cargo run --release -p sandbox -- --frames 600 --no-vsync
```

| Opção | Efeito |
| --- | --- |
| `--scene <arquivo.ron>` | Cena a carregar |
| `--screenshot <arquivo.png>` | Renderiza, salva a imagem e fecha |
| `--frames <n>` | Fecha após n quadros (padrão 90 com `--screenshot`) |
| `--camera x,y,z` / `--look-at x,y,z` | Posiciona a câmera (Y absoluto; no sandbox, ativa o modo voo) |
| `--size LxA` | Tamanho da janela |
| `--no-vsync`, `--no-msaa`, `--no-hot-reload` | Benchmark e depuração |

Ao fechar, o motor imprime uma linha de métricas fácil de ler por scripts:

```
[grimora] frames=80 avg_ms=5.55 p95_ms=5.88 max_ms=6.30 draw_calls=33 objects=392 triangles=48296 gpu="NVIDIA GeForce RTX 3060"
```

## Arquitetura

```
crates/
  grimora_core/    tempo, entrada, Transform, Camera, MeshData, Material, Environment (luz/névoa/céu/pós)
  grimora_render/  renderizador wgpu: sombras, HDR + MSAA 4x, materiais PBR, névoa de altura, bloom, tonemapping
  grimora_scene/   formato de cena RON, importação glTF, terreno procedural, geradores low poly, espalhamento, colisores
  grimora_physics/ colisão com Rapier: mundo estático da cena, raycasts, corpo de personagem (cápsula)
  grimora/         janela (winit), laço principal, trait Game, World, FlyCamera, ThirdPersonController, CLI, hot-reload
examples/
  sandbox/         cena de teste do motor
assets/scenes/     cenas .ron
assets/models/     modelos glTF (com a licença de cada um)
```

Quadro a quadro: mapa de sombra do sol → passe HDR (céu + malhas iluminadas, MSAA) → cadeia de bloom → tonemap ACES, contraste, saturação, vinheta e granulado na tela. Objetos com a mesma malha são desenhados numa única chamada instanciada.

| Componente | Escolha |
| --- | --- |
| Gráficos | `wgpu` 30 (Vulkan/DX12) com shaders WGSL |
| Janela e entrada | `winit` 0.30 |
| Matemática | `glam` 0.34 |
| Física e colisão | `rapier3d` 0.36 |
| Cenas | `ron` + `serde` |
| Modelos | `gltf` 1.4 |

### Criando um jogo

```rust
use grimora::prelude::*;

#[derive(Default)]
struct MeuJogo { jogador: Option<ThirdPersonController> }

impl Game for MeuJogo {
    fn scene_loaded(&mut self, ctx: &mut Context) {
        let inicio = ctx.world.player_spawn.expect("a cena define `player`");
        self.jogador = Some(ThirdPersonController::new(inicio.position, inicio.yaw));
    }

    fn update(&mut self, ctx: &mut Context) {
        let Some(jogador) = &mut self.jogador else { return };
        // Anda, pula e colide com a cena; a câmera segue sem atravessar paredes.
        jogador.update(&ctx.input, &ctx.physics, &mut ctx.camera, ctx.time.delta());
        ctx.set_cursor_grab(true);
        if let Some(obj) = ctx.world.find_mut("altar") {
            obj.transform.rotation = Quat::from_rotation_y(ctx.time.elapsed());
        }
    }
}

fn main() -> anyhow::Result<()> {
    let config = EngineConfig { scene: Some("assets/scenes/minha_cena.ron".into()), ..Default::default() };
    grimora::run(config.with_cli()?, MeuJogo::default())
}
```

### Formato de cena

Tudo é dado: ambiente (sol, luz ambiente, névoa, céu, sombras, pós-processamento), câmera, ponto de início do jogador, terreno, objetos, espalhamento aleatório com semente e luzes pontuais. Todo objeto colide automaticamente com uma forma adequada (tronco para árvores, casco convexo para rochas e pilares, malha exata para o terreno); use `collider: None` para decoração ou escolha `Box(...)`, `Cylinder(...)`, `Ball(...)`, `Hull` ou `Mesh`. Cores em sRGB 0..1, ângulos em graus, distâncias em metros. Malhas disponíveis: `Cube`, `Plane`, `Cylinder`, `Cone`, `Sphere`, `Pine`, `DeadTree`, `Rock`, `Crystal`, `Pillar`, `Wanderer` e `Model(path: "...")`. Veja o exemplo comentado em [sandbox.ron](assets/scenes/sandbox.ron).

### Modelos glTF

Exporte do Blender como glTF 2.0 (`.gltf` + `.bin` + texturas, ou `.glb`) para `assets/models/` e use na cena:

```ron
(mesh: Model(path: "../models/bau/bau.gltf"), position: (2.0, 0.0, 4.0), rotation: (0.0, 30.0, 0.0)),
```

- O caminho é relativo ao arquivo de cena. Se o arquivo faltar ou estiver quebrado, o erro vai para o log e aparece um cubo magenta no lugar.
- A cena inteira do arquivo vira uma malha: transformações dos nós são aplicadas e cada primitiva vira uma submalha com seu material.
- Materiais metallic-roughness: cor base, normal map, metal/rugosidade, oclusão e `doubleSided`. Cores de vértice também são lidas. Faltando normais ou tangentes, o motor gera.
- Texturas maiores que `max_texture_size` (padrão 2048) são reduzidas pela metade até caber: `Model(path: "...", max_texture_size: 1024)`.
- O modelo mantém a origem e a escala do arquivo. O colisor automático é o casco convexo; use `collider: Mesh` para formas côncavas grandes (casas, ruínas) ou `None` para decoração.
- Modelos ficam em cache enquanto o arquivo não muda, então o hot-reload da cena é instantâneo. Editar só o modelo não recarrega a cena: salve o `.ron` para recarregar.
- Ainda não suportados: animação, skinning, transparência (`alphaMode`), emissão por textura e um segundo conjunto de UVs.

## Visão

O objetivo inicial é dar suporte a um jogo de mundo aberto com visual low poly dark fantasy estilizado, priorizando iluminação, atmosfera e identidade em vez de realismo.

Diretrizes:

- Motor próprio, sem Unreal, Godot, Unity ou Three.js como base; bibliotecas especializadas são bem-vindas.
- Sem editor visual como objetivo inicial: cenas e configurações vivem em arquivos de texto.
- Adequado ao desenvolvimento com IA: executar cenas, ler erros e métricas e capturar imagens pela linha de comando.
- Plataforma inicial: Windows para PC.
- Caminho aberto para mundo aberto 3D e para servir de base a outros jogos.

## Roteiro

**Feito (v0.1)**
- [x] Renderizador wgpu: HDR, MSAA 4x, sombra direcional com PCF e estabilização, névoa de altura com dispersão do sol, céu procedural, até 32 luzes pontuais com cintilação, materiais emissivos, bloom, tonemapping ACES e gradação de cor
- [x] Cenas RON com hot-reload, terreno procedural e geradores de props low poly
- [x] Capturas, posicionamento de câmera e métricas por linha de comando
- [x] Colisão com Rapier: terreno, props e colisores declarados na cena
- [x] Personagem em terceira pessoa: andar, correr, pular, degraus e rampas; câmera orbital que não atravessa paredes
- [x] Importação de modelos glTF com materiais PBR (texturas de cor, normal, metal/rugosidade e oclusão, com mipmaps)

**Próximos passos**
- [ ] Transparência recortada (`alphaMode: Mask`) para folhagem e tecidos
- [ ] Corpos dinâmicos (objetos que caem, rolam e podem ser empurrados)
- [ ] Culling por frustum e LOD
- [ ] Animação esquelética
- [ ] Áudio (avaliar `kira`)
- [ ] Mundo em regiões com carregamento por streaming
- [ ] Salvamento de jogo
- [ ] Cascaded shadow maps para sombras nítidas a grandes distâncias
- [ ] ECS (avaliar `hecs` ou `bevy_ecs` quando a quantidade de sistemas justificar)

## Em aberto

- Hardware de referência e critérios de desempenho da primeira versão.
- Licença do projeto.
- Verificação do nome Grimora, caso seja necessário para publicação.
