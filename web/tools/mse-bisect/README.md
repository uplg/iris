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

Ce qui a été mesuré sur Zen 1.21 (base Firefox ~147), fichier x265 open-GOP :

| début du groupe de frames | buffered | frames décodées |
| --- | --- | --- |
| `IDR_N_LP` (t=0, seul IDR du fichier) | `0.0–10.0` | 149 |
| `CRA_NUT` (tout keyframe mi-flux) | **vide** | 0 |
| `CRA_NUT`, muxer mediabunny NON patché | **vide** | 0 |
| `CRA_NUT` → `IDR_N_LP` | `178.1–188.0` | 0, `kVTVideoDecoderBadDataErr` |
| `CRA_NUT` → `BLA_N_LP` | **vide** | 0 |

Conclusion : ce Gecko n'ouvre un groupe de frames que sur un **IDR**. Le CRA et le
BLA sont refusés au stade du tampon ; réétiqueter en IDR passe le tampon puis casse
le décodage, parce qu'un en-tête de tranche d'IDR omet `slice_pic_order_cnt_lsb`.

Convertir proprement un CRA en IDR imposerait de réécrire aussi les POC de toutes
les images suivantes du GOP (un IDR force POC=0 et les suivantes sont relatives) —
de la chirurgie de bitstream dans le chemin chaud, hors de question ici.

Une seule remarque de méthode, apprise à la dure : `HTMLMediaElement.play()` renvoie
une promesse qui **ne se résout jamais** quand la lecture ne démarre pas. Un
`await v.play().catch(…)` bloque alors pour toujours et le `.catch` n'y change rien.
Ne jamais l'attendre dans un banc de test.

## Verdict : régression Firefox 154, corrigée en amont

Ce n'est ni un bug d'Iris ni un bug propre à Zen. Le mécanisme est dans Gecko.

`dom/media/platforms/agnostic/bytestreams/H265.h` — seuls les IDR comptent comme
image intra, alors que CRA et BLA sont aussi des IRAP au sens de la norme :

    bool IsIframe() const {
      return mNalUnitType == NAL_TYPES::IDR_W_RADL ||
             mNalUnitType == NAL_TYPES::IDR_N_LP;
    }

`dom/media/mp4/MP4Demuxer.cpp` — le drapeau `sync` du conteneur est **écrasé** sur
macOS, avec la raison écrite noir sur blanc :

    #ifdef MOZ_APPLEMEDIA
      // VideoToolbox can return a bad data error if a CRA frame is the first
      // sample after a seek. Only IDR_W_RADL/IDR_N_LP are safe starting points.
      auto isIDR = H265::IsKeyFrame(sample);
      bool keyframe = isIDR.isOk() && isIDR.unwrap();

L'échantillon CRA n'est donc pas un keyframe ; MSE, qui exige un point d'accès
aléatoire après un init segment, le jette — ainsi que tous les suivants, puisque
`need random access point` reste vrai. D'où `buffered` vide, sans erreur ni
événement. Et forcer le passage en réétiquetant en IDR déclenche exactement
l'erreur que ce garde-fou évite : `kVTVideoDecoderBadDataErr` (−12909).

Historique : bug 1967475 (oct. 2025, « Only set IDR frame as keyframe for H264 on
MacOS ») introduit l'écrasement ; bug 2049615 en est la régression pour le HEVC
open-GOP, « Fix HEVC seeking on macOS when open-GOP CRA frames are present »,
corrigée pour Firefox 154 et non rétroportée en 153.

Mesuré ici — et attention au piège, la version produit d'un fork ne dit rien de sa
base Gecko, il faut lire `navigator.userAgent` :

| moteur | seek HEVC open-GOP |
| --- | --- |
| Gecko 153 (Firefox de Playwright) | fonctionne |
| Gecko 154 (Zen 1.21.15b, build antérieur au correctif) | `buffered` vide |

Rien à corriger côté Iris : le remède est une mise à jour du navigateur.
