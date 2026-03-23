type EventMap = {
  'show-worker-output': { projectId: number; routeId: number; workerName: string };
  'project-selected': number;
  'project-deselected': undefined;
  'project-created': unknown;
  'cancel-project-setup': undefined;
  'close-settings': undefined;
  'open-backend-settings':
    | {
        section?: 'connection' | 'llm' | 'services';
      }
    | undefined;
  'switch-tab': string;
  'theme-changed': { themeId: string; theme: unknown };
  'shortcuts-changed': undefined;
  'toggle-activity-fullscreen': undefined;
  'shortcut-action': string;
  'shepherd-focus-node': { id: string; name: string };
  'shepherd-editing-islands': unknown;
  'start-project-sync': { force?: boolean } | undefined;
};

export function emit<K extends keyof EventMap>(
  name: K,
  ...args: EventMap[K] extends undefined ? [] : [EventMap[K]]
) {
  window.dispatchEvent(new CustomEvent(name, { detail: args[0] }));
}

export function on<K extends keyof EventMap>(
  name: K,
  handler: (detail: EventMap[K]) => void,
): () => void {
  const listener = (e: Event) => handler((e as CustomEvent).detail);
  window.addEventListener(name, listener);
  return () => window.removeEventListener(name, listener);
}
