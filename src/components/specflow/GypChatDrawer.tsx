/**
 * GypChatDrawer - Collapsible chat panel in the footer
 *
 * Like Facebook Messenger - minimizes to a bar in the footer,
 * expands to a chat panel above it.
 */
import { type Component, For, Show, createEffect, createSignal } from 'solid-js';
import type { ChatMessage } from '../../lib/types';

interface GypChatDrawerProps {
  projectId: number;
  focusNodeId: string | null;
  focusNodeName: string | null;
  messages: ChatMessage[];
  currentMessage: () => Partial<ChatMessage> | null;
  connected: () => boolean;
  connecting: () => boolean;
  gypEditing: () => boolean;
  onSend: (content: string, nodeId?: string, nodeName?: string) => Promise<void>;
  onConnect: () => Promise<void>;
  onFocusNode: (nodeId: string | null, nodeName: string | null) => void;
}

export const GypChatDrawer: Component<GypChatDrawerProps> = (props) => {
  let inputRef: HTMLTextAreaElement | undefined;
  let messagesRef: HTMLDivElement | undefined;

  const [inputText, setInputText] = createSignal('');
  const [sending, setSending] = createSignal(false);
  const [expanded, setExpanded] = createSignal(false);

  const PANEL_WIDTH = 400;
  const PANEL_HEIGHT = 440;
  const BAR_WIDTH = 200;
  const BAR_HEIGHT = 36;

  // Handle send
  const handleSend = async () => {
    const text = inputText().trim();
    if (!text || sending()) return;

    setSending(true);
    setInputText('');
    try {
      await props.onSend(text, props.focusNodeId ?? undefined, props.focusNodeName ?? undefined);
    } finally {
      setSending(false);
    }
  };

  // Handle key press
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      setExpanded(false);
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  // Scroll to bottom when messages change
  createEffect(() => {
    props.messages.length;
    props.currentMessage();
    if (messagesRef) {
      messagesRef.scrollTop = messagesRef.scrollHeight;
    }
  });

  // Focus input when expanded
  createEffect(() => {
    if (expanded()) {
      setTimeout(() => inputRef?.focus(), 100);
    }
  });

  // Auto-connect if not connected when expanded
  createEffect(() => {
    if (expanded() && !props.connected() && !props.connecting()) {
      props.onConnect();
    }
  });

  const recentMessages = () => props.messages.slice(-10);

  return (
    <div
      class="fixed right-0 bottom-0 flex flex-col items-end"
      style={{ 'z-index': 1000 }}
    >
      {/* Chat Panel (shown when expanded) */}
      <Show when={expanded()}>
        <div
          class="rounded-t-xl overflow-hidden flex flex-col mr-4"
          classList={{
            'ring-2 ring-amber-500/50': props.gypEditing(),
          }}
          style={{
            width: `${PANEL_WIDTH}px`,
            height: `${PANEL_HEIGHT}px`,
            background: 'rgba(24,24,27,0.98)',
            border: props.gypEditing()
              ? '1px solid rgba(212,165,116,0.5)'
              : '1px solid rgba(63,63,70,0.8)',
            'border-bottom': 'none',
            'box-shadow': '0 -8px 32px rgba(0,0,0,0.4)',
            'backdrop-filter': 'blur(12px)',
          }}
          onKeyDown={handleKeyDown}
        >
          {/* Messages */}
          <div
            ref={messagesRef}
            class="flex-1 overflow-y-auto px-4 py-3 space-y-3"
            style={{ background: 'rgba(15,15,15,0.5)' }}
          >
            <Show when={!props.connected() && !props.connecting()}>
              <div class="text-center text-xs text-zinc-600 py-8 italic">
                Click to connect...
              </div>
            </Show>

            <Show when={props.connecting()}>
              <div class="text-center text-xs text-zinc-500 py-8 flex items-center justify-center gap-2">
                <div class="w-4 h-4 border-2 border-zinc-600 border-t-amber-500 rounded-full animate-spin" />
                Connecting...
              </div>
            </Show>

            <Show when={props.connected()}>
              <Show when={recentMessages().length === 0 && !props.currentMessage()}>
                <div class="text-center py-8">
                  <img src="/gyp.svg" alt="" class="w-12 h-12 mx-auto mb-3 opacity-30" />
                  <p class="text-sm text-zinc-500 mb-1">Ask Gyp anything</p>
                  <p class="text-xs text-zinc-600 italic">
                    "Add error handling subtasks"<br />
                    "Break this down into smaller pieces"
                  </p>
                </div>
              </Show>

              <For each={recentMessages()}>
                {(msg) => (
                  <div
                    class="text-sm rounded-lg px-3 py-2.5"
                    classList={{
                      'bg-amber-500/15 text-zinc-200 ml-8 border border-amber-500/25': msg.role === 'user',
                      'bg-zinc-800/80 text-zinc-300 mr-8 border border-zinc-700/50': msg.role === 'assistant',
                    }}
                  >
                    <Show when={msg.role === 'assistant' && msg.toolCalls?.length}>
                      <div class="flex items-center gap-1.5 text-[10px] text-zinc-500 mb-1.5">
                        <svg class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                          <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z" />
                        </svg>
                        {msg.toolCalls?.length} tool{msg.toolCalls?.length !== 1 ? 's' : ''} used
                      </div>
                    </Show>
                    <div class="whitespace-pre-wrap break-words">
                      {msg.content}
                    </div>
                  </div>
                )}
              </For>

              {/* Streaming message */}
              <Show when={props.currentMessage()}>
                <div class="text-sm rounded-lg px-3 py-2.5 bg-zinc-800/80 text-zinc-300 mr-8 border border-zinc-700/50">
                  <Show when={props.gypEditing()}>
                    <div class="flex items-center gap-2 text-[10px] text-amber-400 mb-1.5">
                      <div class="w-2.5 h-2.5 border-2 border-amber-400 border-t-transparent rounded-full animate-spin" />
                      Editing board...
                    </div>
                  </Show>
                  <Show when={props.currentMessage()?.toolCalls?.length && !props.gypEditing()}>
                    <div class="flex items-center gap-2 text-[10px] text-zinc-500 mb-1.5">
                      <div class="w-2.5 h-2.5 border-2 border-zinc-500 border-t-transparent rounded-full animate-spin" />
                      Working...
                    </div>
                  </Show>
                  <div class="whitespace-pre-wrap break-words">
                    {props.currentMessage()?.content || (
                      <span class="text-zinc-600 italic">Thinking...</span>
                    )}
                  </div>
                </div>
              </Show>
            </Show>
          </div>

          {/* Input */}
          <div class="px-3 pb-3 pt-2" style={{ background: 'rgba(24,24,27,0.95)', 'border-top': '1px solid rgba(63,63,70,0.4)' }}>
            <div
              class="relative rounded-lg overflow-hidden"
              style={{
                background: 'rgba(0,0,0,0.4)',
                border: '1px solid rgba(63,63,70,0.6)',
              }}
            >
              <textarea
                ref={inputRef}
                value={inputText()}
                onInput={(e) => setInputText(e.currentTarget.value)}
                placeholder={props.focusNodeName ? `Ask about "${props.focusNodeName}"...` : "Ask Gyp anything..."}
                disabled={!props.connected() || props.gypEditing()}
                class="w-full h-14 resize-none text-sm text-zinc-100 placeholder-zinc-600 focus:outline-none p-3 pr-12 bg-transparent disabled:opacity-50"
              />
              <button
                onClick={handleSend}
                disabled={!inputText().trim() || !props.connected() || props.gypEditing() || sending()}
                class="absolute right-2.5 bottom-2.5 p-2 rounded-lg transition-all disabled:opacity-30 hover:scale-105"
                style={{
                  background: 'linear-gradient(180deg, rgba(212,165,116,0.4) 0%, rgba(180,130,80,0.5) 100%)',
                  border: '1px solid rgba(212,165,116,0.5)',
                }}
              >
                <svg class="w-4 h-4 text-amber-200" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
                  <path d="M22 2L11 13M22 2l-7 20-4-9-9-4 20-7z" />
                </svg>
              </button>
            </div>
          </div>
        </div>
      </Show>

      {/* Footer Bar (blends into status bar) */}
      <button
        onClick={() => setExpanded(!expanded())}
        class="flex items-center gap-3 px-5 transition-all border-l border-pasture-600 hover:bg-white/[0.03]"
        classList={{
          'bg-amber-500/5': props.gypEditing(),
        }}
        style={{
          height: `${BAR_HEIGHT}px`,
          'min-width': `${BAR_WIDTH}px`,
          background: expanded() ? 'rgba(255,255,255,0.02)' : 'transparent',
        }}
      >
        <div class="flex items-center gap-2.5">
          <img
            src="/gyp.svg"
            alt="Gyp"
            class="w-5 h-5"
            style={{ filter: props.gypEditing() ? 'none' : 'grayscale(0.3) brightness(0.8)' }}
          />
          <span class="text-sm text-wool-400 font-medium">Gyp</span>
        </div>

        {/* Focus indicator */}
        <Show when={props.focusNodeName}>
          <div class="flex items-center gap-1.5 text-xs text-amber-400/70 truncate max-w-[140px]">
            <span class="text-wool-700">·</span>
            <span class="truncate">{props.focusNodeName}</span>
            <button
              onClick={(e) => { e.stopPropagation(); props.onFocusNode(null, null); }}
              class="text-wool-600 hover:text-wool-400"
            >
              <svg class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M18 6L6 18M6 6l12 12" />
              </svg>
            </button>
          </div>
        </Show>

        {/* Status indicators */}
        <div class="ml-auto flex items-center gap-2">
          <Show when={props.gypEditing()}>
            <div class="flex items-center gap-1.5 text-[10px] text-amber-400">
              <div class="w-1.5 h-1.5 rounded-full bg-amber-400 animate-pulse" />
              <span class="hidden sm:inline">Editing</span>
            </div>
          </Show>
          <Show when={props.currentMessage() && !props.gypEditing()}>
            <div class="w-1.5 h-1.5 rounded-full bg-amber-500 animate-pulse" />
          </Show>

          {/* Expand/collapse chevron */}
          <svg
            class="w-3.5 h-3.5 text-wool-600 transition-transform"
            classList={{ 'rotate-180': expanded() }}
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
          >
            <path d="M18 15l-6-6-6 6" />
          </svg>
        </div>
      </button>
    </div>
  );
};
