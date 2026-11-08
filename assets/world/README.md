# assets/world

Generated worlds (`WorldDesc` `.ron`, read by `grimora_world`) and the Nyraldar macro map they
are built from.

- `island.ron` — the first generated world: a small island at map x 4476, y 1681, with an 8 × 8
  unit window (1 unit = 2109.7 m).
- `nyraldar/` — the macro map, imported from the user's own compendium site
  (`Documents\NYRALDAR\SITE_PROJETO_GITHUB\public\maps`). Not third-party content.
  - `topography.png`, `climate.png`, `water.png` (8192 × 4100) and `political.png`
    (4096 × 2050, 2 units per texel): 8-bit grey grids of category indices, 255 = none. The
    index is the category's place in `legend.ron`. Anti-aliased source pixels took the majority
    of their exact neighbours, never a blended colour.
  - `legend.ron` — map size, layers, bands, biomes, nations and water bodies.
  - `points.ron` — places, minerals and resources, in map units.
  - `coast_<name>.png` — land mask cut from the base painting (detail level, 2 texels per
    unit), which wins over the topography's coast; `art_<name>.png` is the painting it came
    from, for comparison.

Regenerate everything (needs Pillow and NumPy, about 15 s):

```bash
python -I tools/import_world_map.py
```

Add `--coast name,x0,y0,w,h` (map units) for other windows, and `--source <folder>` if the
compendium moves. Check the result with
`cargo run --release -p world_map -- --world assets/world/island.ron --screenshot screenshots/world_island.png`.
