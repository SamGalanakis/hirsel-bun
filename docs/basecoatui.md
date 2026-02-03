# Basecoat UI Component Reference

> CSS utility classes for shadcn/ui-style components.
> https://basecoatui.com/components/
>
> Used with SolidJS in `src/components/`. Examples below show HTML patterns; adapt for JSX.

## Quick Reference

| Component | Class | Notes |
|-----------|-------|-------|
| Button | `btn`, `btn-outline`, `btn-ghost`, `btn-destructive` | Sizes: `btn-sm`, `btn-lg` |
| Card | `card` | Contains `<header>`, `<section>`, `<footer>` |
| Dialog | `dialog` | Uses native `<dialog>` element |
| Badge | `badge`, `badge-secondary`, `badge-outline` | |
| Table | `table` | Wrap in `overflow-x-auto` for responsive |
| Tabs | `tabs` | Uses ARIA roles |
| Toast | JS event | `window.toast.success(msg)` |
| Tooltip | `data-tooltip` | Positions: `data-side`, `data-align` |

---

## Buttons

```html
<!-- Variants -->
<button class="btn">Primary</button>
<button class="btn-secondary">Secondary</button>
<button class="btn-outline">Outline</button>
<button class="btn-ghost">Ghost</button>
<button class="btn-destructive">Destructive</button>
<button class="btn-link">Link</button>

<!-- Sizes -->
<button class="btn-sm">Small</button>
<button class="btn-lg">Large</button>

<!-- Icon button -->
<button class="btn-icon-outline">
  <i data-lucide="plus" class="w-4 h-4"></i>
</button>

<!-- With icon -->
<button class="btn">
  <i data-lucide="plus" class="w-4 h-4"></i>
  Add Item
</button>

<!-- Loading state -->
<button class="btn" disabled>
  <svg class="animate-spin w-4 h-4" ...></svg>
  Loading...
</button>
```

---

## Forms

The `form` class auto-styles all child inputs.

### Field Pattern
```html
<form class="form grid gap-6">
  <div class="grid gap-2">
    <label for="name">Name</label>
    <input id="name" type="text" placeholder="Enter name" />
    <p class="text-muted-foreground text-sm">Helper text</p>
  </div>
</form>
```

### Input States
```html
<!-- Invalid -->
<input type="text" aria-invalid="true" />

<!-- Disabled -->
<input type="text" disabled />
```

### Select (Native)
```html
<select>
  <option value="">Select...</option>
  <option value="a">Option A</option>
</select>
```

### Textarea
```html
<textarea rows="4" placeholder="Enter description"></textarea>
```

### Checkbox
```html
<label class="flex items-center gap-2">
  <input type="checkbox" />
  Accept terms
</label>
```

### Radio Group
```html
<fieldset class="grid gap-2">
  <legend>Choose one</legend>
  <label class="flex items-center gap-2">
    <input type="radio" name="choice" value="a" checked />
    Option A
  </label>
  <label class="flex items-center gap-2">
    <input type="radio" name="choice" value="b" />
    Option B
  </label>
</fieldset>
```

---

## Switch Toggle

```html
<!-- Basic -->
<input type="checkbox" role="switch" />

<!-- With label -->
<label class="flex items-center gap-2">
  <input type="checkbox" role="switch" />
  Enable feature
</label>

<!-- Bordered card (preferred for settings) -->
<div class="flex items-start justify-between rounded-lg border p-4">
  <div class="flex flex-col gap-0.5">
    <label for="switch-id" class="leading-normal">Feature Name</label>
    <p class="text-muted-foreground text-sm">Description of the feature.</p>
  </div>
  <input id="switch-id" type="checkbox" role="switch" />
</div>
```

---

## Custom Dropdown (Styled Select)

Native `<select>` can't be fully styled. Use `dropdown` class with ARIA attributes:

```tsx
const [open, setOpen] = createSignal(false);
const [value, setValue] = createSignal('');

<div class="dropdown">
  <button type="button" class="btn-outline w-full"
          onClick={() => setOpen(!open())}
          aria-haspopup="listbox" aria-expanded={open()}>
    <span class="truncate">{value() || 'Select...'}</span>
    <i data-lucide="chevrons-up-down" class="w-4 h-4 opacity-50 shrink-0" />
  </button>
  <Show when={open()}>
    <div data-popover role="listbox">
      <div role="option" onClick={() => { setValue('Option A'); setOpen(false); }}
           aria-selected={value() === 'Option A'}>Option A</div>
    </div>
  </Show>
</div>
```

**Note:** Use `class="dropdown"` NOT `class="select"`. Basecoat adds checkmarks via CSS based on `aria-selected`.

### MultiSelect Dropdown

For multiple selections, use checkboxes and `aria-multiselectable`:

```tsx
import { MultiSelectDropdown } from '../shared';

<MultiSelectDropdown
  value={selectedFilters()}
  options={[
    { value: 'spec-tasks', label: 'Spec Tasks' },
    { value: 'worker-tasks', label: 'Worker Tasks' },
    { value: 'deleted-nodes', label: 'Deleted Nodes' },
  ]}
  onChange={setSelectedFilters}
  placeholder="Filter..."
  class="w-36 text-xs"
/>
```

The component displays:
- "All" when all options selected
- First selected label when only one selected
- First label + count (e.g., "Spec Tasks +1") for multiple selections

---

## Card

```html
<div class="card">
  <header>
    <h2>Card Title</h2>
    <p>Optional description</p>
  </header>
  <section>
    <!-- Main content -->
  </section>
  <footer>
    <button class="btn-outline">Cancel</button>
    <button class="btn">Save</button>
  </footer>
</div>
```

---

## Dialog (Modal)

Uses native `<dialog>` element:

```html
<button onclick="document.getElementById('my-dialog').showModal()">Open</button>

<dialog id="my-dialog" class="dialog"
        aria-labelledby="dialog-title"
        onclick="if (event.target === this) this.close()">
  <div>
    <header>
      <h2 id="dialog-title">Dialog Title</h2>
      <p>Description text</p>
    </header>
    <section>
      <!-- Content -->
    </section>
    <footer>
      <button class="btn-outline" onclick="this.closest('dialog').close()">Cancel</button>
      <button class="btn" onclick="this.closest('dialog').close()">Confirm</button>
    </footer>
    <!-- Optional close button -->
    <button aria-label="Close" onclick="this.closest('dialog').close()" class="absolute top-4 right-4">
      <i data-lucide="x" class="w-4 h-4"></i>
    </button>
  </div>
</dialog>
```

### Alert Dialog (No backdrop dismiss)
Same structure but remove the `onclick` from `<dialog>` and omit the close button.

---

## Tabs

```html
<div class="tabs">
  <nav role="tablist" aria-orientation="horizontal">
    <button role="tab" aria-controls="panel-1" aria-selected="true" tabindex="0">Tab 1</button>
    <button role="tab" aria-controls="panel-2" aria-selected="false" tabindex="0">Tab 2</button>
  </nav>
  <div id="panel-1" role="tabpanel" aria-labelledby="tab-1">
    Content 1
  </div>
  <div id="panel-2" role="tabpanel" aria-labelledby="tab-2" hidden>
    Content 2
  </div>
</div>
```

---

## Table

```html
<div class="overflow-x-auto">
  <table class="table">
    <thead>
      <tr>
        <th>Name</th>
        <th class="text-right">Amount</th>
      </tr>
    </thead>
    <tbody>
      <tr>
        <td class="font-medium">Item</td>
        <td class="text-right">$100</td>
      </tr>
    </tbody>
  </table>
</div>
```

---

## Badge

```html
<span class="badge">Default</span>
<span class="badge-secondary">Secondary</span>
<span class="badge-destructive">Error</span>
<span class="badge-outline">Outline</span>

<!-- With icon -->
<span class="badge">
  <i data-lucide="check" class="w-3 h-3"></i>
  Active
</span>

<!-- Numeric (pill) -->
<span class="badge rounded-full h-5 min-w-5 px-1 font-mono tabular-nums">8</span>
```

---

## Tooltip

```html
<button data-tooltip="Tooltip text">Hover me</button>

<!-- Positioning -->
<button data-tooltip="Top" data-side="top">Top</button>
<button data-tooltip="Bottom" data-side="bottom">Bottom</button>
<button data-tooltip="Left" data-side="left">Left</button>
<button data-tooltip="Right" data-side="right">Right</button>

<!-- Alignment -->
<button data-tooltip="Aligned start" data-side="bottom" data-align="start">Start</button>
```

---

## Toast

```javascript
// Using helper (if defined)
window.toast.success('Saved successfully');
window.toast.error('Something went wrong');

// Using event dispatch
window.dispatchEvent(new CustomEvent('basecoat:toast', {
  detail: {
    category: 'success',  // success, info, warning, error
    title: 'Success',
    description: 'Item saved',
    duration: 3000
  }
}));
```

---

## Loading States

### Spinner
```html
<svg class="animate-spin w-4 h-4" viewBox="0 0 24 24" fill="none"
     stroke="currentColor" stroke-width="2" role="status" aria-label="Loading">
  <path d="M21 12a9 9 0 1 1-6.219-8.56" />
</svg>
```

### Skeleton
```html
<!-- Text skeleton -->
<div class="bg-accent animate-pulse rounded-md h-4 w-[200px]"></div>

<!-- Avatar + text -->
<div class="flex items-center gap-4">
  <div class="bg-accent animate-pulse size-10 rounded-full"></div>
  <div class="grid gap-2">
    <div class="bg-accent animate-pulse rounded-md h-4 w-[150px]"></div>
    <div class="bg-accent animate-pulse rounded-md h-4 w-[100px]"></div>
  </div>
</div>
```

### Progress Bar
```html
<div class="bg-primary/20 relative h-2 w-full overflow-hidden rounded-full">
  <div class="bg-primary h-full transition-all" style="width: 66%"></div>
</div>
```

---

## Accordion

Uses native `<details>` element:

```html
<div class="accordion">
  <details>
    <summary>
      <span>Section 1</span>
      <i data-lucide="chevron-down" class="w-4 h-4 transition-transform"></i>
    </summary>
    <div>Content for section 1</div>
  </details>
  <details>
    <summary>
      <span>Section 2</span>
      <i data-lucide="chevron-down" class="w-4 h-4 transition-transform"></i>
    </summary>
    <div>Content for section 2</div>
  </details>
</div>
```

---

## Popover

```html
<button id="trigger" aria-expanded="false" aria-controls="popover" class="btn">
  Open Popover
</button>

<div id="popover" data-popover aria-hidden="true">
  <p>Popover content</p>
</div>
```

---

## Best Practices

### 1. Use semantic HTML
- `<dialog>` for modals
- `<details>` for accordions
- `role="switch"` for toggles
- Proper ARIA attributes

### 2. Form structure
- Wrap forms in `<form class="form">`
- Use `grid gap-6` for field spacing
- Each field: `<div class="grid gap-2">` with label → input → helper

### 3. Accessibility
- Link labels to inputs via `for`/`id`
- Use `aria-invalid="true"` for errors
- Include `aria-label` on icon buttons
- Add `role="status"` to spinners

### 4. Icons (Lucide)
- Use the `<Icon name="icon-name" />` component from `src/components/shared`
- Icons render as inline SVGs via SolidJS - no DOM mutation needed
- Standard sizes: `w-4 h-4` (16px), `w-5 h-5` (20px)
- Example: `<Icon name="check" class="w-4 h-4 text-sage" />`

### 5. Colors (Hirsel theme)
- Primary actions: `btn` (amber)
- Secondary: `btn-outline`, `btn-ghost`
- Destructive: `btn-destructive`, `text-terra`
- Success: `text-sage`
- Muted text: `text-muted-foreground`, `text-wool-500`

### 6. Spacing
- Card padding: `p-4`
- Form gaps: `gap-6` between fields, `gap-2` within field
- Button gaps: `gap-2`
