/**
 * Main layout component that composes the application structure
 *
 * Uses SpecBoard as the primary view - a canvas-based delta board that shows:
 * - Draft tree (editable) with visual diff indicators
 * - Live tree (read-only) after first dispatch
 */
import { type Component, Show, createEffect, onCleanup } from 'solid-js';
import { useProject, useRuns } from '../../stores';
import { TitleBar } from './TitleBar';
import { StatusBar } from './StatusBar';
import { SvgDefinitions } from './SvgDefinitions';
import { ProjectSetup } from '../projects/ProjectSetup';
import { ProjectSettings } from '../projects/ProjectSettings';
import { SpecBoard } from '../specflow/SpecBoard';
import { GypMessenger } from '../chat/GypMessenger';
import { WorkerOutputViewer } from '../workers/WorkerOutputViewer';
import { AttachPicker } from '../modals/AttachPicker';
import { SettingsModal } from '../modals/SettingsModal';
import { SheepClicker } from '../fun/SheepClicker';
import { ConfirmDialog } from '../modals/ConfirmDialog';
import { Toaster } from '../shared/Toaster';
import { DebugPanel } from '../shared/DebugPanel';
import { DocsPanel, DocsFullView } from '../docs';

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
        {/* SpecBoard - Primary canvas view (always mounted to avoid re-init issues) */}
        <div class="flex-1 flex overflow-hidden">
          <div class="flex-1 flex flex-col overflow-hidden">
            <SpecBoard />
          </div>
          {/* Docs Panel - side panel overlay */}
          <Show when={project.docsOpen() && !project.docsFullScreen()}>
            <DocsPanel />
          </Show>
        </div>

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

      {/* Gyp Messenger (floats above status bar) */}
      <GypMessenger />

      {/* Modals and overlays */}
      <WorkerOutputViewer />
      <AttachPicker />
      <SettingsModal />
      <SheepClicker />
      <ConfirmDialog />
      <Toaster />
      <DebugPanel />
    </>
  );
};
