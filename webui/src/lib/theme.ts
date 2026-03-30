import { createSignal, onCleanup } from "solid-js";

export type ThemeName = "hirsel" | "hirsel-dark" | "midnight" | "bone";

export interface ThemeOption {
  name: ThemeName;
  label: string;
  mode: "light" | "dark";
}

export const themes: ThemeOption[] = [
  { name: "hirsel", label: "Hirsel", mode: "light" },
  { name: "hirsel-dark", label: "Hirsel Dark", mode: "dark" },
  { name: "midnight", label: "Midnight", mode: "dark" },
  { name: "bone", label: "Bone", mode: "light" },
];

const STORAGE_KEY = "hirsel-theme";

const modeMap: Record<ThemeName, "light" | "dark"> = {
  hirsel: "light",
  "hirsel-dark": "dark",
  midnight: "dark",
  bone: "light",
};

function isValidTheme(value: string | null): value is ThemeName {
  return value != null && value in modeMap;
}

function resolveInitial(): ThemeName {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (isValidTheme(stored)) return stored;
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "hirsel-dark"
    : "hirsel";
}

function applyTheme(name: ThemeName) {
  const mode = modeMap[name];
  document.documentElement.dataset.theme = name;
  document.documentElement.dataset.kbTheme = mode;
  document.documentElement.style.colorScheme = mode;
  if (mode === "dark") {
    document.documentElement.classList.add("dark");
  } else {
    document.documentElement.classList.remove("dark");
  }
}

export function useTheme() {
  const [theme, setThemeSignal] = createSignal<ThemeName>(resolveInitial());

  function setTheme(name: ThemeName) {
    setThemeSignal(name);
    localStorage.setItem(STORAGE_KEY, name);
    applyTheme(name);
  }

  // Listen for system preference changes
  const mql = window.matchMedia("(prefers-color-scheme: dark)");
  const handler = () => {
    if (!localStorage.getItem(STORAGE_KEY)) {
      const auto = mql.matches ? "hirsel-dark" : "hirsel";
      setThemeSignal(auto);
      applyTheme(auto);
    }
  };
  mql.addEventListener("change", handler);
  onCleanup(() => mql.removeEventListener("change", handler));

  // Apply on mount
  applyTheme(theme());

  return { theme, setTheme };
}
