/**
 * SpecFlow Board - Alpine.js component for the spatial canvas
 *
 * A 2D infinite canvas where project features are laid out as "Islands."
 * Each island contains a Trifecta Grid (Spec | Tasks | Eval).
 */

import { select } from 'd3-selection';
import { zoom, zoomIdentity } from 'd3-zoom';
import type { D3ZoomEvent, ZoomBehavior } from 'd3-zoom';
import { CanvasRenderer } from './canvas-renderer';
import type {
  Bookmark,
  DispatchResult,
  DispatchWarning,
  EditingCell,
  Island,
  LOAD,
  Row,
  Transform,
  Wire,
} from './types';

// Re-export types
export * from './types';

type RunMode = 'edit' | 'dispatch';

interface Command {
  id: string;
  label: string;
  icon: string;
  shortcut?: string;
  action: () => void;
}

/**
 * SpecFlow board Alpine.js component
 */
export function specflowBoard() {
  return {
    // State
    projectId: null as number | null,
    islands: [] as Island[],
    wires: [] as Wire[],
    bookmarks: [] as Bookmark[],
    loading: true,
    error: null as string | null,

    // Transform state
    transform: { x: 0, y: 0, k: 1 } as Transform,
    load: 'near' as LOAD,

    // Selection state
    selectedIslandId: null as string | null,
    editingCell: null as EditingCell | null,

    // Run mode
    runMode: 'edit' as RunMode,
    selectedRowsForDispatch: new Set<string>(),
    dispatchWarning: null as DispatchWarning | null,

    // AI prompt bubble
    showAiPrompt: false,
    aiPromptX: 0,
    aiPromptY: 0,
    aiPromptText: '',
    aiPromptContext: null as {
      type: 'new-island' | 'row-insert';
      islandId?: string;
      afterRowId?: string;
    } | null,

    // Command palette
    showCommandPalette: false,
    commandQuery: '',
    filteredIslands: [] as Island[],
    filteredBookmarks: [] as Bookmark[],
    filteredCommands: [] as Command[],

    // Context menus
    canvasMenuVisible: false,
    islandMenuVisible: false,
    rowMenuVisible: false,
    menuX: 0,
    menuY: 0,
    menuWorldX: 0,
    menuWorldY: 0,
    menuIsland: null as Island | null,
    menuRow: null as Row | null,

    // Internal
    _canvasRenderer: null as CanvasRenderer | null,
    _zoomBehavior: null as ZoomBehavior<HTMLDivElement, unknown> | null,
    _eventCleanups: [] as (() => void)[],
    _rafId: null as number | null,

    // ========== LIFECYCLE ==========

    async init() {
      // Ensure clean state on component initialization
      this.resetAllState();

      // Listen for project selection events
      const projectSelectedHandler = (e: Event) => {
        const ce = e as CustomEvent<number>;
        if (ce.detail) {
          this.resetAllState(); // Reset all state before switching projects
          this.projectId = ce.detail;
          this.loadProject();
        }
      };
      window.addEventListener('project-selected', projectSelectedHandler);
      this._eventCleanups.push(() =>
        window.removeEventListener('project-selected', projectSelectedHandler),
      );

      // Listen for project deselection
      const projectDeselectedHandler = () => {
        this.resetAllState(); // Reset all state when deselecting
        this.projectId = null;
        this.islands = [];
        this.wires = [];
        this.bookmarks = [];
      };
      window.addEventListener('project-deselected', projectDeselectedHandler);
      this._eventCleanups.push(() =>
        window.removeEventListener('project-deselected', projectDeselectedHandler),
      );

      // Listen for board-updated events (from daemon sync)
      const boardUpdatedHandler = (e: Event) => {
        const ce = e as CustomEvent<{ projectId: number }>;
        if (ce.detail?.projectId === this.projectId) {
          this.loadProject();
        }
      };
      window.addEventListener('board-updated', boardUpdatedHandler);
      this._eventCleanups.push(() =>
        window.removeEventListener('board-updated', boardUpdatedHandler),
      );

      // Get initial project from appState if already set
      const appStateEl = document.body as HTMLElement & {
        _x_dataStack?: Array<{ selectedProjectId: number | null }>;
      };
      if (appStateEl._x_dataStack?.[0]?.selectedProjectId) {
        this.projectId = appStateEl._x_dataStack[0].selectedProjectId;
        await this.loadProject();
      }

      const refs = (this as unknown as { $refs: Record<string, HTMLElement> }).$refs;

      // Initialize canvas renderer
      const canvas = refs.canvas as HTMLCanvasElement;
      if (canvas) {
        this._canvasRenderer = new CanvasRenderer(canvas);
      }

      // Initialize d3-zoom
      this.initZoom();

      // Handle resize
      const viewport = refs.viewport as HTMLElement;
      if (viewport) {
        const resizeObserver = new ResizeObserver(() => this.handleResize());
        resizeObserver.observe(viewport);
        this._eventCleanups.push(() => resizeObserver.disconnect());
      }

      // Initial resize
      this.handleResize();

      // Initialize command list
      this.initCommands();
    },

    destroy() {
      this._eventCleanups.forEach((fn) => fn());
      this._eventCleanups = [];
      if (this._rafId) cancelAnimationFrame(this._rafId);
    },

    // ========== DATA LOADING ==========

    async loadProject() {
      if (!this.projectId) return;

      this.loading = true;
      this.error = null;
      this.hideAllMenus();

      try {
        const [islands, wires, bookmarks] = await Promise.all([
          window.tauriInvoke<Island[]>('get_project_islands', { projectId: this.projectId }),
          window.tauriInvoke<Wire[]>('get_wires', { projectId: this.projectId }),
          window.tauriInvoke<Bookmark[]>('get_bookmarks', { projectId: this.projectId }),
        ]);

        this.islands = islands;
        this.wires = wires;
        this.bookmarks = bookmarks;
        this.render();
      } catch (e) {
        console.error('Failed to load project:', e);
        this.error = String(e);
        window.toast?.error('Failed to load project');
      } finally {
        this.loading = false;
      }
    },

    // ========== ZOOM/PAN ==========

    initZoom() {
      const refs = (this as unknown as { $refs: Record<string, HTMLElement> }).$refs;
      const viewport = refs.viewport as HTMLDivElement;
      if (!viewport) return;

      this._zoomBehavior = zoom<HTMLDivElement, unknown>()
        .scaleExtent([0.1, 3])
        .filter((event: Event) => {
          const target = event.target as HTMLElement;
          // Don't capture zoom on interactive elements
          if (target.closest('textarea, input, button')) return false;
          if (event.type === 'wheel') return true;
          const mouseEvent = event as MouseEvent;
          return mouseEvent.button === 1 || !target.closest('.island');
        })
        .on('zoom', (event: D3ZoomEvent<HTMLDivElement, unknown>) => {
          const { x, y, k } = event.transform;
          this.transform = { x, y, k };
          this.load = k < 0.4 ? 'far' : k < 0.8 ? 'mid' : 'near';
          this.scheduleRender();
        });

      select(viewport).call(this._zoomBehavior);
    },

    scheduleRender() {
      if (this._rafId) return;
      this._rafId = requestAnimationFrame(() => {
        this._rafId = null;
        this.render();
      });
    },

    render() {
      if (!this._canvasRenderer) return;
      const refs = (this as unknown as { $refs: Record<string, HTMLElement> }).$refs;
      const viewport = refs.viewport as HTMLElement;
      if (!viewport) return;
      this._canvasRenderer.render(
        this.transform,
        this.wires,
        this.islands,
        viewport.clientWidth,
        viewport.clientHeight,
      );
    },

    handleResize() {
      const refs = (this as unknown as { $refs: Record<string, HTMLElement> }).$refs;
      const canvas = refs.canvas as HTMLCanvasElement;
      const viewport = refs.viewport as HTMLElement;
      if (!canvas || !viewport) return;

      const dpr = window.devicePixelRatio || 1;
      canvas.width = viewport.clientWidth * dpr;
      canvas.height = viewport.clientHeight * dpr;
      canvas.style.width = `${viewport.clientWidth}px`;
      canvas.style.height = `${viewport.clientHeight}px`;

      this._canvasRenderer?.resize(viewport.clientWidth, viewport.clientHeight);
      this.render();
    },

    // ========== ZOOM CONTROLS ==========

    zoomIn() {
      if (!this._zoomBehavior) return;
      const refs = (this as unknown as { $refs: Record<string, HTMLElement> }).$refs;
      const viewport = refs.viewport as HTMLDivElement;
      if (!viewport) return;
      select<HTMLDivElement, unknown>(viewport).call(this._zoomBehavior.scaleBy, 1.3);
    },

    zoomOut() {
      if (!this._zoomBehavior) return;
      const refs = (this as unknown as { $refs: Record<string, HTMLElement> }).$refs;
      const viewport = refs.viewport as HTMLDivElement;
      if (!viewport) return;
      select<HTMLDivElement, unknown>(viewport).call(this._zoomBehavior.scaleBy, 0.7);
    },

    fitAll() {
      if (this.islands.length === 0 || !this._zoomBehavior) return;

      let minX = Number.POSITIVE_INFINITY;
      let minY = Number.POSITIVE_INFINITY;
      let maxX = Number.NEGATIVE_INFINITY;
      let maxY = Number.NEGATIVE_INFINITY;

      for (const island of this.islands) {
        minX = Math.min(minX, island.x);
        minY = Math.min(minY, island.y);
        maxX = Math.max(maxX, island.x + island.width);
        maxY = Math.max(maxY, island.y + 300);
      }

      const refs = (this as unknown as { $refs: Record<string, HTMLElement> }).$refs;
      const viewport = refs.viewport as HTMLElement;
      if (!viewport) return;

      const padding = 50;
      const scaleX = (viewport.clientWidth - padding * 2) / (maxX - minX);
      const scaleY = (viewport.clientHeight - padding * 2) / (maxY - minY);
      const scale = Math.min(scaleX, scaleY, 1);

      const centerX = (minX + maxX) / 2;
      const centerY = (minY + maxY) / 2;

      const newTransform = zoomIdentity
        .translate(viewport.clientWidth / 2, viewport.clientHeight / 2)
        .scale(scale)
        .translate(-centerX, -centerY);

      select<HTMLDivElement, unknown>(viewport as HTMLDivElement).call(
        this._zoomBehavior.transform,
        newTransform,
      );
    },

    // ========== ISLAND OPERATIONS ==========

    handleIslandMouseDown(event: MouseEvent, island: Island) {
      // Header drag
      if ((event.target as HTMLElement).closest('.island-header')) {
        this.startIslandDrag(island, event);
      }
      this.selectedIslandId = island.id;
    },

    startIslandDrag(island: Island, event: MouseEvent) {
      const startX = event.clientX;
      const startY = event.clientY;
      const origX = island.x;
      const origY = island.y;

      const onMove = (e: MouseEvent) => {
        const dx = (e.clientX - startX) / this.transform.k;
        const dy = (e.clientY - startY) / this.transform.k;
        island.x = origX + dx;
        island.y = origY + dy;
        this.render();
      };

      const onUp = async () => {
        window.removeEventListener('mousemove', onMove);
        window.removeEventListener('mouseup', onUp);

        try {
          await window.tauriInvoke('update_island', {
            projectId: this.projectId,
            islandId: island.id,
            x: island.x,
            y: island.y,
          });
        } catch (e) {
          console.error('Failed to save island position:', e);
        }
      };

      window.addEventListener('mousemove', onMove);
      window.addEventListener('mouseup', onUp);
    },

    async toggleCollapse(island: Island) {
      island.collapsed = !island.collapsed;
      try {
        await window.tauriInvoke('update_island', {
          projectId: this.projectId,
          islandId: island.id,
          collapsed: island.collapsed,
        });
      } catch (e) {
        console.error('Failed to toggle collapse:', e);
      }
    },

    startEditingIslandName(_island: Island) {
      // Not yet implemented
    },

    showCanvasContextMenu(event: MouseEvent) {
      // Explicitly clear all menu state first
      this.canvasMenuVisible = false;
      this.islandMenuVisible = false;
      this.rowMenuVisible = false;
      this.menuIsland = null;
      this.menuRow = null;

      // Don't show canvas menu if clicking on an island
      if ((event.target as HTMLElement).closest('.island')) return;

      // Convert screen coords to world coords
      const refs = (this as unknown as { $refs: Record<string, HTMLElement> }).$refs;
      const viewport = refs.viewport as HTMLElement;
      if (!viewport) return;

      const rect = viewport.getBoundingClientRect();
      const screenX = event.clientX - rect.left;
      const screenY = event.clientY - rect.top;

      this.menuWorldX = (screenX - this.transform.x) / this.transform.k;
      this.menuWorldY = (screenY - this.transform.y) / this.transform.k;
      this.menuX = event.clientX;
      this.menuY = event.clientY;

      // Show canvas menu only (ensure others remain hidden)
      this.islandMenuVisible = false;
      this.rowMenuVisible = false;
      this.canvasMenuVisible = true;
    },

    hideAllMenus() {
      this.canvasMenuVisible = false;
      this.islandMenuVisible = false;
      this.rowMenuVisible = false;
      this.menuIsland = null;
      this.menuRow = null;
    },

    /**
     * Reset all component state (for clean slate on project switch)
     */
    resetAllState() {
      // Menu state
      this.canvasMenuVisible = false;
      this.islandMenuVisible = false;
      this.rowMenuVisible = false;
      this.menuX = 0;
      this.menuY = 0;
      this.menuWorldX = 0;
      this.menuWorldY = 0;
      this.menuIsland = null;
      this.menuRow = null;

      // Selection state
      this.selectedIslandId = null;
      this.editingCell = null;
      this.selectedRowsForDispatch = new Set();

      // Command palette
      this.showCommandPalette = false;
      this.commandQuery = '';

      // AI prompt
      this.showAiPrompt = false;
      this.aiPromptText = '';
      this.aiPromptContext = null;

      // Dispatch
      this.dispatchWarning = null;
    },

    showIslandMenu(island: Island, event: MouseEvent) {
      this.hideAllMenus();
      this.menuIsland = island;
      this.menuX = event.clientX;
      this.menuY = event.clientY;
      this.islandMenuVisible = true;
    },

    async createIslandAtCenter() {
      if (!this.projectId) return;

      const refs = (this as unknown as { $refs: Record<string, HTMLElement> }).$refs;
      const viewport = refs.viewport as HTMLElement;
      if (!viewport) return;

      const worldX = (viewport.clientWidth / 2 - this.transform.x) / this.transform.k - 200;
      const worldY = (viewport.clientHeight / 2 - this.transform.y) / this.transform.k - 100;

      // Show AI prompt bubble for new island creation
      this.showAiPromptForNewIsland(worldX, worldY);
    },

    async createIslandAtPosition() {
      if (!this.projectId) return;
      this.canvasMenuVisible = false;

      // Show AI prompt bubble at the right-click position
      this.showAiPromptForNewIsland(this.menuWorldX - 200, this.menuWorldY);
    },

    showAiPromptForNewIsland(worldX: number, worldY: number) {
      // Convert world coords back to screen for the bubble position
      const refs = (this as unknown as { $refs: Record<string, HTMLElement> }).$refs;
      const viewport = refs.viewport as HTMLElement;
      if (!viewport) return;

      const rect = viewport.getBoundingClientRect();
      const screenX = worldX * this.transform.k + this.transform.x + rect.left;
      const screenY = worldY * this.transform.k + this.transform.y + rect.top;

      this.aiPromptX = screenX;
      this.aiPromptY = screenY;
      this.aiPromptText = '';
      this.aiPromptContext = { type: 'new-island' };
      this.showAiPrompt = true;
    },

    async submitAiPrompt() {
      if (!this.projectId || !this.aiPromptText.trim()) {
        this.showAiPrompt = false;
        return;
      }

      const text = this.aiPromptText.trim();
      const context = this.aiPromptContext;
      this.showAiPrompt = false;
      this.aiPromptText = '';

      if (context?.type === 'new-island') {
        // Create island with the prompt text as name (for now, later can be AI-generated)
        const refs = (this as unknown as { $refs: Record<string, HTMLElement> }).$refs;
        const viewport = refs.viewport as HTMLElement;
        if (!viewport) return;

        const rect = viewport.getBoundingClientRect();
        const worldX = (this.aiPromptX - rect.left - this.transform.x) / this.transform.k;
        const worldY = (this.aiPromptY - rect.top - this.transform.y) / this.transform.k;

        try {
          const island = await window.tauriInvoke<Island>('create_island', {
            projectId: this.projectId,
            name: text.length > 50 ? `${text.substring(0, 47)}...` : text,
            x: worldX,
            y: worldY,
            width: 400,
          });

          this.islands.push(island);
          this.selectedIslandId = island.id;
          this.render();
        } catch (e) {
          console.error('Failed to create island:', e);
          window.toast?.error('Failed to create island');
        }
      }
    },

    cancelAiPrompt() {
      this.showAiPrompt = false;
      this.aiPromptText = '';
      this.aiPromptContext = null;
    },

    async duplicateIsland(island: Island | null) {
      if (!island || !this.projectId) return;
      this.islandMenuVisible = false;
      window.toast?.info('Duplicate not yet implemented');
    },

    async saveAsBookmark(island: Island | null) {
      if (!island || !this.projectId) return;
      this.islandMenuVisible = false;

      try {
        const bookmark = await window.tauriInvoke<Bookmark>('save_bookmark', {
          projectId: this.projectId,
          name: island.name,
          x: island.x + island.width / 2,
          y: island.y + 150,
          zoom: this.transform.k,
        });
        this.bookmarks.push(bookmark);
        window.toast?.success('Bookmark saved');
      } catch (e) {
        console.error('Failed to save bookmark:', e);
        window.toast?.error('Failed to save bookmark');
      }
    },

    async deleteIsland(island: Island | null) {
      if (!island || !this.projectId) return;
      this.islandMenuVisible = false;

      try {
        await window.tauriInvoke('delete_island', {
          projectId: this.projectId,
          islandId: island.id,
        });
        this.islands = this.islands.filter((i) => i.id !== island.id);
        if (this.selectedIslandId === island.id) {
          this.selectedIslandId = null;
        }
        this.render();
        window.toast?.success('Island deleted');
      } catch (e) {
        console.error('Failed to delete island:', e);
        window.toast?.error('Failed to delete island');
      }
    },

    startResizing(island: Island, event: MouseEvent) {
      event.preventDefault();
      event.stopPropagation();
      const startX = event.clientX;
      const startWidth = island.width;

      const onMove = (e: MouseEvent) => {
        const dx = (e.clientX - startX) / this.transform.k;
        island.width = Math.max(200, startWidth + dx);
      };

      const onUp = async () => {
        window.removeEventListener('mousemove', onMove);
        window.removeEventListener('mouseup', onUp);

        try {
          await window.tauriInvoke('update_island', {
            projectId: this.projectId,
            islandId: island.id,
            width: island.width,
          });
        } catch (e) {
          console.error('Failed to save island width:', e);
        }
      };

      window.addEventListener('mousemove', onMove);
      window.addEventListener('mouseup', onUp);
    },

    // ========== ROW OPERATIONS ==========

    async addRow(island: Island) {
      if (!this.projectId) return;

      try {
        const row = await window.tauriInvoke<Row>('create_row', {
          projectId: this.projectId,
          islandId: island.id,
        });

        island.rows.push(row);
      } catch (e) {
        console.error('Failed to create row:', e);
        window.toast?.error('Failed to add row');
      }
    },

    showRowMenu(island: Island, row: Row, event: MouseEvent) {
      this.hideAllMenus();
      this.menuIsland = island;
      this.menuRow = row;
      this.menuX = event.clientX;
      this.menuY = event.clientY;
      this.rowMenuVisible = true;
    },

    async setTaskStatus(row: Row | null, status: string) {
      if (!row || !this.projectId) return;
      this.rowMenuVisible = false;

      try {
        await window.tauriInvoke('update_row', {
          projectId: this.projectId,
          rowId: row.id,
          taskStatus: status,
        });
        row.taskStatus = status as Row['taskStatus'];
      } catch (e) {
        console.error('Failed to update task status:', e);
        window.toast?.error('Failed to update status');
      }
    },

    async duplicateRow(_island: Island | null, _row: Row | null) {
      this.rowMenuVisible = false;
      window.toast?.info('Duplicate not yet implemented');
    },

    editBlockedBy(_row: Row | null) {
      this.rowMenuVisible = false;
      window.toast?.info('Edit blockers not yet implemented');
    },

    async deleteRow(island: Island | null, row: Row | null) {
      if (!island || !row || !this.projectId) return;
      this.rowMenuVisible = false;

      try {
        await window.tauriInvoke('delete_row', {
          projectId: this.projectId,
          rowId: row.id,
        });
        island.rows = island.rows.filter((r) => r.id !== row.id);
      } catch (e) {
        console.error('Failed to delete row:', e);
        window.toast?.error('Failed to delete row');
      }
    },

    // ========== CELL EDITING ==========

    isEditingCell(islandId: string, rowId: string, column: 'spec' | 'task' | 'eval') {
      return (
        this.editingCell?.islandId === islandId &&
        this.editingCell?.rowId === rowId &&
        this.editingCell?.column === column
      );
    },

    startEditingCell(islandId: string, rowId: string, column: 'spec' | 'task' | 'eval') {
      this.editingCell = { islandId, rowId, column };
    },

    async saveCell(
      islandId: string,
      rowId: string,
      column: 'spec' | 'task' | 'eval',
      value: string,
    ) {
      if (!this.projectId) return;

      const updates: Record<string, string | null> = {};
      if (column === 'spec') updates.specContent = value || null;
      else if (column === 'task') updates.taskTitle = value || null;
      else if (column === 'eval') updates.evalCriterion = value || null;

      try {
        await window.tauriInvoke('update_row', {
          projectId: this.projectId,
          rowId,
          ...updates,
        });

        // Update local state
        const island = this.islands.find((i) => i.id === islandId);
        const row = island?.rows.find((r) => r.id === rowId);
        if (row) {
          if (column === 'spec') row.specContent = value || null;
          else if (column === 'task') row.taskTitle = value || null;
          else if (column === 'eval') row.evalCriterion = value || null;
        }
      } catch (e) {
        console.error('Failed to save cell:', e);
        window.toast?.error('Failed to save');
      }

      this.editingCell = null;
    },

    cancelEditing() {
      this.editingCell = null;
    },

    // ========== RUN MODE ==========

    setRunMode(mode: RunMode) {
      this.runMode = mode;
      if (mode !== 'dispatch') {
        this.selectedRowsForDispatch.clear();
        this.dispatchWarning = null;
      }
    },

    toggleRowSelection(row: Row) {
      if (this.selectedRowsForDispatch.has(row.id)) {
        this.selectedRowsForDispatch.delete(row.id);
      } else {
        this.selectedRowsForDispatch.add(row.id);
      }
      // Force reactivity
      this.selectedRowsForDispatch = new Set(this.selectedRowsForDispatch);
    },

    clearDispatchSelection() {
      this.selectedRowsForDispatch.clear();
      this.selectedRowsForDispatch = new Set();
    },

    selectBlockedByRows() {
      const toAdd: string[] = [];
      for (const rowId of this.selectedRowsForDispatch) {
        for (const island of this.islands) {
          const row = island.rows.find((r) => r.id === rowId);
          if (row?.taskBlockedBy) {
            for (const depId of row.taskBlockedBy) {
              if (!this.selectedRowsForDispatch.has(depId)) {
                toAdd.push(depId);
              }
            }
          }
        }
      }
      for (const id of toAdd) {
        this.selectedRowsForDispatch.add(id);
      }
      this.selectedRowsForDispatch = new Set(this.selectedRowsForDispatch);
    },

    async dispatchSelectedRows() {
      if (!this.projectId || this.selectedRowsForDispatch.size === 0) return;

      try {
        const result = await window.tauriInvoke<DispatchResult>('dispatch_rows', {
          projectId: this.projectId,
          rowIds: Array.from(this.selectedRowsForDispatch),
        });

        if (result.type === 'Warning' && result.message && result.rowIds) {
          this.dispatchWarning = {
            message: result.message,
            rowIds: result.rowIds,
          };
          return;
        }

        if (result.type === 'Success' && result.runName) {
          window.toast?.success(`Created run: ${result.runName}`);
          this.setRunMode('edit');
          await this.loadProject();
          window.dispatchEvent(new CustomEvent('run-selected', { detail: result.runName }));
        }
      } catch (e) {
        console.error('Failed to dispatch:', e);
        window.toast?.error('Failed to create run');
      }
    },

    async confirmDispatch() {
      if (!this.projectId) return;

      try {
        const result = await window.tauriInvoke<DispatchResult>('dispatch_rows_confirm', {
          projectId: this.projectId,
          rowIds: Array.from(this.selectedRowsForDispatch),
        });

        if (result.type === 'Success' && result.runName) {
          window.toast?.success(`Created run: ${result.runName}`);
          this.setRunMode('edit');
          this.dispatchWarning = null;
          await this.loadProject();
          window.dispatchEvent(new CustomEvent('run-selected', { detail: result.runName }));
        }
      } catch (e) {
        console.error('Failed to dispatch:', e);
        window.toast?.error('Failed to create run');
      }
    },

    getRowPreview(rowId: string): string {
      for (const island of this.islands) {
        const row = island.rows.find((r) => r.id === rowId);
        if (row) {
          return row.taskTitle || row.specContent?.substring(0, 40) || rowId;
        }
      }
      return rowId;
    },

    // ========== NAVIGATION ==========

    handleKeydown(e: KeyboardEvent) {
      // Ignore if typing in input
      const target = e.target as HTMLElement;
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;

      // Cmd+K: Command palette
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        this.showCommandPalette = true;
        this.commandQuery = '';
        this.filterCommands();
        return;
      }

      // Escape
      if (e.key === 'Escape') {
        if (this.editingCell) {
          this.cancelEditing();
        } else if (this.showCommandPalette) {
          this.showCommandPalette = false;
        } else if (this.islandMenuVisible || this.rowMenuVisible) {
          this.islandMenuVisible = false;
          this.rowMenuVisible = false;
        } else if (this.dispatchWarning) {
          this.dispatchWarning = null;
        }
      }
    },

    initCommands() {
      this.filteredCommands = [
        {
          id: 'fit-all',
          label: 'Fit All Islands',
          icon: '<i data-lucide="maximize-2" class="w-4 h-4"></i>',
          action: () => this.fitAll(),
        },
        {
          id: 'new-island',
          label: 'Create New Island',
          icon: '<i data-lucide="plus" class="w-4 h-4"></i>',
          action: () => this.createIslandAtCenter(),
        },
        {
          id: 'edit-mode',
          label: 'Switch to Edit Mode',
          icon: '<i data-lucide="edit" class="w-4 h-4"></i>',
          action: () => this.setRunMode('edit'),
        },
        {
          id: 'dispatch-mode',
          label: 'Switch to Dispatch Mode',
          icon: '<i data-lucide="rocket" class="w-4 h-4"></i>',
          action: () => this.setRunMode('dispatch'),
        },
      ];
    },

    filterCommands() {
      const q = this.commandQuery.toLowerCase();
      this.filteredIslands = this.islands.filter((i) => i.name.toLowerCase().includes(q));
      this.filteredBookmarks = this.bookmarks.filter((b) => b.name.toLowerCase().includes(q));
      // Commands are always shown if they match
      this.filteredCommands = this.filteredCommands.filter(
        (c) => !q || c.label.toLowerCase().includes(q),
      );
    },

    jumpToIsland(island: Island) {
      if (!this._zoomBehavior) return;

      const refs = (this as unknown as { $refs: Record<string, HTMLElement> }).$refs;
      const viewport = refs.viewport as HTMLElement;
      if (!viewport) return;

      const centerX = viewport.clientWidth / 2;
      const centerY = viewport.clientHeight / 2;

      const newTransform = zoomIdentity
        .translate(centerX, centerY)
        .scale(1)
        .translate(-island.x - island.width / 2, -island.y - 150);

      select<HTMLDivElement, unknown>(viewport as HTMLDivElement).call(
        this._zoomBehavior.transform,
        newTransform,
      );

      this.showCommandPalette = false;
      this.selectedIslandId = island.id;
    },

    jumpToBookmark(bookmark: Bookmark) {
      if (!this._zoomBehavior) return;

      const refs = (this as unknown as { $refs: Record<string, HTMLElement> }).$refs;
      const viewport = refs.viewport as HTMLDivElement;
      if (!viewport) return;

      const newTransform = zoomIdentity
        .translate(viewport.clientWidth / 2, viewport.clientHeight / 2)
        .scale(bookmark.zoom)
        .translate(-bookmark.x, -bookmark.y);

      select<HTMLDivElement, unknown>(viewport).call(this._zoomBehavior.transform, newTransform);

      this.showCommandPalette = false;
    },

    executeCommand(cmd: Command) {
      cmd.action();
      this.showCommandPalette = false;
    },

    // ========== STATUS HELPERS ==========

    getIslandStatusClass(_island: Island) {
      // Just return empty for now - styling via other classes
      return '';
    },

    getIslandStatusBadgeClass(island: Island) {
      const status = this.computedIslandStatus(island);
      if (status === 'done') return 'bg-sage/20 text-sage';
      if (status === 'dispatched') return 'bg-amber-500/20 text-amber-400';
      if (status === 'partial') return 'bg-amber-500/10 text-amber-500/70';
      return 'bg-wool-500/20 text-wool-500';
    },

    getIslandStatusLabel(island: Island) {
      return this.computedIslandStatus(island);
    },

    computedIslandStatus(island: Island): string {
      const dispatchedRows = island.rows?.filter((r) => r.dispatched) || [];
      if (dispatchedRows.length === 0) return 'draft';
      if (dispatchedRows.length < (island.rows?.length || 0)) return 'partial';
      if (dispatchedRows.every((r) => r.taskStatus === 'done')) return 'done';
      return 'dispatched';
    },

    getSpecCellClass(row: Row) {
      if (row.specStatus === 'approved') return 'border-l-2 border-l-sage';
      return '';
    },

    getTaskCellClass(row: Row) {
      if (row.taskStatus === 'done') return 'border-l-2 border-l-sage';
      if (row.taskStatus === 'doing') return 'border-l-2 border-l-amber-500';
      if (row.taskStatus === 'blocked') return 'border-l-2 border-l-terra';
      return '';
    },

    getEvalCellClass(row: Row) {
      if (row.evalStatus === 'pass') return 'border-l-2 border-l-sage';
      if (row.evalStatus === 'fail') return 'border-l-2 border-l-terra';
      return '';
    },

    getTaskStatusDot(status: string) {
      switch (status) {
        case 'todo':
          return 'bg-wool-500';
        case 'doing':
          return 'bg-amber-500';
        case 'done':
          return 'bg-sage';
        case 'blocked':
          return 'bg-terra';
        case 'deleted':
          return 'bg-wool-700';
        default:
          return 'bg-wool-500';
      }
    },
  };
}
