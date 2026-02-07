import type { BoardNodeTree } from '../types';

export interface FocusIndices {
  dependentsByBlocker: Map<string, Set<string>>;
  repairsByResolved: Map<string, Set<string>>;
  checksByValidatedTarget: Map<string, Set<string>>;
}

export function buildFocusIndices(nodeMap: Map<string, BoardNodeTree>): FocusIndices {
  const dependentsByBlocker = new Map<string, Set<string>>();
  const repairsByResolved = new Map<string, Set<string>>();
  const checksByValidatedTarget = new Map<string, Set<string>>();

  for (const [, node] of nodeMap) {
    for (const blockerId of node.blockedBy || []) {
      let set = dependentsByBlocker.get(blockerId);
      if (!set) {
        set = new Set();
        dependentsByBlocker.set(blockerId, set);
      }
      set.add(node.id);
    }

    if (node.resolves) {
      let set = repairsByResolved.get(node.resolves);
      if (!set) {
        set = new Set();
        repairsByResolved.set(node.resolves, set);
      }
      set.add(node.id);
    }

    if (node.kind === 'check') {
      for (const targetId of node.validates || []) {
        let set = checksByValidatedTarget.get(targetId);
        if (!set) {
          set = new Set();
          checksByValidatedTarget.set(targetId, set);
        }
        set.add(node.id);
      }
    }
  }

  return { dependentsByBlocker, repairsByResolved, checksByValidatedTarget };
}

function collectDescendants(id: string, nodeMap: Map<string, BoardNodeTree>, out: Set<string>) {
  const node = nodeMap.get(id);
  if (!node) return;
  for (const child of node.children || []) {
    if (!out.has(child.id)) {
      out.add(child.id);
      collectDescendants(child.id, nodeMap, out);
    }
  }
}

export function computeFocusSet(
  selectedId: string,
  nodeMap: Map<string, BoardNodeTree>,
  indices: FocusIndices,
): Set<string> {
  const focus = new Set<string>();
  const selected = nodeMap.get(selectedId);
  if (!selected) return focus;

  focus.add(selectedId);

  // Ancestors
  let cur: BoardNodeTree | undefined = selected;
  while (cur?.parentId) {
    const parent = nodeMap.get(cur.parentId);
    if (!parent) break;
    focus.add(parent.id);
    cur = parent;
  }

  // Descendants
  collectDescendants(selectedId, nodeMap, focus);

  // Dependencies: blockers + dependents
  for (const blockerId of selected.blockedBy || []) {
    focus.add(blockerId);
  }
  const dependents = indices.dependentsByBlocker.get(selectedId);
  if (dependents) {
    for (const id of dependents) focus.add(id);
  }

  // Checks / validates relationships
  if (selected.kind === 'check') {
    for (const id of selected.validates || []) focus.add(id);
  } else {
    for (const checkId of selected.validatedBy || []) focus.add(checkId);
    const computedChecks = indices.checksByValidatedTarget.get(selectedId);
    if (computedChecks) {
      for (const id of computedChecks) focus.add(id);
    }
  }

  // Resolves relationships
  if (selected.resolves) focus.add(selected.resolves);
  const repairs = indices.repairsByResolved.get(selectedId);
  if (repairs) {
    for (const id of repairs) focus.add(id);
  }

  return focus;
}
