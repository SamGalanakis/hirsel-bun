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
  connecting: false,
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
    await invoke('save_config', {
      updates: {
        backend: {
          url: state.url,
          apiKey: state.apiKey,
        },
      },
    });
    await invoke('open_backend_window');
  } catch (error) {
    state.connecting = false;
    state.error = `Failed to open backend: ${String(error)}`;
    render();
  }
}

function connectMarkup() {
  return `
    <section class="connect">
      <div class="stack">
        <div>
          <div class="brand">HIRSEL</div>
          <h1>${state.connecting ? 'Opening backend…' : 'Connect Hirsel'}</h1>
          <p class="muted">${
            state.connecting
              ? `Connecting to ${state.url}`
              : 'This shell only appears when backend setup is needed.'
          }</p>
        </div>
        ${state.error ? `<p class="error">${state.error}</p>` : ''}
        <form id="connect-form" class="stack" ${state.connecting ? 'hidden' : ''}>
          <label class="stack">
            <span>Backend URL</span>
            <input class="toolbar-input" type="url" name="url" value="${state.url}" placeholder="http://127.0.0.1:8080" required />
          </label>
          <label class="stack">
            <span>API key</span>
            <input class="toolbar-input" type="password" name="apiKey" value="${state.apiKey}" required />
          </label>
          <button class="primary-btn" type="submit">Save and open</button>
        </form>
      </div>
    </section>
  `;
}

function attachHandlers() {
  const form = document.getElementById('connect-form');
  if (form) {
    form.addEventListener('submit', (event) => {
      void saveConnection(event);
    });
  }
}

function render() {
  app!.innerHTML = connectMarkup();
  attachHandlers();
}

void (async () => {
  await loadConfig();
  render();
})();
