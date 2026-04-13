import { createSignal, onCleanup } from "solid-js";

export type ThemeName = "forge" | "eclipse" | "graphite" | "miasma" | "void" | "cobalt" | "rosepine" | "parchment" | "bone";

export interface ThemeOption {
  name: ThemeName;
  label: string;
  mode: "light" | "dark";
  description: string;
}

export const themes: ThemeOption[] = [
  { name: "forge", label: "Forge", mode: "dark", description: "Warm dark — amber on walnut" },
  { name: "eclipse", label: "Eclipse", mode: "dark", description: "Very dark — black with burnished brass" },
  { name: "graphite", label: "Graphite", mode: "dark", description: "Cool dark — steel and ice" },
  { name: "miasma", label: "Miasma", mode: "dark", description: "Olive dark — moss and fog" },
  { name: "void", label: "Void", mode: "dark", description: "Pure black — white text, electric blue" },
  { name: "cobalt", label: "Cobalt", mode: "dark", description: "Deep blue — cyan and coral accents" },
  { name: "rosepine", label: "Rose Pine", mode: "dark", description: "Muted purple — rose and gold" },
  { name: "parchment", label: "Parchment", mode: "light", description: "Warm light — cream and walnut" },
  { name: "bone", label: "Bone", mode: "light", description: "Cool light — paper and slate" },
];

const STORAGE_KEY = "hirsel-theme";

const modeMap: Record<ThemeName, "light" | "dark"> = {
  forge: "dark",
  eclipse: "dark",
  graphite: "dark",
  miasma: "dark",
  void: "dark",
  cobalt: "dark",
  rosepine: "dark",
  parchment: "light",
  bone: "light",
};

function isValidTheme(value: string | null): value is ThemeName {
  return value != null && value in modeMap;
}

function resolveInitial(): ThemeName {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (isValidTheme(stored)) return stored;
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "forge"
    : "parchment";
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

  const mql = window.matchMedia("(prefers-color-scheme: dark)");
  const handler = () => {
    if (!localStorage.getItem(STORAGE_KEY)) {
      const auto = mql.matches ? "forge" : "parchment";
      setThemeSignal(auto);
      applyTheme(auto);
    }
  };
  mql.addEventListener("change", handler);
  onCleanup(() => mql.removeEventListener("change", handler));

  applyTheme(theme());

  return { theme, setTheme };
}
