/**
 * Main layout component that composes the application structure
 *
 * Project-first workspace layout:
 * - Persistent Shepherd pane on the left
 * - Project focus surface on the right
 * - Machinery revealed underneath on demand
 */
import { type Component, Show, createEffect, onCleanup } from 'solid-js';
import { useProject, useRuns } from '../../stores';
import { TitleBar } from './TitleBar';
import { StatusBar } from './StatusBar';
import { SvgDefinitions } from './SvgDefinitions';
import { ProjectSelector } from './ProjectSelector';
import { RadialMenu } from './RadialMenu';
import { WelcomeScreen } from './WelcomeScreen';
import { ProjectSetup } from '../projects/ProjectSetup';
import { ProjectSettings } from '../projects/ProjectSettings';
import { ShepherdConsole } from '../chat/ShepherdConsole';
import { WorkerOutputViewer } from '../workers/WorkerOutputViewer';
import { AttachPicker } from '../modals/AttachPicker';
import { SettingsModal } from '../modals/SettingsModal';
import { ConfirmDialog } from '../modals/ConfirmDialog';
import { Toaster } from '../shared/Toaster';
import { DebugPanel } from '../shared/DebugPanel';
import { ProjectSurface } from '../project/ProjectSurface';

export const Layout: Component = () => {
  const project = useProject();
  const runs = useRuns();

  // Subscribe to runs polling
  createEffect(() => {
    const unsubscribe = runs.subscribe();
    onCleanup(unsubscribe);
  });

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
              <div class="w-[380px] min-w-[320px] max-w-[420px] border-l border-pasture-700/60 bg-pasture-800/90">
                <ShepherdConsole />
              </div>
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
      <AttachPicker />
      <SettingsModal />
      <ConfirmDialog />
      <Toaster />
      <DebugPanel />

      {/* Pie Menu - global overlay triggered by Alt+Space */}
      <RadialMenu />

      {/* Project Selector dropdown */}
      <ProjectSelector dropdownOnly />
    </>
  );
};
