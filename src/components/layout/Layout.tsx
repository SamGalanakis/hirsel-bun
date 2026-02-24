/**
 * Main layout component that composes the application structure
 *
 * Uses SpecBoard as the primary view - a canvas-based delta board that shows:
 * - Draft tree (editable) with visual diff indicators
 * - Live tree (read-only) after first dispatch
 */
import { type Component, Show, createEffect, onCleanup } from 'solid-js';
import { useApp, useProject, useRuns } from '../../stores';
import { Icon } from '../shared';
import { TitleBar } from './TitleBar';
import { StatusBar } from './StatusBar';
import { SvgDefinitions } from './SvgDefinitions';
import { LeftDrawer } from './LeftDrawer';
import { ProjectSelector } from './ProjectSelector';
import { RadialMenu } from './RadialMenu';
import { WelcomeScreen } from './WelcomeScreen';
import { ProjectSetup } from '../projects/ProjectSetup';
import { ProjectSettings } from '../projects/ProjectSettings';
import { SpecBoard } from '../specflow/SpecBoard';
import { ShepherdConsole } from '../chat/ShepherdConsole';
import { WorkerOutputViewer } from '../workers/WorkerOutputViewer';
import { AttachPicker } from '../modals/AttachPicker';
import { SettingsModal } from '../modals/SettingsModal';
import { SheepClicker } from '../fun/SheepClicker';
import { ConfirmDialog } from '../modals/ConfirmDialog';
import { Toaster } from '../shared/Toaster';
import { DebugPanel } from '../shared/DebugPanel';
import { DocsPanel, DocsFullView } from '../docs';
import { MessagingPanel } from '../messaging';

export const Layout: Component = () => {
  const app = useApp();
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
          {/* Left Drawer - Navigation sidebar */}
          <Show when={project.selectedProject()}>
            <LeftDrawer />
          </Show>

          {/* SpecBoard - Primary canvas view, keyed by project to reset state on switch */}
          <div class="flex-1 flex overflow-hidden">
            <div class="flex-1 flex flex-col overflow-hidden relative">
              <Show when={project.selectedProjectId()} keyed>
                {(_projectId) => <SpecBoard />}
              </Show>
            </div>
            {/* Right side panels */}
            <Show when={project.docsOpen() && !project.docsFullScreen()}>
              <DocsPanel />
            </Show>
            <Show when={project.sheepfoldOpen()}>
              <MessagingPanel />
            </Show>
          </div>
        </Show>

        {/* ProjectSettings - modal overlay on top of SpecBoard */}
        <Show when={project.showProjectSettings()}>
          <ProjectSettings />
        </Show>

        {/* Modal overlays (float above SpecBoard) */}
        <Show when={project.showProjectSetup()}>
          <ProjectSetup />
        </Show>

        {/* Docs Full Screen View */}
        <Show when={project.docsFullScreen()}>
          <DocsFullView />
        </Show>
      </main>

      <StatusBar />

      {/* Shepherd Console (floats above status bar) */}
      <ShepherdConsole />

      {/* Modals and overlays */}
      <WorkerOutputViewer />
      <AttachPicker />
      <SettingsModal />
      <SheepClicker />
      <ConfirmDialog />
      <Toaster />
      <DebugPanel />

      {/* Pie Menu - global overlay triggered by Alt+Space */}
      <RadialMenu />

      {/* Project Selector dropdown (triggered from LeftDrawer) */}
      <ProjectSelector dropdownOnly />
    </>
  );
};
