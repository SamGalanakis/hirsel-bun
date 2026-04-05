import {
  type Component,
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  on,
} from "solid-js";
import {
  ApiError,
  buildWorkspaceDownloadUrl,
  getWorkspaceDiff,
  getWorkspaceDiffFile,
  getWorkspaceFile,
  listWorkspaceRoots,
  listWorkspaceTree,
  saveWorkspaceFile,
  searchWorkspace,
  uploadWorkspaceFiles,
  type WorkspaceDiffEntry,
  type WorkspaceDiffFile,
  type WorkspaceFile,
  type WorkspaceRoot,
  type WorkspaceSearchResult,
  type WorkspaceTreeEntry,
} from "@/lib/api";
import MonacoDiffEditor from "@/components/MonacoDiffEditor";
import MonacoTextEditor from "@/components/MonacoTextEditor";
import { cn } from "@/lib/cn";

interface WorkspaceBrowserProps {
  projectId: number;
  threadId?: string;
}

type SelectedNode =
  | { kind: "root"; rootId: string; path: "" }
  | { kind: "directory"; rootId: string; path: string }
  | { kind: "file"; rootId: string; path: string };

interface ComparePair {
  leftRootId: string;
  rightRootId: string;
}

const SEARCH_SCOPE_ALL = "__all__";

function joinPath(base: string, name: string): string {
  return base ? `${base}/${name}` : name;
}

function dirname(path: string): string {
  const parts = path.split("/").filter(Boolean);
  parts.pop();
  return parts.join("/");
}

function isImageMime(mime: string | null | undefined): boolean {
  return !!mime?.startsWith("image/");
}

function statusDotClass(status: string): string {
  switch (status) {
    case "running":
    case "active":
      return "bg-signal-green";
    case "waiting":
    case "blocked":
      return "bg-signal-amber";
    case "failed":
    case "error":
      return "bg-signal-red";
    default:
      return "bg-signal-blue";
  }
}

function rootLabel(root: WorkspaceRoot): string {
  if (root.kind === "remote") return "Remote";
  if (root.kind === "main") return "Shepherd";
  return root.label;
}

function rootLabelById(rootMap: Map<string, WorkspaceRoot>, rootId: string): string {
  return rootLabel(rootMap.get(rootId) ?? {
    id: rootId,
    kind: "thread",
    label: rootId,
    status: "ready",
    summary: null,
    threadId: null,
    branch: null,
    readOnly: false,
  });
}

function rowPadding(depth: number): string {
  return `${Math.max(10, depth * 14)}px`;
}

function diffStatusLabel(status: WorkspaceDiffEntry["status"] | WorkspaceDiffFile["status"]): string {
  switch (status) {
    case "added":
      return "Added";
    case "deleted":
      return "Deleted";
    default:
      return "Modified";
  }
}

function diffStatusClass(status: WorkspaceDiffEntry["status"] | WorkspaceDiffFile["status"]): string {
  switch (status) {
    case "added":
      return "bg-signal-green/12 text-signal-green";
    case "deleted":
      return "bg-signal-red/12 text-signal-red";
    default:
      return "bg-signal-blue/12 text-signal-blue";
  }
}

function iconBranch() {
  return (
    <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="1.5">
      <circle cx="18" cy="6" r="2" />
      <circle cx="6" cy="6" r="2" />
      <circle cx="6" cy="18" r="2" />
      <path d="M6 8v10M18 8c0 4-4 4-4 8" />
    </svg>
  );
}

function iconChevron(open: boolean) {
  return (
    <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2">
      {open ? <polyline points="6 9 12 15 18 9" /> : <polyline points="9 6 15 12 9 18" />}
    </svg>
  );
}

function iconFolder() {
  return (
    <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="1.5">
      <path d="M3 7h6l2 2h10v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
      <path d="M3 7V5a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v2" />
    </svg>
  );
}

function iconFile() {
  return (
    <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="1.5">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6" />
    </svg>
  );
}

function iconSearch() {
  return (
    <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="2">
      <circle cx="11" cy="11" r="7" />
      <path d="M20 20l-3.5-3.5" />
    </svg>
  );
}

function pairKey(pair: ComparePair | null): string {
  if (!pair) return "";
  return `${pair.leftRootId}::${pair.rightRootId}`;
}

const WorkspaceBrowser: Component<WorkspaceBrowserProps> = (props) => {
  const [roots, setRoots] = createSignal<WorkspaceRoot[]>([]);
  const [rootsLoading, setRootsLoading] = createSignal(false);
  const [rootsError, setRootsError] = createSignal("");
  const [expandedKeys, setExpandedKeys] = createSignal<Set<string>>(new Set());
  const [treeCache, setTreeCache] = createSignal<Record<string, WorkspaceTreeEntry[]>>({});
  const [treeLoading, setTreeLoading] = createSignal<Record<string, boolean>>({});
  const [selectedNode, setSelectedNode] = createSignal<SelectedNode>({
    kind: "root",
    rootId: "main",
    path: "",
  });
  const [currentFile, setCurrentFile] = createSignal<WorkspaceFile | null>(null);
  const [fileLoading, setFileLoading] = createSignal(false);
  const [fileError, setFileError] = createSignal("");
  const [draft, setDraft] = createSignal("");
  const [saveBusy, setSaveBusy] = createSignal(false);
  const [searchQuery, setSearchQuery] = createSignal("");
  const [searchScope, setSearchScope] = createSignal(SEARCH_SCOPE_ALL);
  const [searchLoading, setSearchLoading] = createSignal(false);
  const [searchError, setSearchError] = createSignal("");
  const [searchResults, setSearchResults] = createSignal<WorkspaceSearchResult[]>([]);
  const [searchTruncated, setSearchTruncated] = createSignal(false);
  const [revealLine, setRevealLine] = createSignal<number | null>(null);
  const [dropHover, setDropHover] = createSignal(false);
  const [compareSelection, setCompareSelection] = createSignal<string[]>([]);
  const [activeComparePair, setActiveComparePair] = createSignal<ComparePair | null>(null);
  const [diffEntries, setDiffEntries] = createSignal<WorkspaceDiffEntry[]>([]);
  const [diffLoading, setDiffLoading] = createSignal(false);
  const [diffError, setDiffError] = createSignal("");
  const [diffFilter, setDiffFilter] = createSignal("");
  const [currentDiff, setCurrentDiff] = createSignal<WorkspaceDiffFile | null>(null);
  const [diffFileLoading, setDiffFileLoading] = createSignal(false);
  const [diffFileError, setDiffFileError] = createSignal("");
  let uploadInputRef: HTMLInputElement | undefined;

  const defaultRootId = () => (props.threadId ? `thread:${props.threadId}` : "main");

  const selectedRootId = createMemo(() => selectedNode().rootId);
  const rootMap = createMemo(() => {
    const map = new Map<string, WorkspaceRoot>();
    for (const root of roots()) map.set(root.id, root);
    return map;
  });
  const selectedRoot = createMemo(() => rootMap().get(selectedRootId()));
  const isReadOnly = createMemo(() => selectedRoot()?.readOnly ?? false);
  const dirty = createMemo(() => {
    if (isReadOnly() || activeComparePair()) return false;
    const file = currentFile();
    return !!file?.isText && file.content !== null && draft() !== file.content;
  });
  const currentDownloadUrl = createMemo(() => {
    const file = currentFile();
    if (!file) return "";
    return buildWorkspaceDownloadUrl(props.projectId, {
      rootId: file.rootId,
      path: file.path,
    });
  });
  const uploadTarget = createMemo(() => {
    const node = selectedNode();
    if (node.kind === "file") {
      return { rootId: node.rootId, path: dirname(node.path) };
    }
    return { rootId: node.rootId, path: node.path };
  });
  const compareSelectionIndex = createMemo(() => {
    const map = new Map<string, number>();
    compareSelection().forEach((rootId, index) => map.set(rootId, index + 1));
    return map;
  });
  const canCompare = createMemo(() => compareSelection().length === 2);
  const activeCompareRoots = createMemo(() => {
    const pair = activeComparePair();
    if (!pair) return null;
    return {
      left: rootMap().get(pair.leftRootId),
      right: rootMap().get(pair.rightRootId),
    };
  });
  const filteredDiffEntries = createMemo(() => {
    const query = diffFilter().trim().toLowerCase();
    if (!query) return diffEntries();
    return diffEntries().filter((entry) => entry.path.toLowerCase().includes(query));
  });
  const diffCounts = createMemo(() => {
    const counts = { modified: 0, added: 0, deleted: 0 };
    for (const entry of diffEntries()) {
      counts[entry.status] += 1;
    }
    return counts;
  });
  const sameActiveCompareSelection = createMemo(() => {
    if (!canCompare()) return false;
    const pair = activeComparePair();
    if (!pair) return false;
    return pairKey(pair) === pairKey({
      leftRootId: compareSelection()[0],
      rightRootId: compareSelection()[1],
    });
  });

  const treeKey = (rootId: string, path = "") => `${rootId}::${path}`;
  const rootExpandKey = (rootId: string) => `root:${rootId}`;
  const dirExpandKey = (rootId: string, path: string) => `dir:${rootId}:${path}`;

  const rememberRevealLine = (line: number | null | undefined) => {
    setRevealLine(null);
    queueMicrotask(() => setRevealLine(line ?? null));
  };

  const confirmDiscard = (): boolean => {
    if (!dirty()) return true;
    return window.confirm("Discard unsaved file changes?");
  };

  const setExpanded = (key: string, next: boolean) => {
    setExpandedKeys((current) => {
      const updated = new Set(current);
      if (next) updated.add(key);
      else updated.delete(key);
      return updated;
    });
  };

  const clearCompareMode = () => {
    setActiveComparePair(null);
    setDiffEntries([]);
    setDiffError("");
    setDiffFilter("");
    setCurrentDiff(null);
    setDiffFileError("");
    setDiffFileLoading(false);
  };

  const loadTree = async (rootId: string, path = "", force = false): Promise<void> => {
    const key = treeKey(rootId, path);
    if (!force && (treeCache()[key] || treeLoading()[key])) return;

    setTreeLoading((current) => ({ ...current, [key]: true }));
    try {
      const tree = await listWorkspaceTree(props.projectId, { rootId, path });
      setTreeCache((current) => ({ ...current, [key]: tree.entries }));
    } catch (error) {
      if (error instanceof ApiError) setRootsError(error.message);
      else setRootsError("Failed to load workspace tree");
    } finally {
      setTreeLoading((current) => ({ ...current, [key]: false }));
    }
  };

  const loadWorkspaceDiffFile = async (pair: ComparePair, path: string): Promise<void> => {
    setDiffFileLoading(true);
    setDiffFileError("");
    try {
      const diff = await getWorkspaceDiffFile(props.projectId, {
        leftRootId: pair.leftRootId,
        rightRootId: pair.rightRootId,
        path,
      });
      setCurrentDiff(diff);
    } catch (error) {
      setCurrentDiff(null);
      setDiffFileError(error instanceof Error ? error.message : "Failed to load diff file");
    } finally {
      setDiffFileLoading(false);
    }
  };

  const loadWorkspaceDiffSummary = async (
    pair: ComparePair,
    preferredPath?: string | null,
  ): Promise<void> => {
    setActiveComparePair(pair);
    setDiffLoading(true);
    setDiffError("");
    setCurrentDiff(null);
    setDiffFileError("");
    try {
      const summary = await getWorkspaceDiff(props.projectId, {
        leftRootId: pair.leftRootId,
        rightRootId: pair.rightRootId,
      });
      setDiffEntries(summary.entries);
      const initialPath = preferredPath && summary.entries.some((entry) => entry.path === preferredPath)
        ? preferredPath
        : summary.entries[0]?.path;
      if (initialPath) {
        await loadWorkspaceDiffFile(pair, initialPath);
      }
    } catch (error) {
      setDiffEntries([]);
      setCurrentDiff(null);
      setDiffError(error instanceof Error ? error.message : "Failed to load workspace diff");
    } finally {
      setDiffLoading(false);
    }
  };

  const loadRoots = async (): Promise<void> => {
    setRootsLoading(true);
    setRootsError("");
    setTreeCache({});
    setTreeLoading({});
    setSearchResults([]);
    setSearchError("");
    setSearchTruncated(false);
    setCurrentFile(null);
    setDraft("");

    try {
      const nextRoots = await listWorkspaceRoots(props.projectId);
      const validRootIds = new Set(nextRoots.map((root) => root.id));
      setRoots(nextRoots);
      setCompareSelection((current) => current.filter((rootId) => validRootIds.has(rootId)).slice(-2));

      const pair = activeComparePair();
      if (pair && (!validRootIds.has(pair.leftRootId) || !validRootIds.has(pair.rightRootId))) {
        clearCompareMode();
      }

      const preferred = nextRoots.find((root) => root.id === defaultRootId())?.id
        ?? nextRoots[0]?.id
        ?? "main";
      setSelectedNode({ kind: "root", rootId: preferred, path: "" });
      setSearchScope(SEARCH_SCOPE_ALL);
      setExpandedKeys(new Set([rootExpandKey(preferred)]));
      await loadTree(preferred, "", true);
    } catch (error) {
      setRootsError(error instanceof Error ? error.message : "Failed to load workspace roots");
      setRoots([]);
      clearCompareMode();
    } finally {
      setRootsLoading(false);
    }
  };

  const openFile = async (
    rootId: string,
    path: string,
    line?: number | null,
    force = false,
  ): Promise<void> => {
    if (!force && !confirmDiscard()) return;
    clearCompareMode();
    setSelectedNode({ kind: "file", rootId, path });
    setFileLoading(true);
    setFileError("");
    try {
      const file = await getWorkspaceFile(props.projectId, { rootId, path });
      setCurrentFile(file);
      setDraft(file.content ?? "");
      rememberRevealLine(line);
    } catch (error) {
      setCurrentFile(null);
      setDraft("");
      setFileError(error instanceof Error ? error.message : "Failed to load file");
    } finally {
      setFileLoading(false);
    }
  };

  const refreshCurrentFile = async (file: WorkspaceFile | null = currentFile()): Promise<void> => {
    if (!file) return;
    await openFile(file.rootId, file.path, revealLine(), true);
  };

  const toggleRoot = async (rootId: string): Promise<void> => {
    if (!confirmDiscard()) return;
    clearCompareMode();
    const key = rootExpandKey(rootId);
    const next = !expandedKeys().has(key);
    setExpanded(key, next);
    setSelectedNode({ kind: "root", rootId, path: "" });
    setSearchScope(rootId);
    if (next) await loadTree(rootId, "");
  };

  const toggleDirectory = async (rootId: string, path: string): Promise<void> => {
    if (!confirmDiscard()) return;
    clearCompareMode();
    const key = dirExpandKey(rootId, path);
    const next = !expandedKeys().has(key);
    setExpanded(key, next);
    setSelectedNode({ kind: "directory", rootId, path });
    if (next) await loadTree(rootId, path);
  };

  const runSearch = async (): Promise<void> => {
    const query = searchQuery().trim();
    if (!query) {
      setSearchResults([]);
      setSearchError("");
      setSearchTruncated(false);
      return;
    }
    setSearchLoading(true);
    setSearchError("");
    try {
      const response = await searchWorkspace(props.projectId, {
        query,
        rootId: searchScope() === SEARCH_SCOPE_ALL ? undefined : searchScope(),
      });
      setSearchResults(response.results);
      setSearchTruncated(response.truncated);
    } catch (error) {
      setSearchResults([]);
      setSearchTruncated(false);
      setSearchError(error instanceof Error ? error.message : "Search failed");
    } finally {
      setSearchLoading(false);
    }
  };

  const saveCurrentFile = async (): Promise<void> => {
    const file = currentFile();
    if (!file || !file.isText || file.content === null || saveBusy()) return;
    setSaveBusy(true);
    setFileError("");
    try {
      await saveWorkspaceFile(props.projectId, {
        rootId: file.rootId,
        path: file.path,
        content: draft(),
      });
      setCurrentFile({ ...file, content: draft(), truncated: false });
      await loadTree(file.rootId, dirname(file.path), true);
    } catch (error) {
      setFileError(error instanceof Error ? error.message : "Failed to save file");
    } finally {
      setSaveBusy(false);
    }
  };

  const uploadFiles = async (files: File[]): Promise<void> => {
    if (files.length === 0) return;
    clearCompareMode();
    const target = uploadTarget();
    try {
      await uploadWorkspaceFiles(props.projectId, {
        rootId: target.rootId,
        path: target.path,
        files,
      });
      await loadTree(target.rootId, target.path, true);
      if (files.length === 1) {
        const uploadedPath = joinPath(target.path, files[0].name);
        await openFile(target.rootId, uploadedPath, null, true);
      }
    } catch (error) {
      setFileError(error instanceof Error ? error.message : "Failed to upload files");
    }
  };

  const handleUpload = async (event: Event): Promise<void> => {
    const input = event.currentTarget as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    await uploadFiles(files);
    input.value = "";
  };

  const handleDrop = async (event: DragEvent): Promise<void> => {
    event.preventDefault();
    setDropHover(false);
    const files = Array.from(event.dataTransfer?.files ?? []);
    await uploadFiles(files);
  };

  const triggerDownload = () => {
    const href = currentDownloadUrl();
    if (!href) return;
    const link = document.createElement("a");
    link.href = href;
    link.download = currentFile()?.name ?? "";
    document.body.append(link);
    link.click();
    link.remove();
  };

  const toggleCompareSelection = (rootId: string) => {
    setCompareSelection((current) => {
      const withoutRoot = current.filter((item) => item !== rootId);
      if (withoutRoot.length !== current.length) {
        return withoutRoot;
      }
      const next = [...current, rootId];
      return next.length > 2 ? next.slice(next.length - 2) : next;
    });
  };

  const startCompare = async (): Promise<void> => {
    if (!canCompare()) return;
    if (!confirmDiscard()) return;
    await loadWorkspaceDiffSummary({
      leftRootId: compareSelection()[0],
      rightRootId: compareSelection()[1],
    }, currentDiff()?.path);
  };

  const refreshWorkspaceBrowser = async (): Promise<void> => {
    const previousFile = currentFile();
    const previousPair = activeComparePair();
    const previousDiffPath = currentDiff()?.path ?? null;
    await loadRoots();
    if (previousPair) {
      await loadWorkspaceDiffSummary(previousPair, previousDiffPath);
    } else {
      await refreshCurrentFile(previousFile);
    }
  };

  const diffSideUrl = (
    side: NonNullable<WorkspaceDiffFile["left"] | WorkspaceDiffFile["right"]> | null | undefined,
  ): string => {
    if (!side) return "";
    return buildWorkspaceDownloadUrl(props.projectId, {
      rootId: side.rootId,
      path: side.path,
    });
  };

  createEffect(
    on(
      () => [props.projectId, props.threadId],
      () => {
        setCompareSelection([]);
        clearCompareMode();
        void loadRoots();
      },
      { defer: false },
    ),
  );

  const renderTreeEntries = (rootId: string, path: string, depth: number) => {
    const entries = treeCache()[treeKey(rootId, path)] ?? [];
    const loading = treeLoading()[treeKey(rootId, path)];

    return (
      <>
        <For each={entries}>
          {(entry) => {
            const isDirectory = entry.kind === "directory";
            const isOpen = () => expandedKeys().has(dirExpandKey(rootId, entry.path));
            const isSelected = () => {
              const node = selectedNode();
              return node.rootId === rootId && node.path === entry.path && node.kind === entry.kind;
            };

            return (
              <div>
                <button
                  type="button"
                  class={cn(
                    "flex w-full items-center gap-1.5 py-1 pr-2 text-left text-xs transition-colors hover:bg-secondary/50",
                    isSelected() && "bg-secondary text-foreground",
                  )}
                  style={{ "padding-left": rowPadding(depth) }}
                  onClick={() => {
                    if (isDirectory) void toggleDirectory(rootId, entry.path);
                    else void openFile(rootId, entry.path);
                  }}
                  onContextMenu={(event) => {
                    if (!isDirectory) {
                      event.preventDefault();
                      setSelectedNode({ kind: "file", rootId, path: entry.path });
                      void openFile(rootId, entry.path).then(() => triggerDownload());
                    }
                  }}
                >
                  <span class="w-3 shrink-0 text-muted-foreground/60">
                    {isDirectory ? iconChevron(isOpen()) : null}
                  </span>
                  <span class="shrink-0 text-muted-foreground/60">
                    {isDirectory ? iconFolder() : iconFile()}
                  </span>
                  <span class="min-w-0 truncate">{entry.name}</span>
                </button>

                <Show when={isDirectory && isOpen()}>
                  {renderTreeEntries(rootId, entry.path, depth + 1)}
                </Show>
              </div>
            );
          }}
        </For>

        <Show when={loading}>
          <div class="px-3 py-1.5 text-[11px] text-muted-foreground">Loading…</div>
        </Show>
      </>
    );
  };

  return (
    <div
      class={cn("flex h-full min-h-0 flex-col", dropHover() && "ring-2 ring-inset ring-signal-blue/40")}
      onDragOver={(event) => { event.preventDefault(); setDropHover(true); }}
      onDragLeave={() => setDropHover(false)}
      onDrop={(event) => void handleDrop(event)}
    >
      <div class="flex items-center gap-1.5 border-b border-border bg-card px-2 py-1.5">
        <div class="relative min-w-0 flex-1">
          <span class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground/60">
            {iconSearch()}
          </span>
          <input
            type="text"
            class="h-7 w-full bg-transparent pl-8 pr-2 text-xs text-foreground outline-none placeholder:text-muted-foreground/50"
            placeholder="Search workspace…"
            value={searchQuery()}
            onInput={(event) => setSearchQuery(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                void runSearch();
              }
              if (event.key === "Escape") {
                setSearchQuery("");
                setSearchResults([]);
                setSearchError("");
              }
            }}
          />
        </div>

        <Show when={compareSelection().length > 0}>
          <div class="hidden items-center gap-1 sm:flex">
            <For each={compareSelection()}>
              {(rootId, index) => (
                <span class="flex h-7 items-center gap-1 border border-border/60 px-2 text-[10px] uppercase tracking-[0.12em] text-muted-foreground">
                  <span class="text-foreground/70">{index() + 1}</span>
                  <span>{rootLabelById(rootMap(), rootId)}</span>
                </span>
              )}
            </For>
          </div>
        </Show>

        <Show when={canCompare()}>
          <button
            type="button"
            class="flex h-7 items-center gap-1.5 bg-secondary px-2.5 text-[11px] font-medium text-foreground transition-colors hover:bg-secondary/80 disabled:opacity-50"
            onClick={() => void startCompare()}
            disabled={sameActiveCompareSelection() && !diffError()}
          >
            {sameActiveCompareSelection() ? "Viewing Diff" : "Show Diff"}
          </button>
        </Show>

        <Show when={activeComparePair()}>
          <button
            type="button"
            class="flex h-7 items-center gap-1.5 px-2.5 text-[11px] text-muted-foreground transition-colors hover:text-foreground"
            onClick={clearCompareMode}
          >
            Close
          </button>
        </Show>

        <Show when={dirty()}>
          <button
            type="button"
            class="flex h-7 items-center gap-1.5 bg-foreground px-2.5 text-[11px] font-medium text-background transition-colors hover:bg-foreground/90 disabled:opacity-50"
            onClick={() => void saveCurrentFile()}
            disabled={saveBusy()}
          >
            {saveBusy() ? "…" : "Save"}
          </button>
        </Show>

        <Show when={!activeComparePair() && !isReadOnly()}>
          <button
            type="button"
            class="flex h-7 items-center gap-1.5 bg-secondary px-2.5 text-[11px] font-medium text-foreground transition-colors hover:bg-secondary/80"
            onClick={() => uploadInputRef?.click()}
          >
            Upload
          </button>
        </Show>

        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center text-muted-foreground/60 transition-colors hover:text-foreground"
          onClick={() => void refreshWorkspaceBrowser()}
          title="Refresh"
        >
          <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M21 12a9 9 0 1 1-2.64-6.36" />
            <path d="M21 3v6h-6" />
          </svg>
        </button>
      </div>

      <input
        ref={uploadInputRef}
        type="file"
        class="hidden"
        multiple
        onChange={(event) => void handleUpload(event)}
      />

      <div class="flex min-h-0 flex-1">
        <aside class="flex w-[220px] shrink-0 flex-col bg-card/60">
          <Show when={searchResults().length > 0 || searchLoading() || searchError()}>
            <div class="border-b border-border/60">
              <div class="flex items-center justify-between px-2.5 py-1.5">
                <span class="text-[10px] font-medium uppercase tracking-[0.12em] text-muted-foreground">
                  Results
                </span>
                <button
                  type="button"
                  class="text-[10px] text-muted-foreground/60 transition-colors hover:text-foreground"
                  onClick={() => {
                    setSearchResults([]);
                    setSearchError("");
                    setSearchTruncated(false);
                  }}
                >
                  Clear
                </button>
              </div>
              <div class="max-h-[200px] overflow-y-auto">
                <Show when={searchLoading()}>
                  <div class="px-2.5 py-2 text-[11px] text-muted-foreground">Searching…</div>
                </Show>
                <Show when={searchError()}>
                  <div class="px-2.5 py-2 text-[11px] text-signal-red">{searchError()}</div>
                </Show>
                <For each={searchResults()}>
                  {(result) => (
                    <button
                      type="button"
                      class="block w-full border-t border-border/40 px-2.5 py-1.5 text-left transition-colors hover:bg-secondary/50"
                      onClick={() => void openFile(result.rootId, result.path, result.line)}
                    >
                      <div class="truncate font-mono text-[10px] text-muted-foreground">
                        {result.path}:{result.line}
                      </div>
                      <div class="mt-0.5 truncate text-[11px] text-foreground/80">{result.preview}</div>
                    </button>
                  )}
                </For>
                <Show when={searchTruncated()}>
                  <div class="border-t border-border/40 px-2.5 py-1.5 text-[10px] text-muted-foreground">
                    Truncated.
                  </div>
                </Show>
              </div>
            </div>
          </Show>

          <div class="flex-1 overflow-y-auto chassis-scroll">
            <Show
              when={!rootsLoading()}
              fallback={<div class="px-2.5 py-3 text-[11px] text-muted-foreground">Loading…</div>}
            >
              <Show
                when={!rootsError()}
                fallback={<div class="px-2.5 py-3 text-[11px] text-signal-red">{rootsError()}</div>}
              >
                <For each={roots()}>
                  {(root) => {
                    const expanded = () => expandedKeys().has(rootExpandKey(root.id));
                    const active = () => selectedRootId() === root.id && selectedNode().path === "";
                    const compareOrder = () => compareSelectionIndex().get(root.id);
                    const isThread = root.kind === "thread";
                    const isRemote = root.kind === "remote";

                    return (
                      <div class={cn(isThread && "ml-2 border-l border-border/40")}>
                        <button
                          type="button"
                          class={cn(
                            "flex w-full items-center gap-1.5 py-1.5 text-left transition-colors hover:bg-secondary/50",
                            isThread ? "px-2" : "px-2.5",
                            active() && "bg-secondary",
                            compareOrder() && "bg-signal-blue/6 ring-1 ring-inset ring-signal-blue/20",
                          )}
                          onClick={(event) => {
                            if (event.shiftKey) {
                              event.preventDefault();
                              toggleCompareSelection(root.id);
                              return;
                            }
                            void toggleRoot(root.id);
                          }}
                        >
                          <span class="text-muted-foreground/60">{iconChevron(expanded())}</span>
                          <Show when={isRemote} fallback={
                            <span class={cn("h-1.5 w-1.5 rounded-full shrink-0", statusDotClass(root.status))} />
                          }>
                            <span class="shrink-0 text-signal-blue/70">{iconBranch()}</span>
                          </Show>
                          <div class="min-w-0 flex-1">
                            <span class={cn(
                              "block truncate text-xs",
                              isRemote ? "text-signal-blue/70" : isThread ? "text-muted-foreground" : "font-medium",
                            )}>
                              {rootLabel(root)}
                            </span>
                            <Show when={root.summary}>
                              <span class={cn(
                                "block truncate text-[10px]",
                                isRemote ? "font-mono text-signal-blue/40" : "text-muted-foreground/50",
                              )}>
                                {root.summary}
                              </span>
                            </Show>
                          </div>
                          <Show when={compareOrder()}>
                            <span class="flex h-4 min-w-4 items-center justify-center rounded-full bg-signal-blue/12 px-1 text-[10px] font-medium text-signal-blue">
                              {compareOrder()}
                            </span>
                          </Show>
                        </button>
                        <Show when={expanded()}>
                          {renderTreeEntries(root.id, "", 1)}
                        </Show>
                      </div>
                    );
                  }}
                </For>
              </Show>
            </Show>
          </div>
        </aside>

        <section class="flex min-w-0 flex-1 flex-col bg-background">
          <Show
            when={activeComparePair()}
            fallback={
              <>
                <Show when={currentFile()}>
                  {(file) => (
                    <div class="flex items-center gap-2 border-b border-border/60 bg-card/60 px-3 py-1.5">
                      <span class="min-w-0 truncate font-mono text-[11px] text-foreground/80">
                        {file().path}
                      </span>
                      <span class="shrink-0 text-[10px] text-muted-foreground/60">
                        {file().size.toLocaleString()}b
                      </span>
                      <Show when={dirty()}>
                        <span class="shrink-0 text-[10px] text-signal-amber">modified</span>
                      </Show>
                    </div>
                  )}
                </Show>

                <div class="min-h-0 flex-1">
                  <Show
                    when={!fileLoading()}
                    fallback={<div class="flex h-full items-center justify-center text-xs text-muted-foreground">Loading…</div>}
                  >
                    <Show
                      when={!fileError()}
                      fallback={<div class="flex h-full items-center justify-center px-6 text-xs text-signal-red">{fileError()}</div>}
                    >
                      <Show
                        when={currentFile()}
                        fallback={
                          <div class="flex h-full items-center justify-center px-6 text-center text-xs text-muted-foreground/60">
                            Select a file, or shift-click two workspaces and use Show Diff.
                          </div>
                        }
                      >
                        {(file) => (
                          <Show
                            when={file().isText && file().content !== null}
                            fallback={
                              <Show
                                when={isImageMime(file().mime)}
                                fallback={
                                  <div class="flex h-full items-center justify-center p-6">
                                    <div class="max-w-sm space-y-2 text-center">
                                      <p class="text-sm text-muted-foreground">
                                        {file().isText
                                          ? "File too large to preview."
                                          : "Binary file."}
                                      </p>
                                      <button
                                        type="button"
                                        class="text-xs text-foreground/60 underline underline-offset-4 transition-colors hover:text-foreground"
                                        onClick={triggerDownload}
                                      >
                                        Download
                                      </button>
                                    </div>
                                  </div>
                                }
                              >
                                <div class="flex h-full items-center justify-center bg-background p-4">
                                  <img
                                    src={currentDownloadUrl()}
                                    alt={file().name}
                                    class="max-h-full max-w-full"
                                  />
                                </div>
                              </Show>
                            }
                          >
                            <MonacoTextEditor
                              path={file().path}
                              value={draft()}
                              revealLine={revealLine()}
                              onChange={setDraft}
                            />
                          </Show>
                        )}
                      </Show>
                    </Show>
                  </Show>
                </div>
              </>
            }
          >
            {(pairAccessor) => {
              const compareRoots = activeCompareRoots();
              const leftRoot = () => compareRoots?.left;
              const rightRoot = () => compareRoots?.right;

              return (
                <>
                  <div class="flex items-center gap-2 border-b border-border/60 bg-card/60 px-3 py-1.5">
                    <span class="text-[11px] font-medium text-foreground/80">
                      {rootLabelById(rootMap(), pairAccessor().leftRootId)}
                    </span>
                    <span class="text-[10px] text-muted-foreground/60">vs</span>
                    <span class="text-[11px] font-medium text-foreground/80">
                      {rootLabelById(rootMap(), pairAccessor().rightRootId)}
                    </span>
                    <span class="shrink-0 text-[10px] text-muted-foreground/60">
                      {diffEntries().length} changed
                    </span>
                    <Show when={diffEntries().length > 0}>
                      <div class="ml-auto flex items-center gap-1.5 text-[10px]">
                        <span class="text-signal-blue">{diffCounts().modified} modified</span>
                        <span class="text-signal-green">{diffCounts().added} added</span>
                        <span class="text-signal-red">{diffCounts().deleted} deleted</span>
                      </div>
                    </Show>
                  </div>

                  <div class="flex min-h-0 flex-1">
                    <aside class="flex w-[280px] shrink-0 flex-col border-r border-border/60 bg-card/30">
                      <div class="border-b border-border/60 px-2.5 py-2">
                        <input
                          type="text"
                          class="h-8 w-full bg-transparent text-xs text-foreground outline-none placeholder:text-muted-foreground/50"
                          placeholder="Filter changed files…"
                          value={diffFilter()}
                          onInput={(event) => setDiffFilter(event.currentTarget.value)}
                        />
                      </div>

                      <div class="flex-1 overflow-y-auto chassis-scroll">
                        <Show when={diffLoading()}>
                          <div class="px-3 py-3 text-[11px] text-muted-foreground">Loading diff…</div>
                        </Show>
                        <Show when={diffError()}>
                          <div class="px-3 py-3 text-[11px] text-signal-red">{diffError()}</div>
                        </Show>
                        <Show when={!diffLoading() && !diffError() && filteredDiffEntries().length === 0}>
                          <div class="px-3 py-3 text-[11px] text-muted-foreground">
                            {diffEntries().length === 0
                              ? "These workspaces are identical."
                              : "No changed files match the current filter."}
                          </div>
                        </Show>
                        <For each={filteredDiffEntries()}>
                          {(entry) => (
                            <button
                              type="button"
                              class={cn(
                                "block w-full border-b border-border/40 px-3 py-2 text-left transition-colors hover:bg-secondary/40",
                                currentDiff()?.path === entry.path && "bg-secondary",
                              )}
                              onClick={() => void loadWorkspaceDiffFile(pairAccessor(), entry.path)}
                            >
                              <div class="flex items-center gap-2">
                                <span class={cn(
                                  "rounded px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-[0.12em]",
                                  diffStatusClass(entry.status),
                                )}>
                                  {diffStatusLabel(entry.status)}
                                </span>
                                <span class="min-w-0 truncate font-mono text-[11px] text-foreground/85">
                                  {entry.path}
                                </span>
                              </div>
                              <div class="mt-1 text-[10px] text-muted-foreground/60">
                                {entry.leftSize != null ? `${entry.leftSize.toLocaleString()}b` : "missing"}
                                {" → "}
                                {entry.rightSize != null ? `${entry.rightSize.toLocaleString()}b` : "missing"}
                              </div>
                            </button>
                          )}
                        </For>
                      </div>
                    </aside>

                    <div class="flex min-w-0 flex-1 flex-col">
                      <Show when={currentDiff()}>
                        {(diffAccessor) => (
                          <>
                            <div class="flex items-center gap-2 border-b border-border/60 bg-card/30 px-3 py-1.5">
                              <span class={cn(
                                "rounded px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-[0.12em]",
                                diffStatusClass(diffAccessor().status),
                              )}>
                                {diffStatusLabel(diffAccessor().status)}
                              </span>
                              <span class="min-w-0 truncate font-mono text-[11px] text-foreground/80">
                                {diffAccessor().path}
                              </span>
                              <Show when={diffAccessor().additions != null && diffAccessor().deletions != null}>
                                <div class="ml-auto flex items-center gap-2 text-[10px]">
                                  <span class="text-signal-green">+{diffAccessor().additions}</span>
                                  <span class="text-signal-red">-{diffAccessor().deletions}</span>
                                </div>
                              </Show>
                            </div>

                            <div class="min-h-0 flex-1">
                              <Show
                                when={!diffFileLoading()}
                                fallback={<div class="flex h-full items-center justify-center text-xs text-muted-foreground">Loading diff…</div>}
                              >
                                <Show
                                  when={!diffFileError()}
                                  fallback={<div class="flex h-full items-center justify-center px-6 text-xs text-signal-red">{diffFileError()}</div>}
                                >
                                  <Show
                                    when={
                                      diffAccessor().isText
                                      && !(diffAccessor().left?.truncated ?? false)
                                      && !(diffAccessor().right?.truncated ?? false)
                                    }
                                    fallback={
                                      <div class="flex h-full flex-col">
                                        <Show
                                          when={isImageMime(diffAccessor().left?.mime) || isImageMime(diffAccessor().right?.mime)}
                                          fallback={
                                            <div class="flex h-full items-center justify-center p-6">
                                              <div class="max-w-md space-y-3 text-center">
                                                <p class="text-sm text-muted-foreground">
                                                  {diffAccessor().isText
                                                    ? "This text file is too large to render in the diff editor."
                                                    : "Binary diff preview is not available."}
                                                </p>
                                                <div class="flex items-center justify-center gap-3 text-xs">
                                                  <Show when={diffAccessor().left}>
                                                    <a
                                                      href={diffSideUrl(diffAccessor().left)}
                                                      class="text-foreground/60 underline underline-offset-4 transition-colors hover:text-foreground"
                                                    >
                                                      Download left
                                                    </a>
                                                  </Show>
                                                  <Show when={diffAccessor().right}>
                                                    <a
                                                      href={diffSideUrl(diffAccessor().right)}
                                                      class="text-foreground/60 underline underline-offset-4 transition-colors hover:text-foreground"
                                                    >
                                                      Download right
                                                    </a>
                                                  </Show>
                                                </div>
                                              </div>
                                            </div>
                                          }
                                        >
                                          <div class="grid h-full min-h-0 grid-cols-2 gap-px bg-border/60">
                                            <For each={[diffAccessor().left, diffAccessor().right]}>
                                              {(side, index) => {
                                                const label = () => index() === 0 ? leftRoot()?.label ?? pairAccessor().leftRootId : rightRoot()?.label ?? pairAccessor().rightRootId;
                                                return (
                                                  <div class="flex min-h-0 flex-col bg-background">
                                                    <div class="flex items-center justify-between border-b border-border/60 px-3 py-1.5">
                                                      <span class="text-[11px] font-medium text-foreground/80">{label()}</span>
                                                      <Show when={side}>
                                                        <a
                                                          href={diffSideUrl(side)}
                                                          class="text-[10px] text-muted-foreground/70 underline underline-offset-4 transition-colors hover:text-foreground"
                                                        >
                                                          Download
                                                        </a>
                                                      </Show>
                                                    </div>
                                                    <div class="flex min-h-0 flex-1 items-center justify-center p-4">
                                                      <Show
                                                        when={side}
                                                        fallback={<div class="text-xs text-muted-foreground/60">Missing</div>}
                                                      >
                                                        <img
                                                          src={diffSideUrl(side)}
                                                          alt={side?.name ?? "Image"}
                                                          class="max-h-full max-w-full"
                                                        />
                                                      </Show>
                                                    </div>
                                                  </div>
                                                );
                                              }}
                                            </For>
                                          </div>
                                        </Show>
                                      </div>
                                    }
                                  >
                                    <MonacoDiffEditor
                                      path={diffAccessor().path}
                                      originalValue={diffAccessor().left?.content ?? ""}
                                      modifiedValue={diffAccessor().right?.content ?? ""}
                                    />
                                  </Show>
                                </Show>
                              </Show>
                            </div>
                          </>
                        )}
                      </Show>

                      <Show when={!currentDiff() && !diffLoading() && !diffError()}>
                        <div class="flex h-full items-center justify-center px-6 text-center text-xs text-muted-foreground/60">
                          Pick a changed file to inspect.
                        </div>
                      </Show>
                    </div>
                  </div>
                </>
              );
            }}
          </Show>
        </section>
      </div>
    </div>
  );
};

export default WorkspaceBrowser;
