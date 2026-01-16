# Hirsel Development Guidelines

## UI Components

Use [Basecoat UI](https://basecoatui.com/components/) components wherever one is available. This includes:

- **Buttons**: Use `.btn`, `.btn-ghost`, `.btn-destructive` classes
- **Inputs**: Use `.input` class for text inputs, number inputs
- **Select**: Use `.select` class for native select dropdowns
- **Slider**: Use `<input type="range" class="input">` with proper JS initialization for `--slider-value`
- **Toast**: Use basecoat toaster via `basecoat:toast` events
- **Tooltips**: Use `data-tooltip` and `data-side` attributes
- **Modals/Dialogs**: Follow basecoat dialog patterns
- **Forms**: Use `.label`, `.form` classes

Check https://basecoatui.com/components/ for the full list and proper markup.

## Icons

Use [Lucide Icons](https://lucide.dev/icons/) for all iconography. Search for icons at `https://lucide.dev/icons/?search=<term>`.

Icons are rendered using the `data-lucide` attribute:
```html
<i data-lucide="file-text" class="w-4 h-4"></i>
<i data-lucide="upload" class="w-5 h-5 text-amber-500"></i>
```

Lucide is initialized globally and icons are auto-rendered. After dynamically adding icons, call `lucide.createIcons()` to render them.

## Alpine.js

The app uses Alpine.js for reactivity. Components are defined in `src/lib/components/` and registered globally in `main.ts`.

## Styling

- Tailwind CSS v4 with custom theme colors (pasture, wool, sage, terra, golden, amber)
- Custom styles in `src/styles/main.css`
- Basecoat component styles in `src/styles/output.css`

## Tauri

Backend is Rust with Tauri v2. Commands are invoked via `window.tauriInvoke()`.

## Development & Debugging

When running or testing the app, use `./dev.sh` which sets up the proper environment for debugging:

```bash
./dev.sh
```

This is especially important when spawning agents or running in development mode.
