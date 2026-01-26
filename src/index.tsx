/**
 * Application entry point - renders the SolidJS app
 */
import { render } from 'solid-js/web';
import App from './App';

// Import styles
import './styles/main.css';

// Mount the application
const root = document.getElementById('app');
if (!root) {
  throw new Error('Root element #app not found');
}

render(() => <App />, root);
