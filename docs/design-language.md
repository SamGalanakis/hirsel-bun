# Hirsel Design Language

> **Highland Craft** - Functional warmth inspired by Scottish shepherding traditions

**Principles:** Warm not cold, crafted not generic, functional not decorative, grounded not flashy.

---

## Colors

| Token | Purpose | Description |
|-------|---------|-------------|
| `pasture-*` | Backgrounds | Dark fields at dusk |
| `wool-*` | Text | Warm off-whites to browns |
| `amber-*` | Primary accent | Shepherd's lantern |
| `sage` | Success | Highland sage green |
| `golden` | Warning | Wheat field yellow |
| `terra` | Error | Scottish earth red |

### Dark Theme ("Pasture at Dusk")

```css
/* Backgrounds */
--pasture-900: #1a1a1a;  /* Main bg */
--pasture-800: #242424;  /* Cards */
--pasture-700: #2d2d2d;  /* Hover */
--pasture-600: #333333;  /* Borders */

/* Text */
--wool-100: #e8e4df;  /* Primary */
--wool-300: #b5b0a8;  /* Secondary */
--wool-500: #8a8580;  /* Muted */

/* Accent */
--amber-500: #d4a574;  /* Primary action */
--amber-600: #b8895c;  /* Hover */

/* Status */
--sage: #7d9970;
--golden: #c9a227;
--terra: #c45c4a;
```

### Quick Reference

| Need | Use |
|------|-----|
| Main background | `bg-pasture-900` |
| Card background | `bg-pasture-800` |
| Border | `border-pasture-600` |
| Primary text | `text-wool-100` |
| Muted text | `text-wool-500` |
| Accent | `text-amber-500` |
| Success | `text-sage` |
| Warning | `text-golden` |
| Error | `text-terra` |

---

## Typography

**Primary:** ET Book (Bembo-style serif by Edward Tufte)
```css
--font-primary: 'ET Book', 'Palatino', Georgia, serif;
```

**Monospace:** JetBrains Mono for code/terminal
```css
--font-mono: 'JetBrains Mono', 'Fira Code', Consolas, monospace;
```

| Use | Size | Weight |
|-----|------|--------|
| Display heading | 28px | bold |
| Section heading | 20px | bold |
| Card heading | 16px | 600 |
| Body | 15px | normal |
| Caption | 13px | normal |

---

## Motion

```css
--transition-fast:   150ms ease-out;  /* Hover */
--transition-normal: 200ms ease-out;  /* State changes */
--transition-slow:   300ms ease-out;  /* Modals */
```

Always respect `prefers-reduced-motion`.

---

## Component Patterns

### Status Dots

| Class | Color | Use |
|-------|-------|-----|
| `.status-working` | amber + pulse | Active |
| `.status-done` | sage | Complete |
| `.status-waiting` | golden | Pending |
| `.status-error` | terra | Failed |
| `.status-idle` | wool-700 | Inactive |

### Borders & Shadows

- Default radius: 6px (`rounded-md`)
- Cards: 8px (`rounded-lg`)
- Pills: 9999px (`rounded-full`)

```css
--shadow-sm: 0 1px 2px rgba(0,0,0,0.2);
--shadow-md: 0 4px 12px rgba(0,0,0,0.25);
```

### Empty States

Center content with sheep SVG, brief message in `text-wool-400`, optional action button.

---

## Voice & Tone

**Naming:** Pastoral metaphors (hirsel=flock, runs=herding, workers=sheepdogs, Gyp=AI assistant)

**Microcopy:** Friendly but professional. Concise. Helpful hints over bare descriptions.

- Loading: "Gathering the flock..." (sparingly)
- Error: "Something went wrong" (clear, not cute)
- Empty: "No workers active" + hint
