# Créditos

## Recursos deste projeto

- **Texturas e sprites** (`assets/textures/`): pixel art feita pelo autor do projeto, parte do motor. A licença ainda não foi definida, como a do resto do projeto (veja "Em aberto" no [README](README.md)).
- **Corpo-base do personagem** (`assets/models/wanderer`): gerado por `tools/blender/build_wanderer.py`, com clipes de `tools/gen_wanderer.py` e `tools/blender/sword_moves.py`.
- **Modelo de teste atual** (`assets/models/chartest`): fornecido pelo usuário em `chartest.blend`, com topologia e esqueleto nativo preservados por `tools/blender/import_chartest.py`; os pesos recebem uma suavização local no punho e `hand_grip.py` ajusta a pose e a superfície local dos dedos ao cabo. O arquivo contém uma hierarquia de importação Sketchfab/FBX, sem URL de origem ou metadados de licença; não se atribui autoria a partir desses nomes. A textura (`Chartest-Grimora-v02-128.png`) é do autor do projeto, nas mesmas condições das outras. Detalhes em `assets/models/chartest/README.md`.
- **Armas** (`assets/models/weapons/`): modelos feitos pelo autor do projeto, nas mesmas condições das texturas.
- **Estandarte** (`assets/models/banner`): gerado por `tools/gen_banner.py`.
- **Sons** (`assets/sounds/`): gerados por `tools/gen_sounds.py`, menos os listados em "Terceiros" e os passos em `steps/`, feitos pelo autor do projeto.

## Terceiros

- **Sons gravados** (`assets/sounds/`: `clang_1`, `clang_2`, `draw_sword`, `draw_knife`, `squelch`, `fire`, `wind`, `river`, `gun_dry`): CC0, da coletânea "500 Game Sound Effects" (Vehicle, artisticdude, PagDev, Luke.RUSTLTD, rubberduck, via OpenGameArt) e do [RPG Audio](https://kenney.nl/assets/rpg-audio) da Kenney. Arquivo por arquivo em `assets/sounds/CC0_SOURCES.txt`.

- **Departure Mono**, de Helena Zhang: a fonte da interface (`assets/fonts`, assada em `crates/grimora_render/src/font_data.rs`). SIL Open Font License 1.1. https://departuremono.com/

- **Animação de descanso com espada:** **Breathing Idle**, da [Adobe Mixamo](https://www.mixamo.com/#/?page=1&query=breathing+idle&type=Motion%2CMotionPack), baixada pela sessão do usuário e adaptada ao esqueleto do personagem por `tools/blender/mixamo_idle.py`. Fonte e parâmetros em `assets/animations/mixamo/README.md`.
- **Fonte do HUD:** glifos 8x8 do crate [`font8x8`](https://crates.io/crates/font8x8), em domínio público.
