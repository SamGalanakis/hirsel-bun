import { type Component, createEffect, createSignal, on, Show } from "solid-js";
import { cn } from "@/lib/cn";
import Button from "@/components/ui/button";
import Input from "@/components/ui/input";
import Label from "@/components/ui/label";

interface Settings {
  llm_provider: "codex" | "openrouter";
  openrouter_api_key_masked: string | null;
  openrouter_base_url: string | null;
  tavily_api_key_masked: string | null;
}

async function fetchSettings(): Promise<Settings> {
  const res = await fetch("/api/settings", { credentials: "same-origin" });
  if (!res.ok) throw new Error("Failed to load settings");
  return res.json();
}

async function saveSection(path: string, body: Record<string, unknown>): Promise<void> {
  const res = await fetch(`/api/settings/${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.message || `Save failed (${res.status})`);
  }
}

const SettingsPage: Component = () => {
  const [settings, setSettings] = createSignal<Settings | null>(null);
  const [error, setError] = createSignal("");

  // LLM section
  const [llmProvider, setLlmProvider] = createSignal<"codex" | "openrouter">("codex");
  const [llmSaving, setLlmSaving] = createSignal(false);
  const [llmStatus, setLlmStatus] = createSignal("");

  // OpenRouter section
  const [orApiKey, setOrApiKey] = createSignal("");
  const [orBaseUrl, setOrBaseUrl] = createSignal("");
  const [orSaving, setOrSaving] = createSignal(false);
  const [orStatus, setOrStatus] = createSignal("");

  // Tavily section
  const [tavilyKey, setTavilyKey] = createSignal("");
  const [tavilySaving, setTavilySaving] = createSignal(false);
  const [tavilyStatus, setTavilyStatus] = createSignal("");

  createEffect(() => {
    fetchSettings()
      .then((s) => {
        setSettings(s);
        setLlmProvider(s.llm_provider);
        setOrBaseUrl(s.openrouter_base_url ?? "");
      })
      .catch((err) => setError(err instanceof Error ? err.message : "Failed to load settings"));
  });

  const handleSaveLlm = async () => {
    setLlmSaving(true);
    setLlmStatus("");
    try {
      await saveSection("llm", { provider: llmProvider() });
      setLlmStatus("Saved");
      setTimeout(() => setLlmStatus(""), 2000);
    } catch (err) {
      setLlmStatus(err instanceof Error ? err.message : "Save failed");
    } finally {
      setLlmSaving(false);
    }
  };

  const handleSaveOpenRouter = async () => {
    setOrSaving(true);
    setOrStatus("");
    try {
      const body: Record<string, string> = {};
      if (orApiKey().trim()) body.api_key = orApiKey().trim();
      if (orBaseUrl().trim()) body.base_url = orBaseUrl().trim();
      await saveSection("openrouter", body);
      setOrApiKey("");
      setOrStatus("Saved");
      setTimeout(() => setOrStatus(""), 2000);
      // Refresh to update masked value
      const s = await fetchSettings();
      setSettings(s);
    } catch (err) {
      setOrStatus(err instanceof Error ? err.message : "Save failed");
    } finally {
      setOrSaving(false);
    }
  };

  const handleSaveTavily = async () => {
    setTavilySaving(true);
    setTavilyStatus("");
    try {
      await saveSection("tavily", { api_key: tavilyKey().trim() });
      setTavilyKey("");
      setTavilyStatus("Saved");
      setTimeout(() => setTavilyStatus(""), 2000);
      const s = await fetchSettings();
      setSettings(s);
    } catch (err) {
      setTavilyStatus(err instanceof Error ? err.message : "Save failed");
    } finally {
      setTavilySaving(false);
    }
  };

  return (
    <div class="flex items-center justify-center min-h-screen bg-background">
      <div class="w-full max-w-md px-6 py-12 space-y-8">
        <div class="flex items-center justify-between">
          <h1 class="font-display text-2xl text-foreground">Settings</h1>
          <a
            href="#"
            class="text-xs text-muted-foreground hover:text-foreground transition-colors"
          >
            Back
          </a>
        </div>

        <Show when={error()}>
          <p class="text-xs text-signal-red">{error()}</p>
        </Show>

        <Show when={settings()}>
          {/* LLM Provider */}
          <div class="space-y-3 border border-border p-4">
            <Label>LLM Provider</Label>
            <div class="flex gap-2">
              <Button
                variant={llmProvider() === "codex" ? "primary" : "default"}
                size="sm"
                onClick={() => setLlmProvider("codex")}
              >
                Codex
              </Button>
              <Button
                variant={llmProvider() === "openrouter" ? "primary" : "default"}
                size="sm"
                onClick={() => setLlmProvider("openrouter")}
              >
                OpenRouter
              </Button>
            </div>
            <div class="flex items-center gap-2">
              <Button
                size="sm"
                loading={llmSaving()}
                onClick={handleSaveLlm}
              >
                Save
              </Button>
              <Show when={llmStatus()}>
                <span class="text-xs text-muted-foreground">{llmStatus()}</span>
              </Show>
            </div>
          </div>

          {/* OpenRouter */}
          <Show when={llmProvider() === "openrouter"}>
            <div class="space-y-3 border border-border p-4">
              <Label>OpenRouter</Label>

              <div class="space-y-1.5">
                <Label for="or-key">API Key</Label>
                <Show when={settings()!.openrouter_api_key_masked}>
                  <p class="text-xs text-muted-foreground font-mono">
                    Current: {settings()!.openrouter_api_key_masked}
                  </p>
                </Show>
                <Input
                  id="or-key"
                  type="password"
                  placeholder="sk-or-..."
                  value={orApiKey()}
                  onInput={(e) => setOrApiKey(e.currentTarget.value)}
                />
              </div>

              <div class="space-y-1.5">
                <Label for="or-url">Base URL (optional)</Label>
                <Input
                  id="or-url"
                  type="text"
                  placeholder="https://openrouter.ai/api/v1"
                  value={orBaseUrl()}
                  onInput={(e) => setOrBaseUrl(e.currentTarget.value)}
                />
              </div>

              <div class="flex items-center gap-2">
                <Button
                  size="sm"
                  loading={orSaving()}
                  onClick={handleSaveOpenRouter}
                >
                  Save
                </Button>
                <Show when={orStatus()}>
                  <span class="text-xs text-muted-foreground">{orStatus()}</span>
                </Show>
              </div>
            </div>
          </Show>

          {/* Tavily */}
          <div class="space-y-3 border border-border p-4">
            <Label>Tavily Search</Label>

            <div class="space-y-1.5">
              <Label for="tavily-key">API Key</Label>
              <Show when={settings()!.tavily_api_key_masked}>
                <p class="text-xs text-muted-foreground font-mono">
                  Current: {settings()!.tavily_api_key_masked}
                </p>
              </Show>
              <Input
                id="tavily-key"
                type="password"
                placeholder="tvly-..."
                value={tavilyKey()}
                onInput={(e) => setTavilyKey(e.currentTarget.value)}
              />
            </div>

            <div class="flex items-center gap-2">
              <Button
                size="sm"
                loading={tavilySaving()}
                onClick={handleSaveTavily}
                disabled={!tavilyKey().trim()}
              >
                Save
              </Button>
              <Show when={tavilyStatus()}>
                <span class="text-xs text-muted-foreground">{tavilyStatus()}</span>
              </Show>
            </div>
          </div>
        </Show>
      </div>
    </div>
  );
};

export default SettingsPage;
