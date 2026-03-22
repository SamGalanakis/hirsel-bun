import { type Component, Show } from 'solid-js';

interface RunStatusPillProps {
  status: string | null;
  onPause: () => void;
  onResume: () => void;
}

export const RunStatusPill: Component<RunStatusPillProps> = (props) => {
  const canPause = () => ['working', 'idle', 'waiting', 'eval', 'starting'].includes(props.status || '');
  const canResume = () => props.status === 'paused';

  // Status styling per design language
  const statusColor = () => {
    switch (props.status) {
      case 'working':
      case 'starting':
      case 'idle':
        return 'var(--amber-400)';
      case 'done':
        return 'var(--sage)';
      case 'failed':
        return 'var(--terra)';
      case 'paused':
        return 'var(--golden)';
      default:
        return 'var(--wool-500)';
    }
  };

  const isPulsing = () => props.status === 'working' || props.status === 'starting';
  const statusText = () => props.status === 'starting' ? 'starting...' : props.status;

  return (
    <div class="flex items-center gap-1.5">
      {/* Status dot + text */}
      <div class="flex items-center gap-1">
        <div
          class={`w-1.5 h-1.5 rounded-none ${isPulsing() ? 'animate-pulse' : ''}`}
          style={{ background: statusColor() }}
        />
        <span
          class={`text-[9px] font-medium ${isPulsing() ? 'animate-pulse' : ''}`}
          style={{ color: statusColor() }}
        >
          {statusText()}
        </span>
      </div>

      {/* Pause button */}
      <Show when={canPause()}>
        <button
          onClick={(e) => {
            e.stopPropagation();
            props.onPause();
          }}
          class="flex items-center justify-center w-4 h-4 rounded-none transition-all duration-150 hover:scale-110"
          style={{
            background: 'rgba(201, 162, 39, 0.2)',
            border: '1px solid rgba(201, 162, 39, 0.3)',
          }}
          title="Pause run"
        >
          <svg
            class="w-2 h-2"
            viewBox="0 0 24 24"
            fill="currentColor"
            style={{ color: 'var(--golden)' }}
          >
            <rect x="6" y="4" width="4" height="16" rx="1" />
            <rect x="14" y="4" width="4" height="16" rx="1" />
          </svg>
        </button>
      </Show>

      {/* Resume button */}
      <Show when={canResume()}>
        <button
          onClick={(e) => {
            e.stopPropagation();
            props.onResume();
          }}
          class="flex items-center justify-center w-4 h-4 rounded-none transition-all duration-150 hover:scale-110"
          style={{
            background: 'rgba(139, 168, 110, 0.2)',
            border: '1px solid rgba(139, 168, 110, 0.3)',
          }}
          title="Resume run"
        >
          <svg
            class="w-2 h-2"
            viewBox="0 0 24 24"
            fill="currentColor"
            style={{ color: 'var(--sage)' }}
          >
            <path d="M8 5.14v14l11-7-11-7z" />
          </svg>
        </button>
      </Show>
    </div>
  );
};
