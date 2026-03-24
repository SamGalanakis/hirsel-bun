import { invoke } from '@tauri-apps/api/core';

type ConfigResponse = {
  backend?: {
    url?: string | null;
    apiKey?: string | null;
  };
};

const app = document.getElementById('app');

if (!app) {
  throw new Error('App root not found');
}

const state = {
  url: '',
  apiKey: '',
  error: '',
  connected: false,
};

async function loadConfig() {
  try {
    const config = await invoke<ConfigResponse>('get_config');
    state.url = config.backend?.url ?? '';
    state.apiKey = config.backend?.apiKey ?? '';
  } catch (error) {
    state.error = String(error);
  }
}

function backendBootstrapUrl(): string {
  const base = state.url.replace(/\/+$/, '');
  const url = new URL(`${base}/connect/bootstrap`);
  url.searchParams.set('api_key', state.apiKey);
  url.searchParams.set('return_to', '/app');
  return url.toString();
}

async function saveConnection(event: Event) {
  event.preventDefault();
  const form = event.currentTarget as HTMLFormElement;
  const formData = new FormData(form);
  state.url = String(formData.get('url') || '').trim();
  state.apiKey = String(formData.get('apiKey') || '').trim();
  state.error = '';

  if (!state.url || !state.apiKey) {
    state.error = 'Backend URL and API key are required.';
    render();
    return;
  }

  try {
    await invoke('check_backend_health', { url: state.url, apiKey: state.apiKey });
    await invoke('save_config', {
      updates: {
        backend: {
          url: state.url,
          apiKey: state.apiKey,
        },
      },
    });
    state.connected = true;
    render();
  } catch (error) {
    state.error = `Failed to reach backend: ${String(error)}`;
    render();
  }
}

async function disconnect() {
  await invoke('save_config', {
    updates: {
      backend: {
        url: null,
        apiKey: null,
      },
    },
  });
  state.url = '';
  state.apiKey = '';
  state.connected = false;
  state.error = '';
  render();
}

function connectMarkup() {
  return `
    <section class="connect">
      <div class="stack">
        <div>
          <div class="brand">HIRSEL</div>
          <h1>Connect wrapper</h1>
          <p class="muted">This desktop shell only stores your backend connection, then loads the backend-served Hirsel app.</p>
        </div>
        ${state.error ? `<p class="error">${state.error}</p>` : ''}
        <form id="connect-form" class="stack">
          <label class="stack">
            <span>Backend URL</span>
            <input class="toolbar-input" type="url" name="url" value="${state.url}" placeholder="http://127.0.0.1:8080" required />
          </label>
          <label class="stack">
            <span>API key</span>
            <input class="toolbar-input" type="password" name="apiKey" value="${state.apiKey}" required />
          </label>
          <button class="primary-btn" type="submit">Open backend</button>
        </form>
      </div>
    </section>
  `;
}

function connectedMarkup() {
  return `
    <div class="shell">
      <header class="topbar">
        <div class="brand">HIRSEL</div>
        <div class="topbar-actions">
          <input class="toolbar-input" type="text" value="${state.url}" readonly />
          <button id="disconnect-btn" class="ghost-btn" type="button">Change backend</button>
        </div>
      </header>
      <iframe class="frame" src="${backendBootstrapUrl()}" title="Hirsel backend app"></iframe>
    </div>
  `;
}

function attachHandlers() {
  const form = document.getElementById('connect-form');
  if (form) {
    form.addEventListener('submit', (event) => {
      void saveConnection(event);
    });
  }

  const disconnectButton = document.getElementById('disconnect-btn');
  if (disconnectButton) {
    disconnectButton.addEventListener('click', () => {
      void disconnect();
    });
  }
}

function render() {
  state.connected = Boolean(state.url && state.apiKey);
  app!.innerHTML = state.connected ? connectedMarkup() : connectMarkup();
  attachHandlers();
}

void (async () => {
  await loadConfig();
  render();
})();
