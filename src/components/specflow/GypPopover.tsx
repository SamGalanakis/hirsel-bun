/**
 * GypPopover - Floating AI chat interface for the SpecFlow board
 *
 * A compact, draggable popover for asking Gyp to modify islands.
 */
import { type Component, For, Show, createEffect, createSignal, onCleanup, onMount } from 'solid-js';
import type { ChatMessage } from '../../lib/types';

interface GypPopoverProps {
  projectId: number;
  focusIslandId: string;
  focusIslandName: string;
  position: { x: number; y: number };
  messages: ChatMessage[];
  currentMessage: () => Partial<ChatMessage> | null;
  connected: () => boolean;
  connecting: () => boolean;
  gypEditing: () => boolean;
  onClose: () => void;
  onSend: (content: string) => Promise<void>;
  onConnect: () => Promise<void>;
}

export const GypPopover: Component<GypPopoverProps> = (props) => {
  let containerRef: HTMLDivElement | undefined;
  let headerRef: HTMLDivElement | undefined;
  let inputRef: HTMLTextAreaElement | undefined;
  let messagesRef: HTMLDivElement | undefined;

  const [inputText, setInputText] = createSignal('');
  const [sending, setSending] = createSignal(false);

  // Dragging state
  const [position, setPosition] = createSignal({ x: props.position.x, y: props.position.y });
  const [dragging, setDragging] = createSignal(false);
  const [dragOffset, setDragOffset] = createSignal({ x: 0, y: 0 });

  const POPOVER_WIDTH = 320;
  const POPOVER_HEIGHT = 380;

  // Clamp position to viewport
  const clampedPosition = () => {
    const padding = 16;
    const pos = position();
    return {
      x: Math.max(POPOVER_WIDTH / 2 + padding, Math.min(window.innerWidth - POPOVER_WIDTH / 2 - padding, pos.x)),
      y: Math.max(padding, Math.min(window.innerHeight - POPOVER_HEIGHT - padding, pos.y)),
    };
  };

  // Drag handlers
  const handleDragStart = (e: MouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    setDragging(true);
    const pos = position();
    setDragOffset({ x: e.clientX - pos.x, y: e.clientY - pos.y });
  };

  const handleDragMove = (e: MouseEvent) => {
    if (!dragging()) return;
    setPosition({
      x: e.clientX - dragOffset().x,
      y: e.clientY - dragOffset().y,
    });
  };

  const handleDragEnd = () => {
    setDragging(false);
  };

  // Handle send
  const handleSend = async () => {
    const text = inputText().trim();
    if (!text || sending()) return;

    setSending(true);
    setInputText('');
    try {
      await props.onSend(text);
    } finally {
      setSending(false);
    }
  };

  // Handle key press
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      props.onClose();
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  // Click outside to close (but not while dragging)
  const handleClickOutside = (e: MouseEvent) => {
    if (dragging()) return;
    if (containerRef && !containerRef.contains(e.target as Node)) {
      props.onClose();
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

  onMount(() => {
    setTimeout(() => inputRef?.focus(), 50);
    document.addEventListener('mousedown', handleClickOutside);
    document.addEventListener('mousemove', handleDragMove);
    document.addEventListener('mouseup', handleDragEnd);
  });

  onCleanup(() => {
    document.removeEventListener('mousedown', handleClickOutside);
    document.removeEventListener('mousemove', handleDragMove);
    document.removeEventListener('mouseup', handleDragEnd);
  });

  // Auto-connect if not connected
  createEffect(() => {
    if (!props.connected() && !props.connecting()) {
      props.onConnect();
    }
  });

  const recentMessages = () => props.messages.slice(-5);
  const pos = () => clampedPosition();

  return (
    <div
      ref={containerRef}
      class="rounded-lg overflow-hidden flex flex-col select-none"
      style={{
        position: 'fixed',
        left: `${pos().x}px`,
        top: `${pos().y}px`,
        transform: 'translate(-50%, 0)',
        width: `${POPOVER_WIDTH}px`,
        'max-height': `${POPOVER_HEIGHT}px`,
        background: 'var(--pasture-800, #242424)',
        border: props.gypEditing()
          ? '1px solid var(--amber-500, #d4a574)'
          : '1px solid var(--pasture-600, #333)',
        'box-shadow': '0 8px 32px rgba(0,0,0,0.4)',
        'z-index': 100,
      }}
      onKeyDown={handleKeyDown}
    >
      {/* Draggable Header */}
      <div
        ref={headerRef}
        class="flex items-center justify-between px-3 py-2.5 cursor-move"
        style={{
          background: props.gypEditing()
            ? 'linear-gradient(180deg, rgba(212,165,116,0.12) 0%, transparent 100%)'
            : 'linear-gradient(180deg, rgba(255,255,255,0.02) 0%, transparent 100%)',
          'border-bottom': '1px solid var(--pasture-600, #333)',
        }}
        onMouseDown={handleDragStart}
      >
        <div class="flex items-center gap-2.5">
          {/* Gyp SVG */}
          <div
            class="w-7 h-7 rounded flex items-center justify-center"
            classList={{
              'bg-amber-500/20': props.gypEditing(),
              'bg-pasture-700': !props.gypEditing(),
            }}
          >
            <img
              src="/gyp.svg"
              alt="Gyp"
              class="w-5 h-5"
              style={{ filter: props.gypEditing() ? 'none' : 'grayscale(0.3) brightness(0.9)' }}
            />
          </div>
          <div>
            <div class="text-sm font-semibold text-wool-100">Gyp</div>
            <div class="text-[11px] text-wool-500 truncate max-w-[180px]">
              {props.focusIslandName}
            </div>
          </div>
        </div>
        <button
          onClick={(e) => { e.stopPropagation(); props.onClose(); }}
          class="p-1.5 rounded text-wool-500 hover:text-wool-200 hover:bg-pasture-700 transition-colors"
        >
          <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M18 6L6 18M6 6l12 12" />
          </svg>
        </button>
      </div>

      {/* Messages */}
      <div
        ref={messagesRef}
        class="flex-1 overflow-y-auto px-3 py-2.5 space-y-2 min-h-[100px] max-h-[180px]"
        style={{ background: 'var(--pasture-900, #1a1a1a)' }}
      >
        <Show when={!props.connected() && !props.connecting()}>
          <div class="text-center text-xs text-wool-600 py-6 italic">
            Connecting...
          </div>
        </Show>

        <Show when={props.connecting()}>
          <div class="text-center text-xs text-wool-500 py-6 flex items-center justify-center gap-2">
            <div class="w-3 h-3 border-2 border-wool-600 border-t-amber-500 rounded-full animate-spin" />
            Connecting...
          </div>
        </Show>

        <Show when={props.connected()}>
          <Show when={recentMessages().length === 0 && !props.currentMessage()}>
            <div class="text-center py-6">
              <img src="/gyp.svg" alt="" class="w-10 h-10 mx-auto mb-2 opacity-40" />
              <p class="text-xs text-wool-600 italic">What should I do?</p>
            </div>
          </Show>

          <For each={recentMessages()}>
            {(msg) => (
              <div
                class="text-xs rounded-md px-2.5 py-2"
                classList={{
                  'bg-amber-500/10 text-wool-200 ml-6 border border-amber-500/20': msg.role === 'user',
                  'bg-pasture-800 text-wool-300 mr-6': msg.role === 'assistant',
                }}
              >
                <Show when={msg.role === 'assistant' && msg.toolCalls?.length}>
                  <div class="flex items-center gap-1 text-[10px] text-wool-500 mb-1">
                    <svg class="w-2.5 h-2.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z" />
                    </svg>
                    {msg.toolCalls?.length} tool{msg.toolCalls?.length !== 1 ? 's' : ''}
                  </div>
                </Show>
                <div class="whitespace-pre-wrap break-words line-clamp-4">
                  {msg.content}
                </div>
              </div>
            )}
          </For>

          {/* Streaming message */}
          <Show when={props.currentMessage()}>
            <div class="text-xs rounded-md px-2.5 py-2 bg-pasture-800 text-wool-300 mr-6">
              <Show when={props.gypEditing()}>
                <div class="flex items-center gap-1.5 text-[10px] text-amber-400 mb-1">
                  <div class="w-2 h-2 border border-amber-400 border-t-transparent rounded-full animate-spin" />
                  Editing...
                </div>
              </Show>
              <Show when={props.currentMessage()?.toolCalls?.length && !props.gypEditing()}>
                <div class="flex items-center gap-1.5 text-[10px] text-wool-500 mb-1">
                  <div class="w-2 h-2 border border-wool-500 border-t-transparent rounded-full animate-spin" />
                  Working...
                </div>
              </Show>
              <div class="whitespace-pre-wrap break-words">
                {props.currentMessage()?.content || (
                  <span class="text-wool-600 italic">Thinking...</span>
                )}
              </div>
            </div>
          </Show>
        </Show>
      </div>

      {/* Input */}
      <div class="px-3 pb-3 pt-2" style={{ background: 'var(--pasture-800, #242424)' }}>
        <div
          class="relative rounded-md overflow-hidden"
          style={{
            background: 'var(--pasture-900, #1a1a1a)',
            border: '1px solid var(--pasture-600, #333)',
          }}
        >
          <textarea
            ref={inputRef}
            value={inputText()}
            onInput={(e) => setInputText(e.currentTarget.value)}
            disabled={!props.connected() || props.gypEditing()}
            class="w-full h-14 resize-none text-sm text-wool-100 placeholder-wool-600 focus:outline-none p-2.5 pr-10 bg-transparent disabled:opacity-50"
            style={{ 'font-family': 'var(--font-primary)' }}
          />
          <button
            onClick={handleSend}
            disabled={!inputText().trim() || !props.connected() || props.gypEditing() || sending()}
            class="absolute right-2 bottom-2 p-1.5 rounded transition-all disabled:opacity-30 hover:scale-105"
            style={{
              background: 'var(--amber-600, #b8895c)',
            }}
          >
            <svg class="w-3.5 h-3.5 text-white" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
              <path d="M22 2L11 13M22 2l-7 20-4-9-9-4 20-7z" />
            </svg>
          </button>
        </div>
        <div class="flex items-center justify-between mt-1.5 px-0.5">
          <span class="text-[10px] text-wool-600">
            Enter to send
          </span>
          <Show when={props.gypEditing()}>
            <span class="text-[10px] text-amber-400 flex items-center gap-1.5">
              <div class="w-1.5 h-1.5 rounded-full bg-amber-400 animate-pulse" />
              Editing...
            </span>
          </Show>
        </div>
      </div>
    </div>
  );
};
