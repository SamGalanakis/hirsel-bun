/**
 * Eval tab Alpine component
 */

import type { Eval } from '../../types';

declare const Alpine: {
  store: (name: string) => { selectedRun?: string | null } | undefined;
};

/**
 * Eval tab component
 */
export function evalTab() {
  return {
    evals: [] as Eval[],
    isRunning: false,
    pollInterval: null as ReturnType<typeof setInterval> | null,

    async init() {
      window.addEventListener('run-selected', async (e: Event) => {
        const customEvent = e as CustomEvent<string | null>;
        if (customEvent.detail) {
          await this.loadEvals(customEvent.detail);
        } else {
          this.evals = [];
        }
      });

      const runName = this.getSelectedRun();
      if (runName) {
        await this.loadEvals(runName);
      }
    },

    destroy() {
      if (this.pollInterval) {
        clearInterval(this.pollInterval);
        this.pollInterval = null;
      }
    },

    getSelectedRun(): string | null {
      if (typeof Alpine !== 'undefined' && Alpine.store && Alpine.store('app')) {
        return Alpine.store('app')?.selectedRun || null;
      }
      // @ts-expect-error Alpine.js $root magic property
      return this.$root?.selectedRun || null;
    },

    async loadEvals(runName: string) {
      if (!runName) return;

      if (this.pollInterval) {
        clearInterval(this.pollInterval);
      }

      try {
        if (window.tauriInvoke) {
          this.evals = await window.tauriInvoke<Eval[]>('get_evals', { runName });
        }

        this.pollInterval = setInterval(async () => {
          if (window.tauriInvoke) {
            const currentRun = this.getSelectedRun();
            if (currentRun) {
              this.evals = await window.tauriInvoke<Eval[]>('get_evals', { runName: currentRun });
            }
          }
        }, 3000);
      } catch (e) {
        console.error('Failed to load evals:', e);
      }
    },

    async runEval() {
      const runName = this.getSelectedRun();
      if (!runName) return;

      this.isRunning = true;
      try {
        if (window.tauriInvoke) {
          await window.tauriInvoke('run_eval', { runName });
          await this.loadEvals(runName);
        }
      } catch (e) {
        const error = e as Error;
        console.error('Failed to run eval:', error);
        window.toast?.error(error.message || String(error), 'Eval failed');
      } finally {
        this.isRunning = false;
      }
    },

    formatTime(timestamp: string | null | undefined): string {
      if (!timestamp) return '';
      const date = new Date(timestamp);
      return date.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
    },
  };
}
