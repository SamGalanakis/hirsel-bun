/**
 * Application entry point - renders the SolidJS app
 */
import { render } from 'solid-js/web';
import App from './App';
import { initProfiling } from './lib/profiling';

// Import styles
import './styles/main.css';

// Initialize profiling (no-op if HIRSEL_PROFILING is not set)
initProfiling();

// Mount the application
const root = document.getElementById('app');
if (!root) {
  throw new Error('Root element #app not found');
}

render(() => <App />, root);
