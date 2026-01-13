/**
 * Settings modal Alpine component
 */

interface Settings {
  agentCommand: string;
  evalTimeout: number;
  autoLearn: boolean;
  maxIterations: number | null;
  userMessagePause: string;
  humanInTheLoop: boolean;
  compactionThreshold: number | null;
  compactionKeepMessages: number;
  contextWarningThreshold: number;
  coordinatorPort: number;
}

/**
 * Settings modal component
 */
export function settingsModal() {
  return {
    loading: false,
    saving: false,
    error: null as string | null,
    settings: {
      agentCommand: 'claude-code-acp',
      evalTimeout: 1800,
      autoLearn: false,
      maxIterations: null,
      userMessagePause: 'sender',
      humanInTheLoop: true,
      compactionThreshold: 10000,
      compactionKeepMessages: 40,
      contextWarningThreshold: 0.5,
      coordinatorPort: 19700,
    } as Settings,

    async loadSettings() {
      this.loading = true;
      this.error = null;

      try {
        if (window.tauriInvoke) {
          const config = await window.tauriInvoke<{
            runsDir: string;
            agentCommand: string[];
            evalTimeout: number;
            autoLearn: boolean;
            maxIterations: number | null;
            userMessagePause: string;
            humanInTheLoop: boolean;
            compactionThreshold: number | null;
            compactionKeepMessages: number;
            contextWarningThreshold: number;
            coordinatorPort: number;
          }>('get_config');

          this.settings = {
            agentCommand: config.agentCommand.join(' '),
            evalTimeout: config.evalTimeout,
            autoLearn: config.autoLearn,
            maxIterations: config.maxIterations,
            userMessagePause: config.userMessagePause,
            humanInTheLoop: config.humanInTheLoop,
            compactionThreshold: config.compactionThreshold,
            compactionKeepMessages: config.compactionKeepMessages,
            contextWarningThreshold: config.contextWarningThreshold,
            coordinatorPort: config.coordinatorPort,
          };
        }
      } catch (err) {
        const error = err as Error;
        console.error('[settingsModal] Error loading settings:', error);
        this.error = error.message || String(error);
      } finally {
        this.loading = false;
      }
    },

    async saveSettings() {
      this.saving = true;
      this.error = null;

      try {
        if (window.tauriInvoke) {
          // Parse agent command string into array
          const agentCommand = this.settings.agentCommand
            .split(/\s+/)
            .filter((s: string) => s.length > 0);

          await window.tauriInvoke('save_config', {
            updates: {
              agentCommand,
              evalTimeout: this.settings.evalTimeout,
              autoLearn: this.settings.autoLearn,
              maxIterations: this.settings.maxIterations || null,
              userMessagePause: this.settings.userMessagePause,
              humanInTheLoop: this.settings.humanInTheLoop,
              compactionThreshold: this.settings.compactionThreshold || null,
              compactionKeepMessages: this.settings.compactionKeepMessages,
              contextWarningThreshold: this.settings.contextWarningThreshold,
              coordinatorPort: this.settings.coordinatorPort,
            },
          });

          // Close modal on success
          // @ts-expect-error Alpine.js $root magic property
          this.$root.showSettings = false;

          // Show success toast
          if (window.toast) {
            window.toast.success('Settings saved successfully');
          }
        }
      } catch (err) {
        const error = err as Error;
        console.error('[settingsModal] Error saving settings:', error);
        this.error = error.message || String(error);
      } finally {
        this.saving = false;
      }
    },
  };
}
