/**
 * WorkerHalfMoon — compact dropdown toolbar below a hovered worker avatar.
 *
 * Renders a glassmorphic pill with Spectate / Details / Message actions.
 * The hovered avatar gets an amber ring highlight via CSS in CanvasToolbar.
 */
import { type Component, For } from 'solid-js';
import { Icon } from '../shared';
import type { WorkerDisplay } from '../../lib/types';

interface WorkerHalfMoonProps {
  worker: WorkerDisplay;
  index: number;
  onDetails: () => void;
  onSpectate: () => void;
  onMessage: () => void;
  onClose: () => void;
}

interface ToolbarAction {
  id: string;
  icon: string;
  label: string;
  onClick: () => void;
}

export const WorkerHalfMoon: Component<WorkerHalfMoonProps> = (props) => {
  const actions = (): ToolbarAction[] => [
    { id: 'spectate', icon: 'eye', label: 'Spectate', onClick: props.onSpectate },
    { id: 'details', icon: 'info', label: 'Details', onClick: props.onDetails },
    { id: 'message', icon: 'message-circle', label: 'Message', onClick: props.onMessage },
  ];

  return (
    <div
      data-halfmoon={props.worker.name}
      class="absolute z-50 pointer-events-auto"
      style={{
        left: '50%',
        top: '100%',
        transform: 'translateX(-50%)',
        'padding-top': '6px',
      }}
    >
      {/* Connecting nub — small amber triangle anchoring toolbar to avatar */}
      <div
        class="absolute"
        style={{
          left: '50%',
          top: '2px',
          transform: 'translateX(-50%)',
          width: '0',
          height: '0',
          'border-left': '5px solid transparent',
          'border-right': '5px solid transparent',
          'border-bottom': '5px solid var(--pasture-700)',
        }}
      />
      <div
        class="flex items-center rounded-lg overflow-hidden"
        style={{
          background: 'var(--pasture-800)',
          border: '1px solid var(--pasture-600)',
          'box-shadow': '0 4px 16px rgba(0, 0, 0, 0.35), 0 0 1px rgba(0, 0, 0, 0.2)',
          animation: 'halfmoon-enter 120ms ease-out',
        }}
      >
        <For each={actions()}>
          {(action) => (
            <button
              type="button"
              class="group flex items-center gap-1.5 px-2.5 py-1.5 transition-colors whitespace-nowrap"
              style={{
                color: 'var(--wool-400)',
                'border-right': '1px solid var(--pasture-700)',
              }}
              onClick={(e) => {
                e.stopPropagation();
                action.onClick();
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.background = 'var(--pasture-700)';
                e.currentTarget.style.color = 'var(--wool-100)';
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = 'transparent';
                e.currentTarget.style.color = 'var(--wool-400)';
              }}
            >
              <Icon name={action.icon} class="w-3 h-3" />
              <span style={{ 'font-size': '11px', 'font-weight': '500' }}>{action.label}</span>
            </button>
          )}
        </For>
      </div>
    </div>
  );
};

export default WorkerHalfMoon;
