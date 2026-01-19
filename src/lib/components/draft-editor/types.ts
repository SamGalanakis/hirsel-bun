/**
 * Type definitions for draft editor component
 */

import type { UnlistenFn } from '@tauri-apps/api/event';
import type { RunnerConfig, RunnerEntry } from '../../types';

/**
 * Draft editor component data
 */
export interface DraftEditorData {
  runName: string | null;
  name: string;
  spec: string;
  eval: string;
  workerScale: string;
  timeLimitMinutes: number | null;
  timeLimitInput: string;
  humanInTheLoop: boolean;
  projectPath: string;
  saving: boolean;
  savingSpec: boolean;
  savingEval: boolean;
  starting: boolean;
  loading: boolean;
  error: string | null;
  saveTimeout: ReturnType<typeof setTimeout> | null;
  evalSaveTimeout: ReturnType<typeof setTimeout> | null;
  isEditing: boolean;
  activeTab: 'spec' | 'eval' | 'settings';
  previewMode: boolean;
  // Branch selection state
  selectedBranch: string;
  availableBranches: string[];
  repoValidating: boolean;
  repoError: string | null;
  repoIsRemote: boolean;
  normalizedRepoUrl: string;
  repoValidateTimeout: ReturnType<typeof setTimeout> | null;
  // Field validation errors
  workerScaleError: string | null;
  timeLimitError: string | null;
  // Project setup flags (non-git directory handling)
  needsDirCreate: boolean;
  needsGitInit: boolean;
  // File drop state
  specDragOver: boolean;
  evalDragOver: boolean;
  specDragCounter: number;
  // Assets path for image rendering
  assetsPath: string;
  evalDragCounter: number;
  // Tauri event unlisten function
  _unlistenDragDrop: UnlistenFn | null;
  // Cursor position tracking for insertion
  specCursorPos: number;
  evalCursorPos: number;
  // Runner selection
  runnerDefault: string | null;
  workerRunners: Record<string, string>;
  availableRunners: RunnerEntry[];
}

/**
 * Draft editor component methods
 */
export interface DraftEditorMethods {
  init(): void;
  destroy(): void;
  loadDraft(name: string): Promise<void>;
  clearDraft(): void;
  refreshFiles(): Promise<void>;
  saveSpec(): Promise<void>;
  saveEval(): Promise<void>;
  saveDraft(): Promise<void>;
  debouncedSaveSpec(): void;
  debouncedSaveEval(): void;
  debouncedSave(): void;
  startRun(): Promise<void>;
  deleteDraft(): Promise<void>;
  renameDraft(newName: string): Promise<void>;
  formatTimeLimitDisplay(minutes: number | null): string;
  handleNameEdit(): void;
  finishNameEdit(): void;
  renderMarkdown(content: string): string;
  validateProjectPath(): Promise<void>;
  debouncedValidateProjectPath(): void;
  onBranchChange(): void;
  canStart(): boolean;
  validateTimeLimit(): boolean;
  validateWorkerScale(): boolean;
  hasValidationErrors(): boolean;
  handleFileDrop(type: 'spec' | 'eval', event: DragEvent): Promise<void>;
  handleDragEnter(type: 'spec' | 'eval', event: DragEvent): void;
  handleDragOver(type: 'spec' | 'eval', event: DragEvent): void;
  handleDragLeave(type: 'spec' | 'eval', event: DragEvent): void;
  handleNativeFileDrop(paths: string[], position: { x: number; y: number }): Promise<void>;
  trackCursorPosition(type: 'spec' | 'eval', event: Event): void;
  openFilePicker(type: 'spec' | 'eval'): void;
  openAssets(): Promise<void>;
  // Runner methods
  loadRunners(): Promise<void>;
  getWorkerNames(): string[];
  getWorkerRunner(workerName: string): string;
  setWorkerRunner(workerName: string, runnerName: string): void;
  applyRunnerToAll(): void;
  hasCustomRunnerAssignments(): boolean;
  getRunnerIcon(runner: RunnerEntry | null): string;
  getRunnerDisplayName(runner: RunnerEntry | null): string;
}

/**
 * Combined draft editor component type
 */
export type DraftEditorComponent = DraftEditorData & DraftEditorMethods;
