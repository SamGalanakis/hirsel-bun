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
      {/* Architectural motif */}
      <div class="mb-8">
        <svg class="w-16 h-16 text-white" viewBox="0 0 64 64">
          <use href="#nautilus-loader" />
        </svg>
      </div>

      {/* Welcome text */}
      <h1 class="text-[28px] font-semibold text-wool-100 mb-2">
        Welcome to Hirsel
      </h1>
      <p class="text-[14px] text-wool-500 mb-8 max-w-md text-center">
        Your orchestration ledger, ready to architect.
      </p>

      {/* Start button */}
      <button
        onClick={handleStartProject}
        class="flex items-center gap-2 px-6 py-3 text-[14px] font-medium transition-colors border border-white text-white bg-transparent hover:bg-white hover:text-black"
      >
        <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M12 5v14M5 12h14" stroke-linecap="round" />
        </svg>
        Start a Project
      </button>

      {/* Subtle hint */}
      <p class="text-[11px] text-wool-700 mt-6">
        Or press <kbd class="px-1.5 py-0.5 rounded-none bg-pasture-700 text-wool-500">⌘N</kbd> to create a new project
      </p>
    </div>
  );
};

export default WelcomeScreen;
