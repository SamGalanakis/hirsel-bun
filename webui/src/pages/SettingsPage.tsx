import { type Component, Show, createSignal, onCleanup, onMount } from "solid-js";
import Badge from "@/components/ui/badge";
import Button from "@/components/ui/button";
import Input from "@/components/ui/input";
import Label from "@/components/ui/label";
import Select, { type SelectOption } from "@/components/ui/select";
import Tabs, { TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  type CodexDeviceStartResponse,
  type SettingsResponse,
  getSettings,
  pollCodexDeviceFlow,
  saveGithubToken,
  saveOpenRouterSettings,
  saveSettingsProvider,
  saveTavilyKey,
  startCodexDeviceFlow,
} from "@/lib/api";
import { openUrl } from "@/lib/open-url";

const providerOptions: SelectOption[] = [
  { value: "codex", label: "Codex", description: "Built-in OAuth" },
  { value: "openrouter", label: "OpenRouter", description: "Custom endpoint / API key" },
];

type CodexAuthFlow = CodexDeviceStartResponse & { error: string | null };

const SettingsPage: Component = () => {
  const [settings, setSettings] = createSignal<SettingsResponse | null>(null);
  const [error, setError] = createSignal("");

  const [llmProvider, setLlmProvider] = createSignal<"codex" | "openrouter">("codex");
  const [llmSaving, setLlmSaving] = createSignal(false);
  const [llmReconnecting, setLlmReconnecting] = createSignal(false);
  const [llmStatus, setLlmStatus] = createSignal("");
  const [codexFlow, setCodexFlow] = createSignal<CodexAuthFlow | null>(null);
  const [codeCopied, setCodeCopied] = createSignal(false);

  const [orApiKey, setOrApiKey] = createSignal("");
  const [orBaseUrl, setOrBaseUrl] = createSignal("");

  const [tavilyKey, setTavilyKey] = createSignal("");
  const [tavilySaving, setTavilySaving] = createSignal(false);
  const [tavilyStatus, setTavilyStatus] = createSignal("");
  const [githubToken, setGithubToken] = createSignal("");
  const [githubSaving, setGithubSaving] = createSignal(false);
  const [githubStatus, setGithubStatus] = createSignal("");
  let codexPollTimer: number | undefined;

  const clearCodexPollTimer = () => {
    if (codexPollTimer !== undefined) {
      window.clearTimeout(codexPollTimer);
      codexPollTimer = undefined;
    }
  };

  const scheduleCodexPoll = (intervalSeconds: number) => {
    clearCodexPollTimer();
    codexPollTimer = window.setTimeout(() => {
      void pollCodexFlow();
    }, Math.max(intervalSeconds, 1) * 1000);
  };

  const reloadSettings = async () => {
    try {
      const next = await getSettings();
      setSettings(next);
      setLlmProvider(next.provider);
      setOrBaseUrl(next.openrouter_base_url ?? "");
      setError("");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load settings");
    }
  };

  onMount(() => { void reloadSettings(); });
  onCleanup(() => clearCodexPollTimer());

  const handleSaveProvider = async () => {
    setLlmSaving(true);
    setLlmStatus("");
    try {
      await saveSettingsProvider(llmProvider());
      if (llmProvider() === "openrouter") {
        const body: { api_key?: string; base_url?: string } = {};
        if (orApiKey().trim()) body.api_key = orApiKey().trim();
        body.base_url = orBaseUrl().trim();
        await saveOpenRouterSettings(body);
        setOrApiKey("");
      }
      setLlmStatus("Saved");
      setTimeout(() => setLlmStatus(""), 2000);
      await reloadSettings();
    } catch (err) {
      setLlmStatus(err instanceof Error ? err.message : "Save failed");
    } finally {
      setLlmSaving(false);
    }
  };

  const pollCodexFlow = async () => {
    const flow = codexFlow();
    if (!flow) return;
    try {
      const result = await pollCodexDeviceFlow({
        deviceAuthId: flow.deviceAuthId,
        userCode: flow.userCode,
      });
      if (result.status === "connected") {
        clearCodexPollTimer();
        setCodexFlow(null);
        setLlmStatus("Connected");
        setTimeout(() => setLlmStatus(""), 2000);
        await reloadSettings();
        return;
      }
      setCodexFlow({ ...flow, error: null });
      scheduleCodexPoll(flow.interval);
    } catch (err) {
      setCodexFlow({
        ...flow,
        error: err instanceof Error ? err.message : "Polling failed",
      });
    }
  };

  const handleConnectCodex = async () => {
    setLlmReconnecting(true);
    setLlmStatus("");
    clearCodexPollTimer();
    try {
      const flow = await startCodexDeviceFlow();
      setCodexFlow({ ...flow, error: null });
      scheduleCodexPoll(flow.interval);
    } catch (err) {
      setLlmStatus(err instanceof Error ? err.message : "Connect failed");
    } finally {
      setLlmReconnecting(false);
    }
  };

  const handleSaveTavily = async () => {
    setTavilySaving(true);
    setTavilyStatus("");
    try {
      await saveTavilyKey(tavilyKey().trim());
      setTavilyKey("");
      setTavilyStatus("Saved");
      setTimeout(() => setTavilyStatus(""), 2000);
      await reloadSettings();
    } catch (err) {
      setTavilyStatus(err instanceof Error ? err.message : "Save failed");
    } finally {
      setTavilySaving(false);
    }
  };

  const handleSaveGithub = async () => {
    setGithubSaving(true);
    setGithubStatus("");
    try {
      await saveGithubToken(githubToken().trim());
      setGithubToken("");
      setGithubStatus("Saved");
      setTimeout(() => setGithubStatus(""), 2000);
      await reloadSettings();
    } catch (err) {
      setGithubStatus(err instanceof Error ? err.message : "Save failed");
    } finally {
      setGithubSaving(false);
    }
  };

  // Derived state
  const codexReady = () => settings()?.codex_configured && settings()?.provider === "codex";
  const codexFromEnv = () => settings()?.codex_source === "env";
  const showConnectBtn = () => llmProvider() === "codex" && !codexFromEnv();

  return (
    <div class="min-h-screen bg-background text-foreground">
      <header class="flex h-[54px] shrink-0 items-center justify-between border-b border-border bg-card/95 px-4 backdrop-blur">
        <div class="flex items-center gap-2">
          <a href="#" class="font-display text-base font-semibold tracking-tight text-foreground">HIRSEL</a>
          <span class="text-xs text-muted-foreground">/</span>
          <span class="text-sm font-medium">Settings</span>
        </div>
        <a href="#" class="text-xs text-muted-foreground transition-colors hover:text-foreground">
          Back
        </a>
      </header>

      <main class="mx-auto max-w-xl px-6 py-10">
        <Show when={error()}>
          <div class="mb-6 border border-signal-red/30 bg-signal-red/10 px-4 py-3 text-sm text-signal-red">
            {error()}
          </div>
        </Show>

        <Tabs defaultValue="provider">
          <TabsList>
            <TabsTrigger value="provider">Provider</TabsTrigger>
            <TabsTrigger value="tools">Tools</TabsTrigger>
          </TabsList>

          {/* ── Provider ── */}
          <TabsContent value="provider">
            <div class="space-y-5">
              {/* Provider selector row */}
              <div class="space-y-2">
                <div class="flex items-center gap-2">
                  <Label class="flex-1">LLM Provider</Label>
                  <Show when={codexReady()}>
                    <Badge variant="success">Connected</Badge>
                  </Show>
                  <Show when={codexFromEnv()}>
                    <Badge>Env</Badge>
                  </Show>
                </div>
                <div class="flex items-center gap-3">
                  <Select
                    options={providerOptions}
                    value={llmProvider()}
                    onChange={(v) => {
                      const provider = v as "codex" | "openrouter";
                      setLlmProvider(provider);
                      setCodexFlow(null);
                      clearCodexPollTimer();
                      // Auto-save provider choice immediately
                      void saveSettingsProvider(provider).then(() => reloadSettings());
                    }}
                    class="w-56"
                  />
                  <Show when={showConnectBtn()}>
                    <Button
                      size="sm"
                      variant={codexReady() ? "ghost" : "primary"}
                      loading={llmReconnecting()}
                      onClick={handleConnectCodex}
                    >
                      {codexReady() ? "Reconnect" : "Connect"}
                    </Button>
                  </Show>
                </div>
              </div>

              {/* Codex device flow */}
              <Show when={llmProvider() === "codex" && codexFlow()}>
                <div class="border border-border bg-muted/10 p-4 space-y-4">
                  <div class="flex items-center gap-2">
                    <span class="chassis-label">Device code</span>
                    <Badge variant="warning">Waiting</Badge>
                    <span class="ml-auto text-[10px] text-muted-foreground font-mono">
                      polling {codexFlow()!.interval}s
                    </span>
                  </div>
                  <button
                    type="button"
                    class="group/code flex w-full items-center justify-between border border-border bg-background px-4 py-3 text-left transition-colors hover:bg-muted/30"
                    onClick={() => {
                      void navigator.clipboard.writeText(codexFlow()!.userCode);
                      setCodeCopied(true);
                      setTimeout(() => setCodeCopied(false), 1500);
                    }}
                  >
                    <span class="font-mono text-2xl tracking-[0.22em] text-foreground select-all">
                      {codexFlow()!.userCode}
                    </span>
                    <span class="text-xs text-muted-foreground group-hover/code:text-foreground transition-colors">
                      {codeCopied() ? "Copied" : "Copy"}
                    </span>
                  </button>
                  <div class="flex items-center gap-3">
                    <Button
                      size="sm"
                      variant="primary"
                      onClick={() => void openUrl(codexFlow()!.verifyUrl)}
                    >
                      Open verification page
                    </Button>
                    <Button size="sm" variant="default" onClick={() => void pollCodexFlow()}>
                      Check now
                    </Button>
                  </div>
                  <Show when={codexFlow()!.error}>
                    <div class="border border-signal-red/30 bg-signal-red/10 px-3 py-2 text-xs text-signal-red">
                      {codexFlow()!.error}
                    </div>
                  </Show>
                </div>
              </Show>

              {/* OpenRouter fields */}
              <Show when={llmProvider() === "openrouter"}>
                <div class="border-l-[2px] border-border pl-5 space-y-4">
                  <div class="space-y-1.5">
                    <Label for="or-key">API Key</Label>
                    <Show when={settings()?.openrouter_key_masked}>
                      <p class="font-mono text-xs text-muted-foreground">
                        Current: {settings()!.openrouter_key_masked}
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
                    <Label for="or-url">Base URL</Label>
                    <Input
                      id="or-url"
                      type="text"
                      placeholder="https://openrouter.ai/api/v1"
                      value={orBaseUrl()}
                      onInput={(e) => setOrBaseUrl(e.currentTarget.value)}
                    />
                  </div>
                </div>
              </Show>

              {/* Save credentials (provider choice auto-saves) */}
              <Show when={llmProvider() === "openrouter"}>
                <div class="flex items-center gap-3">
                  <Button size="sm" loading={llmSaving()} onClick={handleSaveProvider}>
                    Save credentials
                  </Button>
                  <Show when={llmStatus()}>
                    <span class="text-xs text-muted-foreground">{llmStatus()}</span>
                  </Show>
                </div>
              </Show>
              <Show when={llmProvider() === "codex" && llmStatus()}>
                <span class="text-xs text-muted-foreground">{llmStatus()}</span>
              </Show>
            </div>
          </TabsContent>

          {/* ── Tools ── */}
          <TabsContent value="tools">
            <div class="space-y-5">
              <div class="flex items-center gap-2">
                <Label class="flex-1">GitHub Publishing</Label>
                <Show when={settings()?.github_configured}>
                  <Badge variant="success">Configured</Badge>
                </Show>
                <Show when={settings()?.github_source === "env"}>
                  <Badge>Env</Badge>
                </Show>
              </div>

              <div class="space-y-1.5">
                <Label for="github-token">Token</Label>
                <Show when={settings()?.github_token_masked}>
                  <p class="font-mono text-xs text-muted-foreground">
                    Current: {settings()!.github_token_masked}
                  </p>
                </Show>
                <Input
                  id="github-token"
                  type="password"
                  placeholder="github_pat_..."
                  value={githubToken()}
                  onInput={(e) => setGithubToken(e.currentTarget.value)}
                />
                <p class="text-xs text-muted-foreground">
                  Hirsel forwards this into shepherd containers as{" "}
                  <code class="mx-0.5 bg-muted px-1 py-px text-[11px]">GITHUB_TOKEN</code> and{" "}
                  <code class="mx-0.5 bg-muted px-1 py-px text-[11px]">GH_TOKEN</code>. Host
                  environment values still win if present.
                </p>
              </div>

              <div class="flex items-center gap-3">
                <Button
                  size="sm"
                  loading={githubSaving()}
                  onClick={handleSaveGithub}
                  disabled={!githubToken().trim()}
                >
                  Save
                </Button>
                <Show when={githubStatus()}>
                  <span class="text-xs text-muted-foreground">{githubStatus()}</span>
                </Show>
              </div>

              <div class="border-t border-border/70 pt-5" />

              <div class="flex items-center gap-2">
                <Label class="flex-1">Tavily Search</Label>
                <Show when={settings()?.tavily_configured}>
                  <Badge variant="success">Configured</Badge>
                </Show>
                <Show when={!settings()?.tavily_configured}>
                  <Badge variant="warning">Required</Badge>
                </Show>
                <Show when={settings()?.tavily_source === "env"}>
                  <Badge>Env</Badge>
                </Show>
              </div>

              <div class="space-y-1.5">
                <Label for="tavily-key">API Key</Label>
                <Show when={settings()?.tavily_key_masked}>
                  <p class="font-mono text-xs text-muted-foreground">
                    Current: {settings()!.tavily_key_masked}
                  </p>
                </Show>
                <Input
                  id="tavily-key"
                  type="password"
                  placeholder="tvly-..."
                  value={tavilyKey()}
                  onInput={(e) => setTavilyKey(e.currentTarget.value)}
                />
                <p class="text-xs text-muted-foreground">
                  Or set <code class="mx-0.5 bg-muted px-1 py-px text-[11px]">TAVILY_API_KEY</code> in
                  the environment.
                </p>
              </div>

              <div class="flex items-center gap-3">
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
          </TabsContent>
        </Tabs>
      </main>
    </div>
  );
};

export default SettingsPage;
