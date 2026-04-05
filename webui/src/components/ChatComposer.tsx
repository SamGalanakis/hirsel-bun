import {
  type Component,
  For,
  Show,
  createEffect,
  createMemo,
  createResource,
  createSignal,
  on,
  onCleanup,
  onMount,
} from "solid-js";
import { Badge } from "@/components/ui/badge";
import { completeWorkspacePath, listProjectSkills, type WorkspaceCompletionEntry } from "@/lib/api";
import { cn } from "@/lib/cn";
import {
  collectRecognizedFileMentions,
  collectRecognizedSkillMentions,
  findActiveComposerToken,
} from "@/lib/chat-composer";

interface ChatComposerProps {
  projectId: number;
  threadId?: string;
  value: string;
  running: boolean;
  justStopped: boolean;
  focusNonce?: number;
  onValueChange: (value: string) => void;
  onSubmit: () => void | Promise<void>;
  onStop: () => void | Promise<void>;
}

type ComposerSuggestion =
  | {
      kind: "command";
      id: string;
      label: string;
      description: string;
    }
  | {
      kind: "skill";
      id: string;
      label: string;
      description: string;
      name: string;
    }
  | {
      kind: "file";
      id: string;
      label: string;
      description: string;
      entry: WorkspaceCompletionEntry;
    };

function iconSlash() {
  return (
    <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="1.8">
      <path d="M16 4L8 20" />
    </svg>
  );
}

function iconFile() {
  return (
    <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="1.6">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6" />
    </svg>
  );
}

function iconFolder() {
  return (
    <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="1.6">
      <path d="M3 7h6l2 2h10v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
      <path d="M3 7V5a2 2 0 0 1 2-2h4l2 2" />
    </svg>
  );
}

function iconSpark() {
  return (
    <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="1.8">
      <path d="M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z" />
    </svg>
  );
}

const ChatComposer: Component<ChatComposerProps> = (props) => {
  const [skills] = createResource(() => props.projectId, listProjectSkills);
  const [cursor, setCursor] = createSignal(props.value.length);
  const [suggestions, setSuggestions] = createSignal<ComposerSuggestion[]>([]);
  const [suggestionsLoading, setSuggestionsLoading] = createSignal(false);
  const [suggestionsError, setSuggestionsError] = createSignal("");
  const [selectedSuggestionIndex, setSelectedSuggestionIndex] = createSignal(0);
  const [skillBrowserOpen, setSkillBrowserOpen] = createSignal(false);
  const [skillBrowserIndex, setSkillBrowserIndex] = createSignal(0);
  const [pendingSelection, setPendingSelection] = createSignal<number | null>(null);
  let textareaRef: HTMLTextAreaElement | undefined;

  const activeToken = createMemo(() => findActiveComposerToken(props.value, cursor()));
  const recognizedSkills = createMemo(() =>
    collectRecognizedSkillMentions(
      props.value,
      (skills() ?? []).map((skill) => skill.name),
    ),
  );
  const recognizedFiles = createMemo(() => {
    if (props.threadId) return [];
    return collectRecognizedFileMentions(props.value);
  });
  const skillBrowserItems = createMemo(() => skills() ?? []);
  const showAutocomplete = createMemo(
    () =>
      !skillBrowserOpen() &&
      !!activeToken() &&
      (suggestionsLoading() || suggestions().length > 0 || !!suggestionsError()),
  );

  const autoGrow = () => {
    if (!textareaRef) return;
    textareaRef.style.height = "auto";
    textareaRef.style.height = `${Math.min(textareaRef.scrollHeight, 220)}px`;
  };

  const syncCursorFromTextarea = () => {
    if (!textareaRef) return;
    setCursor(textareaRef.selectionStart ?? textareaRef.value.length);
  };

  const focusTextarea = (position?: number) => {
    queueMicrotask(() => {
      if (!textareaRef) return;
      textareaRef.focus();
      const next = position ?? textareaRef.value.length;
      textareaRef.selectionStart = next;
      textareaRef.selectionEnd = next;
      setCursor(next);
      autoGrow();
    });
  };

  const openSkillBrowser = () => {
    props.onValueChange("");
    setSkillBrowserOpen(true);
    setSkillBrowserIndex(0);
    setSuggestions([]);
    setSuggestionsError("");
    setSuggestionsLoading(false);
    focusTextarea(0);
  };

  const replaceToken = (nextText: string) => {
    const token = activeToken();
    if (!token) return;
    const value =
      props.value.slice(0, token.rangeStart) + nextText + props.value.slice(token.rangeEnd);
    props.onValueChange(value);
    setPendingSelection(token.rangeStart + nextText.length);
  };

  const insertSkill = (name: string) => {
    setSkillBrowserOpen(false);
    const token = activeToken();
    if (token) {
      replaceToken(`/${name} `);
      return;
    }
    const value = `/${name} `;
    props.onValueChange(value);
    setPendingSelection(value.length);
  };

  const acceptSuggestion = (suggestion: ComposerSuggestion | undefined) => {
    if (!suggestion) return;
    if (suggestion.kind === "command") {
      openSkillBrowser();
      return;
    }
    if (suggestion.kind === "skill") {
      replaceToken(`/${suggestion.name} `);
      return;
    }
    const entry = suggestion.entry;
    if (entry.kind === "directory") {
      replaceToken(`@${entry.path}`);
      return;
    }
    replaceToken(`@${entry.path} `);
  };

  const isSubmitKey = (event: Pick<KeyboardEvent, "key" | "code" | "altKey" | "ctrlKey" | "metaKey" | "shiftKey">) => {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) {
      return false;
    }
    return (
      event.key === "Enter" ||
      event.key === "Return" ||
      event.code === "Enter" ||
      event.code === "NumpadEnter"
    );
  };

  const handleSubmitIntent = (event: Pick<KeyboardEvent, "key" | "code" | "altKey" | "ctrlKey" | "metaKey" | "shiftKey"> & {
    preventDefault: () => void;
    stopPropagation?: () => void;
  }) => {
    if (!isSubmitKey(event)) {
      return false;
    }

    event.preventDefault();
    event.stopPropagation?.();

    if (skillBrowserOpen()) {
      const skill = skillBrowserItems()[skillBrowserIndex()];
      if (skill) {
        insertSkill(skill.name);
      }
      return true;
    }

    if (showAutocomplete()) {
      acceptSuggestion(suggestions()[selectedSuggestionIndex()]);
      return true;
    }

    if (props.value.trim() === "/skills") {
      openSkillBrowser();
      return true;
    }

    void props.onSubmit();
    return true;
  };

  onMount(() => {
    const handler = (event: KeyboardEvent) => {
      if (!textareaRef) return;
      if (document.activeElement !== textareaRef) return;
      void handleSubmitIntent(event);
    };

    window.addEventListener("keydown", handler, true);
    onCleanup(() => window.removeEventListener("keydown", handler, true));
  });

  createEffect(
    on(
      () => props.value,
      () => {
        autoGrow();
        if (skillBrowserOpen() && props.value.trim()) {
          setSkillBrowserOpen(false);
        }
      },
    ),
  );

  createEffect(
    on(
      () => props.focusNonce,
      () => {
        focusTextarea();
      },
    ),
  );

  createEffect(() => {
    const next = pendingSelection();
    if (next === null) return;
    focusTextarea(next);
    setPendingSelection(null);
  });

  createEffect(
    on(
      () => suggestions().map((suggestion) => suggestion.id).join("|"),
      () => {
        setSelectedSuggestionIndex(0);
      },
    ),
  );

  createEffect(
    on(
      () => [
        activeToken(),
        props.projectId,
        props.threadId,
        skillBrowserOpen(),
        skills.loading,
        skills.error,
        (skills() ?? []).map((skill) => skill.name).join("|"),
      ],
      ([token]) => {
        if (skillBrowserOpen()) return;

        if (!token) {
          setSuggestions([]);
          setSuggestionsLoading(false);
          setSuggestionsError("");
          return;
        }

        if (token.kind === "slash") {
          const query = token.query.trim().toLowerCase();
          const nextSuggestions: ComposerSuggestion[] = [];
          if ("/skills".startsWith(`/${query}`) || query === "") {
            nextSuggestions.push({
              kind: "command",
              id: "command:skills",
              label: "/skills",
              description: "Browse loaded skills",
            });
          }
          for (const skill of skills() ?? []) {
            const label = `/${skill.name}`;
            if (query && !label.toLowerCase().startsWith(`/${query}`)) {
              continue;
            }
            nextSuggestions.push({
              kind: "skill",
              id: `skill:${skill.name}`,
              label,
              description: skill.description || "Invoke skill",
              name: skill.name,
            });
          }
          setSuggestions(nextSuggestions);
          setSuggestionsLoading(skills.loading);
          setSuggestionsError(skills.error instanceof Error ? skills.error.message : "");
          return;
        }

        if (props.threadId) {
          setSuggestions([]);
          setSuggestionsLoading(false);
          setSuggestionsError("");
          return;
        }

        const controller = new AbortController();
        setSuggestionsLoading(true);
        setSuggestionsError("");
        void completeWorkspacePath(props.projectId, {
          rootId: "main",
          prefix: token.query,
          signal: controller.signal,
        })
          .then((entries) => {
            setSuggestions(
              entries.map((entry) => ({
                kind: "file" as const,
                id: `file:${entry.path}`,
                label: `@${entry.path}`,
                description: entry.kind === "directory" ? "Directory" : "File",
                entry,
              })),
            );
          })
          .catch((error) => {
            if (controller.signal.aborted) return;
            setSuggestions([]);
            setSuggestionsError(error instanceof Error ? error.message : "Failed to load files");
          })
          .finally(() => {
            if (!controller.signal.aborted) {
              setSuggestionsLoading(false);
            }
          });

        return () => controller.abort();
      },
    ),
  );

  return (
    <div class="relative">
      <Show when={skillBrowserOpen()}>
        <div class="absolute inset-x-0 bottom-[calc(100%+12px)] z-20 border border-border bg-card shadow-lift">
          <div class="flex items-center gap-2 border-b border-border px-3 py-2">
            <span class="flex h-7 w-7 items-center justify-center border border-signal-blue/20 bg-signal-blue/8 text-signal-blue">
              {iconSpark()}
            </span>
            <div class="min-w-0">
              <div class="text-sm font-medium text-foreground">Skills</div>
              <div class="text-[11px] text-muted-foreground">
                Pick a skill to insert into the message.
              </div>
            </div>
            <button
              type="button"
              class="ml-auto text-[11px] text-muted-foreground transition-colors hover:text-foreground"
              onClick={() => {
                setSkillBrowserOpen(false);
                focusTextarea();
              }}
            >
              close
            </button>
          </div>
          <div class="max-h-72 overflow-y-auto p-2">
            <Show
              when={!skills.loading}
              fallback={<div class="px-2 py-6 text-center text-xs text-muted-foreground">Loading skills…</div>}
            >
              <Show
                when={skillBrowserItems().length > 0}
                fallback={<div class="px-2 py-6 text-center text-xs text-muted-foreground">No skills found for this project.</div>}
              >
                <For each={skillBrowserItems()}>
                  {(skill, index) => (
                    <button
                      type="button"
                      class={cn(
                        "flex w-full items-start gap-3 border px-3 py-2 text-left transition-colors",
                        index() === skillBrowserIndex()
                          ? "border-signal-blue/30 bg-signal-blue/8"
                          : "border-transparent hover:border-border hover:bg-background",
                      )}
                      onMouseEnter={() => setSkillBrowserIndex(index())}
                      onClick={() => insertSkill(skill.name)}
                    >
                      <div class="flex h-8 w-8 shrink-0 items-center justify-center border border-border bg-background text-muted-foreground">
                        {iconSlash()}
                      </div>
                      <div class="min-w-0">
                        <div class="font-mono text-[12px] text-foreground">/{skill.name}</div>
                        <div class="mt-0.5 text-xs leading-5 text-muted-foreground">
                          {skill.description || "Invoke skill"}
                        </div>
                      </div>
                    </button>
                  )}
                </For>
              </Show>
            </Show>
          </div>
        </div>
      </Show>

      <Show when={showAutocomplete()}>
        <div class="absolute inset-x-0 bottom-[calc(100%+12px)] z-20 border border-border bg-card shadow-lift">
          <div class="max-h-72 overflow-y-auto p-1.5">
            <Show when={suggestionsLoading()}>
              <div class="px-2 py-2 text-xs text-muted-foreground">Loading…</div>
            </Show>
            <Show when={!suggestionsLoading() && suggestionsError()}>
              <div class="px-2 py-2 text-xs text-signal-red">{suggestionsError()}</div>
            </Show>
            <For each={suggestions()}>
              {(suggestion, index) => (
                <button
                  type="button"
                  class={cn(
                    "flex w-full items-start gap-2 px-2 py-2 text-left text-xs transition-colors",
                    index() === selectedSuggestionIndex()
                      ? "bg-secondary text-foreground"
                      : "text-muted-foreground hover:bg-secondary/70 hover:text-foreground",
                  )}
                  onMouseEnter={() => setSelectedSuggestionIndex(index())}
                  onClick={() => acceptSuggestion(suggestion)}
                >
                  <span class="mt-0.5 shrink-0 text-muted-foreground/70">
                    {suggestion.kind === "command"
                      ? iconSpark()
                      : suggestion.kind === "skill"
                        ? iconSlash()
                        : suggestion.entry.kind === "directory"
                          ? iconFolder()
                          : iconFile()}
                  </span>
                  <div class="min-w-0">
                    <div class="font-mono text-[12px] text-foreground">{suggestion.label}</div>
                    <div class="mt-0.5 leading-5 text-muted-foreground">
                      {suggestion.description}
                    </div>
                  </div>
                </button>
              )}
            </For>
            <Show when={!suggestionsLoading() && !suggestionsError() && suggestions().length === 0}>
              <div class="px-2 py-2 text-xs text-muted-foreground">No matches.</div>
            </Show>
          </div>
        </div>
      </Show>

      <form
        onSubmit={(event) => {
          event.preventDefault();
          void props.onSubmit();
        }}
      >
        <div class={cn(
          "border bg-background shadow-sm transition-[border-color] duration-300",
          props.running ? "border-signal-amber/25" : "border-border focus-within:border-ring",
        )}>
          <div class="flex items-end gap-1 p-1">
            <textarea
              ref={textareaRef}
              class="min-h-[42px] max-h-[220px] flex-1 resize-none appearance-none border-0 bg-transparent px-3 py-2 text-sm text-foreground outline-none placeholder:text-muted-foreground"
              rows={1}
              value={props.value}
              aria-label={props.threadId ? "Thread message" : "Project message"}
              onInput={(event) => {
                props.onValueChange(event.currentTarget.value);
                setCursor(event.currentTarget.selectionStart ?? event.currentTarget.value.length);
                autoGrow();
              }}
              onClick={syncCursorFromTextarea}
              onKeyUp={syncCursorFromTextarea}
              onSelect={syncCursorFromTextarea}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  if (skillBrowserOpen()) {
                    event.preventDefault();
                    setSkillBrowserOpen(false);
                    return;
                  }
                  if (showAutocomplete()) {
                    event.preventDefault();
                    setSuggestions([]);
                    setSuggestionsError("");
                    setSuggestionsLoading(false);
                    return;
                  }
                  return;
                }

                if (skillBrowserOpen()) {
                  if (event.key === "ArrowDown") {
                    event.preventDefault();
                    if (skillBrowserItems().length === 0) return;
                    setSkillBrowserIndex((current) =>
                      Math.min(skillBrowserItems().length - 1, current + 1),
                    );
                    return;
                  }
                  if (event.key === "ArrowUp") {
                    event.preventDefault();
                    if (skillBrowserItems().length === 0) return;
                    setSkillBrowserIndex((current) => Math.max(0, current - 1));
                    return;
                  }
                  if (event.key === "Tab") {
                    event.preventDefault();
                    const skill = skillBrowserItems()[skillBrowserIndex()];
                    if (skill) {
                      insertSkill(skill.name);
                    }
                    return;
                  }
                }

                if (showAutocomplete()) {
                  if (event.key === "ArrowDown") {
                    event.preventDefault();
                    if (suggestions().length === 0) return;
                    setSelectedSuggestionIndex((current) =>
                      Math.min(suggestions().length - 1, current + 1),
                    );
                    return;
                  }
                  if (event.key === "ArrowUp") {
                    event.preventDefault();
                    if (suggestions().length === 0) return;
                    setSelectedSuggestionIndex((current) => Math.max(0, current - 1));
                    return;
                  }
                  if (event.key === "Tab") {
                    event.preventDefault();
                    acceptSuggestion(suggestions()[selectedSuggestionIndex()]);
                    return;
                  }
                }
              }}
            />

            <Show when={props.running}>
              <button
                type="button"
                class="relative flex h-9 w-9 shrink-0 items-center justify-center transition-transform hover:scale-105 active:scale-[0.92]"
                onClick={() => void props.onStop()}
                aria-label="Stop"
                title="Stop (Esc)"
              >
                {/* Outer arc — clockwise, slow */}
                <svg class="absolute inset-0 h-full w-full animate-spin-arc" viewBox="0 0 36 36" fill="none">
                  <circle cx="18" cy="18" r="16.5" stroke="hsl(var(--signal-amber))" stroke-width="1" stroke-dasharray="20 80" stroke-linecap="round" opacity="0.45" />
                </svg>
                {/* Inner arc — counter-clockwise, faster */}
                <svg class="absolute inset-[4px] h-[calc(100%-8px)] w-[calc(100%-8px)] animate-spin-arc-reverse" viewBox="0 0 36 36" fill="none">
                  <circle cx="18" cy="18" r="16.5" stroke="hsl(var(--foreground))" stroke-width="1.5" stroke-dasharray="28 72" stroke-linecap="round" opacity="0.5" />
                </svg>
                {/* Stop square */}
                <svg viewBox="0 0 24 24" class="relative h-2.5 w-2.5 text-foreground" fill="currentColor">
                  <rect x="6" y="6" width="12" height="12" rx="2" />
                </svg>
              </button>
            </Show>
          </div>

          <Show when={recognizedSkills().length > 0 || recognizedFiles().length > 0}>
            <div class="flex flex-wrap gap-1 border-t border-border/70 px-3 py-2">
              <For each={recognizedSkills()}>
                {(skill) => (
                  <Badge variant="outline" class="gap-1 border-signal-blue/25 bg-signal-blue/8 text-signal-blue">
                    {iconSlash()}
                    {skill.label}
                  </Badge>
                )}
              </For>
              <For each={recognizedFiles()}>
                {(file) => (
                  <Badge variant="outline" class="gap-1 border-signal-green/20 bg-signal-green/8 text-signal-green">
                    {iconFile()}
                    {file.label}
                  </Badge>
                )}
              </For>
            </div>
          </Show>

          {/* Streaming shimmer — thin gradient sweep at bottom edge */}
          <Show when={props.running}>
            <div class="h-[2px] w-full overflow-hidden">
              <div class="h-full w-1/3 animate-shimmer-flow bg-gradient-to-r from-transparent via-signal-amber/40 to-transparent" />
            </div>
          </Show>
        </div>

        <Show when={props.justStopped}>
          <div class="mt-1 text-center text-[11px] text-muted-foreground">
            Stopped. Send a message to continue.
          </div>
        </Show>
      </form>
    </div>
  );
};

export default ChatComposer;
