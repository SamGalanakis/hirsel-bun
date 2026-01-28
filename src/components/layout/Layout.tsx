/**
 * Main layout component that composes the application structure
 *
 * Uses SpecBoard as the primary view - a canvas-based delta board that shows:
 * - Draft tree (editable) with visual diff indicators
 * - Live tree (read-only) after first dispatch
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
import { ProjectSetup } from '../projects/ProjectSetup';
import { ProjectSettings } from '../projects/ProjectSettings';
import { SpecBoard } from '../specflow/SpecBoard';
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

  // Determine what content to show
  const showDraftEditor = () => {
    const detail = runs.runDetail();
    return runs.selectedRun() && detail?.status === 'draft';
  };

  const showRunDetail = () => {
    const detail = runs.runDetail();
    return runs.selectedRun() && detail?.status !== 'draft';
  };

  // Show SpecBoard as base when not viewing run details
  const showSpecBoard = () =>
    !project.showProjectSettings() &&
    !showDraftEditor() &&
    !showRunDetail();

  return (
    <>
      <SvgDefinitions />

      <TitleBar />

      {/* Main Content Area */}
      <main class="flex-1 flex overflow-hidden bg-pasture-900 relative">
        {/* SpecBoard - Primary canvas view (always rendered as base layer) */}
        <Show when={showSpecBoard()}>
          <SpecBoard />
        </Show>

        {/* Full-screen overlays (replace OneBoard) */}
        <Show when={project.showProjectSettings()}>
          <ProjectSettings />
        </Show>

        <Show when={showDraftEditor()}>
          <DraftEditor />
        </Show>

        <Show when={showRunDetail()}>
          <RunDetail />
        </Show>

        {/* Modal overlays (float above SpecBoard) */}
        <Show when={project.showProjectSetup()}>
          <ProjectSetup />
        </Show>
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
