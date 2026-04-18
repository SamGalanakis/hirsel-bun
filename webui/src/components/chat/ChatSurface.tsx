import { type Component, type JSX, Show } from "solid-js";
import ChatComposer from "@/components/ChatComposer";
import ChatTranscript, {
  type ChatRuntimeBanner,
} from "@/components/chat/ChatTranscript";
import type { ChatMessage as ApiChatMessage, LiveTurn } from "@/lib/api";
import { cn } from "@/lib/cn";

export type ChatSurfaceVariant = "main" | "overlay" | "panel";
export type ChatSurfaceScope = "root" | "thread";

export interface ChatSurfaceProps {
  scope: ChatSurfaceScope;
  projectId: number;
  threadId?: string;
  title: string;

  // transcript
  loaded: boolean;
  messages: ApiChatMessage[];
  liveTurn: LiveTurn | null;
  runtimeError: ChatRuntimeBanner | null;
  stickToBottom: boolean;

  // composer
  inputValue: string;
  running: boolean;
  justStopped: boolean;
  composerFocusNonce: number;

  // callbacks
  onInputChange: (value: string) => void;
  onSubmit: () => void | Promise<void>;
  onStop: () => void | Promise<void>;
  onTranscriptRef?: (el: HTMLDivElement) => void;
  onScroll: () => void;
  onScrollToBottom: () => void;
  onOpenSettings: () => void;
  onDismissRuntimeError: (raw: string) => void;
  onSuggestion?: (text: string) => void;

  // presentation
  variant?: ChatSurfaceVariant;
  header?: JSX.Element;
  composerMaxWidth?: string;
  class?: string;
}

/**
 * Unified chat surface used by every chat in the app — project-root
 * shepherd and thread focus overlay. Owns only layout and a consistent
 * composer wrapper; all data flows in as props so callers remain the
 * single source of truth.
 *
 * Variants change only spacing and chrome, never behavior:
 *  - `main`    — fills the main pane
 *  - `overlay` — drops into a focus overlay body (thread focus)
 *  - `panel`   — right-side dockable pane (shepherd companion)
 */
const ChatSurface: Component<ChatSurfaceProps> = (props) => {
  return (
    <div
      class={cn(
        "chat-surface",
        `chat-surface-${props.variant ?? "main"}`,
        props.class,
      )}
    >
      <Show when={props.header}>{props.header}</Show>
      <div class="chat-surface-body">
        <ChatTranscript
          title={props.title}
          threadId={props.threadId}
          loaded={props.loaded}
          messages={props.messages}
          liveTurn={props.liveTurn}
          runtimeError={props.runtimeError}
          stickToBottom={props.stickToBottom}
          onTranscriptRef={props.onTranscriptRef}
          onScroll={props.onScroll}
          onScrollToBottom={props.onScrollToBottom}
          onOpenSettings={props.onOpenSettings}
          onDismissRuntimeError={props.onDismissRuntimeError}
          onSuggestion={props.onSuggestion}
        />
      </div>
      <div class="chat-surface-composer">
        <div
          class="chat-surface-composer-inner"
          style={
            props.composerMaxWidth
              ? { "max-width": props.composerMaxWidth }
              : undefined
          }
        >
          <ChatComposer
            projectId={props.projectId}
            threadId={props.threadId}
            value={props.inputValue}
            running={props.running}
            justStopped={props.justStopped}
            focusNonce={props.composerFocusNonce}
            onValueChange={props.onInputChange}
            onSubmit={props.onSubmit}
            onStop={props.onStop}
          />
        </div>
      </div>
    </div>
  );
};

export default ChatSurface;
