import { type Component } from 'solid-js';
import { Layout } from './components/layout/Layout';
import './lib/toast';
import { initDevLogger } from './lib/dev-logger';
import { useAppBoot } from './lib/app-boot';
import { useMcpBrowserBridge } from './lib/mcp-browser-bridge';
import {
  AppProvider,
  DeliveryProvider,
  ProjectProvider,
  RouteProvider,
  WorkspaceProvider,
} from './stores';

// Initialize dev logger early
initDevLogger();

const App: Component = () => {
  useAppBoot();
  useMcpBrowserBridge();

  return (
    <AppProvider>
      <ProjectProvider>
        <WorkspaceProvider>
          <RouteProvider>
            <DeliveryProvider>
              <Layout />
            </DeliveryProvider>
          </RouteProvider>
        </WorkspaceProvider>
      </ProjectProvider>
    </AppProvider>
  );
};

export default App;
