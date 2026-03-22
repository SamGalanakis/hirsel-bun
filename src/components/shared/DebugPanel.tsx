/**
 * Debug panel component - only shown in development via Ctrl+D
 *
 * Shows:
 * - Frontend version info
 * - Daemon health status
 * - Process counts
 */
import { invoke } from '../../lib/invoke';
import {
  type Component,
  Show,
  createEffect,
  createResource,
  createSignal,
  onCleanup,
  onMount,
} from 'solid-js';
import { Icon } from './Icon';

interface VersionInfo {
  version: string;
  gitSha: string;
  buildDate: string;
  features: string[];
  fullVersion: string;
}

interface DaemonHealth {
  running: boolean;
  version?: string;
  gitSha?: string;
  buildDate?: string;
  uptimeSecs?: number;
  activeRuns?: number;
  pid?: number;
  runsDir?: string;
  error?: string;
}

interface ProcessCounts {
  hirsel: number;
  node: number;
  details: string;
}

export const DebugPanel: Component = () => {
  const [visible, setVisible] = createSignal(false);
  const [isDev, setIsDev] = createSignal(false);

  // Fetch version info
  const [versionInfo] = createResource(visible, async () => {
    if (!visible()) return null;
    return invoke<VersionInfo>('get_version');
  });

  // Fetch daemon health (auto-refresh when visible)
  const [daemonHealth, { refetch: refetchDaemon }] = createResource(visible, async () => {
    if (!visible()) return null;
    return invoke<DaemonHealth>('get_daemon_health');
  });

  // Fetch process counts
  const [processCounts, { refetch: refetchProcesses }] = createResource(visible, async () => {
    if (!visible()) return null;
    return invoke<ProcessCounts>('get_process_counts');
  });

  // Auto-refresh daemon health every 5 seconds when panel is open
  createEffect(() => {
    if (!visible()) return;

    const interval = setInterval(() => {
      refetchDaemon();
      refetchProcesses();
    }, 5000);

    onCleanup(() => clearInterval(interval));
  });

  onMount(() => {
    // Check if in dev mode
    setIsDev(import.meta.env.DEV);
  });

  // Ctrl+D keyboard shortcut (only in dev mode)
  createEffect(() => {
    if (!isDev()) return;

    const handleKeydown = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === 'd') {
        e.preventDefault();
        setVisible((v) => !v);
      }
      // Also close on Escape
      if (e.key === 'Escape' && visible()) {
        setVisible(false);
      }
    };

    window.addEventListener('keydown', handleKeydown);
    onCleanup(() => window.removeEventListener('keydown', handleKeydown));
  });

  const handleStartDaemon = async () => {
    try {
      await invoke('ensure_daemon_running');
      refetchDaemon();
    } catch (e) {
      console.error('Failed to start daemon:', e);
    }
  };

  const handleKillOrphanedProcesses = async () => {
    try {
      await invoke('kill_orphaned_worker_processes');
      refetchProcesses();
    } catch (e) {
      console.error('Failed to kill orphaned processes:', e);
    }
  };

  const formatUptime = (secs?: number) => {
    if (!secs) return '-';
    const hours = Math.floor(secs / 3600);
    const minutes = Math.floor((secs % 3600) / 60);
    const seconds = secs % 60;
    if (hours > 0) return `${hours}h ${minutes}m`;
    if (minutes > 0) return `${minutes}m ${seconds}s`;
    return `${seconds}s`;
  };

  // Only render in development
  return (
    <Show when={isDev() && visible()}>
      <div
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
        onClick={(e) => {
          if (e.target === e.currentTarget) setVisible(false);
        }}
      >
        <div class="bg-pasture-800 border border-pasture-600 rounded-none shadow-xl p-4 w-[480px] max-h-[80vh] overflow-y-auto">
          {/* Header */}
          <div class="flex items-center justify-between mb-4">
            <h3 class="text-sm font-medium text-wool-300 flex items-center gap-2">
              <Icon name="bug" class="w-4 h-4" />
              Debug Panel
            </h3>
            <button
              onClick={() => setVisible(false)}
              class="p-1 rounded-none hover:bg-pasture-700 text-wool-500"
            >
              <Icon name="x" class="w-4 h-4" />
            </button>
          </div>

          {/* Frontend Version */}
          <section class="mb-4">
            <h4 class="text-xs font-semibold text-wool-400 uppercase tracking-wider mb-2">
              Frontend
            </h4>
            <Show
              when={versionInfo()}
              fallback={<p class="text-xs text-wool-500">Loading...</p>}
            >
              <div class="grid grid-cols-2 gap-1 text-xs">
                <span class="text-wool-500">Version:</span>
                <span class="text-wool-300 font-mono">{versionInfo()?.version}</span>
                <span class="text-wool-500">Git SHA:</span>
                <span class="text-wool-300 font-mono text-[10px]">
                  {versionInfo()?.gitSha?.slice(0, 8)}
                </span>
                <span class="text-wool-500">Build:</span>
                <span class="text-wool-300 font-mono text-[10px]">
                  {versionInfo()?.buildDate}
                </span>
                <span class="text-wool-500">Features:</span>
                <span class="text-wool-300 font-mono text-[10px]">
                  {versionInfo()?.features?.join(', ') || 'none'}
                </span>
              </div>
            </Show>
          </section>

          <hr class="border-pasture-600 my-3" />

          {/* Daemon Health */}
          <section class="mb-4">
            <div class="flex items-center justify-between mb-2">
              <h4 class="text-xs font-semibold text-wool-400 uppercase tracking-wider">
                Daemon
              </h4>
              <Show when={!daemonHealth()?.running}>
                <button
                  onClick={handleStartDaemon}
                  class="text-xs px-2 py-0.5 rounded-none bg-sage/20 text-sage hover:bg-sage/30"
                >
                  Start
                </button>
              </Show>
            </div>
            <Show
              when={daemonHealth()}
              fallback={<p class="text-xs text-wool-500">Checking...</p>}
            >
              <div class="grid grid-cols-2 gap-1 text-xs">
                <span class="text-wool-500">Status:</span>
                <span
                  class={`font-medium ${daemonHealth()?.running ? 'text-sage' : 'text-rust'}`}
                >
                  {daemonHealth()?.running ? 'Running' : 'Not Running'}
                </span>
                <Show when={daemonHealth()?.running}>
                  <span class="text-wool-500">Version:</span>
                  <span class="text-wool-300 font-mono">
                    {daemonHealth()?.version || '-'}
                  </span>
                  <span class="text-wool-500">Git SHA:</span>
                  <span class="text-wool-300 font-mono text-[10px]">
                    {daemonHealth()?.gitSha?.slice(0, 8) || '-'}
                  </span>
                  <span class="text-wool-500">PID:</span>
                  <span class="text-wool-300 font-mono">{daemonHealth()?.pid || '-'}</span>
                  <span class="text-wool-500">Uptime:</span>
                  <span class="text-wool-300">
                    {formatUptime(daemonHealth()?.uptimeSecs)}
                  </span>
                  <span class="text-wool-500">Active Runs:</span>
                  <span class="text-wool-300">{daemonHealth()?.activeRuns ?? '-'}</span>
                  <span class="text-wool-500">Runs Dir:</span>
                  <span class="text-wool-300 font-mono text-[10px] truncate" title={daemonHealth()?.runsDir}>
                    {daemonHealth()?.runsDir || '-'}
                  </span>
                </Show>
                <Show when={daemonHealth()?.error}>
                  <span class="text-wool-500">Error:</span>
                  <span class="text-rust text-[10px]">{daemonHealth()?.error}</span>
                </Show>
              </div>
              {/* Version mismatch warning */}
              <Show
                when={
                  daemonHealth()?.running &&
                  versionInfo() &&
                  daemonHealth()?.gitSha !== versionInfo()?.gitSha
                }
              >
                <div class="mt-2 p-2 bg-honey/10 border border-honey/30 rounded-none text-[10px] text-honey">
                  <Icon name="alert-triangle" class="w-3 h-3 inline mr-1" />
                  Version mismatch: Daemon ({daemonHealth()?.gitSha?.slice(0, 8)}) differs
                  from frontend ({versionInfo()?.gitSha?.slice(0, 8)})
                </div>
              </Show>
            </Show>
          </section>

          <hr class="border-pasture-600 my-3" />

          {/* Process Counts */}
          <section class="mb-4">
            <div class="flex items-center justify-between mb-2">
              <h4 class="text-xs font-semibold text-wool-400 uppercase tracking-wider">
                Processes
              </h4>
              <button
                onClick={handleKillOrphanedProcesses}
                class="text-xs px-2 py-0.5 rounded-none bg-rust/20 text-rust hover:bg-rust/30"
              >
                Kill Orphans
              </button>
            </div>
            <Show
              when={processCounts()}
              fallback={<p class="text-xs text-wool-500">Loading...</p>}
            >
              <div class="grid grid-cols-2 gap-2 text-xs text-center">
                <div class="bg-pasture-700/50 rounded-none p-2">
                  <div class="text-wool-300 font-mono text-lg">
                    {processCounts()?.hirsel || 0}
                  </div>
                  <div class="text-wool-500 text-[10px]">Hirsel</div>
                </div>
                <div class="bg-pasture-700/50 rounded-none p-2">
                  <div class="text-wool-300 font-mono text-lg">
                    {processCounts()?.node || 0}
                  </div>
                  <div class="text-wool-500 text-[10px]">Node</div>
                </div>
              </div>
            </Show>
          </section>

          <hr class="border-pasture-600 my-3" />

          {/* Footer */}
          <div class="text-[10px] text-wool-600 text-center">
            Press <kbd class="px-1 py-0.5 bg-pasture-700 rounded">Ctrl+D</kbd> to toggle
            &bull; Auto-refreshes every 5s
          </div>
        </div>
      </div>
    </Show>
  );
};
