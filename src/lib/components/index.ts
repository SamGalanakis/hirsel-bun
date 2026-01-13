/**
 * Alpine.js component exports for Hirsel GUI
 *
 * This module exports all Alpine component functions that can be
 * registered globally and used in HTML templates.
 */

// Core components
export { toastContainer, initToastApi } from './toast';
export { appState } from './app-state';

// Run management
export { runList } from './run-list';
export { runDetail } from './run-detail';

// Worker and task panels (overview tab)
export { workerPanel } from './worker-panel';
export { taskPanel } from './task-panel';

// Activity log
export { activityLog } from './activity-log';

// Chat
export { chatPanel } from './chat-panel';

// Tabs
export { tasksTab, evalTab } from './tabs';

// Extras
export { sheepClickerGame } from './sheep-clicker';

// Modals
export { settingsModal } from './settings-modal';
