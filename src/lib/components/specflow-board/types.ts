/**
 * TypeScript types for SpecFlow board
 */

export type TaskStatus = 'todo' | 'doing' | 'done' | 'blocked' | 'deleted';
export type SpecStatus = 'draft' | 'approved';
export type RowEvalStatus = 'pending' | 'pass' | 'fail';

export interface Row {
  id: string;
  islandId: string;
  position: number;
  // Spec
  specContent: string | null;
  specStatus: SpecStatus;
  // Task
  taskTitle: string | null;
  taskDescription: string | null;
  taskStatus: TaskStatus;
  taskWorker: string | null;
  taskBlockedBy: string[];
  // Eval
  evalCriterion: string | null;
  evalStatus: RowEvalStatus;
  evalResult: string | null;
  // Dispatch
  dispatched: boolean;
  runName: string | null;
  // Timestamps
  createdAt: string;
  updatedAt: string;
}

export interface Island {
  id: string;
  name: string;
  x: number;
  y: number;
  width: number;
  collapsed: boolean;
  summary: string | null;
  rows: Row[];
  createdAt: string;
  updatedAt: string;
}

export interface Wire {
  id: string;
  fromIslandId: string;
  toIslandId: string;
  createdAt: string;
}

export interface Bookmark {
  id: string;
  name: string;
  x: number;
  y: number;
  zoom: number;
  createdAt: string;
}

export interface Transform {
  x: number;
  y: number;
  k: number; // scale
}

export type LOAD = 'far' | 'mid' | 'near';

export interface EditingCell {
  islandId: string;
  rowId: string;
  column: 'spec' | 'task' | 'eval';
}

export interface DispatchWarning {
  message: string;
  rowIds: string[];
}

export interface DispatchResult {
  type: 'Success' | 'Warning';
  runName?: string;
  message?: string;
  rowIds?: string[];
}
