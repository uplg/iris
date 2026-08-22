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
