# How much code do patches get, and can they be shared safely?

_Research notes, 2026-11-18. Feeds "Open directions: Patch code and graph patches" in [`STATUS.md`](../STATUS.md)._

## The question

Three routes were on the table (see STATUS):

1. **Middle ground (today):** sandboxed Luau, declarative chains, component UIs drawn as SVG.
2. **Graph only, no code:** static graph data plus a generic node-graph view.
3. **Full code-your-plugin:** patches draw to a canvas or GPU directly, or use web views. It was assumed
   this rules out sharing patches.

Personal use doesn't care: Flemming writes his own chains and lets the AI listen and adjust. The
question only matters for a public release, if users share patches.

**Reframing.** The tension isn't "code vs. no code". It's **what the code may touch**. Sandboxed code
with a small, safe API is shareable; a powerful escape hatch (process execution, file system, raw
GPU, a browser bridge) is not. "Pretty UIs" and "safe sharing" only conflict if visual power is handed
out as raw access instead of through safe primitives.

## Precedents: do other ecosystems share unsafe code?

| Ecosystem | What gets shared | What the code can do | Verdict |
|---|---|---|---|
| **REAPER ReaScripts** (Lua/EEL/Python, shared via ReaPack) | scripts | full Lua `io`/`os`, and `reaper.ExecProcess` runs arbitrary commands ([example](https://huggingface.co/spaces/TechAudio/ReaSpeech-Cloud/blob/e378a99bed2b88db8d80ca298df0ffc1f9da6a73/reascripts/ReaSpeech/source/ExecProcess.lua), [API](https://www.reaper.fm/sdk/reascript/reascripthelp.html)) | **unsafe code, shared widely** |
| **REAPER JSFX** (effects with custom `gfx` drawing, shared via [ReaPack](https://github.com/ReaTeam/JSFX)) | effect source | reads files, incl. absolute paths since v6.17; writes only its own state in `@serialize` ([reference](https://www.reaper.fm/sdk/js/file.php)); no process execution found | **mostly sandboxed** (read-anywhere) |
| **Max for Live** (maxforlive.com) | devices | `js` has the full Max/Live API ([docs](https://docs.cycling74.com/max8/vignettes/javascript_usage_topic)); frozen devices embed externals, i.e. native binaries ([forum](https://cycling74.com/forums/max-for-live-patches-and-external-fileslibraries)) | **unsafe code, shared widely** |
| **JUCE / iPlug2 web views** | (nothing separately) | the web UI is part of the plugin binary | **not comparable**: you trust the vendor's binary anyway |
| **WebCLAP / WCLAP** | plugins as WebAssembly + web view ([WebCLAP](https://github.com/WebCLAP), [webview-gui](https://awesome.ecosyste.ms/projects/github.com%2Fgeraintluff%2Fwebview-gui)) | WebAssembly sandbox + browser engine | **sandboxed by design**; young (draft webview extension) |
| **Kontakt** (third-party libraries, sold and shared) | instruments with KSP scripts and Komplete UI | NI's own languages, no general I/O | **sandboxed, with rich UIs** (below) |
| **Roblox** | user scripts at huge scale | sandboxed Luau with a capability system ([docs](https://create.roblox.com/docs/scripting/capabilities)) | **sandboxed**: same language and model as ours |

**Finding:** the audio world *does* share unsafe code (ReaScripts, Max for Live), and the community
accepts the risk. JUCE web views are no precedent: there's no separate code to share. The two models
closest to ours, Kontakt and Roblox, show that rich UIs and safe sharing go together when the API is
the boundary.

## Kontakt as orientation

_Deeper dive: [`kontakt-ui.md`](kontakt-ui.md) (terminology, assets, KSP vs. Komplete UI, parameters,
modulation, MIDI)._

How Kontakt libraries draw their UIs:

- **Classic KSP:** fixed control types (`ui_knob`, `ui_slider`, `ui_switch`, `ui_label`, `ui_panel`,
  …) skinned with images: `$CONTROL_PAR_PICTURE` points at a PNG, animated knobs are image strips
  (one frame per value step), custom fonts are PNGs too
  ([KSP UI commands](https://www.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands.html)).
  Most "pretty" third-party UIs are pre-rendered artwork: backgrounds plus filmstrips. This is where
  the typical Kontakt look comes from, and it's cheap to render.
- **Komplete UI** (new; v1.0 in Kontakt 8.2, Feb 2025): a component framework with its own language,
  **Komplete Script**, for interface and instrument logic, next to KSP for audio/MIDI
  ([language tour](https://developer.native-instruments.com/komplete-ui/docs/LanguageTour/),
  [KSP → Komplete UI](https://developer.native-instruments.com/komplete-ui/docs/KSPtoKompleteUI/)).
  - Reactive: component properties and **Canvas paint callbacks** re-run when their dependencies
    change; vector drawing via `ui.Path` ([changelog](https://developer.native-instruments.com/komplete-ui/docs/CHANGELOG/)).
  - Custom components bind to KSP controls by id
    ([tutorial](https://developer.native-instruments.com/komplete-ui/docs/KontaktControls/Tutorials/Create%20Your%20Own%20Components/)).
  - Tooling: VS Code extension with highlighting, hover docs and snippets; Lua in Creator Tools only for
    building instruments, not at runtime ([Building in Kontakt](https://www.native-instruments.com/en/products/komplete/samplers/kontakt-8/building-in-kontakt/)).

**Takeaway:** NI arrived at almost exactly our design: a sandboxed scripting language, reactive
components, a vector canvas with paint callbacks, bound controls, editor tooling. Their libraries
are shared and sold at scale without carrying malware risk, because the language can't reach the
system. Visual quality comes from artwork (images, filmstrips) and vector drawing, not from raw GPU
access.

What we could adopt from Kontakt:
- **Filmstrip / image-strip components** (pre-rendered artwork per value step): cheap and pretty.
- A **paint-callback canvas** as a component type (we draw via SVG; a canvas with path primitives
  would be the immediate-mode equivalent).
- The split between **audio/logic language** and **UI language** stays as is (one Luau runtime, two
  preludes).

## Would WebAssembly + WebGL solve it?

Only half of it.

- **WebAssembly** isolates logic: a module can only call what the host imports. We already have that
  with Luau (no I/O, an instruction budget, a memory cap), and Luau is much friendlier for LLMs.
- **WebGL only exists inside a browser.** Two ways to get it:
  1. **Embed a web view.** The browser engine is the sandbox, and JS, Canvas, WebGL and WebGPU come
     with it. This is the WebCLAP path. Costs: one browser per plugin instance (memory, startup), web
     view embedding on Linux is fiddly (WebKitGTK in X11 hosts), and the UI API stops being small and
     LLM-friendly.
  2. **Expose our own GPU API.** That means redoing a browser's security work. wgpu, which Firefox uses
     for WebGPU, treats its shader validator naga as the trust boundary for untrusted shaders
     ([wgpu notes](https://raw.githubusercontent.com/gfx-rs/wgpu/HEAD/AGENTS.md)). It still produces
     memory-safety CVEs ([CVE-2026-6773](https://bugzilla.mozilla.org/show_bug.cgi?id=2015959),
     [CVE-2026-12293](https://bugzilla.mozilla.org/show_bug.cgi?id=2039568)), and Firefox isolates it
     in a GPU process.
- **Middle option worth a spike:** a single **shader layer** primitive (Shadertoy-style fragment
  shader, validated by naga, inputs: time, size, bound value, meter level). It gives GPU visuals with a
  tiny surface. A hung shader (GPU freeze) remains a risk and would need time limits and a fallback.

## Threat model for a shared patch

| Entry point | Threat | Mitigation (status) |
|---|---|---|
| Luau API | sandbox escape, file/network access | Luau sandbox, no `io`/`os`/`loadstring`, `require` limited to the patch folder (done); keep the API capability-minded |
| CPU / memory | freeze the DAW or UI | instruction budget per call, 16 MB memory cap (done); DSP cost limits per patch (open) |
| Audio output | full-scale noise, hearing or speaker damage | **safety limiter in the shell** for untrusted patches (open, cheap) |
| Parsers | crafted SVG, image or font exploits a decoder (resvg/usvg, png, fonts) | images only from the patch folder (done); **fuzz the SVG/image path** (open) |
| DSP blocks | NaN/inf, denormals, huge allocations from settings | settings clamped, buffers sized at load, finite checks in tests (mostly done); a NaN guard on the output (open) |
| Future escape hatches | web view bridge, raw GPU, file access | don't add them for untrusted patches; gate behind a "trusted patch" flag |
| Social engineering | a patch that pretends to be something else | out of scope for the code; a matter for the sharing platform |

## Options compared

Scores are 1 (bad) to 5 (good) and only a first guess.

| Criterion | 1 Middle (today) | 2 Graph only | 3a Full code, unsafe | 3b Full code via safe primitives |
|---|---|---|---|---|
| LLM-friendliness | 5 | 4 (data) | 2 (web stack / GL) | 5 |
| Visual expressiveness | 3 | 1 | 5 | 4 (canvas, filmstrips, shaders) |
| Shareability / safety | 4 | 5 | 1 | 4 |
| Stability / performance | 4 | 5 | 2 | 4 |
| Effort from here | 5 (built) | 3 (graph editor) | 2 | 4 |
| Fits "AI adjusts my chain" | 5 | 5 | 4 | 5 |

Routes 1 and 3b are the same architecture: 3b is route 1 with more visual primitives. A graph
**view** (read-only, or a layer for tapping and metering) fits on top of either.

## Spikes before deciding

Each spike has a go/no-go question.

1. **Shader layer:** a component whose layer is a WGSL fragment shader (naga-validated, inputs time,
   size, value, level), rendered into the existing layer cache.
   - Go if: renders in Reaper and Carla on Linux and Windows, < 0.5 ms per frame for a 300×300 layer,
     and an infinite loop in a shader can't freeze the UI for more than a second.
2. **Web view in the plugin:** an `nih-plug` web view inside Reaper and Carla on Linux.
   - Go if: it embeds reliably and costs < 100 MB RAM and < 1 s startup per instance.
3. **Canvas + filmstrips (Kontakt-style):** a `Canvas` component with path drawing (paint callback,
   reactive like our components) and an image-strip knob.
   - Go if: an LLM produces a good-looking panel with it in one prompt.
4. **Sharing check:** fuzz the SVG/image path for a day; add the shell's safety limiter.

## Decision

Flemming's rule (2026-11-18): **if comparable ecosystems share unsafe code, we allow it too.**

What the research found:
- **Yes:** REAPER ReaScripts and Max for Live share fully unsafe code (process execution, native
  externals), and those communities live with it.
- **Partly:** JSFX is mostly sandboxed (it reads files anywhere, but has no process execution found).
- **Not comparable:** JUCE web views (part of the vendor's binary).
- **No:** Kontakt (the closest product to ours) and Roblox share code safely, through the API boundary.

So the rule's condition is met. That **permits** route 3a, but doesn't require it.

**Recommendation:** go route 3b. Grow visual power through safe primitives (canvas/paths, filmstrips,
maybe shaders), Kontakt-style. If a truly unsafe capability is ever wanted (web view, raw GPU, file
system), add it as an **opt-in "trusted patch" capability**. Untrusted patches without it still load:
the chain, params and macros as usual, with the generic UI instead of the coded one. That's the
locked-down mode idea, and it costs nothing today.

_Open: Flemming confirms 3a ("unsafe allowed, like ReaScripts / M4L") or 3b ("safe primitives,
trusted-patch flag for anything else")._
