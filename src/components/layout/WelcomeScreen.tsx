/**
 * WelcomeScreen - Shown when no project is selected
 *
 * Minimal Alchemical Ledger welcome with architectural motif.
 */
import { type Component } from 'solid-js';
import { useProject } from '../../stores';

export const WelcomeScreen: Component = () => {
  const project = useProject();

  const handleStartProject = () => {
    project.openProjectSetup();
  };

  return (
    <div class="flex-1 flex flex-col items-center justify-center bg-pasture-900 select-none">
      {/* Architectural grid motif */}
      <div class="mb-10">
        <svg class="w-12 h-12 text-wool-700/40" viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="0.75">
          <rect x="6" y="6" width="36" height="36" />
          <line x1="6" y1="24" x2="42" y2="24" />
          <line x1="24" y1="6" x2="24" y2="42" />
          <rect x="14" y="14" width="20" height="20" opacity="0.3" />
        </svg>
      </div>

      <h1 class="text-[10px] uppercase tracking-[0.35em] text-wool-500 mb-8">
        Hirsel
      </h1>

      <button
        onClick={handleStartProject}
        class="flex items-center gap-2.5 px-6 py-2.5 text-[11px] uppercase tracking-[0.18em] border border-wool-700/50 text-wool-300 bg-transparent hover:border-wool-500 hover:text-wool-100"
      >
        <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <path d="M12 5v14M5 12h14" stroke-linecap="square" />
        </svg>
        New Project
      </button>

      <p class="text-[9px] uppercase tracking-[0.2em] text-wool-700 mt-6">
        <kbd class="px-1.5 py-0.5 bg-pasture-800 text-wool-600 border border-pasture-700/40">⌘N</kbd>
      </p>
    </div>
  );
};

export default WelcomeScreen;
