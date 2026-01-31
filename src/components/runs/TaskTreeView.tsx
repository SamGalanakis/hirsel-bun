/**
 * TaskTreeView - Proper tree visualization for run tasks and evals
 *
 * Displays tasks as a hierarchical tree with connecting lines,
 * with eval nodes connected to the tasks they validate.
 */
import { type Component, For, Show, createMemo } from 'solid-js';
import { generateSheepSvg } from '../../lib/sheep-avatar';
import type { Task, TaskDisplay, WorkerDisplay } from '../../lib/types';

interface TaskTreeViewProps {
  tasks: Task[];
  workers: WorkerDisplay[];
  selectedTaskId: string | null;
  onSelectTask: (taskId: string) => void;
}

interface TreeNode extends TaskDisplay {
  x: number;
  y: number;
  width: number;
  height: number;
  isEval?: boolean;
}

interface TreeEdge {
  id: string;
  parentId: string;
  childId: string;
  path: string;
  isEvalEdge?: boolean;
}

// Layout constants - compact vertical layout
const NODE_WIDTH = 150;
const NODE_HEIGHT = 36;
const INDENT_WIDTH = 24; // Indentation per depth level
const VERTICAL_GAP = 8;
const ROOT_PADDING_TOP = 8;
const ORPHAN_GAP = 16; // Gap before orphan grid

// Eval node styling
const EVAL_NODE_WIDTH = 140;
const EVAL_NODE_HEIGHT = 32;

/**
 * Build tree structure from flat task list, separating work and eval tasks
 *
 * Tree hierarchy is built from blocking relationships:
 * - Scope task at root (blocks other tasks)
 * - Tasks blocked by scope are children of scope
 * - Subtasks (via parentId) are children of their parent
 *
 * Returns:
 * - treeRoots: Tasks that are roots of actual trees (have children or block others)
 * - orphanTasks: Standalone tasks not part of any tree
 * - evalTasks: Eval tasks (displayed separately)
 */
function buildTree(tasks: Task[]): { treeRoots: TaskDisplay[]; orphanTasks: TaskDisplay[]; evalTasks: Task[] } {
  const workTasks = tasks.filter(t => t.taskType !== 'eval');
  const evalTasks = tasks.filter(t => t.taskType === 'eval');

  const taskMap = new Map<string, TaskDisplay>();
  const childrenAdded = new Set<string>(); // Track which tasks have been added as children

  // Create TaskDisplay objects for work tasks
  for (const task of workTasks) {
    const display: TaskDisplay = {
      ...task,
      isBlocked: (task.blockedBy || []).length > 0,
      children: [],
      depth: 0,
    };
    taskMap.set(task.id, display);
  }

  // First pass: build subtask hierarchy via parentId
  for (const task of workTasks) {
    if (task.parentId && taskMap.has(task.parentId)) {
      const node = taskMap.get(task.id)!;
      const parent = taskMap.get(task.parentId)!;
      parent.children.push(node);
      childrenAdded.add(task.id);
    }
  }

  // Second pass: connect tasks via blockedBy (for scope -> leaf relationships)
  // Only for tasks not already added as children via parentId
  for (const task of workTasks) {
    if (childrenAdded.has(task.id)) continue;

    const blockedBy = task.blockedBy || [];
    if (blockedBy.length > 0) {
      // Find the first blocker that exists in our task map
      const blockerId = blockedBy.find(id => taskMap.has(id));
      if (blockerId) {
        const node = taskMap.get(task.id)!;
        const blocker = taskMap.get(blockerId)!;
        blocker.children.push(node);
        childrenAdded.add(task.id);
      }
    }
  }

  // Separate roots into tree roots (have children) and orphans (standalone)
  const treeRoots: TaskDisplay[] = [];
  const orphanTasks: TaskDisplay[] = [];

  for (const task of workTasks) {
    if (!childrenAdded.has(task.id)) {
      const node = taskMap.get(task.id)!;
      if (node.children.length > 0) {
        treeRoots.push(node);
      } else {
        orphanTasks.push(node);
      }
    }
  }

  // Calculate depths via DFS from roots
  function setDepths(node: TaskDisplay, depth: number) {
    node.depth = depth;
    for (const child of node.children) {
      setDepths(child, depth + 1);
    }
  }
  for (const root of treeRoots) {
    setDepths(root, 0);
  }

  return { treeRoots, orphanTasks, evalTasks };
}

/**
 * Calculate tree layout positions using vertical list layout with indentation
 */
function calculateLayout(
  treeRoots: TaskDisplay[],
  evalTasks: Task[],
  taskMap: Map<string, TreeNode>
): { nodes: TreeNode[]; edges: TreeEdge[]; bounds: { width: number; height: number } } {
  const nodes: TreeNode[] = [];
  const edges: TreeEdge[] = [];
  let currentY = ROOT_PADDING_TOP;
  let maxWidth = 0;

  // Position nodes vertically with indentation based on depth
  function positionNode(node: TaskDisplay, depth: number, parentNode: TreeNode | null): void {
    const nodeX = depth * INDENT_WIDTH;
    const nodeY = currentY;

    const treeNode: TreeNode = {
      ...node,
      x: nodeX,
      y: nodeY,
      width: NODE_WIDTH,
      height: NODE_HEIGHT,
      isEval: false,
    };
    nodes.push(treeNode);
    taskMap.set(node.id, treeNode);

    maxWidth = Math.max(maxWidth, nodeX + NODE_WIDTH);
    currentY += NODE_HEIGHT + VERTICAL_GAP;

    // Create edge from parent
    if (parentNode) {
      // Vertical line with small horizontal connector
      const startX = parentNode.x + 12; // Left side of parent
      const startY = parentNode.y + parentNode.height;
      const endX = nodeX + 12; // Left side of child
      const endY = nodeY + NODE_HEIGHT / 2;

      edges.push({
        id: `${parentNode.id}-${node.id}`,
        parentId: parentNode.id,
        childId: node.id,
        path: `M ${startX} ${startY} L ${startX} ${endY} L ${endX} ${endY}`,
        isEvalEdge: false,
      });
    }

    // Position children
    for (const child of node.children) {
      positionNode(child, depth + 1, treeNode);
    }
  }

  // Layout all tree roots
  for (const root of treeRoots) {
    positionNode(root, 0, null);
    currentY += 8; // Extra gap between trees
  }

  // Position eval nodes - inline with small indent
  for (const evalTask of evalTasks) {
    const validates = evalTask.validates || [];

    // Find the deepest validated task to position eval below it
    let maxDepth = 0;
    let lastValidatedNode: TreeNode | null = null;
    for (const taskId of validates) {
      const taskNode = taskMap.get(taskId);
      if (taskNode && taskNode.depth >= maxDepth) {
        maxDepth = taskNode.depth;
        lastValidatedNode = taskNode;
      }
    }

    const evalX = (maxDepth + 1) * INDENT_WIDTH;
    const evalY = currentY;

    const evalNode: TreeNode = {
      ...evalTask,
      isBlocked: (evalTask.blockedBy || []).length > 0,
      children: [],
      depth: maxDepth + 1,
      x: evalX,
      y: evalY,
      width: EVAL_NODE_WIDTH,
      height: EVAL_NODE_HEIGHT,
      isEval: true,
    };
    nodes.push(evalNode);
    taskMap.set(evalTask.id, evalNode);

    maxWidth = Math.max(maxWidth, evalX + EVAL_NODE_WIDTH);
    currentY += EVAL_NODE_HEIGHT + VERTICAL_GAP;

    // Create edges from validated tasks to eval
    for (const taskId of validates) {
      const taskNode = taskMap.get(taskId);
      if (taskNode) {
        const startX = taskNode.x + taskNode.width;
        const startY = taskNode.y + taskNode.height / 2;
        const endX = evalX;
        const endY = evalY + EVAL_NODE_HEIGHT / 2;

        edges.push({
          id: `${taskId}-${evalTask.id}`,
          parentId: taskId,
          childId: evalTask.id,
          path: `M ${startX} ${startY} Q ${startX + 20} ${startY}, ${startX + 20} ${(startY + endY) / 2} Q ${startX + 20} ${endY}, ${endX} ${endY}`,
          isEvalEdge: true,
        });
      }
    }
  }

  return {
    nodes,
    edges,
    bounds: { width: maxWidth + 16, height: currentY + 8 },
  };
}

/**
 * Generate SVG path for edge between parent and child
 */
function generateEdgePath(x1: number, y1: number, x2: number, y2: number): string {
  const midY = y1 + (y2 - y1) / 2;
  return `M ${x1} ${y1} C ${x1} ${midY}, ${x2} ${midY}, ${x2} ${y2}`;
}

/**
 * Get status class for a task
 */
function getStatusClass(task: TaskDisplay, isEval: boolean): string {
  if (isEval) {
    switch (task.evalResult) {
      case 'pass': return 'eval-passed';
      case 'fail': return 'eval-failed';
      default:
        if (task.status === 'doing') return 'eval-running';
        return 'eval-pending';
    }
  }

  if (task.isBlocked) return 'blocked';
  switch (task.status) {
    case 'done':
    case 'validated':
      return 'done';
    case 'doing':
      return 'doing';
    case 'awaiting_eval':
      return 'awaiting';
    default:
      return 'todo';
  }
}

export const TaskTreeView: Component<TaskTreeViewProps> = (props) => {
  // Build tree structure and calculate layout
  const treeData = createMemo(() => {
    const { treeRoots, orphanTasks, evalTasks } = buildTree(props.tasks);
    const taskMap = new Map<string, TreeNode>();
    const layout = calculateLayout(treeRoots, evalTasks, taskMap);
    return { layout, orphanTasks };
  });

  // Get worker by name
  const getWorker = (name: string | null) => {
    if (!name) return null;
    return props.workers.find((w) => w.name === name);
  };

  const renderNode = (node: TreeNode, isOrphan = false) => {
    const isEval = () => node.isEval || node.taskType === 'eval';
    const statusClass = () => getStatusClass(node, isEval());
    const isSelected = () => props.selectedTaskId === node.id;
    const worker = () => getWorker(node.claimedBy);

    return (
      <div
        class={`task-tree-node ${isEval() ? 'eval-node' : ''} status-${statusClass()}`}
        classList={{ selected: isSelected() }}
        style={isOrphan ? {} : {
          position: 'absolute',
          left: `${node.x}px`,
          top: `${node.y}px`,
          width: `${node.width}px`,
        }}
        onClick={() => props.onSelectTask(node.id)}
      >
        {/* Status indicator */}
        <div class={`task-tree-node-status ${statusClass()}`} />

        {/* Content */}
        <div class="task-tree-node-content">
          <div class="task-tree-node-name">{node.description}</div>
          <Show when={!isEval() && (node.isBlocked || node.children.length > 0)}>
            <div class="task-tree-node-meta">
              <Show when={node.isBlocked}>
                <span class="task-tree-node-badge blocked">blocked</span>
              </Show>
              <Show when={node.children.length > 0 && !node.isBlocked && node.status !== 'done'}>
                <span class="task-tree-node-badge subtasks">{node.children.length}</span>
              </Show>
            </div>
          </Show>
          <Show when={isEval()}>
            <div class="task-tree-node-meta">
              <span class="task-tree-node-badge eval">eval</span>
            </div>
          </Show>
        </div>

        {/* Worker avatar */}
        <Show when={worker()}>
          {(w) => (
            <div
              innerHTML={generateSheepSvg(w().sheepConfig, 18)}
              class="task-tree-node-worker"
              title={w().name}
            />
          )}
        </Show>
      </div>
    );
  };

  return (
    <div class="task-tree-view">
      {/* Tree section */}
      <Show when={treeData().layout.nodes.length > 0}>
        <div
          class="task-tree-container"
          style={{
            width: '100%',
            height: `${treeData().layout.bounds.height}px`,
            'min-height': '100px',
          }}
        >
          {/* SVG layer for edges */}
          <svg
            class="task-tree-edges"
            width="100%"
            height={treeData().layout.bounds.height}
            style={{ position: 'absolute', top: 0, left: 0, 'pointer-events': 'none' }}
          >
            <defs>
              <linearGradient id="edge-gradient" x1="0%" y1="0%" x2="0%" y2="100%">
                <stop offset="0%" stop-color="rgba(212, 165, 116, 0.4)" />
                <stop offset="100%" stop-color="rgba(212, 165, 116, 0.15)" />
              </linearGradient>
              <linearGradient id="eval-edge-gradient" x1="0%" y1="0%" x2="0%" y2="100%">
                <stop offset="0%" stop-color="rgba(125, 153, 112, 0.4)" />
                <stop offset="100%" stop-color="rgba(125, 153, 112, 0.15)" />
              </linearGradient>
            </defs>
            <For each={treeData().layout.edges}>
              {(edge) => (
                <path
                  d={edge.path}
                  fill="none"
                  stroke={edge.isEvalEdge ? "rgba(125, 153, 112, 0.4)" : "rgba(212, 165, 116, 0.35)"}
                  stroke-width="1.5"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-dasharray={edge.isEvalEdge ? "3 2" : "none"}
                />
              )}
            </For>
          </svg>

          {/* Nodes layer */}
          <For each={treeData().layout.nodes}>
            {(node) => renderNode(node)}
          </For>
        </div>
      </Show>

      {/* Orphan tasks grid */}
      <Show when={treeData().orphanTasks.length > 0}>
        <div class="task-orphan-grid" style={{ 'margin-top': treeData().layout.nodes.length > 0 ? `${ORPHAN_GAP}px` : '0' }}>
          <For each={treeData().orphanTasks}>
            {(task) => {
              const orphanNode: TreeNode = {
                ...task,
                x: 0,
                y: 0,
                width: NODE_WIDTH,
                height: NODE_HEIGHT,
                isEval: false,
              };
              return renderNode(orphanNode, true);
            }}
          </For>
        </div>
      </Show>

      <Show when={treeData().layout.nodes.length === 0 && treeData().orphanTasks.length === 0}>
        <div class="task-tree-empty">
          <p class="text-wool-500 text-sm">No tasks</p>
        </div>
      </Show>
    </div>
  );
};

export default TaskTreeView;
