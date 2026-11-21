> _Imported 2026-11-20 from Flemming's notes: **the source of truth for ScriptFX's modulation**
> (STATUS "Decisions"). Built from it so far: slots for macros (amount, polarity, curve as tension,
> plus bypass; our deviation: no invert, the amount's sign is the direction, and bipolar swings ±amount/2 so both polarities cover the same range), additive normalized deltas, the knob's range arc + live dot. Not built:
> aux source, output gain, remap tables, meta-modulation, typed amounts in units. The Matrix can
> inspect and edit existing macro slots; other sources and route editing remain open._

# Technical Whitepaper: Normalized Modulation Architecture & UX Patterns in Xfer Records Serum & Serum 2

**Target Audience:** Audio DSP Engineers, Synthesizer Architects, Plugin UI/UX Developers  
**Subject:** Reference Specification for Implementing a Normalized, Slot-Based Modulation Engine  

---

## 1. Executive Summary

Modern software synthesizers rely on deep, intuitive modulation systems to transform static sound-design parameters into dynamic, expressive instruments. Xfer Records' **Serum** and **Serum 2** established the industry standard for wavetable synthesizer modulation UX by decoupling **modulation math** from **parameter unit scaling**, combining **direct-on-knob visual feedback** with a **tabular modulation matrix**, and implementing a deterministic **control-rate evaluation graph**.

This whitepaper defines the data structures, mathematical signal flow, DSP graph constraints, and UI rendering patterns required to replicate Serum's modulation architecture in a custom audio plugin.

---

## 2. Core Architectural Philosophy

Serum’s modulation engine is built on three foundational principles:

1. **Normalized Signal Domain ($[0.0, 1.0]$):** All parameters, modulation sources, and matrix depths operate in normalized floating-point space. Parameter skewing (e.g., logarithmic frequency scaling) occurs strictly *after* all modulation deltas are summed and clamped.
2. **Non-Destructive Additive Offsets:** Modulations do not overwrite a parameter's manual value; they compute real-time bipolar or unipolar deltas ($\Delta$) that are summed onto the parameter's static base value ($B$).
3. **Dual-Layer UX (Direct Manipulation + Matrix):** Every routing exists simultaneously as an interactive visual arc on the destination control and as an explicit row in the global Modulation Matrix.

---

## 3. Data Model: The Modulation Slot

Each modulation routing is represented as an independent `ModulationSlot` object within an ordered collection (the Matrix).

### 3.1 Slot Properties

| Field | Data Type / Range | Default | Description |
| :--- | :--- | :--- | :--- |
| `id` | `uint32` / UUID | Auto | Unique slot identifier for meta-modulation targeting. |
| `bypass` | `bool` | `false` | Mutes slot contribution without destroying configuration. |
| `source` | `SourceID` | `None` | Primary modulator (LFO, Envelope, Macro, MIDI/Note state). |
| `destination` | `TargetID` | `None` | Any modulatable DSP parameter **or** another slot's `amount`. |
| `amount` | `float` $[-1.0, +1.0]$ | `0.0` | Modulation depth (`-100%` to `+100%`) relative to parameter span. |
| `polarity` | `Enum { Unipolar, Bipolar }` | `Unipolar` | `->` (one-sided sweep) or `<->` (symmetric bidirectional sweep). |
| `sourceCurve` | `float` $[-1.0, +1.0]$ | `0.0` | Tension exponent applied to `source` prior to polarity mapping. |
| `sourceRemap` | `LookupTable / Curve` | Linear | *(Serum 2)* Multi-breakpoint transfer function $f_{\text{remap}}(x)$. |
| `auxSource` | `SourceID` | `None` | Secondary modulator that scales the slot's effective `amount`. |
| `auxMode` | `Enum { Mul, MulInv, Bypass }` | `Mul` | `*` (multiply), `* (inv)` (multiply by $1 - a$), or `Bypass`. |
| `auxCurve` | `float` $[-1.0, +1.0]$ | `0.0` | Tension exponent applied to `auxSource`. |
| `auxRemap` | `LookupTable / Curve` | Linear | *(Serum 2)* Multi-breakpoint transfer function for `auxSource`. |
| `outputGain` | `float` $[0.0, 1.0]$ | `1.0` | Final post-Aux attenuation scalar (`0%` to `100%`). |
| `stereoMode` | `Enum { Mono, StereoSplit }` | `Mono` | Enables independent L/R modulation offsets per voice. |

### 3.2 Supported Modulation Sources
* **Per-Voice Sources (Polyphonic):** Envelopes (`ENV 1–3+`), LFOs (`LFO 1–4+` in Trigger/Env modes), `Velocity`, `Note` (keytracking), `Poly Aftertouch`, `Note-On Random` (unipolar random value latched at `noteOn`), `Note-On Alternate` (alternates `0.0` / `1.0` on consecutive notes).
* **Global Sources (Monophonic):** `Macros 1–4+`, Free-running `LFOs`, `Mod Wheel (CC1)`, `Channel Aftertouch`, `Pitch Bend`, `Chaos / Noise` generators, and `Audio In` (envelope follower).

---

## 4. Mathematical Model & Signal Flow

For any given destination parameter $P$ on voice $v$ at control tick $t$, the engine computes the final DSP parameter value $V_{\text{DSP}}(t)$ through a five-stage pipeline.

### Stage 1: Source Shaping (Curve & Remap)
Given a raw unipolar source value $s(t) \in [0.0, 1.0]$:
1. If a **Remap Table** (Serum 2) is active, evaluate piecewise interpolation:
   $$s_{\text{remap}}(t) = \text{LUT}_{\text{source}}\big(s(t)\big) \quad \in [0.0, 1.0]$$
2. Apply **Tension Curve** $c \in [-1.0, 1.0]$ (where $c = 0$ is linear):
   $$s'(t) = \text{applyCurve}\big(s_{\text{remap}}(t),\ c\big) \quad \in [0.0, 1.0]$$
   *(Implementation note: A standard tension mapping uses power scaling $x^{\gamma}$ where $\gamma = 2^{k \cdot c}$, or rational polynomial shaping $\frac{x(1 + k)}{1 + k \cdot x}$ for fast CPU execution without `pow()` calls).*

### Stage 2: Polarity Mapping
Convert the shaped unipolar signal $s'(t) \in [0.0, 1.0]$ into a directional signal $p(t)$:
* **Unipolar (`->`):**
  $$p(t) = s'(t) \quad \in [0.0, 1.0]$$
* **Bipolar (`<->`):**
  $$p(t) = 2.0 \cdot s'(t) - 1.0 \quad \in [-1.0, +1.0]$$

> **Design Note on Bipolar Scaling:** Because $p(t)$ spans $[-1.0, +1.0]$, setting `Amount = +1.0` (`100%`) yields a modulation swing of $\pm 1.0$ around the base value $B$. When $B = 0.5$ (12 o'clock), `Amount = 100%` sweeps the entire parameter range from `0.0` to `1.0`.

### Stage 3: Aux Source Multiplier Evaluation
Evaluate the secondary scaling factor $a(t) \in [0.0, 1.0]$:
* If `auxSource == None` or `auxMode == Bypass`:
  $$a(t) = 1.0$$
* Otherwise, shape the raw Aux signal $s_{\text{aux}}(t) \in [0.0, 1.0]$ through `auxRemap` and `auxCurve` to obtain $s'_{\text{aux}}(t)$:
  * If `auxMode == Mul` (`*`):
    $$a(t) = s'_{\text{aux}}(t)$$
  * If `auxMode == MulInv` (`* inv`):
    $$a(t) = 1.0 - s'_{\text{aux}}(t)$$

### Stage 4: Slot Delta ($\Delta_i$) & Additive Summation
Let $A_{\text{eff}, i}(t)$ be the slot's effective `amount` (which equals its static `amount` property plus any meta-modulation deltas targeting this slot's `amount`).

The modulation delta produced by slot $i$ is:
$$\Delta_i(t) = p_i(t) \cdot A_{\text{eff}, i}(t) \cdot a_i(t) \cdot G_i$$
where $G_i \in [0.0, 1.0]$ is `outputGain`.

All active slots $i \in \{1 \dots N\}$ targeting parameter $P$ sum linearly onto the parameter's normalized manual base value $B \in [0.0, 1.0]$, followed by a hard clamp:
$$V_{\text{norm}}(t) = \text{clamp}\left( B + \sum_{i=1}^{N} \Delta_i(t),\ 0.0,\ 1.0 \right)$$

### Stage 5: Parameter Taper / Unit Conversion
The clamped normalized value $V_{\text{norm}}(t)$ is transformed into real-world DSP units via the parameter's forward mapping function $f_{\text{param}}$:
$$V_{\text{DSP}}(t) = f_{\text{param}}\big(V_{\text{norm}}(t)\big)$$

* **Logarithmic Parameters (Filter Cutoff, LFO Rate):**
  $$f_{\text{log}}(x) = V_{\min} \cdot \left(\frac{V_{\max}}{V_{\min}}\right)^x$$
* **Linear Parameters (Mix, Pan, Pitch Semitones):**
  $$f_{\text{lin}}(x) = V_{\min} + x \cdot (V_{\max} - V_{\min})$$

#### Handling Musical Unit Inputs (e.g., Pitch Semitones)
When a user enters a musical unit such as `7st` into the Matrix `Amount` for a bipolar pitch knob spanning $[-64\text{ st}, +64\text{ st}]$ (total span $S = 128\text{ st}$):
$$A = \frac{\Delta_{\text{units}}}{S} = \frac{7}{128} \approx 0.0546875 \quad (\approx 5.5\%)$$
Because $f_{\text{lin}}$ is linear across the span, adding $\Delta = 0.0546875$ to $B$ shifts the resulting pitch by exactly $+7.0\text{ semitones}$ regardless of the knob's starting position.

---

## 5. DSP Engine Constraints & Graph Execution

### 5.1 Control-Rate Sub-Block Processing
Modulation matrices should not be evaluated per audio sample unless specifically performing audio-rate FM/AM (which Serum handles via dedicated oscillator warp modes rather than the Matrix).
* **Control Block Size:** Evaluate the Modulation Matrix once every $K$ samples (typically $K = 32$ or $K = 64$ samples at $44.1/48\text{ kHz}$, yielding a control rate of $\approx 750\text{ Hz}–1.5\text{ kHz}$).
* **Parameter Smoothing:** Apply a lightweight one-pole low-pass filter or linear ramp across the $K$ samples inside the DSP module for sensitive targets (Filter Cutoff, Oscillator Gain, Pan) to eliminate zipper noise.

### 5.2 Preventing Feedback Loops (Meta-Modulation)
Serum permits matrix slots to modulate:
1. Modulator parameters (e.g., `LFO 1 -> LFO 2 Rate`).
2. Other Matrix rows' `Amount` fields (e.g., `Macro 1 -> Mod 3 Amount`).

To prevent infinite recursion without rejecting valid creative patches:
* **Single-Pass Top-Down Evaluation:** Iterate through Matrix rows `0` to `N-1` strictly once per control tick.
* **Implicit Unit Delay on Back-Edges:** Store each slot's computed `effectiveAmount` in a state buffer. If Row `5` modulates Row `2`'s `Amount`, Row `2` reads Row `5`'s output from tick $t - 1$ (a 1-control-block delay of $\sim 0.7\text{ ms}$). This guarantees $O(N)$ deterministic execution time and naturally damps feedback oscillations.

### 5.3 Polyphonic vs. Monophonic Voice Isolation
* Maintain a **Global Mod State** per control tick for monophonic sources (`Macros`, `ModWheel`, `Free LFOs`).
* Maintain a **Per-Voice Mod State** inside each active voice's `process()` loop for polyphonic sources (`Envelopes`, `Triggered LFOs`, `Velocity`, `Note`).
* For a voice $v$, a parameter's final value sums both global and voice-local deltas before clamping.

---

## 6. UI & UX Specification

### 6.1 Multi-Layer Knob Rendering
A modulated rotary knob must render **four visual layers** in back-to-front Z-order:

1. **Track Background:** Dark circular groove spanning the knob's rotation arc (typically $270^\circ$, from $-135^\circ$ to $+135^\circ$).
2. **Modulation Range Arc (Colored Outer Ring):**
   * Starts at the angle corresponding to Base Value $B$.
   * **Unipolar (`->`):** Draws an arc from $\theta(B)$ to $\theta\big(\text{clamp}(B + A, 0, 1)\big)$.
   * **Bipolar (`<->`):** Draws two symmetric arcs from $\theta(B)$ to $\theta\big(\text{clamp}(B + A, 0, 1)\big)$ and $\theta\big(\text{clamp}(B - A, 0, 1)\big)$.
   * **Interactive Drag Handle:** Renders a small grab node at the end of the primary arc. Dragging this node radially/vertically updates `slot.amount` in real time.
3. **Knob Cap & Base Indicator:** The physical rotary control graphic with a high-contrast tick mark at $\theta(B)$.
4. **Live Calculated Value Indicator (Moving Dot / Glowing Rim):**
   * Renders a bright indicator dot along the outer perimeter at $\theta\big(V_{\text{norm}}(t)\big)$.
   * **Polyphonic UI Telemetry:** The audio thread writes $V_{\text{norm}}(t)$ of the **most recently triggered active voice** (or Voice 0 when idle) into an atomic/lock-free telemetry buffer read by the UI thread at $60\text{ FPS}$.

### 6.2 Drag-and-Drop Assignment Workflow
* **Source Draggables:** Every LFO, Envelope, and Macro header displays a draggable handle/badge.
* **Hover Hit-Testing:** While dragging a source, highlight valid target knobs with a bounding glow.
* **Drop Action:** Releasing over a knob appends a new `ModulationSlot(source, target, amount = 0.0, polarity = Unipolar)` and selects it as the active arc on that knob so the user can immediately drag the range handle.
* **LFO Breakpoint Drops (Serum 2 Pattern):** Hovering over an LFO graph node reveals dual `X` and `Y` drop zones, binding the source to that node's normalized time (`X`) or amplitude (`Y`) parameter.

### 6.3 Knob Right-Click Context Menu
Right-clicking any modulatable control must generate a dynamic context menu:

* **`Mod Source` $\rightarrow$ `[Submenu]`:** Lists all sources grouped by category (`Envelopes`, `LFOs`, `Macros`, `MIDI/Note`), allowing assignment of non-draggable sources (`Velocity`, `Keytrack`, `Aftertouch`, `Note-On Rand`).
* *(If 1+ routings exist on this knob):*
  * **`Bypass Mod: [Source Name]`** (Checkable toggle per routing).
  * **`Remove Mod: [Source Name]`** (Deletes specific Matrix row).
  * **`Modulate [Source Name] Amount` $\rightarrow$ `[Submenu]`** (Creates a second Matrix row targeting the first row's `Amount`).
  * **`Remove All Modulations`** (Clears all slots targeting this parameter).

### 6.4 Dedicated Matrix View Features
The tabular Matrix view exposes advanced controls not shown on the main synthesis panel:
* **Drag-to-Reorder Rows:** Allows users to organize routings and control evaluation priority for meta-modulation.
* **Columns (Left-to-Right):** `Bypass` | `Source` | `Amount` (slider + double-click text entry) | `Polarity` (`->` / `<->` toggle) | `Destination` | `Curve / Remap` (mini graph editor) | `Aux Source` | `Aux Mode` (`*`, `* inv`, `Bypass`) | `Output` (`0–100%`).
* **Smart Text Parser on `Amount`:** Double-clicking an `Amount` cell accepts raw percentages (`50`, `-25%`) as well as destination-aware units (`7st`, `-12st`, `1oct`).

---

## 7. Implementation Checklist for Plugin Developers

- [ ] **Parameter Engine:** Ensure all DSP parameters expose normalized `toNormalized()` and `fromNormalized()` conversions with proper skew factors.
- [ ] **Lock-Free Matrix Sync:** Store Matrix routing topology in an immutable snapshot swapped via atomic pointer / double-buffer between the UI thread and Audio thread; use atomic floats for continuous `amount` and `outputGain` edits.
- [ ] **Control-Rate Tick:** Decouple modulation evaluation (`32–64` samples) from audio sample rendering and apply parameter smoothing on destinations.
- [ ] **UI Telemetry Ring/Atomic:** Publish active-voice normalized values to the GUI thread at display refresh rates (`60–120 Hz`) without locks.
- [ ] **Multi-Arc Knob Component:** Build a custom rotary slider widget capable of rendering base ticks, unipolar/bipolar arcs, interactive amount handles, and live telemetry dots.