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

// Resolve a CSS token (OKLCH, HSL, RGB, or named) to a concrete rgb() string
// via a 1×1 canvas, which forces coercion through sRGB regardless of input space.
function resolveTokenToRgb(tokenName: string, fallback: string): string {
  try {
    const el = document.createElement("div");
    el.style.background = `var(${tokenName})`;
    document.body.appendChild(el);
    const computed = getComputedStyle(el).backgroundColor;
    el.remove();

    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 1;
    const ctx = canvas.getContext("2d");
    if (!ctx) return fallback;
    ctx.fillStyle = computed;
    ctx.fillRect(0, 0, 1, 1);
    const [r, g, b] = ctx.getImageData(0, 0, 1, 1).data;
    return `rgb(${r}, ${g}, ${b})`;
  } catch {
    return fallback;
  }
}

function getTerminalTheme(): Record<string, string> {
  const bg = resolveTokenToRgb("--color-background", "rgb(26, 25, 20)");
  const fg = resolveTokenToRgb("--color-foreground", "rgb(218, 210, 192)");
  const amberRgb = resolveTokenToRgb("--color-signal-amber", "rgb(201, 165, 84)");
  // Selection uses amber with alpha — extract numbers and wrap in rgba
  const match = amberRgb.match(/\d+/g);
  const selection = match
    ? `rgba(${match[0]}, ${match[1]}, ${match[2]}, 0.28)`
    : "rgba(201, 165, 84, 0.28)";
  return {
    background: bg,
    foreground: fg,
    cursor: fg,
    selectionBackground: selection,
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
      fontFamily: "Red Hat Mono, ui-monospace, monospace",
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
      <div class="flex h-8 shrink-0 items-center gap-0.5 border-b border-border/60 px-1.5">
        <button
          type="button"
          class={cn(
            "relative px-2.5 py-1 text-[11px] font-medium transition-colors",
            scope() === "host"
              ? "text-foreground after:absolute after:inset-x-1.5 after:bottom-[-1px] after:h-[1.5px] after:bg-foreground"
              : "text-muted-foreground hover:text-foreground",
          )}
          onClick={() => switchScope("host")}
        >
          Host
        </button>
        <button
          type="button"
          class={cn(
            "relative px-2.5 py-1 text-[11px] font-medium transition-colors",
            scope() === "shepherd"
              ? "text-foreground after:absolute after:inset-x-1.5 after:bottom-[-1px] after:h-[1.5px] after:bg-foreground"
              : "text-muted-foreground hover:text-foreground",
          )}
          onClick={() => switchScope("shepherd")}
        >
          Shepherd
        </button>
        <For each={props.threads}>
          {(thread) => {
            const isActive = () =>
              typeof scope() === "object" && scope() !== null && (scope() as { thread: string }).thread === thread.id;
            return (
              <button
                type="button"
                class={cn(
                  "relative max-w-[120px] truncate px-2.5 py-1 text-[11px] font-medium transition-colors",
                  isActive()
                    ? "text-foreground after:absolute after:inset-x-1.5 after:bottom-[-1px] after:h-[1.5px] after:bg-foreground"
                    : "text-muted-foreground hover:text-foreground",
                )}
                onClick={() => switchScope({ thread: thread.id, title: thread.title })}
              >
                {thread.title}
              </button>
            );
          }}
        </For>

        <div class="ml-auto flex items-center gap-2">
          <div class="flex items-center gap-1.5">
            <span class={cn(
              "h-2 w-2 rounded-full transition-colors",
              connected() ? "bg-signal-green" : "bg-muted-foreground/30",
            )} />
            <Show when={error()}>
              <span class="text-[10px] text-signal-red">{error()}</span>
            </Show>
          </div>
          <button
            type="button"
            class="flex h-6 w-6 items-center justify-center text-muted-foreground/40 transition-colors hover:text-foreground"
            onClick={props.onClose}
            aria-label="Close terminal"
          >
            <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M6 6l12 12" />
              <path d="M18 6L6 18" />
            </svg>
          </button>
        </div>
      </div>

      {/* Terminal container */}
      <div ref={containerRef} class="min-h-0 flex-1 px-1.5 pb-1.5" />
    </div>
  );
};

export default TerminalPanel;
