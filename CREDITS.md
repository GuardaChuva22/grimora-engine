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

- **Sons de rio** (`assets/sounds/river_small.wav`, `river_big.wav`): de DRAGONSTUDIO, no [Pixabay](https://pixabay.com/sound-effects/), pela [Licença de Conteúdo do Pixabay](https://pixabay.com/service/license-summary/) (uso livre, sem atribuição obrigatória). Cortados, emendados em crossfade para o loop e nivelados (-24 e -21 LUFS).

- **Sons de rolamento** (`assets/sounds/roll/`): `roll_light.mp3` de [imagne_impossible](https://pixabay.com/users/imagne_impossible-30314574/) e `roll_armor.mp3` (a armadura batendo) de [u_xjrmmgxfru](https://pixabay.com/users/u_xjrmmgxfru-47169417/), no Pixabay, pela [Licença de Conteúdo do Pixabay](https://pixabay.com/service/license-summary/).

- **Sons do inventário** (`assets/sounds/ui/`): `open_inv.mp3` de [freesound_community](https://pixabay.com/pt/users/freesound_community-46691455/) e `unequip.mp3` de [freesound_community](https://pixabay.com/pt/users/freesound_community-46691455/), e `close_inv.mp3` de [yodguard](https://pixabay.com/pt/users/yodguard-12455005/), no Pixabay, pela [Licença de Conteúdo do Pixabay](https://pixabay.com/service/license-summary/).

- **Sons de porta de madeira** (`assets/sounds/doors/wooden_door_open.mp3`, `wooden_door_close.mp3`): de [Dragon Studio](https://pixabay.com/pt/users/dragon-studio-38165424/), no Pixabay, pela [Licença de Conteúdo do Pixabay](https://pixabay.com/service/license-summary/).

- **Departure Mono**, de Helena Zhang: a fonte da interface (`assets/fonts`, assada em `crates/grimora_render/src/font_data.rs`). SIL Open Font License 1.1. https://departuremono.com/

- **Animação de descanso com espada:** **Breathing Idle**, da [Adobe Mixamo](https://www.mixamo.com/#/?page=1&query=breathing+idle&type=Motion%2CMotionPack), baixada pela sessão do usuário e adaptada ao esqueleto do personagem por `tools/blender/mixamo_idle.py`. Fonte e parâmetros em `assets/animations/mixamo/README.md`.
- **Fonte do HUD:** glifos 8x8 do crate [`font8x8`](https://crates.io/crates/font8x8), em domínio público.

- Animações dos moradores (sentar, dormir, conversar, ouvir, rezar, cavar, pescar) e do nado: trazidas pelo usuário, da [Adobe Mixamo](https://www.mixamo.com) e no esqueleto do manequim UE (licença destas a confirmar), em `assets/animations/village/` (veja o README de lá), adaptadas por `tools/import_village_clips.py`.
- Animações de combate, pulo e da Arremetida: Quaternius, Universal Animation Library 1 e 2 (CC0), em `assets/animations/quaternius/`. https://quaternius.com
- Animações de voo e levitação (`assets/animations/quaternius/flying_forward.glb`, `Flying_Forward` em `leap_glide`; `levitate_idle.glb`, `Levitate_Idle` em `leap_hover`): trazidas pelo usuário, exportadas de um visualizador three.js no esqueleto do manequim UE. Licença a confirmar.
