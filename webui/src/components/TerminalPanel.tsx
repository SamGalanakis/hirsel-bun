import { type Component, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { cn } from "@/lib/cn";
import "@xterm/xterm/css/xterm.css";

export type TerminalScope = "host" | "shepherd" | { thread: string; title: string };

interface TerminalPanelProps {
  projectId: number;
  threads: Array<{ id: string; title: string }>;
  onClose: () => void;
}

function scopeLabel(scope: TerminalScope): string {
  if (scope === "host") return "Host";
  if (scope === "shepherd") return "Shepherd";
  return scope.title || "Thread";
}

function scopeId(scope: TerminalScope): string {
  if (typeof scope === "string") return scope;
  return `thread:${scope.thread}`;
}

function getTerminalTheme(): Record<string, string> {
  const style = getComputedStyle(document.documentElement);
  const resolve = (varName: string, fallback: string): string => {
    const raw = style.getPropertyValue(varName).trim();
    if (!raw) return fallback;
    return `hsl(${raw})`;
  };
  return {
    background: resolve("--background", "#111114"),
    foreground: resolve("--foreground", "#c8c8c8"),
    cursor: resolve("--foreground", "#c8c8c8"),
    selectionBackground: resolve("--signal-amber", "#d4a843") + "40",
  };
}

const TerminalPanel: Component<TerminalPanelProps> = (props) => {
  const [scope, setScope] = createSignal<TerminalScope>("shepherd");
  const [connected, setConnected] = createSignal(false);
  const [error, setError] = createSignal("");
  let containerRef: HTMLDivElement | undefined;
  let terminal: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let ws: WebSocket | null = null;
  let resizeObserver: ResizeObserver | undefined;

  const connect = (targetScope: TerminalScope) => {
    // Clean up previous
    ws?.close();
    terminal?.clear();
    setConnected(false);
    setError("");

    const scopeParam = scopeId(targetScope);
    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const url = `${protocol}//${window.location.host}/api/projects/${props.projectId}/terminal?scope=${encodeURIComponent(scopeParam)}`;

    const socket = new WebSocket(url);
    ws = socket;

    socket.onopen = () => {
      setConnected(true);
      setError("");
      // Send initial resize
      if (terminal) {
        socket.send(JSON.stringify({ type: "resize", cols: terminal.cols, rows: terminal.rows }));
      }
    };

    socket.onmessage = (event) => {
      if (terminal && typeof event.data === "string") {
        try {
          const msg = JSON.parse(event.data);
          if (msg.type === "output" && msg.data) {
            terminal.write(msg.data);
          } else if (msg.type === "error") {
            setError(msg.message || "Terminal error");
          }
        } catch {
          // Raw text fallback
          terminal.write(event.data);
        }
      }
    };

    socket.onclose = () => {
      setConnected(false);
    };

    socket.onerror = () => {
      setError("Connection failed");
      setConnected(false);
    };
  };

  const switchScope = (next: TerminalScope) => {
    setScope(next);
    connect(next);
  };

  onMount(() => {
    if (!containerRef) return;

    const theme = getTerminalTheme();
    terminal = new Terminal({
      fontFamily: "Martian Mono, ui-monospace, monospace",
      fontSize: 12,
      lineHeight: 1.4,
      cursorBlink: true,
      cursorStyle: "bar",
      theme,
      scrollback: 5000,
      allowProposedApi: true,
    });

    fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(containerRef);

    // Handle terminal input → send to WebSocket
    terminal.onData((data) => {
      if (ws?.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: "input", data }));
      }
    });

    // Fit on resize
    resizeObserver = new ResizeObserver(() => {
      fitAddon?.fit();
      if (terminal && ws?.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: "resize", cols: terminal.cols, rows: terminal.rows }));
      }
    });
    resizeObserver.observe(containerRef);

    // Initial fit
    requestAnimationFrame(() => fitAddon?.fit());

    // Connect to default scope
    connect(scope());
  });

  onCleanup(() => {
    ws?.close();
    resizeObserver?.disconnect();
    terminal?.dispose();
  });

  return (
    <div class="flex h-full flex-col bg-background">
      {/* Terminal header */}
      <div class="flex h-7 shrink-0 items-center gap-1 px-1">
        <button
          type="button"
          class={cn(
            "px-2 py-0.5 text-[11px] transition-colors",
            scope() === "host" ? "text-foreground" : "text-muted-foreground/40 hover:text-foreground",
          )}
          onClick={() => switchScope("host")}
        >
          Host
        </button>
        <button
          type="button"
          class={cn(
            "px-2 py-0.5 text-[11px] transition-colors",
            scope() === "shepherd" ? "text-foreground" : "text-muted-foreground/40 hover:text-foreground",
          )}
          onClick={() => switchScope("shepherd")}
        >
          Shepherd
        </button>
        <For each={props.threads}>
          {(thread) => (
            <button
              type="button"
              class={cn(
                "max-w-[120px] truncate px-2 py-0.5 text-[11px] transition-colors",
                typeof scope() === "object" && scope() !== null && (scope() as { thread: string }).thread === thread.id
                  ? "text-foreground"
                  : "text-muted-foreground/40 hover:text-foreground",
              )}
              onClick={() => switchScope({ thread: thread.id, title: thread.title })}
            >
              {thread.title}
            </button>
          )}
        </For>

        <div class="ml-auto flex items-center gap-1">
          <span class={cn(
            "h-1.5 w-1.5 rounded-full",
            connected() ? "bg-signal-green" : "bg-muted-foreground/30",
          )} />
          <Show when={error()}>
            <span class="text-[10px] text-signal-red">{error()}</span>
          </Show>
          <button
            type="button"
            class="flex h-5 w-5 items-center justify-center text-muted-foreground/30 transition-colors hover:text-foreground"
            onClick={props.onClose}
          >
            <svg viewBox="0 0 24 24" class="h-2.5 w-2.5" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M6 6l12 12" />
              <path d="M18 6L6 18" />
            </svg>
          </button>
        </div>
      </div>

      {/* Terminal container */}
      <div ref={containerRef} class="min-h-0 flex-1 px-1 pb-1" />
    </div>
  );
};

export default TerminalPanel;
