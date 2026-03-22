# Design System Specification: The Alchemical Ledger

## 1. Overview & Creative North Star

### The Creative North Star: "The Architect’s Forensic Record"
This design system moves away from the ephemeral nature of standard software to embrace the weight of a physical artifact. It is a digital substrate for high-stakes agent orchestration, designed to feel like a master mathematician’s personal ledger. We are building an environment that is **intellectual, clinical, and deeply tactile.**

The interface rejects the "friendly" softness of modern SaaS. By utilizing a strictly rectangular geometry (0px border radius) and a high-contrast typographical pairing, we create a sense of absolute precision and permanence. The layout should feel like a series of technical plates in a scientific journal, where every line and character serves a specific, rigorous purpose.

**Key Deviations from Standard UI:**
- **Zero Softness:** Absolute 0px corners on every element.
- **Asymmetric Balance:** Use white space and technical annotations to create focal points rather than centered, "perfect" grids.
- **Substrate over Surface:** The UI is not a "screen"; it is a textured parchment that reacts to data like ink on paper.

---

## 2. Colors

The palette is a monochromatic study in depth, utilizing a "Deep Ink" philosophy.

### The Palette (Core Tokens)
- **Background (`#131313`):** The primary substrate. Use a subtle grain overlay (2-3% opacity) to mimic textured cardstock.
- **Primary (`#ffffff`):** Reserved for high-priority technical data and active states.
- **On-Surface / Tertiary (`#e5e2e1`):** A pale vellum/champagne. This is the primary text color, providing a softer contrast than pure white.
- **Secondary (`#c1c8cb`):** A slate-grey for secondary annotations and diagrammatic lines.

### The "No-Line" Rule
Prohibit the use of 1px solid borders for general sectioning or layout containment. Boundaries must be defined through:
1. **Background Color Shifts:** A `surface-container-low` (`#1c1b1b`) section sitting on a `background` (`#131313`).
2. **Vertical Rhythms:** Using spacing increments (e.g., `spacing-16`) to create visual breaks.

### Surface Hierarchy & Nesting
Treat the UI as a series of physical layers. Each "inner" container uses a slightly higher tier to define importance:
- **Root Substrate:** `surface` (`#131313`)
- **Main Content Well:** `surface-container-low` (`#1c1b1b`)
- **Active Dialogue/Focus:** `surface-container-high` (`#2a2a2a`)

### Signature Textures
Main CTAs or hero headers should not be flat. Apply a very subtle linear gradient (from `primary` to `primary-container`) to give text a "metallic foil" or "fresh ink" sheen.

---

## 3. Typography

The typographic system is the soul of the ledger. It relies on the tension between the intellectual (Newsreader) and the technical (Space Grotesk).

### The Editorial Pair
- **Newsreader (Italic):** Used for `display` and `headline` levels. It represents the "human" element—intellectual notes, philosophical labels, and high-level orchestration summaries. It should always feel like a marginalia note in a masterwork.
- **Space Grotesk:** Used for `title`, `body`, and `label`. This is the "technical" element—agent status, code snippets, micro-typography, and coordinates.

### Typographic Hierarchy
- **Display-LG (Newsreader Italic, 3.5rem):** Reserved for major system states or section headers.
- **Title-MD (Space Grotesk, 1.125rem):** Used for agent names and active data streams.
- **Label-SM (Space Grotesk, 0.6875rem):** Technical metadata, timestamps, and diagram annotations. Always set in uppercase with a 0.05em letter spacing for a "technical stamp" feel.

---

## 4. Elevation & Depth

### The Layering Principle
Depth is achieved through **Tonal Layering** rather than structural lines.
- **Stacking:** Place a `surface-container-lowest` (`#0e0e0e`) card on a `surface-container-low` (`#1c1b1b`) section to create a "recessed" effect, as if the content is engraved into the parchment.

### Ambient Shadows
Shadows must mimic natural, ambient light on paper.
- **Shadow Token:** Large blur values (24px–48px) at 6% opacity using a tint of `on-surface` (`#e5e2e1`). Avoid black shadows; use the "glow" of the paper’s reflection.

### The "Ghost Border"
When a line is absolutely necessary for diagrammatic clarity (e.g., a nautilus spiral or a connecting node), use the **Ghost Border**:
- **Token:** `outline-variant` (`#474747`) at 20% opacity. It must look like a faint pencil guideline that hasn't been erased yet.

---

## 5. Components

### Buttons
- **Primary:** No background. 1px `primary` border (exception to the no-line rule for high-action clarity). Text in `primary` Space Grotesk.
- **Secondary:** `surface-container-high` background. No border. Text in `on-surface`.
- **States:** On hover, the background should shift to `primary` and text to `on-primary`. Transition must be 0ms (instant) to feel like a mechanical switch.

### Inputs & Orchestration Fields
- **Text Inputs:** Use a single bottom border (`outline-variant`). No enclosing box.
- **Helper Text:** Newsreader Italic (`body-sm`) placed in the margins to look like a handwritten annotation.

### Cards & Lists
- **Forbid dividers.** Separate list items using `spacing-2` (0.7rem) of vertical white space or a subtle shift from `surface-container-low` to `surface-container-lowest`.
- **Diagrammatic Elements:** Every card should feature a 1px technical line annotation in the corner, acting as a "crop mark" or "registration mark" to ground it in the architect vibe.

### Custom Component: The "Nautilus Loader"
Instead of a circular spinner, use a 1px technical line drawing of a nautilus spiral that "sketches" itself in and out to indicate agent processing.

---

## 6. Do's and Don'ts

### Do:
- **Embrace Asymmetry:** Offset content blocks. Let labels sit in the margins, not just above inputs.
- **Use Micro-Typography:** Populate empty spaces with "technical noise"—coordinates, version numbers, or grid references in `label-sm`.
- **Maintain 0px Radius:** Everything is a hard edge. No exceptions for buttons, chips, or cards.

### Don't:
- **Avoid "Dark Mode Blue":** Never use blue-tinted greys. Stick to the charcoal, slate, and champagne tokens provided.
- **No Rounded Corners:** If a component library defaults to 4px or 8px, it must be overridden to 0px.
- **No Heavy Borders:** Never use high-contrast white borders for layout containers; use tonal surface shifts instead.
- **Don't Center Everything:** Modern ledgers are read left-to-right with heavy margins. Avoid the "centered hero" trap.

---

## 7. Spacing Scale Reference
| Name | Value | Use Case |
| :--- | :--- | :--- |
| **spacing-px** | 1px | Technical lines, registration marks. |
| **spacing-1** | 0.35rem | Tight metadata groupings. |
| **spacing-4** | 1.4rem | Content block padding. |
| **spacing-10** | 3.5rem | Section breathing room (The "Editorial Margin"). |
