# Research, concepts, measurements

What each file is and how current it is. The project's state and decisions are in
[`../STATUS.md`](../STATUS.md); a file here is background, not the last word, unless STATUS says so.

| File | What | Status |
|---|---|---|
| [`serum-modulation.md`](serum-modulation.md) | How Serum does modulation (slots, normalized deltas, knob layers, matrix) | **Source of truth** for our modulation; partly built |
| [`blocks.md`](blocks.md) | What blocks are for (building blocks: expose every parameter), the catalogue, what's missing for professional chains (sidechain, convolution, …) and the elementary layer (DelayLine, TransientSplit, Feedback, …) | **Source of truth** for the block roadmap; compressor, lookahead, linear phase built |
| [`block-displays.md`](block-displays.md) | Block modal: a display per block type (build order), live data from the audio thread (taps, subscription, timing), knob groups and choices | **Source of truth** for the displays; modal built, displays next |
| [`chain-editor.md`](chain-editor.md) | Concept: chain view, block pages, drawer, modulators, Matrix, knob cell (mockup session) | Reference; partly built, deviations listed at its top |
| [`llm-adapter.md`](llm-adapter.md) | Which layer the LLM integration builds on (backends, CLI runners, patch tools) | Decided (own `Backend` layer); not built |
| [`patch-code-and-sharing.md`](patch-code-and-sharing.md) | How much code patches get; sharing them safely (precedents, threat model) | Background for "Open directions" in STATUS |
| [`kontakt-ui.md`](kontakt-ui.md) | How Kontakt does instrument UIs | Background |
| [`raster-ui-plan.md`](raster-ui-plan.md) | Raster layers, sprite sheets, shader layers, effects toggle | Its "scrap the components" decision was reversed; the rest are open ideas |
| [`patch-fonts.md`](patch-fonts.md) | Image-based and SVG fonts for patches | Built for titles; ControlPanel text next |
| [`findings.md`](findings.md) | Measurements and lessons from the proofs of concept (resvg, chains, components, Linux) | Archive; numbers from the time |

Images: `img/` (mockup screenshots in `img/chain-editor/`, spike screenshots).
