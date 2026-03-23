/**
 * Main layout component that composes the application structure
 *
 * Project-first workspace layout:
 * - Persistent Shepherd pane on the left
 * - Project focus surface on the right
 * - Machinery revealed underneath on demand
 */
import { type Component, Show } from 'solid-js';
import { useProject, useWorkspace } from '../../stores';
import { TitleBar } from './TitleBar';
import { StatusBar } from './StatusBar';
import { SvgDefinitions } from './SvgDefinitions';
import { ProjectSelector } from './ProjectSelector';
import { WelcomeScreen } from './WelcomeScreen';
import { ProjectSetup } from '../projects/ProjectSetup';
import { ProjectSettings } from '../projects/ProjectSettings';
import { ShepherdConsole } from '../chat/ShepherdConsole';
import { WorkerOutputViewer } from '../workers/WorkerOutputViewer';
import { SettingsModal } from '../modals/SettingsModal';
import { ConfirmDialog } from '../modals/ConfirmDialog';
import { Toaster } from '../shared/Toaster';
import { DebugPanel } from '../shared/DebugPanel';
import { ProjectSurface } from '../project/ProjectSurface';
import { Icon } from '../shared';

export const Layout: Component = () => {
  const project = useProject();
  const workspace = useWorkspace();

  return (
    <>
      <SvgDefinitions />

      <TitleBar />

      {/* Main Content Area */}
      <main class="flex-1 flex overflow-hidden bg-pasture-900 relative">
        {/* Welcome screen - only when no projects exist at all */}
        <Show when={!project.loading() && project.projects().length === 0}>
          <WelcomeScreen />
        </Show>

        {/* Normal UI - when projects exist */}
        <Show when={project.projects().length > 0}>
          <div class="flex-1 flex overflow-hidden">
            <div class="flex-1 flex min-w-0 overflow-hidden">
              <Show when={project.selectedProjectId()}>
                <ProjectSurface />
              </Show>
            </div>
            <Show when={project.selectedProject()}>
              <Show
                when={!workspace.shepherdMinimized()}
                fallback={
                  <button
                    type="button"
                    onClick={() => workspace.setShepherdMinimized(false)}
                    class="shrink-0 w-8 border-l border-pasture-700/30 bg-pasture-800/60 flex flex-col items-center pt-3 gap-2 hover:bg-pasture-700/40 cursor-pointer"
                    title="Expand chat"
                  >
                    <Icon name="message-square" class="w-3.5 h-3.5 text-wool-600" />
                    <span class="text-[8px] text-wool-700 uppercase tracking-[0.2em] [writing-mode:vertical-lr]">
                      Chat
                    </span>
                  </button>
                }
              >
                <div class="w-[380px] min-w-[320px] max-w-[420px] border-l border-pasture-700/30 bg-pasture-800/90">
                  <ShepherdConsole />
                </div>
              </Show>
            </Show>
          </div>
        </Show>

        <Show when={project.showProjectSettings()}>
          <ProjectSettings />
        </Show>

        <Show when={project.showProjectSetup()}>
          <ProjectSetup />
        </Show>
      </main>

      <StatusBar />

      {/* Modals and overlays */}
      <WorkerOutputViewer />
      <SettingsModal />
      <ConfirmDialog />
      <Toaster />
      <DebugPanel />

      {/* Project Selector dropdown */}
      <ProjectSelector dropdownOnly />
    </>
  );
};
