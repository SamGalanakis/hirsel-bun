type EventMap = {
  'show-worker-output': { runName: string; workerName: string };
  'run-selected': string | null;
  'draft-selected': string | null;
  'draft-created': undefined;
  'create-draft': undefined;
  'route-changed': { projectId: number; routeId: number };
  'project-selected': number;
  'project-deselected': undefined;
  'project-created': unknown;
  'cancel-project-setup': undefined;
  'board-refresh': number;
  'radial-start-shepherd': undefined;
  'radial-deliver': undefined;
  'radial-open-ide': undefined;
  'open-fork-dialog': undefined;
  'close-settings': undefined;
  'switch-tab': string;
  'theme-changed': { themeId: string; theme: unknown };
  'shortcuts-changed': undefined;
  'toggle-activity-fullscreen': undefined;
  'shortcut-action': string;
  'shepherd-focus-node': { id: string; name: string };
  'shepherd-editing-islands': unknown;
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
