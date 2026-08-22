# mse-bisect

Mounts the real `mountTierB` in a bare page driven by Playwright Firefox, with
no React, no chrome and no Iris app around it. Built to settle the Firefox
"resume stalls, t=0 plays" hunt: it reproduces the engine faithfully and does
NOT reproduce the bug, which is how we learned the fault is not in Tier B.

    bun build ../../src/lib/iris-core/tiers/tier-b-mse.ts --outfile tierb.js \
      --format esm --target browser --external mediabunny --external libav.js
    bun build ../../src/lib/iris-core/decode/libav-audio-decoder.ts --outfile libavdec.js \
      --format esm --external mediabunny --external libav.js

    node serve.mjs &
    node drive.mjs '?at=0,178,373'                  # local file, MKV path in serve.mjs
    node drive.mjs '?src=/proxy/stream&at=82.7,373' # against a real /stream endpoint

`serve.mjs` serves the MKV with byte ranges, mediabunny's browser bundle, the
libav wasm (drop the `-iris` variant into `libavjs/` for E-AC-3) and the page.
Each run prints `buffered=[…]` per start position — empty means Firefox took the
appends and stored nothing, which is the failure signature.

The AC-3 / E-AC-3 decode needs the Docker-built libav variant:

    docker build --target libav-builder -t iris-libav . && cid=$(docker create iris-libav)
    docker cp $cid:/libav-iris.wasm      libavjs/libav-6.10.9.0-iris.wasm.wasm
    docker cp $cid:/libav-iris.wasm.mjs  libavjs/libav-6.10.9.0-iris.wasm.mjs
    docker cp $cid:/libav-iris.wasm.js   libavjs/libav-6.10.9.0-iris.wasm.js

## zen-check.html — point d'entrée aléatoire HEVC

Ouvre-la dans le navigateur à tester, sans automatisation : Playwright ne pilote
que son propre Firefox, pas un fork comme Zen (geckodriver le peut, mais ne
survit pas à des lancements répétés).

    bun gen-variants.mjs /chemin/vers/film.mkv 180   # écrit midstream.mp4
    node serve.mjs &
    # puis http://127.0.0.1:8099/zen-check.html dans le navigateur visé

La page réétiquette le premier NAL de tranche du fragment et mesure ce que MSE
en fait pour chaque type. Lecture du tableau :

- `buffered` **vide** → le navigateur refuse d'entrer sur ce type de NAL ;
- `buffered` rempli mais `frames=0` → il l'accepte et ne sait pas le décoder ;
- `frames` > 0 et variance de pixels non nulle → image réelle.

Ce qui a été mesuré sur Zen 1.21 (base Firefox ~147) contre Firefox 153 :

| début du groupe de frames | Firefox 153 | Zen 1.21 |
| --- | --- | --- |
| `IDR_N_LP` (t=0) | joue | joue |
| `CRA_NUT` (tout keyframe mi-flux) | joue | `buffered` vide |
| `CRA_NUT` réétiqueté `IDR_N_LP` | — | bufferise, puis `kVTVideoDecoderBadDataErr` |

`BLA_N_LP` reste à mesurer : c'est le candidat sérieux, parce que son en-tête de
tranche a la même structure que celle d'un CRA — contrairement à `IDR_N_LP`, qui
omet `slice_pic_order_cnt_lsb` et fait donc lire la suite de travers.
