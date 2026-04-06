import { type Component, For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { createStore } from "solid-js/store";
import Badge from "@/components/ui/badge";
import Button from "@/components/ui/button";
import Input from "@/components/ui/input";
import Label from "@/components/ui/label";
import Select, { type SelectOption } from "@/components/ui/select";
import Tabs, { TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/cn";
import { getActions, getBinding, setBinding, resetBindings, eventToBinding } from "@/lib/keybindings";
import { themes, useTheme } from "@/lib/theme";
import {
  type CodexDeviceStartResponse,
  type SettingsResponse,
  getSettings,
  pollCodexDeviceFlow,
  saveRoleModels,
  saveGithubToken,
  saveOpenRouterSettings,
  saveSettingsProvider,
  saveTavilyKey,
  startCodexDeviceFlow,
} from "@/lib/api";
import { openUrl } from "@/lib/open-url";

const THEME_SWATCHES: Record<string, [string, string, string]> = {
  hirsel:        ["hsl(42 20% 95%)", "hsl(30 8% 10%)", "hsl(36 80% 50%)"],
  "hirsel-dark": ["hsl(40 8% 6%)",  "hsl(40 10% 88%)", "hsl(36 80% 50%)"],
  midnight:      ["hsl(230 25% 7%)", "hsl(210 15% 88%)", "hsl(185 80% 55%)"],
  bone:          ["hsl(38 40% 95%)", "hsl(20 8% 10%)",  "hsl(20 60% 40%)"],
};

const providerOptions: SelectOption[] = [
  { value: "codex", label: "Codex", description: "Built-in OAuth" },
  { value: "openrouter", label: "OpenRouter", description: "Custom endpoint / API key" },
];

const ROLE_MODEL_KEYS = ["shepherd", "librarian", "thread"] as const;
type RoleModelKey = (typeof ROLE_MODEL_KEYS)[number];
const DEFAULT_MODEL_SENTINEL = "__default_model__";
const DEFAULT_VARIANT_SENTINEL = "__default_variant__";

const ROLE_MODEL_META: Record<RoleModelKey, { label: string; description: string }> = {
  shepherd: {
    label: "Shepherd",
    description: "Main project coordinator and top-level chat.",
  },
  librarian: {
    label: "Librarian",
    description: "Knowledge graph and repo intelligence agent.",
  },
  thread: {
    label: "Threads",
    description: "Focused execution threads and branch workers.",
  },
};

type CodexAuthFlow = CodexDeviceStartResponse & { error: string | null };
type RoleModelDraft = Record<RoleModelKey, { model: string; model_variant: string }>;

type RoleModelOptionsMap = Record<RoleModelKey, SelectOption[]>;

const KeybindingsEditor: Component = () => {
  const [recording, setRecording] = createSignal<string | null>(null);
  const actions = getActions();

  const handleKeyDown = (event: KeyboardEvent) => {
    const actionId = recording();
    if (!actionId) return;
    event.preventDefault();
    event.stopPropagation();
    const combo = eventToBinding(event);
    if (!combo) return; // modifier-only press
    setBinding(actionId, combo);
    setRecording(null);
  };

  return (
    <div class="space-y-4">
      <div class="flex items-center justify-between">
        <Label>Keyboard Shortcuts</Label>
        <button
          type="button"
          class="text-[11px] text-muted-foreground/50 transition-colors hover:text-foreground"
          onClick={() => { resetBindings(); setRecording(null); }}
        >
          Reset defaults
        </button>
      </div>
      <div class="space-y-1">
        <For each={actions}>
          {(action) => {
            const isRecording = () => recording() === action.id;
            const binding = () => getBinding(action.id);

            return (
              <div class="flex items-center justify-between py-1.5">
                <span class="text-sm text-foreground">{action.label}</span>
                <button
                  type="button"
                  class={cn(
                    "min-w-[100px] border px-2.5 py-1 text-center font-mono text-[11px] transition-colors",
                    isRecording()
                      ? "border-signal-amber bg-signal-amber/10 text-signal-amber"
                      : "border-border text-muted-foreground hover:border-foreground/20",
                  )}
                  onClick={() => setRecording(isRecording() ? null : action.id)}
                  onKeyDown={(e) => {
                    if (isRecording()) {
                      handleKeyDown(e);
                    }
                  }}
                >
                  {isRecording() ? "Press key…" : binding()}
                </button>
              </div>
            );
          }}
        </For>
      </div>
    </div>
  );
};

function dispatchSettingsChanged() {
  window.dispatchEvent(new CustomEvent("hirsel-settings-changed"));
}

function defaultRoleDraft(): RoleModelDraft {
  return {
    shepherd: { model: DEFAULT_MODEL_SENTINEL, model_variant: DEFAULT_VARIANT_SENTINEL },
    librarian: { model: DEFAULT_MODEL_SENTINEL, model_variant: DEFAULT_VARIANT_SENTINEL },
    thread: { model: DEFAULT_MODEL_SENTINEL, model_variant: DEFAULT_VARIANT_SENTINEL },
  };
}

const SettingsForm: Component<{ onClose?: () => void }> = (props) => {
  const [settings, setSettings] = createSignal<SettingsResponse | null>(null);
  const [error, setError] = createSignal("");

  const [llmProvider, setLlmProvider] = createSignal<"codex" | "openrouter">("codex");
  const [llmSaving, setLlmSaving] = createSignal(false);
  const [llmReconnecting, setLlmReconnecting] = createSignal(false);
  const [llmStatus, setLlmStatus] = createSignal("");
  const [roleModelDraft, setRoleModelDraft] = createStore<RoleModelDraft>(defaultRoleDraft());
  const [modelSaving, setModelSaving] = createSignal(false);
  const [modelStatus, setModelStatus] = createSignal("");
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

  const { theme, setTheme } = useTheme();

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
      setRoleModelDraft({
        shepherd: {
          model: next.role_models.shepherd.configured_model ?? DEFAULT_MODEL_SENTINEL,
          model_variant:
            next.role_models.shepherd.configured_model_variant ?? DEFAULT_VARIANT_SENTINEL,
        },
        librarian: {
          model: next.role_models.librarian.configured_model ?? DEFAULT_MODEL_SENTINEL,
          model_variant:
            next.role_models.librarian.configured_model_variant ?? DEFAULT_VARIANT_SENTINEL,
        },
        thread: {
          model: next.role_models.thread.configured_model ?? DEFAULT_MODEL_SENTINEL,
          model_variant:
            next.role_models.thread.configured_model_variant ?? DEFAULT_VARIANT_SENTINEL,
        },
      });
      setError("");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load settings");
    }
  };

  onMount(() => { void reloadSettings(); });
  onCleanup(() => clearCodexPollTimer());

  const modelOptions = createMemo(() => settings()?.model_catalog.model_options ?? []);
  const variantOptionsByModel = createMemo(() => settings()?.model_catalog.variant_options ?? {});
  const defaultVariantsByModel = createMemo(() => settings()?.model_catalog.default_variants ?? {});

  const roleEffective = (role: RoleModelKey) => settings()?.role_models[role];
  const roleActiveModel = (role: RoleModelKey) => {
    const draft = roleModelDraft[role];
    if (draft.model !== DEFAULT_MODEL_SENTINEL) return draft.model;
    return roleEffective(role)?.effective_model ?? "";
  };
  const roleModelOptions = createMemo<RoleModelOptionsMap>(() => ({
    shepherd: [
      (() => {
        const effective = roleEffective("shepherd");
        return {
          value: DEFAULT_MODEL_SENTINEL,
          label: "Provider default",
          description: effective
            ? `${effective.effective_model}${effective.effective_model_variant ? ` · ${effective.effective_model_variant}` : ""}`
            : "Use the provider default for this role.",
        };
      })(),
      ...modelOptions().map((option) => ({
        value: option.value,
        label: option.label,
        description: option.description ?? undefined,
      })),
    ],
    librarian: [
      (() => {
        const effective = roleEffective("librarian");
        return {
          value: DEFAULT_MODEL_SENTINEL,
          label: "Provider default",
          description: effective
            ? `${effective.effective_model}${effective.effective_model_variant ? ` · ${effective.effective_model_variant}` : ""}`
            : "Use the provider default for this role.",
        };
      })(),
      ...modelOptions().map((option) => ({
        value: option.value,
        label: option.label,
        description: option.description ?? undefined,
      })),
    ],
    thread: [
      (() => {
        const effective = roleEffective("thread");
        return {
          value: DEFAULT_MODEL_SENTINEL,
          label: "Provider default",
          description: effective
            ? `${effective.effective_model}${effective.effective_model_variant ? ` · ${effective.effective_model_variant}` : ""}`
            : "Use the provider default for this role.",
        };
      })(),
      ...modelOptions().map((option) => ({
        value: option.value,
        label: option.label,
        description: option.description ?? undefined,
      })),
    ],
  }));
  const roleVariantOptions = createMemo<RoleModelOptionsMap>(() => {
    const variantsByModel = variantOptionsByModel();
    const defaultsByModel = defaultVariantsByModel();
    const buildOptions = (role: RoleModelKey): SelectOption[] => {
      const model = roleActiveModel(role);
      const variants = variantsByModel[model] ?? [];
      const defaultVariant = defaultsByModel[model] ?? null;
      if (variants.length === 0) {
        return [{ value: DEFAULT_VARIANT_SENTINEL, label: "No variant", description: "This model does not expose configurable variants." }];
      }
      return [
        {
          value: DEFAULT_VARIANT_SENTINEL,
          label: defaultVariant ? `Default (${defaultVariant})` : "Default",
          description: "Use the provider's recommended variant.",
        },
        ...variants.map((variant) => ({
          value: variant,
          label: variant.charAt(0).toUpperCase() + variant.slice(1),
          description: variant === defaultVariant ? "Recommended by lash defaults." : undefined,
        })),
      ];
    };
    return {
      shepherd: buildOptions("shepherd"),
      librarian: buildOptions("librarian"),
      thread: buildOptions("thread"),
    };
  });

  const updateRoleDraft = (role: RoleModelKey, patch: Partial<{ model: string; model_variant: string }>) => {
    if (patch.model !== undefined) {
      setRoleModelDraft(role, "model", patch.model);
    }
    if (patch.model_variant !== undefined) {
      setRoleModelDraft(role, "model_variant", patch.model_variant);
    }
  };

  const handleRoleModelChange = (role: RoleModelKey, model: string) => {
    const nextOptions = (() => {
      const effectiveModel =
        model === DEFAULT_MODEL_SENTINEL ? roleEffective(role)?.effective_model ?? "" : model;
      return variantOptionsByModel()[effectiveModel] ?? [];
    })();
    const currentVariant = roleModelDraft[role].model_variant;
    const variantStillValid =
      currentVariant === DEFAULT_VARIANT_SENTINEL || nextOptions.includes(currentVariant);
    updateRoleDraft(role, {
      model,
      model_variant: variantStillValid ? currentVariant : DEFAULT_VARIANT_SENTINEL,
    });
  };

  const handleSaveRoleModels = async () => {
    setModelSaving(true);
    setModelStatus("");
    const draft = roleModelDraft;
    try {
      await saveRoleModels({
        shepherd: {
          model: draft.shepherd.model === DEFAULT_MODEL_SENTINEL ? null : draft.shepherd.model,
          model_variant:
            draft.shepherd.model_variant === DEFAULT_VARIANT_SENTINEL
              ? null
              : draft.shepherd.model_variant,
        },
        librarian: {
          model: draft.librarian.model === DEFAULT_MODEL_SENTINEL ? null : draft.librarian.model,
          model_variant:
            draft.librarian.model_variant === DEFAULT_VARIANT_SENTINEL
              ? null
              : draft.librarian.model_variant,
        },
        thread: {
          model: draft.thread.model === DEFAULT_MODEL_SENTINEL ? null : draft.thread.model,
          model_variant:
            draft.thread.model_variant === DEFAULT_VARIANT_SENTINEL
              ? null
              : draft.thread.model_variant,
        },
      });
      setModelStatus("Saved");
      setTimeout(() => setModelStatus(""), 2000);
      dispatchSettingsChanged();
      await reloadSettings();
    } catch (err) {
      setModelStatus(err instanceof Error ? err.message : "Save failed");
    } finally {
      setModelSaving(false);
    }
  };

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
      dispatchSettingsChanged();
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
        dispatchSettingsChanged();
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
      dispatchSettingsChanged();
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
      dispatchSettingsChanged();
      await reloadSettings();
    } catch (err) {
      setGithubStatus(err instanceof Error ? err.message : "Save failed");
    } finally {
      setGithubSaving(false);
    }
  };

  const codexReady = () => settings()?.codex_configured && settings()?.provider === "codex";
  const codexFromEnv = () => settings()?.codex_source === "env";
  const showConnectBtn = () => llmProvider() === "codex" && !codexFromEnv();

  return (
    <div>
      <Show when={error()}>
        <div class="mb-4 border border-signal-red/30 bg-signal-red/10 px-4 py-3 text-sm text-signal-red">
          {error()}
        </div>
      </Show>

      <Tabs defaultValue="provider">
        <TabsList>
          <TabsTrigger value="provider">Provider</TabsTrigger>
          <TabsTrigger value="models">Models</TabsTrigger>
          <TabsTrigger value="tools">Tools</TabsTrigger>
          <TabsTrigger value="keys">Keys</TabsTrigger>
          <TabsTrigger value="appearance">Appearance</TabsTrigger>
        </TabsList>

        {/* ── Provider ── */}
        <TabsContent value="provider">
          <div class="space-y-5">
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
                    void saveSettingsProvider(provider).then(() => {
                      dispatchSettingsChanged();
                      return reloadSettings();
                    });
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

        {/* ── Models ── */}
        <TabsContent value="models">
          <div class="space-y-5">
            <div class="space-y-1">
              <Label>Role Models</Label>
              <p class="text-xs leading-5 text-muted-foreground">
                Choose different model/variant pairs for the main shepherd, the librarian, and execution threads.
              </p>
            </div>

            <For each={ROLE_MODEL_KEYS}>
              {(role) => {
                const meta = ROLE_MODEL_META[role];
                const effective = () => roleEffective(role);
                const currentDraft = () => roleModelDraft[role];
                const variantDisabled = () => (variantOptionsByModel()[roleActiveModel(role)] ?? []).length === 0;

                return (
                  <div class="space-y-3 border border-border bg-muted/10 p-4">
                    <div class="space-y-1">
                      <div class="flex items-center justify-between gap-3">
                        <div class="font-medium text-foreground">{meta.label}</div>
                        <div class="text-[11px] text-muted-foreground">
                          {effective()?.effective_model}
                          {effective()?.effective_model_variant ? ` · ${effective()!.effective_model_variant}` : ""}
                        </div>
                      </div>
                      <p class="text-xs leading-5 text-muted-foreground">{meta.description}</p>
                    </div>
                    <div class="grid gap-3 md:grid-cols-2">
                      <div class="space-y-1.5">
                        <Label>{meta.label} model</Label>
                        <Select
                          options={roleModelOptions()[role]}
                          value={currentDraft().model}
                          onChange={(value) => handleRoleModelChange(role, value)}
                        />
                      </div>
                      <div class="space-y-1.5">
                        <Label>{meta.label} variant</Label>
                        <Select
                          options={roleVariantOptions()[role]}
                          value={variantDisabled() ? DEFAULT_VARIANT_SENTINEL : currentDraft().model_variant}
                          onChange={(value) => updateRoleDraft(role, { model_variant: value })}
                          disabled={variantDisabled()}
                        />
                      </div>
                    </div>
                  </div>
                );
              }}
            </For>

            <div class="flex items-center gap-3">
              <Button size="sm" loading={modelSaving()} onClick={handleSaveRoleModels}>
                Save models
              </Button>
              <Show when={modelStatus()}>
                <span class="text-xs text-muted-foreground">{modelStatus()}</span>
              </Show>
            </div>
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
                Forwarded into containers as{" "}
                <code class="mx-0.5 bg-muted px-1 py-px text-[11px]">GITHUB_TOKEN</code> and{" "}
                <code class="mx-0.5 bg-muted px-1 py-px text-[11px]">GH_TOKEN</code>.
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

        {/* ── Keybindings ── */}
        <TabsContent value="keys">
          <KeybindingsEditor />
        </TabsContent>

        {/* ── Appearance ── */}
        <TabsContent value="appearance">
          <div class="space-y-4">
            <Label>Theme</Label>
            <div class="grid gap-2">
              <For each={themes}>
                {(t) => (
                  <button
                    type="button"
                    class={cn(
                      "flex items-center gap-3 border px-3 py-2.5 text-left text-sm transition-colors",
                      theme() === t.name
                        ? "border-foreground/20 bg-secondary/50 text-foreground"
                        : "border-border text-muted-foreground hover:border-foreground/10 hover:bg-secondary/30",
                    )}
                    onClick={() => setTheme(t.name)}
                  >
                    <div class="flex shrink-0 items-center gap-px">
                      <For each={THEME_SWATCHES[t.name] ?? []}>
                        {(color) => (
                          <span
                            class="h-4 w-2 border border-border"
                            style={{ background: color }}
                          />
                        )}
                      </For>
                    </div>
                    <span class="flex-1 font-medium">{t.label}</span>
                    <Show when={theme() === t.name}>
                      <span class="font-mono text-[11px] text-muted-foreground">active</span>
                    </Show>
                  </button>
                )}
              </For>
            </div>
          </div>
        </TabsContent>
      </Tabs>
    </div>
  );
};

export default SettingsForm;
