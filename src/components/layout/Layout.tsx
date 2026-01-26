/**
 * Main layout component that composes the application structure
 */
import { type Component, Show, createEffect, onCleanup, onMount } from 'solid-js';
import { useApp, useProject, useRuns, useSelection } from '../../stores';
import { initLucideIcons } from '../../lib/icons';
import { TitleBar } from './TitleBar';
import { StatusBar } from './StatusBar';
import { SvgDefinitions } from './SvgDefinitions';
import { RunListPanel } from '../runs/RunListPanel';
import { RunDetail } from '../runs/RunDetail';
import { DraftEditor } from '../runs/DraftEditor';
import { ProjectNav } from '../projects/ProjectNav';
import { ProjectSetup } from '../projects/ProjectSetup';
import { ProjectSettings } from '../projects/ProjectSettings';
import { SpecflowBoard } from '../specflow/SpecflowBoard';
import { GypMessenger } from '../chat/GypMessenger';
import { WorkerOutputViewer } from '../workers/WorkerOutputViewer';
import { AttachPicker } from '../modals/AttachPicker';
import { HelpModal } from '../modals/HelpModal';
import { SettingsModal } from '../modals/SettingsModal';
import { SheepClicker } from '../fun/SheepClicker';
import { ConfirmDialog } from '../modals/ConfirmDialog';
import { Toaster } from '../shared/Toaster';
import { DebugPanel } from '../shared/DebugPanel';

export const Layout: Component = () => {
  const app = useApp();
  const project = useProject();
  const runs = useRuns();
  const selection = useSelection();

  // Subscribe to runs polling
  createEffect(() => {
    const unsubscribe = runs.subscribe();
    onCleanup(unsubscribe);
  });

  // Reinitialize Lucide icons after renders
  onMount(() => {
    initLucideIcons();
  });

  createEffect(() => {
    // Re-run icons when key state changes
    void runs.selectedRun();
    void project.selectedProjectId();
    void app.showHelp();
    void app.showSettings();
    void app.aiChatOpen();

    // Delay to ensure DOM is updated
    queueMicrotask(() => {
      initLucideIcons();
    });
  });

  // Determine what main content to show
  const showRunList = () =>
    project.selectedProjectId() &&
    project.activeProjectView() === 'runs' &&
    !project.showProjectSettings();

  const showProjectSetup = () => project.showProjectSetup() && !project.showProjectSettings();
  const showProjectSettings = () => project.showProjectSettings();

  const showDraftEditor = () => {
    const detail = runs.runDetail();
    return runs.selectedRun() && detail?.status === 'draft';
  };

  const showRunDetail = () => {
    const detail = runs.runDetail();
    return runs.selectedRun() && detail?.status !== 'draft';
  };

  const showSpecflowBoard = () =>
    project.selectedProjectId() &&
    project.activeProjectView() === 'board' &&
    !runs.selectedRun() &&
    !project.showProjectSetup() &&
    !project.showProjectSettings();

  return (
    <>
      <SvgDefinitions />

      <TitleBar />

      {/* Main Content Area */}
      <main class="flex-1 flex overflow-hidden bg-pasture-900">
        <ProjectNav />

        {/* Main Content (changes based on nav selection) */}
        <div class="flex-1 flex flex-col overflow-hidden">
          {/* Inner content container */}
          <div class="flex-1 flex overflow-hidden">
            <Show when={showRunList()}>
              <RunListPanel />
            </Show>

            <Show when={showProjectSetup()}>
              <ProjectSetup />
            </Show>

            <Show when={showProjectSettings()}>
              <ProjectSettings />
            </Show>

            <Show when={showDraftEditor()}>
              <DraftEditor />
            </Show>

            <Show when={showSpecflowBoard()}>
              <SpecflowBoard />
            </Show>

            <Show when={showRunDetail()}>
              <RunDetail />
            </Show>
          </div>
        </div>

      </main>

      <StatusBar />

      {/* Gyp Messenger (floats above status bar) */}
      <GypMessenger />

      {/* Modals and overlays */}
      <WorkerOutputViewer />
      <AttachPicker />
      <HelpModal />
      <SettingsModal />
      <SheepClicker />
      <ConfirmDialog />
      <Toaster />
      <DebugPanel />
    </>
  );
};
