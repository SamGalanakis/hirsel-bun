/**
 * Tree utilities for the SpecFlow board
 *
 * Handles transformations between flat task lists and nested tree structures.
 */

import type { BoardTask, TaskTree } from '../types';

/**
 * Build a nested tree from flat task list
 *
 * @param tasks - Flat list of tasks from the database
 * @returns Array of root TaskTree nodes with children populated
 */
export function buildTaskTree(tasks: BoardTask[]): TaskTree[] {
  // Create a map of id -> TaskTree for O(1) lookup
  const treeMap = new Map<string, TaskTree>();

  // Initialize all nodes
  for (const task of tasks) {
    treeMap.set(task.id, {
      id: task.id,
      name: task.name,
      status: task.status,
      content: task.content,
      children: [],
      x: task.x,
      y: task.y,
      validated: undefined,
    });
  }

  // Build parent-child relationships and collect roots
  const roots: TaskTree[] = [];

  for (const task of tasks) {
    const node = treeMap.get(task.id)!;

    if (task.parentId) {
      const parent = treeMap.get(task.parentId);
      if (parent) {
        parent.children.push(node);
      } else {
        // Orphan node - treat as root
        roots.push(node);
      }
    } else {
      roots.push(node);
    }
  }

  // Sort children by position
  const sortByPosition = (a: TaskTree, b: TaskTree): number => {
    const taskA = tasks.find((t) => t.id === a.id);
    const taskB = tasks.find((t) => t.id === b.id);
    return (taskA?.position ?? 0) - (taskB?.position ?? 0);
  };

  const sortChildren = (node: TaskTree): void => {
    node.children.sort(sortByPosition);
    for (const child of node.children) {
      sortChildren(child);
    }
  };

  roots.sort(sortByPosition);
  for (const root of roots) {
    sortChildren(root);
  }

  return roots;
}

/**
 * Flatten a tree back into a flat list (for iteration)
 *
 * @param tree - Array of root TaskTree nodes
 * @returns Flat array of all nodes in tree order (pre-order traversal)
 */
export function flattenTree(tree: TaskTree[]): TaskTree[] {
  const result: TaskTree[] = [];

  const traverse = (node: TaskTree): void => {
    result.push(node);
    for (const child of node.children) {
      traverse(child);
    }
  };

  for (const root of tree) {
    traverse(root);
  }

  return result;
}

/**
 * Find a node by ID in the tree
 *
 * @param tree - Array of root TaskTree nodes
 * @param id - ID to search for
 * @returns The node if found, undefined otherwise
 */
export function findNodeById(tree: TaskTree[], id: string): TaskTree | undefined {
  for (const node of tree) {
    if (node.id === id) return node;

    const found = findNodeById(node.children, id);
    if (found) return found;
  }

  return undefined;
}

/**
 * Find the parent of a node by ID
 *
 * @param tree - Array of root TaskTree nodes
 * @param id - ID of the child node
 * @returns The parent node if found, undefined otherwise
 */
export function findParentById(tree: TaskTree[], id: string): TaskTree | undefined {
  for (const node of tree) {
    for (const child of node.children) {
      if (child.id === id) return node;
    }

    const found = findParentById(node.children, id);
    if (found) return found;
  }

  return undefined;
}

/**
 * Get the path from root to a node
 */
export function getNodePath(roots: TaskTree[], id: string): TaskTree[] | undefined {
  for (const root of roots) {
    if (root.id === id) return [root];
    const childPath = getNodePath(root.children, id);
    if (childPath) return [root, ...childPath];
  }
  return undefined;
}

/**
 * Get all ancestor IDs for a node (from immediate parent to root)
 *
 * @param tree - Array of root TaskTree nodes
 * @param id - ID of the node
 * @returns Array of ancestor IDs, starting with immediate parent
 */
export function getAncestorIds(tree: TaskTree[], id: string): string[] {
  const ancestors: string[] = [];
  let currentId = id;

  while (true) {
    const parent = findParentById(tree, currentId);
    if (!parent) break;
    ancestors.push(parent.id);
    currentId = parent.id;
  }

  return ancestors;
}

/**
 * Get all descendant IDs for a node (including the node itself)
 *
 * @param tree - Array of root TaskTree nodes
 * @param id - ID of the node
 * @returns Array of all descendant IDs including the node
 */
export function getDescendantIds(tree: TaskTree[], id: string): string[] {
  const node = findNodeById(tree, id);
  if (!node) return [];

  const ids: string[] = [id];

  const collectIds = (n: TaskTree): void => {
    for (const child of n.children) {
      ids.push(child.id);
      collectIds(child);
    }
  };

  collectIds(node);
  return ids;
}

/**
 * Get all leaf nodes in a tree
 */
export function getLeafNodes(roots: TaskTree[]): TaskTree[] {
  const leaves: TaskTree[] = [];

  const collect = (node: TaskTree) => {
    if (node.children.length === 0) {
      leaves.push(node);
    } else {
      node.children.forEach(collect);
    }
  };

  roots.forEach(collect);
  return leaves;
}

/**
 * Count total nodes in tree
 */
export function countNodes(tree: TaskTree[]): number {
  let count = 0;

  const traverse = (node: TaskTree): void => {
    count++;
    for (const child of node.children) {
      traverse(child);
    }
  };

  for (const root of tree) {
    traverse(root);
  }

  return count;
}

/**
 * Get tree depth (maximum nesting level)
 */
export function getTreeDepth(tree: TaskTree[]): number {
  let maxDepth = 0;

  const traverse = (node: TaskTree, depth: number): void => {
    maxDepth = Math.max(maxDepth, depth);
    for (const child of node.children) {
      traverse(child, depth + 1);
    }
  };

  for (const root of tree) {
    traverse(root, 1);
  }

  return maxDepth;
}

/**
 * Check if a node is a descendant of another node
 */
export function isDescendantOf(
  roots: TaskTree[],
  nodeId: string,
  potentialAncestorId: string,
): boolean {
  const path = getNodePath(roots, nodeId);
  if (!path) return false;
  return path.some((n) => n.id === potentialAncestorId && n.id !== nodeId);
}
