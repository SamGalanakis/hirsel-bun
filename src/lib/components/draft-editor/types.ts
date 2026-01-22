/**
 * Type definitions for draft editor component
 */

import type { UnlistenFn } from '@tauri-apps/api/event';
import type { RunnerConfig, RunnerEntry } from '../../types';

/** Starting point type for new drafts */
export type StartingPointType = 'greenfield' | 'local' | 'git';

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
  // Starting point selection phase
  startingPointChosen: boolean; // false = show selection UI, true = show full editor
  // Starting point configuration (replaces projectPath)
  startingPointType: StartingPointType;
  localPath: string;
  gitUrl: string;
  gitBranch: string;
  // Workspace path after creation (read-only display)
  workspacePath: string | null;
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
  // Git URL validation state
  gitValidating: boolean;
  gitError: string | null;
  availableBranches: string[];
  gitValidateTimeout: ReturnType<typeof setTimeout> | null;
  // Field validation errors
  workerScaleError: string | null;
  timeLimitError: string | null;
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
  validateGitUrl(): Promise<void>;
  debouncedValidateGitUrl(): void;
  canStart(): boolean;
  confirmStartingPoint(): void;
  canConfirmStartingPoint(): boolean;
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
  browseLocalFolder(): Promise<void>;
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
 * Alpine.js runtime methods injected into components
 */
export interface AlpineRuntime {
  $nextTick(callback: () => void): void;
  $watch<T>(property: string, callback: (value: T) => void): void;
}

/**
 * Combined draft editor component type
 */
export type DraftEditorComponent = DraftEditorData & DraftEditorMethods & Partial<AlpineRuntime>;
