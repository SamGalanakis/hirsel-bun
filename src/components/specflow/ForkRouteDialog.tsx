/**
 * ForkRouteDialog - Modal for creating a new route (fork)
 *
 * Allows forking from the current route at its current state
 * or from a specific historical version.
 */

import { type Component, Show, createSignal, createEffect } from 'solid-js';
import { useEscapeKey } from '../../hooks';
import { useRoute } from '../../stores/route-context';
import { Icon } from '../shared';

interface ForkRouteDialogProps {
  onClose: () => void;
}

export const ForkRouteDialog: Component<ForkRouteDialogProps> = (props) => {
  const route = useRoute();

  // Form state
  const [name, setName] = createSignal('');
  const [error, setError] = createSignal<string | null>(null);
  const [loading, setLoading] = createSignal(false);

  let inputRef: HTMLInputElement | undefined;

  // Focus input on mount
  createEffect(() => {
    setTimeout(() => inputRef?.focus(), 50);
  });

  useEscapeKey(() => {
    if (!loading()) {
      props.onClose();
    }
  });

  // Validate name
  const validateName = (value: string): string | null => {
    if (!value.trim()) {
      return 'Route name is required';
    }
    // Must be lowercase, no spaces, valid slug
    if (!/^[a-z0-9][a-z0-9-]*[a-z0-9]$|^[a-z0-9]$/.test(value)) {
      return 'Use lowercase letters, numbers, and hyphens only';
    }
    // Check for duplicates
    if (route.routes().some((r) => r.name === value)) {
      return 'A route with this name already exists';
    }
    return null;
  };

  const handleSubmit = async (e: Event) => {
    e.preventDefault();

    const trimmedName = name().trim().toLowerCase();
    const validationError = validateName(trimmedName);
    if (validationError) {
      setError(validationError);
      return;
    }

    setError(null);
    setLoading(true);

    try {
      const newRoute = await route.createRoute(trimmedName);
      if (newRoute) {
        // Switch to the new route
        await route.setActiveRoute(newRoute.id);
        props.onClose();
      } else {
        setError('Failed to create route');
      }
    } catch (e) {
      setError(`Failed to create route: ${e}`);
    } finally {
      setLoading(false);
    }
  };

  const handleNameChange = (value: string) => {
    // Auto-convert to lowercase and replace spaces with hyphens
    const normalized = value.toLowerCase().replace(/\s+/g, '-');
    setName(normalized);
    // Clear error when typing
    if (error()) setError(null);
  };

  return (
    <div
      class="fixed inset-0 z-50 flex items-center justify-center"
      style={{ background: 'rgba(0, 0, 0, 0.6)' }}
      onClick={(e) => {
        if (e.target === e.currentTarget && !loading()) {
          props.onClose();
        }
      }}
    >
      <div
        class="w-full max-w-sm rounded-lg shadow-2xl overflow-hidden"
        style={{
          background: 'linear-gradient(180deg, #2a2a2a 0%, #242424 100%)',
          border: '1px solid rgba(64, 64, 64, 0.6)',
        }}
      >
        {/* Header */}
        <div
          class="flex items-center justify-between px-4 py-3"
          style={{ 'border-bottom': '1px solid rgba(64, 64, 64, 0.4)' }}
        >
          <div class="flex items-center gap-2">
            <Icon name="git-branch" class="w-4 h-4 text-amber-500" />
            <h2 class="text-sm font-medium text-wool-200">Fork Route</h2>
          </div>
          <button
            onClick={props.onClose}
            disabled={loading()}
            class="text-wool-500 hover:text-wool-300 disabled:opacity-50"
          >
            <Icon name="x" class="w-4 h-4" />
          </button>
        </div>

        {/* Content */}
        <form onSubmit={handleSubmit} class="p-4">
          {/* Fork source */}
          <div class="mb-4">
            <p class="text-[11px] text-wool-500 mb-1">Forking from:</p>
            <div
              class="flex items-center gap-2 px-2.5 py-1.5 rounded"
              style={{
                background: 'rgba(36, 36, 36, 0.6)',
                border: '1px solid rgba(64, 64, 64, 0.4)',
              }}
            >
              <Icon name="git-branch" class="w-3 h-3 text-wool-500" />
              <span class="text-[12px] text-wool-300 font-medium">
                {route.activeRoute()?.name || 'main'}
              </span>
              <span class="text-[10px] text-wool-600">(current state)</span>
            </div>
          </div>

          {/* Route name input */}
          <div class="mb-4">
            <label class="block text-[11px] text-wool-400 mb-1.5">Route name</label>
            <input
              ref={inputRef}
              type="text"
              value={name()}
              onInput={(e) => handleNameChange(e.currentTarget.value)}
              placeholder="feature-auth"
              disabled={loading()}
              class="w-full px-3 py-2 rounded text-[12px] placeholder:text-wool-600 disabled:opacity-50"
              style={{
                background: 'rgba(20, 20, 20, 0.6)',
                border: error() ? '1px solid var(--terra)' : '1px solid rgba(64, 64, 64, 0.5)',
                color: 'var(--wool-200)',
              }}
            />
            <Show when={error()}>
              <p class="mt-1 text-[10px]" style={{ color: 'var(--terra)' }}>
                {error()}
              </p>
            </Show>
            <p class="mt-1 text-[10px] text-wool-600">Lowercase letters, numbers, and hyphens</p>
          </div>

          {/* Actions */}
          <div class="flex items-center justify-end gap-2">
            <button
              type="button"
              onClick={props.onClose}
              disabled={loading()}
              class="px-3 py-1.5 rounded text-[11px] font-medium transition-colors disabled:opacity-50"
              style={{
                background: 'transparent',
                border: '1px solid rgba(64, 64, 64, 0.5)',
                color: 'var(--wool-400)',
              }}
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={loading() || !name().trim()}
              class="px-3 py-1.5 rounded text-[11px] font-medium transition-colors disabled:opacity-50"
              style={{
                background: 'rgba(212, 165, 116, 0.2)',
                border: '1px solid rgba(212, 165, 116, 0.35)',
                color: 'var(--amber-300)',
              }}
            >
              {loading() ? 'Creating...' : 'Fork'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
};

export default ForkRouteDialog;
