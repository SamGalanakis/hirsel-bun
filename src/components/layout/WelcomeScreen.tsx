/**
 * WelcomeScreen - Shown when no project is selected
 *
 * Displays a friendly welcome message with animated sheep
 * and a button to create a new project.
 */
import { type Component } from 'solid-js';
import { useProject } from '../../stores';
import { SheepAvatar } from '../shared';
import type { SheepConfig } from '../../lib/types';
import { amber } from '../../lib/theme-colors';

// Some fun sheep configurations for the welcome screen
const WELCOME_SHEEP: SheepConfig[] = [
  { hat: 1, fluffiness: 2, bodyWidth: 0, bodyHeight: 0, earPosition: 0, legLength: 0, hueShift: 0, glasses: 0, bowtie: 3 }, // Crown sheep with gold bowtie
  { hat: 0, fluffiness: 3, bodyWidth: 1, bodyHeight: 0, earPosition: 0, legLength: 0, hueShift: 30, glasses: 1, bowtie: 0 }, // Fluffy sheep with round glasses
  { hat: 2, fluffiness: 1, bodyWidth: 0, bodyHeight: 1, earPosition: 1, legLength: 1, hueShift: 180, glasses: 0, bowtie: 0 }, // Cowboy sheep (blue-ish)
  { hat: 5, fluffiness: 2, bodyWidth: -1, bodyHeight: 0, earPosition: -1, legLength: 0, hueShift: 270, glasses: 0, bowtie: 4 }, // Wizard sheep with pink bowtie
];

export const WelcomeScreen: Component = () => {
  const project = useProject();

  const handleStartProject = () => {
    project.openProjectSetup();
  };

  return (
    <div class="flex-1 flex flex-col items-center justify-center bg-pasture-900 select-none">
      {/* Sheep parade */}
      <div class="flex items-end gap-4 mb-8">
        {WELCOME_SHEEP.map((config, i) => (
          <div
            class="transform transition-transform hover:scale-110"
            style={{
              animation: `sheep-bounce 2s ease-in-out ${i * 0.2}s infinite`,
            }}
          >
            <SheepAvatar
              config={config}
              size={i === 1 ? 72 : 56}
            />
          </div>
        ))}
      </div>

      {/* Welcome text */}
      <h1 class="text-[28px] font-semibold text-wool-100 mb-2">
        Welcome to Hirsel
      </h1>
      <p class="text-[14px] text-wool-500 mb-8 max-w-md text-center">
        Your flock of AI workers, ready to help build your projects.
      </p>

      {/* Start button */}
      <button
        onClick={handleStartProject}
        class="flex items-center gap-2 px-6 py-3 rounded-lg text-[14px] font-medium transition-all hover:scale-105"
        style={{
          background: 'linear-gradient(135deg, var(--amber-500) 0%, var(--amber-600) 100%)',
          color: 'var(--pasture-900)',
          'box-shadow': `0 4px 16px ${amber(0.3)}`,
        }}
      >
        <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M12 5v14M5 12h14" stroke-linecap="round" />
        </svg>
        Start a Project
      </button>

      {/* Subtle hint */}
      <p class="text-[11px] text-wool-700 mt-6">
        Or press <kbd class="px-1.5 py-0.5 rounded bg-pasture-700 text-wool-500">⌘N</kbd> to create a new project
      </p>
    </div>
  );
};

export default WelcomeScreen;
