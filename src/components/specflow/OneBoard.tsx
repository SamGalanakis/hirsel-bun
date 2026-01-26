/**
 * OneBoard - Unified semantic zoom canvas
 *
 * At portfolio level (zoomed out): Shows all projects as draggable cards
 * At project level (zoomed in): Shows that project's task/eval board (SpecflowBoard)
 *
 * Navigation happens through zoom, not sidebar clicks.
 */

import { invoke } from '@tauri-apps/api/core';
import {
  type Component,
  For,
  Show,
  batch,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  onMount,
} from 'solid-js';
import { useProject, type Project } from '../../stores';
import { ProjectCard } from './ProjectCard';
import { SpecflowBoard } from './SpecflowBoard';
import { getProjectLOADLevel, RENDER_CONFIGS } from './use-tree-layout';
import type { RunSummary } from '../../lib/types';

interface Transform {
  x: number;
  y: number;
  k: number;
}

interface ContextMenuState {
  x: number;
  y: number;
  worldX: number;
  worldY: number;
  projectId: number | null;
}

// Default grid layout for projects without positions
const GRID_SPACING = 300;

export const OneBoard: Component = () => {
  const project = useProject();

  let viewportRef: HTMLDivElement | undefined;
  let canvasRef: HTMLCanvasElement | undefined;

  // Transform state
  const [transform, setTransform] = createSignal<Transform>({ x: 100, y: 100, k: 0.5 });

  // Run counts per project (for status display)
  const [runCounts, setRunCounts] = createSignal<Map<number, { total: number; active: number }>>(
    new Map()
  );

  // Interaction states
  const [panning, setPanning] = createSignal(false);
  const [panStart, setPanStart] = createSignal({ x: 0, y: 0 });
  const [dragging, setDragging] = createSignal<{
    projectId: number;
    startX: number;
    startY: number;
    initialX: number;
    initialY: number;
  } | null>(null);
  const [dragPosition, setDragPosition] = createSignal<{ x: number; y: number } | null>(null);

  // Context menu
  const [contextMenu, setContextMenu] = createSignal<ContextMenuState | null>(null);

  // Computed: current LOAD level for projects
  const projectLoad = createMemo(() => getProjectLOADLevel(transform().k));

  // Computed: whether we're at portfolio level (zoomed out)
  const isPortfolioView = createMemo(() => transform().k < 0.15);

  // Computed: focused project (zoomed into)
  const focusedProject = createMemo(() => {
    const id = project.focusedProjectId();
    return id ? project.projects().find((p) => p.id === id) : null;
  });

  // Get project position (from DB or default grid layout)
  const getProjectPosition = (p: Project, index: number) => {
    if (p.x != null && p.y != null) {
      return { x: p.x, y: p.y };
    }
    // Default grid layout
    const cols = Math.ceil(Math.sqrt(project.projects().length));
    const row = Math.floor(index / cols);
    const col = index % cols;
    return {
      x: col * GRID_SPACING + 150,
      y: row * GRID_SPACING + 150,
    };
  };

  // Load run counts for all projects
  const loadRunCounts = async () => {
    try {
      const runs = await invoke<RunSummary[]>('get_runs', {});
      const counts = new Map<number, { total: number; active: number }>();

      for (const p of project.projects()) {
        // Count runs for this project (based on project ID in run name pattern)
        const projectRuns = runs.filter((r) => r.name.startsWith(`${p.name}-`));
        const activeRuns = projectRuns.filter((r) =>
          ['working', 'eval', 'waiting'].includes(r.status)
        );
        counts.set(p.id, {
          total: projectRuns.length,
          active: activeRuns.length,
        });
      }

      setRunCounts(counts);
    } catch (e) {
      console.error('Failed to load run counts:', e);
    }
  };

  // Load run counts when projects change
  createEffect(() => {
    if (project.projects().length > 0) {
      loadRunCounts();
    }
  });

  // Poll run counts periodically
  createEffect(() => {
    const interval = setInterval(loadRunCounts, 5000);
    onCleanup(() => clearInterval(interval));
  });

  // ==========================================================================
  // Canvas Rendering (Grid)
  // ==========================================================================

  const renderGrid = () => {
    if (!canvasRef || !viewportRef) return;

    const ctx = canvasRef.getContext('2d');
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const { x, y, k } = transform();
    const vw = viewportRef.clientWidth;
    const vh = viewportRef.clientHeight;

    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, vw * dpr, vh * dpr);
    ctx.setTransform(dpr * k, 0, 0, dpr * k, dpr * x, dpr * y);

    const majorSpacing = 100;
    const minorSpacing = 20;
    const showMinor = k >= 0.4;

    const worldLeft = Math.floor(-x / k / majorSpacing) * majorSpacing - majorSpacing;
    const worldTop = Math.floor(-y / k / majorSpacing) * majorSpacing - majorSpacing;
    const worldRight = Math.ceil((-x + vw) / k / majorSpacing) * majorSpacing + majorSpacing;
    const worldBottom = Math.ceil((-y + vh) / k / majorSpacing) * majorSpacing + majorSpacing;

    // Minor grid dots
    if (showMinor) {
      ctx.fillStyle = 'rgba(255, 255, 255, 0.03)';
      for (let wx = worldLeft; wx <= worldRight; wx += minorSpacing) {
        for (let wy = worldTop; wy <= worldBottom; wy += minorSpacing) {
          ctx.beginPath();
          ctx.arc(wx, wy, 1 / k, 0, Math.PI * 2);
          ctx.fill();
        }
      }
    }

    // Major grid dots
    ctx.fillStyle = 'rgba(255, 255, 255, 0.08)';
    for (let wx = worldLeft; wx <= worldRight; wx += majorSpacing) {
      for (let wy = worldTop; wy <= worldBottom; wy += majorSpacing) {
        ctx.beginPath();
        ctx.arc(wx, wy, 2 / k, 0, Math.PI * 2);
        ctx.fill();
      }
    }
  };

  const handleResize = () => {
    if (!canvasRef || !viewportRef) return;
    const dpr = window.devicePixelRatio || 1;
    canvasRef.width = viewportRef.clientWidth * dpr;
    canvasRef.height = viewportRef.clientHeight * dpr;
    canvasRef.style.width = `${viewportRef.clientWidth}px`;
    canvasRef.style.height = `${viewportRef.clientHeight}px`;
    renderGrid();
  };

  onMount(() => {
    if (canvasRef) {
      handleResize();
      const resizeObserver = new ResizeObserver(handleResize);
      if (viewportRef) resizeObserver.observe(viewportRef);
      onCleanup(() => resizeObserver.disconnect());
    }
  });

  createEffect(() => {
    transform();
    renderGrid();
  });

  // ==========================================================================
  // Pan & Zoom
  // ==========================================================================

  const handleWheel = (e: WheelEvent) => {
    e.preventDefault();
    if (!viewportRef) return;

    const rect = viewportRef.getBoundingClientRect();
    const t = transform();
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;

    const delta = e.deltaY > 0 ? 0.9 : 1.1;
    const newK = Math.max(0.05, Math.min(3, t.k * delta));

    const newX = mouseX - (mouseX - t.x) * (newK / t.k);
    const newY = mouseY - (mouseY - t.y) * (newK / t.k);

    setTransform({ x: newX, y: newY, k: newK });

    // Auto-unfocus when zooming out past threshold
    if (newK < 0.1 && project.focusedProjectId()) {
      project.setFocusedProjectId(null);
    }
  };

  const handleMouseDown = (e: MouseEvent) => {
    if (e.button === 1 || (e.button === 0 && e.altKey)) {
      e.preventDefault();
      setPanning(true);
      setPanStart({ x: e.clientX, y: e.clientY });
    }
  };

  const handleMouseMove = (e: MouseEvent) => {
    const drag = dragging();
    if (drag) {
      const t = transform();
      const dx = (e.clientX - drag.startX) / t.k;
      const dy = (e.clientY - drag.startY) / t.k;
      setDragPosition({
        x: drag.initialX + dx,
        y: drag.initialY + dy,
      });
      return;
    }

    if (panning()) {
      const dx = e.clientX - panStart().x;
      const dy = e.clientY - panStart().y;
      setTransform((t) => ({ ...t, x: t.x + dx, y: t.y + dy }));
      setPanStart({ x: e.clientX, y: e.clientY });
    }
  };

  const handleMouseUp = async () => {
    const drag = dragging();
    if (drag) {
      const pos = dragPosition();
      if (pos) {
        await project.updateProjectPosition(drag.projectId, pos.x, pos.y);
      }
      setDragging(null);
      setDragPosition(null);
      return;
    }

    setPanning(false);
  };

  // ==========================================================================
  // Project Interactions
  // ==========================================================================

  const handleProjectDragStart = (e: MouseEvent, projectId: number) => {
    e.preventDefault();
    const p = project.projects().find((pr) => pr.id === projectId);
    if (!p) return;

    const pos = getProjectPosition(p, project.projects().indexOf(p));
    setDragging({
      projectId,
      startX: e.clientX,
      startY: e.clientY,
      initialX: pos.x,
      initialY: pos.y,
    });
  };

  const handleProjectClick = (p: Project) => {
    project.selectProject(p);
  };

  const handleProjectDoubleClick = (p: Project) => {
    // Zoom into project
    project.selectProject(p);
    project.setFocusedProjectId(p.id);

    if (viewportRef) {
      const pos = getProjectPosition(p, project.projects().indexOf(p));
      const vw = viewportRef.clientWidth;
      const vh = viewportRef.clientHeight;

      // Animate zoom to project
      setTransform({
        x: vw / 2 - pos.x * 0.8,
        y: vh / 2 - pos.y * 0.8,
        k: 0.8,
      });
    }
  };

  // ==========================================================================
  // Context Menu
  // ==========================================================================

  const handleContextMenu = (e: MouseEvent, projectId: number | null = null) => {
    e.preventDefault();
    e.stopPropagation();
    if (!viewportRef) return;

    const rect = viewportRef.getBoundingClientRect();
    const t = transform();
    const worldX = (e.clientX - rect.left - t.x) / t.k;
    const worldY = (e.clientY - rect.top - t.y) / t.k;

    setContextMenu({
      x: e.clientX,
      y: e.clientY,
      worldX,
      worldY,
      projectId,
    });
  };

  const hideContextMenu = () => setContextMenu(null);

  // Close context menu on click outside
  createEffect(() => {
    const handler = () => setContextMenu(null);
    document.addEventListener('click', handler);
    onCleanup(() => document.removeEventListener('click', handler));
  });

  // ==========================================================================
  // Zoom Controls
  // ==========================================================================

  const fitAll = () => {
    const projects = project.projects();
    if (projects.length === 0 || !viewportRef) return;

    let minX = Infinity;
    let maxX = -Infinity;
    let minY = Infinity;
    let maxY = -Infinity;

    projects.forEach((p, i) => {
      const pos = getProjectPosition(p, i);
      minX = Math.min(minX, pos.x - 100);
      maxX = Math.max(maxX, pos.x + 100);
      minY = Math.min(minY, pos.y - 60);
      maxY = Math.max(maxY, pos.y + 60);
    });

    const padding = 80;
    const vw = viewportRef.clientWidth;
    const vh = viewportRef.clientHeight;
    const scaleX = (vw - padding * 2) / (maxX - minX);
    const scaleY = (vh - padding * 2) / (maxY - minY);
    const scale = Math.min(scaleX, scaleY, 1);

    const centerX = (minX + maxX) / 2;
    const centerY = (minY + maxY) / 2;

    setTransform({
      x: vw / 2 - centerX * scale,
      y: vh / 2 - centerY * scale,
      k: scale,
    });

    project.setFocusedProjectId(null);
  };

  // ==========================================================================
  // Keyboard Shortcuts
  // ==========================================================================

  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;

      if (e.key === 'Escape') {
        if (contextMenu()) {
          setContextMenu(null);
        } else if (project.focusedProjectId()) {
          // Zoom out from focused project
          project.setFocusedProjectId(null);
          fitAll();
        }
      }
    };

    document.addEventListener('keydown', handler);
    onCleanup(() => document.removeEventListener('keydown', handler));
  });

  // ==========================================================================
  // Render
  // ==========================================================================

  return (
    <div
      class="flex-1 flex flex-col overflow-hidden relative"
      style={{
        background: 'linear-gradient(145deg, #0f0f0f 0%, #1a1815 50%, #0f0f0f 100%)',
      }}
    >
      {/* Canvas layer for grid */}
      <canvas
        ref={canvasRef}
        class="absolute inset-0 w-full h-full pointer-events-none"
        style={{ 'z-index': 0 }}
      />

      {/* Portfolio view - show when not focused on a project */}
      <Show when={!focusedProject()}>
        <div
          ref={viewportRef}
          class="absolute inset-0 overflow-hidden"
          style={{
            'z-index': 1,
            cursor: dragging() ? 'grabbing' : panning() ? 'grabbing' : 'grab',
          }}
          onMouseDown={handleMouseDown}
          onMouseMove={handleMouseMove}
          onMouseUp={handleMouseUp}
          onMouseLeave={handleMouseUp}
          onWheel={handleWheel}
          onContextMenu={(e) => handleContextMenu(e, null)}
        >
          {/* Transformed container */}
          <div
            class="absolute origin-top-left will-change-transform"
            style={{
              transform: `translate(${transform().x}px, ${transform().y}px) scale(${transform().k})`,
            }}
          >
            {/* Render Project Cards */}
            <For each={project.projects()}>
              {(p, i) => {
                const pos = () => {
                  const drag = dragging();
                  if (drag?.projectId === p.id && dragPosition()) {
                    return dragPosition()!;
                  }
                  return getProjectPosition(p, i());
                };

                const counts = () => runCounts().get(p.id) || { total: 0, active: 0 };

                return (
                  <ProjectCard
                    project={p}
                    position={pos()}
                    load={projectLoad()}
                    selected={project.selectedProjectId() === p.id}
                    focused={!!project.focusedProjectId() && project.focusedProjectId() !== p.id}
                    runCount={counts().total}
                    activeRunCount={counts().active}
                    onClick={() => handleProjectClick(p)}
                    onDoubleClick={() => handleProjectDoubleClick(p)}
                    onContextMenu={(e) => handleContextMenu(e, p.id)}
                    onDragStart={handleProjectDragStart}
                  />
                );
              }}
            </For>
          </div>
        </div>
      </Show>

      {/* Project view - show when focused on a project */}
      <Show when={focusedProject()}>
        <SpecflowBoard />
      </Show>

      {/* HUD Toolbar */}
      <div
        class="absolute top-4 right-4 flex items-center gap-2"
        style={{ 'z-index': 50, 'pointer-events': 'auto' }}
      >
        <Show when={project.focusedProjectId()}>
          <button
            onClick={() => {
              project.setFocusedProjectId(null);
              fitAll();
            }}
            class="px-3 py-1.5 rounded-lg text-xs font-medium text-wool-300 hover:text-wool-100 transition-all flex items-center gap-1.5"
            style={{
              background: 'rgba(39,39,42,0.9)',
              border: '1px solid rgba(63,63,70,0.5)',
            }}
            title="Back to portfolio (Escape)"
          >
            <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M10 19l-7-7m0 0l7-7m-7 7h18"
              />
            </svg>
            Portfolio
          </button>
        </Show>

        <button
          onClick={fitAll}
          class="p-2 rounded-lg text-zinc-500 hover:text-zinc-200 hover:bg-zinc-800 transition-all"
          style={{ background: 'rgba(39,39,42,0.8)', border: '1px solid rgba(63,63,70,0.5)' }}
          title="Fit all"
        >
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5l-5-5m5 5v-4m0 4h-4"
            />
          </svg>
        </button>
      </div>

      {/* Context Menu */}
      <Show when={contextMenu()}>
        <div
          class="fixed rounded-lg overflow-hidden shadow-xl"
          style={{
            'z-index': 100,
            left: `${contextMenu()!.x}px`,
            top: `${contextMenu()!.y}px`,
            background: 'rgba(26,26,26,0.98)',
            border: '1px solid rgba(63,63,70,0.8)',
            'min-width': '160px',
          }}
          onClick={(e) => e.stopPropagation()}
        >
          <Show when={!contextMenu()!.projectId}>
            {/* Canvas context menu */}
            <button
              class="w-full px-3 py-2 text-left text-sm text-wool-200 hover:bg-pasture-700 flex items-center gap-2"
              onClick={() => {
                project.openProjectSetup();
                hideContextMenu();
              }}
            >
              <svg class="w-4 h-4 text-amber-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
              </svg>
              Add Project
            </button>
            <button
              class="w-full px-3 py-2 text-left text-sm text-wool-200 hover:bg-pasture-700 flex items-center gap-2"
              onClick={() => {
                fitAll();
                hideContextMenu();
              }}
            >
              <svg class="w-4 h-4 text-wool-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5l-5-5m5 5v-4m0 4h-4" />
              </svg>
              Fit All Projects
            </button>
          </Show>

          <Show when={contextMenu()!.projectId}>
            {/* Project context menu */}
            <button
              class="w-full px-3 py-2 text-left text-sm text-wool-200 hover:bg-pasture-700 flex items-center gap-2"
              onClick={() => {
                const p = project.projects().find((pr) => pr.id === contextMenu()!.projectId);
                if (p) handleProjectDoubleClick(p);
                hideContextMenu();
              }}
            >
              <svg class="w-4 h-4 text-amber-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0zM10 7v3m0 0v3m0-3h3m-3 0H7" />
              </svg>
              Open Project
            </button>
            <button
              class="w-full px-3 py-2 text-left text-sm text-wool-200 hover:bg-pasture-700 flex items-center gap-2"
              onClick={() => {
                const p = project.projects().find((pr) => pr.id === contextMenu()!.projectId);
                if (p) {
                  project.selectProject(p);
                  project.setShowProjectSettings(true);
                }
                hideContextMenu();
              }}
            >
              <svg class="w-4 h-4 text-wool-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
              </svg>
              Settings
            </button>
            <div style={{ 'border-top': '1px solid rgba(63,63,70,0.5)', margin: '4px 0' }} />
            <button
              class="w-full px-3 py-2 text-left text-sm text-terra hover:bg-terra/10 flex items-center gap-2"
              onClick={async () => {
                const pid = contextMenu()!.projectId;
                if (pid) {
                  const confirmed = await window.confirmDialog?.delete(
                    project.projects().find((p) => p.id === pid)?.name || 'this project',
                    'project'
                  );
                  if (confirmed) {
                    await project.removeProject(pid);
                  }
                }
                hideContextMenu();
              }}
            >
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
              </svg>
              Delete Project
            </button>
          </Show>
        </div>
      </Show>

      {/* Empty state */}
      <Show when={project.projects().length === 0 && !project.loading()}>
        <div
          class="absolute inset-0 flex items-center justify-center pointer-events-none"
          style={{ 'z-index': 5 }}
        >
          <div class="text-center">
            <div
              class="w-20 h-20 mx-auto mb-4 rounded-2xl flex items-center justify-center"
              style={{
                background: 'linear-gradient(145deg, rgba(212,165,116,0.1) 0%, rgba(212,165,116,0.05) 100%)',
                border: '1px solid rgba(212,165,116,0.2)',
              }}
            >
              <svg class="w-10 h-10 text-amber-500/50" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z"
                />
              </svg>
            </div>
            <h2 class="text-lg font-medium text-wool-300 mb-1">No Projects Yet</h2>
            <p class="text-sm text-wool-600 mb-4">
              Right-click to add your first project
            </p>
            <button
              onClick={() => project.openProjectSetup()}
              class="pointer-events-auto px-4 py-2 rounded-lg text-sm font-medium transition-all"
              style={{
                background: 'linear-gradient(180deg, rgba(212,165,116,0.3) 0%, rgba(212,165,116,0.2) 100%)',
                border: '1px solid rgba(212,165,116,0.5)',
                color: 'rgb(232,193,154)',
              }}
            >
              Add Project
            </button>
          </div>
        </div>
      </Show>

      {/* Pulse glow animation */}
      <style>{`
        .pulse-glow {
          animation: pulse-glow 2s ease-in-out infinite;
        }
        @keyframes pulse-glow {
          0%, 100% { box-shadow: 0 0 8px rgba(212, 165, 116, 0.5); }
          50% { box-shadow: 0 0 16px rgba(212, 165, 116, 0.8); }
        }
      `}</style>
    </div>
  );
};

export default OneBoard;
