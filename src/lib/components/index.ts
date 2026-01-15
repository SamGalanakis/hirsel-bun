/**
 * Alpine.js component exports for Hirsel GUI
 *
 * This module exports all Alpine component functions that can be
 * registered globally and used in HTML templates.
 */

// Core components
export { appState } from './app-state';

// Run management
export { runList } from './run-list';
export { runDetail } from './run-detail';
export { draftEditor } from './draft-editor';

// Worker and task panels (overview tab)
export { workerPanel } from './worker-panel';
export { taskPanel } from './task-panel';

// Activity log
export { activityLog } from './activity-log';

// Chat
export { chatPanel } from './chat-panel';
export { directChat } from './direct-chat';
export { permissionModal } from './permission-modal';

// AI message stream (shared component)
export { aiMessageStream } from './ai-message-stream';
export { workerOutputViewer } from './worker-output-viewer';

// Notifications
export { notifications } from './notifications';

// Tabs
export { tasksTab } from './tabs';

// Extras
export { sheepClickerGame } from './sheep-clicker';

// Modals
export { settingsModal } from './settings-modal';

// Toast
export { toastContainer } from './toast-container';
