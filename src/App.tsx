import { type Component } from 'solid-js';
import { Layout } from './components/layout/Layout';
import './lib/toast';
import { initDevLogger } from './lib/dev-logger';
import { useAppBoot } from './lib/app-boot';
import { useMcpBrowserBridge } from './lib/mcp-browser-bridge';
import {
  AppProvider,
  DeliveryProvider,
  DeltaProvider,
  ProjectProvider,
  RouteProvider,
  RunsProvider,
  SelectionProvider,
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
            <RunsProvider>
              <SelectionProvider>
                <DeltaProvider>
                  <DeliveryProvider>
                    <Layout />
                  </DeliveryProvider>
                </DeltaProvider>
              </SelectionProvider>
            </RunsProvider>
          </RouteProvider>
        </WorkspaceProvider>
      </ProjectProvider>
    </AppProvider>
  );
};

export default App;
