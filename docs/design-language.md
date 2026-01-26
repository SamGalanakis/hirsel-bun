# Hirsel Design Language

> **Highland Craft** - A design system inspired by Scottish shepherding traditions

## Philosophy

Hirsel draws from the rugged beauty of the Scottish Highlands - the craft of herding sheep across misty glens, the warmth of a croft at dusk, the texture of raw wool, and the practical elegance of pastoral tools. This isn't decorative rustic cosplay; it's **functional craft** that brings warmth and character to a technical tool.

**Core Principles:**
1. **Warm, not cold** - Technology doesn't have to feel sterile
2. **Crafted, not generic** - Every detail should feel intentional
3. **Functional, not decorative** - Aesthetics serve usability
4. **Grounded, not flashy** - Confidence without showiness

---

## Color System

### Semantic Naming

Colors use **pastoral metaphors** that connect to the herding theme:

| Token | Purpose | Description |
|-------|---------|-------------|
| `pasture-*` | Backgrounds | The land - dark fields at dusk |
| `wool-*` | Text/foreground | Sheep wool - warm off-whites to browns |
| `amber-*` | Primary accent | The shepherd's lantern - warm golden light |
| `sage` | Success/done | Highland sage - natural green |
| `golden` | Warning/waiting | Wheat fields - alert yellow |
| `terra` | Error/danger | Scottish earth - terracotta red |

### Palette: Hirsel Dark

**"Pasture at Dusk"** - The default theme evokes twilight on the hills.

```css
/* Backgrounds (dark to light) */
--pasture-900: #1a1a1a;  /* Deepest - main background */
--pasture-800: #242424;  /* Cards, panels */
--pasture-700: #2d2d2d;  /* Elevated surfaces, hover */
--pasture-600: #333333;  /* Borders, dividers */
--pasture-500: #404040;  /* Subtle accents */
--pasture-400: #525252;  /* Disabled backgrounds */

/* Text (light to dark) */
--wool-100: #e8e4df;  /* Primary text - like raw wool */
--wool-200: #d4cfc7;  /* Secondary text */
--wool-300: #b5b0a8;  /* Tertiary, descriptions */
--wool-400: #9d9890;  /* Placeholder text */
--wool-500: #8a8580;  /* Muted text, captions */
--wool-600: #706b66;  /* Very muted */
--wool-700: #5a5550;  /* Disabled text */

/* Primary accent - Shepherd's Lantern */
--amber-300: #f0d4ac;  /* Lightest glow */
--amber-400: #e8c19a;  /* Highlight */
--amber-500: #d4a574;  /* Primary action */
--amber-600: #b8895c;  /* Hover state */
--amber-700: #9a7048;  /* Pressed state */
--amber-800: #7a5838;  /* Dark accent */

/* Status colors */
--sage:        #7d9970;  /* Success - highland sage */
--sage-light:  #9ab88c;
--sage-dark:   #5c7852;

--golden:       #c9a227;  /* Warning - wheat fields */
--golden-light: #dbb94a;
--golden-dark:  #a6841c;

--terra:       #c45c4a;  /* Error - Scottish earth */
--terra-light: #d4786a;
--terra-dark:  #a34538;
```

### Palette: Hirsel Light

**"Pasture at Dawn"** - Bright and warm for light mode users.

```css
/* Backgrounds (light to dark) */
--pasture-900: #faf9f7;  /* Main background - morning mist */
--pasture-800: #f4f2ef;  /* Cards, panels */
--pasture-700: #eae7e3;  /* Elevated surfaces */
--pasture-600: #ddd9d3;  /* Borders */

/* Text (dark to light) */
--wool-100: #1f1d1a;  /* Primary text */
--wool-300: #4a4744;  /* Secondary */
--wool-500: #6b6864;  /* Muted */
--wool-700: #a09c97;  /* Disabled */

/* Amber adjusts for light backgrounds */
--amber-500: #a67c4e;
--amber-600: #8a6640;
```

---

## Typography

### Font Stack

Hirsel uses **ET Book** throughout - a Bembo-style serif designed by Edward Tufte for his books on data visualization. This choice reflects Hirsel's core purpose: bringing clarity to complex systems.

**Primary (ET Book):**
```css
--font-primary: 'ET Book', 'Palatino', 'Palatino Linotype', Georgia, serif;
```

ET Book brings:
- **Scholarly gravitas** without being stuffy
- **Excellent readability** at all sizes (designed for dense information)
- **Oldstyle figures** (3, 4, 5 sit on baseline) - beautiful for status numbers
- **Distinctive character** - stands apart from typical app fonts

| Weight | Use |
|--------|-----|
| Roman | Body text, descriptions, UI labels |
| Semi-bold | Card titles, navigation items |
| Bold | Headings, emphasis |
| Display Italic | Decorative, quotes, empty states |

**Monospace / Code:**
```css
--font-mono: 'JetBrains Mono', 'Fira Code', 'SF Mono', Consolas, monospace;
```
Keep *JetBrains Mono* for code, terminal output, and technical content.

### Font Loading

ET Book is self-hosted. Download from [GitHub](https://github.com/edwardtufte/et-book) and add to your project:

```
src/fonts/
├── et-book-roman-line-figures.woff2
├── et-book-roman-old-style-figures.woff2
├── et-book-semi-bold-old-style-figures.woff2
├── et-book-bold-line-figures.woff2
└── et-book-display-italic-old-style-figures.woff2
```

Add to CSS:
```css
@font-face {
  font-family: 'ET Book';
  src: url('/fonts/et-book-roman-line-figures.woff2') format('woff2');
  font-weight: normal;
  font-style: normal;
  font-display: swap;
}

@font-face {
  font-family: 'ET Book';
  src: url('/fonts/et-book-semi-bold-old-style-figures.woff2') format('woff2');
  font-weight: 600;
  font-style: normal;
  font-display: swap;
}

@font-face {
  font-family: 'ET Book';
  src: url('/fonts/et-book-bold-line-figures.woff2') format('woff2');
  font-weight: bold;
  font-style: normal;
  font-display: swap;
}

@font-face {
  font-family: 'ET Book';
  src: url('/fonts/et-book-display-italic-old-style-figures.woff2') format('woff2');
  font-weight: normal;
  font-style: italic;
  font-display: swap;
}
```

For quick prototyping, use jsDelivr CDN:
```html
<link rel="stylesheet" href="https://cdn.jsdelivr.net/gh/edwardtufte/et-book@gh-pages/et-book.css">
```

### Type Scale

ET Book is optimized for readability. Use slightly larger sizes than typical sans-serif UIs:

| Use | Size | Weight | Line Height | Notes |
|-----|------|--------|-------------|-------|
| Display heading | 1.75rem (28px) | bold | 1.2 | Hero text, page titles |
| Section heading | 1.25rem (20px) | bold | 1.3 | Panel headers |
| Card heading | 1rem (16px) | 600 | 1.4 | Card titles, nav items |
| Body | 0.9375rem (15px) | normal | 1.6 | Primary UI text |
| Small body | 0.875rem (14px) | normal | 1.5 | Secondary text |
| Caption / Helper | 0.8125rem (13px) | normal | 1.5 | Muted descriptions |
| Code | 0.8125rem (13px) | 400 | 1.5 | JetBrains Mono |

**Oldstyle vs Lining Figures:**
- Use **lining figures** (1234567890) for tabular data, timestamps
- Use **oldstyle figures** for prose, descriptions (blends better with lowercase)

---

## Visual Motifs

### The Sheep

Sheep are the heart of Hirsel's character. Use them purposefully:

| Symbol | When to Use |
|--------|-------------|
| `#sheep-sleeping` | Idle/empty states - "No runs yet" |
| `#sheep-walking` | Loading states - with bounce animation |
| `#sheep-alert` | Error states - eyes wide, ears up |
| `#sheep-flock` | Collection empty states |

**Guidelines:**
- Sheep add warmth but don't overuse - one per context
- Color with `text-wool-600` for subtle presence
- Scale appropriately (48-96px typical for empty states)

### Subtle Texture (Optional Enhancement)

For added depth, consider a very subtle noise/grain overlay on backgrounds:

```css
.bg-textured::after {
  content: '';
  position: absolute;
  inset: 0;
  background-image: url("data:image/svg+xml,%3Csvg viewBox='0 0 256 256' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='noise'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.8' numOctaves='4' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23noise)'/%3E%3C/svg%3E");
  opacity: 0.015;
  pointer-events: none;
  mix-blend-mode: overlay;
}
```

Use sparingly on large background areas for a "crafted paper" feel.

### Border Treatments

- **Default radius:** 6px (`rounded-md`) - slightly rounded, friendly
- **Cards:** 8px (`rounded-lg`) - inviting containers
- **Pills/badges:** 9999px (`rounded-full`) - for status indicators
- **Buttons:** 6px - consistent with inputs

### Shadows

Use sparingly. When needed:

```css
--shadow-sm: 0 1px 2px rgba(0,0,0,0.2);
--shadow-md: 0 4px 12px rgba(0,0,0,0.25);
--shadow-lg: 0 8px 24px rgba(0,0,0,0.3);
```

Dark themes need deeper shadows for depth perception.

---

## Motion Language

### Principles

1. **Natural, not mechanical** - Ease-out curves feel organic
2. **Quick, not sluggish** - 150-300ms for most transitions
3. **Purposeful, not decorative** - Motion should communicate

### Timing

```css
--transition-fast:   150ms ease-out;  /* Hover states, micro-interactions */
--transition-normal: 200ms ease-out;  /* State changes, reveals */
--transition-slow:   300ms ease-out;  /* Panel slides, modals */
```

### Animation Patterns

**Enter (slide + fade):**
```css
@keyframes enter-from-bottom {
  from { opacity: 0; transform: translateY(8px); }
  to   { opacity: 1; transform: translateY(0); }
}
```

**Stagger (lists):**
- Each item delayed 50ms from previous
- Max 8 items staggered, then instant

**Pulse (activity):**
```css
@keyframes pulse-glow {
  0%, 100% { opacity: 1; box-shadow: 0 0 8px var(--amber-500); }
  50%      { opacity: 0.7; box-shadow: 0 0 12px var(--amber-500); }
}
```
Use for "working" status dots.

**Sheep bounce (loading):**
```css
@keyframes sheep-bounce {
  0%, 100% { transform: translateY(0); }
  50%      { transform: translateY(-4px); }
}
```

### Reduced Motion

Always respect `prefers-reduced-motion`:
```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 0.01ms !important;
    transition-duration: 0.01ms !important;
  }
}
```

---

## Component Patterns

### Status Indicators

**Dot indicators:**
```
.status-dot (base: 8px circle)
  .status-working  → amber-500 + pulse glow
  .status-waiting  → golden
  .status-done     → sage
  .status-idle     → wool-700
  .status-error    → terra + subtle pulse
  .status-draft    → sky-500
```

**Badges:**
```
.badge (default: muted background)
.badge-success → sage background
.badge-warning → golden background
.badge-error   → terra background
```

### Cards

Cards follow the **card** pattern from Basecoat:
- Background: `pasture-800`
- Border: `pasture-600` (optional, 1px)
- Radius: `8px`
- Padding: `16px`
- Hover: `translateY(-2px)` + shadow (if interactive)

### Buttons

| Variant | Use Case |
|---------|----------|
| `.btn` (primary) | Main actions - amber background |
| `.btn-outline` | Secondary actions |
| `.btn-ghost` | Tertiary, toolbar buttons |
| `.btn-success` | Positive confirmations |
| `.btn-warning` | Caution actions |
| `.btn-destructive` | Dangerous/delete actions |

### Empty States

1. Center the content vertically and horizontally
2. Include a sheep SVG (sleeping or flock)
3. Brief message in `wool-400` text
4. Optional action button below

```tsx
<div class="flex flex-col items-center justify-center gap-4 py-12">
  <svg class="w-24 h-12 text-wool-600">
    <use href="#sheep-flock" />
  </svg>
  <p class="text-wool-400 text-sm">No runs yet</p>
  <p class="text-wool-500 text-xs">
    Use <code class="text-amber-500">hirsel go</code> to start
  </p>
</div>
```

---

## Voice & Tone

### Naming Conventions

Continue the pastoral theme in naming:
- **Hirsel** - the flock itself (the app)
- **Runs** - like herding runs across the pasture
- **Workers** - the sheepdogs doing the work
- **Tasks** - individual sheep to tend
- **Gyp** - traditional Scottish sheepdog name (AI assistant)

### Microcopy

- **Friendly but professional** - "No runs yet" not "You have no runs!"
- **Concise** - Every word earns its place
- **Helpful** - Guide users, don't just describe

**Examples:**
- Loading: "Gathering the flock..." (only if whimsy is appropriate)
- Error: "Something went wrong" (be clear, not cute about errors)
- Empty: "No workers active" with hint for what to do

---

## Implementation Checklist

### Fonts
- [ ] Download ET Book from [GitHub](https://github.com/edwardtufte/et-book)
- [ ] Add font files to `src/fonts/`
- [ ] Add `@font-face` declarations to `main.css`
- [ ] Update `--font-primary` CSS variable (replace system-ui stack)
- [ ] Increase base body font size to 15px
- [ ] Test oldstyle figures in numeric displays

### CSS Updates
- [ ] Document all color tokens in `main.css`
- [ ] Add Tailwind utilities for theme colors (`text-wool-*`, `bg-pasture-*`)
- [ ] Create `.font-display` utility class
- [ ] Add noise texture class (optional)

### Components
- [ ] Audit all components for consistent use of theme tokens
- [ ] Ensure sheep SVGs are used in appropriate empty states
- [ ] Verify motion respects reduced-motion preference

---

## Quick Reference

### Color Cheatsheet

| Need | Use |
|------|-----|
| Main background | `bg-pasture-900` |
| Card/panel background | `bg-pasture-800` |
| Hover background | `bg-pasture-700` |
| Border | `border-pasture-600` |
| Primary text | `text-wool-100` |
| Secondary text | `text-wool-300` |
| Muted text | `text-wool-500` |
| Accent/highlight | `text-amber-500` |
| Success | `text-sage` |
| Warning | `text-golden` |
| Error | `text-terra` |

### Font Cheatsheet

| Need | Use |
|------|-----|
| All UI text | `font-primary` (ET Book - default) |
| Headings | `font-primary` + `font-weight: bold` |
| Emphasis | `font-primary` + `font-weight: 600` |
| Code/terminal | `font-mono` (JetBrains Mono) |

---

## Inspiration Sources

- Scottish Highland landscapes at different times of day
- Traditional wool textures and tartan weaving patterns
- Craft tool interfaces (woodworking, pottery software)
- Warm, inviting productivity apps
- The quiet confidence of well-worn tools

---

*"A hirsel is more than a flock - it's the shepherd's entire domain, the land and the sheep together. This app is your domain for herding AI agents."*
