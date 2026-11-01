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
| `--no-audio` | Sem som (com `--screenshot` o áudio já roda mudo, sem abrir o dispositivo) |

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
  grimora_physics/ física com Rapier: mundo estático da cena, corpos dinâmicos, raycasts, corpo de personagem (cápsula)
  grimora_audio/   som com kira: efeitos, loops de ambiente e emissores 3D que seguem a câmera
  grimora/         janela (winit), laço principal, trait Game, World, FlyCamera, ThirdPersonController, CLI, hot-reload
examples/
  sandbox/         cena de teste do motor
assets/scenes/     cenas .ron
assets/models/     modelos glTF (com a licença de cada um)
assets/sounds/     sons WAV/OGG (os do sandbox são gerados por tools/gen_sounds.py)
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
| Áudio | `kira` 0.12 (WAV e OGG) |

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
        jogador.update(&ctx.input, &mut ctx.physics, &mut ctx.camera, ctx.time.delta());
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

Tudo é dado: ambiente (sol, luz ambiente, névoa, céu, sombras, pós-processamento), câmera, ponto de início do jogador, terreno, objetos, espalhamento aleatório com semente e luzes pontuais. Todo objeto colide automaticamente com uma forma adequada (tronco para árvores, casco convexo para rochas e pilares, malha exata para o terreno); use `collider: None` para decoração ou escolha `Box(...)`, `Cylinder(...)`, `Ball(...)`, `Hull` ou `Mesh`. Com `body: Dynamic(...)` o objeto entra na simulação: cai, rola e é empurrado pelo personagem; campos opcionais `density` (kg/m³, padrão 1000) ou `mass` (kg), `friction` (0.6), `restitution` (0) e `angular_damping` (0.5). Malhas exatas viram casco convexo em corpos dinâmicos. Cores em sRGB 0..1, ângulos em graus, distâncias em metros. Malhas disponíveis: `Cube`, `Plane`, `Cylinder`, `Cone`, `Sphere`, `Pine`, `DeadTree`, `Rock`, `Crystal`, `Pillar`, `Wanderer` e `Model(path: "...")`. Veja o exemplo comentado em [sandbox.ron](assets/scenes/sandbox.ron).

### Modelos glTF

Exporte do Blender como glTF 2.0 (`.gltf` + `.bin` + texturas, ou `.glb`) para `assets/models/` e use na cena:

```ron
(mesh: Model(path: "../models/banner/banner.gltf"), position: (-2.6, 0.0, 9.0), rotation: (0.0, 15.0, 0.0)),
```

![Estandarte glTF: pano recortado por alfa e runa emissiva](docs/banner.png)

- O caminho é relativo ao arquivo de cena. Se o arquivo faltar ou estiver quebrado, o erro vai para o log e aparece um cubo magenta no lugar.
- A cena inteira do arquivo vira uma malha: transformações dos nós são aplicadas e cada primitiva vira uma submalha com seu material.
- Materiais metallic-roughness: cor base, normal map, metal/rugosidade, oclusão, emissão (`emissiveFactor`, `emissiveTexture` e `KHR_materials_emissive_strength`) e `doubleSided`. Cores de vértice também são lidas. Faltando normais ou tangentes, o motor gera.
- Emissão acima de 1 alimenta o bloom: no Blender, aumente a força (Strength) do Emission para a runa ou brasa brilhar.
- Transparência (`alphaMode`): `MASK` recorta o que estiver abaixo do `alphaCutoff` (folhagem, pano rasgado, grades), com bordas suavizadas pelo MSAA (alpha-to-coverage) e recorte também na sombra. `BLEND` é aproximado por alpha-to-coverage: não precisa ordenar objetos, mas tem poucos níveis de transparência e um leve pontilhado; para vidro e água de verdade ainda falta mistura ordenada.
- Texturas maiores que `max_texture_size` (padrão 2048) são reduzidas pela metade até caber: `Model(path: "...", max_texture_size: 1024)`.
- O modelo mantém a origem e a escala do arquivo. O colisor automático é o casco convexo; use `collider: Mesh` para formas côncavas grandes (casas, ruínas) ou `None` para decoração.
- Modelos ficam em cache enquanto o arquivo não muda, então o hot-reload da cena é instantâneo. Editar só o modelo não recarrega a cena: salve o `.ron` para recarregar.
- Ainda não suportados: animação, skinning, mistura de transparência ordenada e um segundo conjunto de UVs.
- O estandarte do sandbox é gerado por [tools/gen_banner.py](tools/gen_banner.py) (Python puro), um exemplo de como criar um glTF por código.

### Áudio

Sons em loop fazem parte da cena. Sem `position`, o som é ambiente (igual em todo lugar); com `position`, ele vira um emissor 3D: volume cheio até `min_distance`, cai pela distância (-6 dB a cada vez que ela dobra) e some em `max_distance`, com o pan acompanhando a câmera. Com terreno, `y` é medido a partir do chão. Objetos também carregam um som, como carregam uma luz; aí `position` é um deslocamento a partir do objeto:

```ron
sounds: [
    (path: "../sounds/wind.wav", volume: 0.35),
    (path: "../sounds/hum.wav", position: (0.0, 1.6, 0.0), volume: 0.4, min_distance: 2.0, max_distance: 14.0),
],
objects: [
    (mesh: Cone(segments: 5), position: (4.5, 1.7, 6.0), sound: (path: "../sounds/fire.wav", position: (0.0, 0.3, 0.0))),
],
```

No jogo, use `ctx.audio` (volumes são ganhos lineares, 1 = como gravado):

```rust
ctx.audio.play("assets/sounds/step_1.wav", 0.5);                       // efeito único
ctx.audio.play_at("assets/sounds/fire.wav", 1.0, Emitter::at(posicao)); // efeito único no mundo
let vento = ctx.audio.play_loop("assets/sounds/wind.wav", 0.3);         // loop ambiente
let tocha = ctx.audio.play_loop_at("assets/sounds/fire.wav", 0.8, Emitter::at(posicao));
if let Some(id) = tocha { ctx.audio.set_position(id, nova_posicao); }   // e ctx.audio.stop(id)
ctx.audio.set_volume(Category::Ambient, 0.5);                           // e set_master_volume
```

- Arquivos ficam em cache pelo caminho; se o arquivo mudar no disco, é lido de novo no próximo uso.
- A cada (re)carga da cena, os loops da cena são trocados: os que não mudaram continuam tocando sem reiniciar, os removidos somem com fade. Loops iniciados pelo jogo ficam até `stop(id)`.
- Sem dispositivo de áudio, o motor avisa no log e segue mudo. Capturas (`--screenshot`) carregam e mixam tudo, mas não tocam nada.
- Os sons do sandbox (vento, fogo, zumbido do cristal e passos) são sintetizados por [tools/gen_sounds.py](tools/gen_sounds.py) (Python puro, sementes fixas): `python -I tools/gen_sounds.py`.

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
- [x] Transparência recortada (`alphaMode: Mask`) com alpha-to-coverage e sombras recortadas; mapas de emissão
- [x] Corpos dinâmicos: objetos que caem, rolam e são empurrados pelo personagem
- [x] Áudio com `kira`: efeitos, loops de ambiente e emissores 3D na cena, passos do personagem

**Próximos passos**
- [ ] Culling por frustum e LOD
- [ ] Animação esquelética
- [ ] Mundo em regiões com carregamento por streaming
- [ ] Salvamento de jogo
- [ ] Cascaded shadow maps para sombras nítidas a grandes distâncias
- [ ] ECS (avaliar `hecs` ou `bevy_ecs` quando a quantidade de sistemas justificar)

## Em aberto

- Hardware de referência e critérios de desempenho da primeira versão.
- Licença do projeto.
- Verificação do nome Grimora, caso seja necessário para publicação.
