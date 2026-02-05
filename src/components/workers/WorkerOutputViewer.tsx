/**
 * Worker output viewer modal with event streaming
 */
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { type Component, For, Show, createEffect, createSignal, onCleanup } from 'solid-js';
import { useEscapeKey } from '../../hooks';
import { createStore, produce } from 'solid-js/store';
import type { WorkerEvent, WorkerStatus, WorkerStreamEvent } from '../../lib/types';
import { getEffectiveToolStatus } from '../../lib/tool-utils';
import { Icon, ThinkingBlock, ToolCard, ToolCluster, type ClusterToolGroup } from '../shared';

export const WorkerOutputViewer: Component = () => {
  const [visible, setVisible] = createSignal(false);
  const [runName, setRunName] = createSignal<string | null>(null);
  const [workerName, setWorkerName] = createSignal<string | null>(null);
  const [events, setEvents] = createStore<WorkerEvent[]>([]);
  const [workerStatus, setWorkerStatus] = createSignal<WorkerStatus | null>(null);
  const [autoScroll, setAutoScroll] = createSignal(true);
  const [showThinking, setShowThinking] = createSignal(true);
  const [expandedTools, setExpandedTools] = createSignal<Set<string>>(new Set());

  let containerRef: HTMLDivElement | undefined;
  let unlisten: UnlistenFn | undefined;

  // Listen for show-worker-output events
  createEffect(() => {
    const handler = (e: Event) => {
      const customEvent = e as CustomEvent<{ runName: string; workerName: string }>;
      setRunName(customEvent.detail.runName);
      setWorkerName(customEvent.detail.workerName);
      setVisible(true);
    };

    window.addEventListener('show-worker-output', handler);
    onCleanup(() => window.removeEventListener('show-worker-output', handler));
  });

  // Start/stop event stream when modal opens/closes
  createEffect(() => {
    const name = runName();
    const worker = workerName();
    const isVisible = visible();

    if (isVisible && name && worker) {
      startStream(name, worker);
    } else {
      stopStream();
    }
  });

  // Auto-scroll when new events arrive
  createEffect(() => {
    const _ = events.length;
    if (autoScroll() && containerRef) {
      setTimeout(() => {
        containerRef?.scrollTo({ top: containerRef.scrollHeight, behavior: 'smooth' });
      }, 50);
    }
  });

  useEscapeKey(() => {
    if (visible()) setVisible(false);
  });

  const startStream = async (name: string, worker: string) => {
    // Clear previous events
    setEvents([]);
    setWorkerStatus(null);
    setExpandedTools(new Set<string>());

    // Subscribe to events
    unlisten = await listen<WorkerStreamEvent>('worker-event', (event) => {
      const payload = event.payload;

      // Filter to our worker
      if (payload.runName !== runName() || payload.workerName !== workerName()) {
        return;
      }

      switch (payload.type) {
        case 'history':
          setEvents(payload.events);
          if (payload.workerStatus) {
            setWorkerStatus(payload.workerStatus as WorkerStatus);
          }
          break;
        case 'event':
          setEvents(produce((draft) => {
            draft.push(payload.event);
          }));
          break;
        case 'status':
          if (payload.workerStatus) {
            setWorkerStatus(payload.workerStatus as WorkerStatus);
          }
          break;
        case 'ended':
          setWorkerStatus('idle');
          break;
      }
    });

    // Start the stream
    try {
      await invoke('start_worker_event_stream', { runName: name, workerName: worker });
    } catch (e) {
      console.error('Failed to start worker event stream:', e);
    }
  };

  const stopStream = async () => {
    const name = runName();
    const worker = workerName();

    if (unlisten) {
      unlisten();
      unlisten = undefined;
    }

    if (name && worker) {
      try {
        await invoke('stop_worker_event_stream', { runName: name, workerName: worker });
      } catch {
        // Ignore errors when stopping
      }
    }
  };

  const toggleTool = (toolCallId: string) => {
    setExpandedTools((prev) => {
      const next = new Set(prev);
      if (next.has(toolCallId)) {
        next.delete(toolCallId);
      } else {
        next.add(toolCallId);
      }
      return next;
    });
  };

  // Group type for display - includes tool_cluster for consecutive tools
  type GroupType = 'text' | 'thinking' | 'tool' | 'tool_cluster';
  type EventGroup = {
    type: GroupType;
    content: string;
    events: WorkerEvent[];
    tools?: ClusterToolGroup[];
  };

  // Group consecutive text events for display and cluster consecutive tools
  // Note: We use events.length to ensure SolidJS tracks array changes,
  // then slice() to create a plain array for iteration
  const groupedEvents = () => {
    // Track the array length to ensure reactivity when events are added
    const _len = events.length;
    const eventsCopy = events.slice();

    const result: EventGroup[] = [];
    let currentText: WorkerEvent[] = [];
    let currentThinking: WorkerEvent[] = [];

    const flushText = () => {
      if (currentText.length > 0) {
        result.push({
          type: 'text',
          content: currentText.map((e) => e.content || '').join(''),
          events: [...currentText],
        });
        currentText = [];
      }
    };

    const flushThinking = () => {
      if (currentThinking.length > 0) {
        result.push({
          type: 'thinking',
          content: currentThinking.map((e) => e.content || '').join(''),
          events: [...currentThinking],
        });
        currentThinking = [];
      }
    };

    for (const event of eventsCopy) {
      if (event.eventType === 'text') {
        flushThinking();
        currentText.push(event);
      } else if (event.eventType === 'thought') {
        flushText();
        currentThinking.push(event);
      } else if (event.eventType === 'tool_start' || event.eventType === 'tool_update') {
        flushText();
        flushThinking();

        // Find existing tool group or create new one
        const existingIdx = result.findIndex(
          (r) => r.type === 'tool' && r.events[0]?.toolCallId === event.toolCallId
        );
        if (existingIdx >= 0) {
          result[existingIdx].events.push(event);
        } else {
          result.push({ type: 'tool', content: '', events: [event] });
        }
      }
    }

    flushText();
    flushThinking();

    // Phase 2: Cluster consecutive tool groups
    const clustered: EventGroup[] = [];
    let toolBuffer: EventGroup[] = [];

    const flushToolBuffer = () => {
      if (toolBuffer.length >= 2) {
        // Create a cluster from consecutive tools
        clustered.push({
          type: 'tool_cluster',
          content: '',
          events: [],
          tools: toolBuffer.map((t) => ({ events: t.events })),
        });
      } else if (toolBuffer.length === 1) {
        // Single tool, keep as-is
        clustered.push(toolBuffer[0]);
      }
      toolBuffer = [];
    };

    for (const group of result) {
      if (group.type === 'tool') {
        toolBuffer.push(group);
      } else {
        flushToolBuffer();
        clustered.push(group);
      }
    }
    flushToolBuffer();

    return clustered;
  };

  const statusColor = () => {
    switch (workerStatus()) {
      case 'working': return 'bg-amber-500';
      case 'waiting': return 'bg-golden';
      case 'idle': return 'bg-wool-500';
      case 'error': return 'bg-terra';
      default: return 'bg-wool-600';
    }
  };

  return (
    <Show when={visible()}>
      <div
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/80"
        onClick={(e) => {
          if (e.target === e.currentTarget) setVisible(false);
        }}
      >
        <div class="bg-pasture-800 border border-pasture-600 rounded-lg shadow-xl w-[90vw] h-[90vh] max-w-6xl flex flex-col overflow-hidden">
          {/* Header */}
          <div class="p-4 border-b border-pasture-600 flex items-center justify-between shrink-0">
            <div class="flex items-center gap-3">
              <div class="w-10 h-10 rounded-full bg-pasture-700 flex items-center justify-center">
                <span class="text-xl">🐑</span>
              </div>
              <div>
                <div class="flex items-center gap-2">
                  <h2 class="text-lg font-medium text-wool-100">{workerName()}</h2>
                  <Show when={workerStatus()}>
                    <span class={`w-2 h-2 rounded-full ${statusColor()}`} />
                    <span class="text-xs text-wool-500">{workerStatus()}</span>
                  </Show>
                </div>
                <p class="text-xs text-wool-500">{runName()}</p>
              </div>
            </div>
            <div class="flex items-center gap-2">
              {/* Toggle thinking */}
              <button
                class={`px-2 py-1 text-xs rounded ${
                  showThinking()
                    ? 'bg-amber-500/20 text-amber-400'
                    : 'bg-pasture-700 text-wool-500'
                }`}
                onClick={() => setShowThinking(!showThinking())}
                data-tooltip={showThinking() ? 'Hide thinking' : 'Show thinking'}
              >
                <Icon name="brain" class="w-3.5 h-3.5 inline-block mr-1" />
                Thinking
              </button>
              {/* Toggle auto-scroll */}
              <button
                class={`px-2 py-1 text-xs rounded ${
                  autoScroll()
                    ? 'bg-amber-500/20 text-amber-400'
                    : 'bg-pasture-700 text-wool-500'
                }`}
                onClick={() => setAutoScroll(!autoScroll())}
                data-tooltip={autoScroll() ? 'Disable auto-scroll' : 'Enable auto-scroll'}
              >
                <Icon name="arrow-down-to-line" class="w-3.5 h-3.5 inline-block mr-1" />
                Auto-scroll
              </button>
              {/* Close */}
              <button
                onClick={() => setVisible(false)}
                class="p-2 rounded hover:bg-pasture-700 text-wool-500"
              >
                <Icon name="x" class="w-5 h-5" />
              </button>
            </div>
          </div>

          {/* Content */}
          <div
            ref={containerRef}
            class="flex-1 overflow-auto p-4 bg-pasture-900 font-mono text-sm"
          >
            <Show when={events.length === 0}>
              <div class="text-wool-500 text-center py-8">
                <Show when={workerStatus() === 'working'}>
                  <div class="spinner w-8 h-8 mx-auto mb-4" />
                  <p>Waiting for events...</p>
                </Show>
                <Show when={workerStatus() !== 'working'}>
                  <Icon name="terminal" class="w-12 h-12 mx-auto mb-4 text-wool-600" />
                  <p>No events yet</p>
                </Show>
              </div>
            </Show>

            <div class="space-y-3">
              <For each={groupedEvents()}>
                {(group) => (
                  <>
                    {/* Text block */}
                    <Show when={group.type === 'text'}>
                      <div class="text-wool-200 whitespace-pre-wrap">{group.content}</div>
                    </Show>

                    {/* Thinking block */}
                    <Show when={group.type === 'thinking' && showThinking()}>
                      <ThinkingBlock content={group.content} />
                    </Show>

                    {/* Tool card (single tool) */}
                    <Show when={group.type === 'tool'}>
                      {(() => {
                        // Find event with title (usually tool_start, but handle race conditions
                        // where tool_update arrives before tool_start in DB ordering)
                        const titleEvent = group.events.find(e => e.toolTitle) ?? group.events[0];
                        // Latest event has current status/output
                        const latestEvent = group.events[group.events.length - 1];
                        // Determine effective status: if started but not completed/failed, it's running
                        const hasStarted = group.events.some(e => e.eventType === 'tool_start');
                        const effectiveStatus = getEffectiveToolStatus(hasStarted, latestEvent?.toolStatus);
                        return (
                          <ToolCard
                            title={titleEvent?.toolTitle}
                            kind={titleEvent?.toolKind}
                            status={effectiveStatus}
                            input={titleEvent?.toolInput}
                            output={latestEvent?.toolOutput}
                            expanded={expandedTools().has(group.events[0]?.toolCallId || '')}
                            onToggle={() => toggleTool(group.events[0]?.toolCallId || '')}
                          />
                        );
                      })()}
                    </Show>

                    {/* Tool cluster (multiple consecutive tools) */}
                    <Show when={group.type === 'tool_cluster' && group.tools}>
                      <ToolCluster tools={group.tools!} />
                    </Show>
                  </>
                )}
              </For>
            </div>
          </div>
        </div>
      </div>
    </Show>
  );
};
