import { createSignal } from "solid-js";

export interface KeyBinding {
  key: string; // e.g. "ctrl+`", "ctrl+b", "escape"
  label: string; // human-readable label
}

export interface KeyAction {
  id: string;
  label: string;
  defaultKey: string;
  category: string;
}

const ACTIONS: KeyAction[] = [
  { id: "toggle-terminal", label: "Toggle terminal", defaultKey: "ctrl+`", category: "General" },
  { id: "toggle-sidebar", label: "Toggle sidebar", defaultKey: "ctrl+b", category: "General" },
  { id: "stop-generation", label: "Stop generation", defaultKey: "escape", category: "Chat" },
  { id: "close-panel", label: "Close panel / settings", defaultKey: "escape", category: "General" },
];

const STORAGE_KEY = "hirsel_keybindings";

function loadCustomBindings(): Record<string, string> {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    return stored ? JSON.parse(stored) : {};
  } catch {
    return {};
  }
}

function saveCustomBindings(bindings: Record<string, string>) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(bindings));
}

const [customBindings, setCustomBindings] = createSignal(loadCustomBindings());

export function getActions(): KeyAction[] {
  return ACTIONS;
}

export function getBinding(actionId: string): string {
  const custom = customBindings()[actionId];
  if (custom) return custom;
  const action = ACTIONS.find((a) => a.id === actionId);
  return action?.defaultKey ?? "";
}

export function setBinding(actionId: string, key: string) {
  const action = ACTIONS.find((a) => a.id === actionId);
  const isDefault = action && key === action.defaultKey;
  const next = { ...customBindings() };
  if (isDefault) {
    delete next[actionId];
  } else {
    next[actionId] = key;
  }
  setCustomBindings(next);
  saveCustomBindings(next);
}

export function resetBindings() {
  setCustomBindings({});
  localStorage.removeItem(STORAGE_KEY);
}

/** Normalize a KeyboardEvent into our binding format */
export function eventToBinding(event: KeyboardEvent): string {
  const parts: string[] = [];
  if (event.ctrlKey || event.metaKey) parts.push("ctrl");
  if (event.altKey) parts.push("alt");
  if (event.shiftKey) parts.push("shift");

  let key = event.key.toLowerCase();
  if (key === " ") key = "space";
  if (key === "backquote" || key === "`") key = "`";
  if (key === "control" || key === "meta" || key === "alt" || key === "shift") return "";

  parts.push(key);
  return parts.join("+");
}

/** Check if a keyboard event matches an action's binding */
export function matchesAction(event: KeyboardEvent, actionId: string): boolean {
  const binding = getBinding(actionId);
  if (!binding) return false;
  const pressed = eventToBinding(event);
  return pressed === binding;
}
