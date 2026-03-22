/**
 * RadialMenu - Blender-style pie menu
 *
 * Opens at mouse cursor. Move mouse in any direction to highlight items,
 * click to select. Tab to cycle, Escape to cancel.
 *
 * Triggered by: ` (tilde/backtick) - configurable in settings
 */
import { type Component, Show, For, createSignal, createEffect, onCleanup } from 'solid-js';
import { emit } from '../../lib/events';
import { useApp, useProject, useWorkspace, useDelivery } from '../../stores';
import { useDelta } from '../../stores/delta-context';
import { Icon } from '../shared';
import { findClosestItem, getRadialItemStyle, renderSlicePath } from '../../lib/radial-utils';
import { amber } from '../../lib/theme-colors';

export const RadialMenu: Component = () => {
  const app = useApp();
  const project = useProject();
  const workspace = useWorkspace();
  const delta = useDelta();
  const delivery = useDelivery();

  const isOpen = () => app.radialMenuOpen();
  const position = () => app.radialMenuPosition();

  // Track cumulative mouse movement from center for direction detection
  const [mouseOffset, setMouseOffset] = createSignal({ x: 0, y: 0 });
  const [selectedIndex, setSelectedIndex] = createSignal<number | null>(null);
  const [currentAngle, setCurrentAngle] = createSignal<number | null>(null);

  // Threshold before direction is detected (dead zone in center)
  const deadZone = 15;
  const radius = 110;

  // Calculate which item is selected based on mouse direction
  const calculateSelectedItem = (offsetX: number, offsetY: number) => {
    return findClosestItem(offsetX, offsetY, menuItems(), deadZone);
  };

  // Handle mouse movement for direction selection
  createEffect(() => {
    if (!isOpen()) return;

    // Keep cursor visible - track position relative to menu center
    const handleMouseMove = (e: MouseEvent) => {
      // Calculate offset from menu center
      const offsetX = e.clientX - position().x;
      const offsetY = e.clientY - position().y;

      setMouseOffset({ x: offsetX, y: offsetY });
      const result = calculateSelectedItem(offsetX, offsetY);
      setSelectedIndex(result?.index ?? null);
      setCurrentAngle(result?.angle ?? null);
    };

    const handleClick = (e: MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();

      const idx = selectedIndex();
      if (idx !== null) {
        const items = menuItems();
        const item = items[idx];
        if (item && !item.disabled) {
          item.onClick();
        }
      }
      app.closeRadialMenu();
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        app.closeRadialMenu();
        return;
      }

      // Tab to cycle through enabled items only
      if (e.key === 'Tab') {
        e.preventDefault();
        const items = menuItems();
        const enabledIndices = items.map((item, i) => ({ i, disabled: item.disabled }))
          .filter(x => !x.disabled)
          .map(x => x.i);

        if (enabledIndices.length === 0) return;

        const currentIdx = selectedIndex();
        const currentEnabledPos = currentIdx !== null ? enabledIndices.indexOf(currentIdx) : -1;

        if (e.shiftKey) {
          // Shift+Tab goes backwards
          const newPos = currentEnabledPos <= 0 ? enabledIndices.length - 1 : currentEnabledPos - 1;
          setSelectedIndex(enabledIndices[newPos]);
        } else {
          // Tab goes forwards
          const newPos = currentEnabledPos < 0 || currentEnabledPos >= enabledIndices.length - 1 ? 0 : currentEnabledPos + 1;
          setSelectedIndex(enabledIndices[newPos]);
        }
        return;
      }

      // Enter to select current item
      if (e.key === 'Enter') {
        e.preventDefault();
        const idx = selectedIndex();
        if (idx !== null) {
          const item = menuItems()[idx];
          if (item && !item.disabled) {
            item.onClick();
          }
        }
        return;
      }

      // Number key shortcuts
      const item = menuItems().find(i => i.shortcut === e.key);
      if (item && !item.disabled) {
        item.onClick();
      }
    };

    const handleContextMenu = (e: MouseEvent) => {
      e.preventDefault();
      app.closeRadialMenu();
    };

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('click', handleClick);
    document.addEventListener('keydown', handleKeyDown);
    document.addEventListener('contextmenu', handleContextMenu);

    onCleanup(() => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('click', handleClick);
      document.removeEventListener('keydown', handleKeyDown);
      document.removeEventListener('contextmenu', handleContextMenu);
      setMouseOffset({ x: 0, y: 0 });
      setSelectedIndex(null);
      setCurrentAngle(null);
    });
  });

  // Compute draft node count badge
  const draftBadge = () => {
    if (!delta.hasDraftNodes()) return undefined;
    const trees = delta.boardTree();
    const countDrafts = (nodes: typeof trees): number =>
      nodes.reduce((sum, n) => sum + (n.status === 'draft' ? 1 : 0) + countDrafts(n.children), 0);
    const total = countDrafts(trees);
    return total > 0 ? total : undefined;
  };

  // Check if deliver is available
  const canDeliver = () => {
    return delta.hasDispatchedNodes();
  };

  // Action handlers
  const handleStartShepherd = () => {
    app.closeRadialMenu();
    emit('radial-start-shepherd');
  };

  const handleDeliver = () => {
    app.closeRadialMenu();
    emit('radial-deliver');
  };

  const handleBoard = () => {
    app.closeRadialMenu();
    workspace.setActiveMachineryTab('board');
    workspace.setMachineryOpen(true);
  };

  const handleWorkers = () => {
    app.closeRadialMenu();
    workspace.setActiveThread('chat');
    workspace.setActiveMachineryTab('workers');
    workspace.setMachineryOpen(true);
  };

  const handleIde = () => {
    app.closeRadialMenu();
    emit('radial-open-ide');
  };

  const handleSettings = () => {
    app.closeRadialMenu();
    if (project.selectedProject()) {
      project.setShowProjectSettings(true);
    } else {
      app.setShowSettings(true);
    }
  };

  // 6 project-surface actions evenly spaced (60° intervals)
  // Starting from top (0°) going clockwise
  const menuItems = () => [
    { id: 'deliver', angle: 0, icon: 'package', label: 'Deliver', shortcut: '8', badge: undefined, variant: canDeliver() ? 'success' : 'default', disabled: !canDeliver() || delivery.deliveryPending(), onClick: handleDeliver },
    { id: 'workers', angle: 60, icon: 'message-circle', label: 'Workers', shortcut: '9', badge: undefined, variant: 'default', disabled: false, onClick: handleWorkers },
    { id: 'settings', angle: 120, icon: 'settings', label: 'Settings', shortcut: '3', badge: undefined, variant: 'default', disabled: false, onClick: handleSettings },
    { id: 'ide', angle: 180, icon: 'folder-open', label: 'IDE', shortcut: '2', badge: undefined, variant: 'default', disabled: !delta.projectRun(), onClick: handleIde },
    { id: 'board', angle: 240, icon: 'blocks', label: 'Board', shortcut: '1', badge: undefined, variant: workspace.machineryOpen() && workspace.activeMachineryTab() === 'board' ? 'primary' : 'default', disabled: false, onClick: handleBoard },
    { id: 'start-shepherd', angle: 300, icon: 'rocket', label: 'Start', shortcut: '7', badge: draftBadge(), variant: delta.hasDraftNodes() ? 'primary' : 'default', disabled: !delta.hasDraftNodes() || delta.shepherdStartPending(), onClick: handleStartShepherd },
  ];

  const getItemStyles = (index: number, item: ReturnType<typeof menuItems>[0]) => {
    return getRadialItemStyle(item.variant, selectedIndex() === index, item.disabled);
  };

  return (
    <Show when={isOpen()}>
      {/* Full screen capture layer */}
      <div class="fixed inset-0 z-[9998]" />

      {/* Center indicator */}
      <div
        class="fixed z-[10000] pointer-events-none"
        style={{
          left: `${position().x}px`,
          top: `${position().y}px`,
          transform: 'translate(-50%, -50%)',
        }}
      >
        {/* Outer glow ring */}
        <div
          class="absolute w-12 h-12 rounded-none"
          style={{
            left: '50%',
            top: '50%',
            transform: 'translate(-50%, -50%)',
            background: `radial-gradient(circle, ${amber(0.15)} 0%, transparent 70%)`,
          }}
        />
        {/* Center ring */}
        <div
          class="relative w-7 h-7 rounded-none flex items-center justify-center"
          style={{
            background: 'rgba(25, 25, 25, 0.98)',
            border: `2px solid ${amber(0.7)}`,
            'box-shadow': `0 0 15px ${amber(0.4)}, inset 0 1px 2px rgba(255, 255, 255, 0.1)`,
          }}
        >
          <div
            class="w-2 h-2 rounded-none"
            style={{
              background: amber(0.9),
              'box-shadow': `0 0 6px ${amber(0.8)}`,
            }}
          />
        </div>
      </div>

      {/* Pie slice zones and direction indicator */}
      <svg
        class="fixed z-[9998] pointer-events-none"
        style={{ left: 0, top: 0, width: '100%', height: '100%' }}
      >
        <defs>
          {/* Gradient for selected slice */}
          <radialGradient id="selectedSliceGradient" cx="50%" cy="50%" r="50%">
            <stop offset="0%" stop-color={amber(0.3)} />
            <stop offset="100%" stop-color={amber(0.05)} />
          </radialGradient>
          {/* Glow filter */}
          <filter id="sliceGlow" x="-50%" y="-50%" width="200%" height="200%">
            <feGaussianBlur stdDeviation="3" result="blur" />
            <feMerge>
              <feMergeNode in="blur" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>

        {/* Divider lines between slices */}
        <For each={menuItems()}>
          {(item) => {
            const items = menuItems();
            const sliceAngle = 360 / items.length;
            const dividerAngle = item.angle - sliceAngle / 2;
            const rads = (dividerAngle - 90) * Math.PI / 180;
            const innerR = 20;
            const outerR = radius - 15;

            return (
              <line
                x1={position().x + Math.cos(rads) * innerR}
                y1={position().y + Math.sin(rads) * innerR}
                x2={position().x + Math.cos(rads) * outerR}
                y2={position().y + Math.sin(rads) * outerR}
                stroke="rgba(255, 255, 255, 0.08)"
                stroke-width="1"
              />
            );
          }}
        </For>

        {/* Selected slice highlight */}
        <Show when={selectedIndex() !== null}>
          {(() => {
            const idx = selectedIndex()!;
            const item = menuItems()[idx];
            if (item.disabled) return null;

            const items = menuItems();
            const sliceAngle = 360 / items.length;
            const startAngle = item.angle - sliceAngle / 2;
            const endAngle = item.angle + sliceAngle / 2;

            const innerR = 18;
            const outerR = radius - 10;

            const path = renderSlicePath(position().x, position().y, startAngle, endAngle, innerR, outerR);

            return (
              <path
                d={path}
                fill="url(#selectedSliceGradient)"
                stroke={amber(0.5)}
                stroke-width="1"
                filter="url(#sliceGlow)"
              />
            );
          })()}
        </Show>

        {/* Direction indicator line - points to selected item */}
        <Show when={selectedIndex() !== null}>
          {(() => {
            const idx = selectedIndex()!;
            const item = menuItems()[idx];
            const rads = (item.angle - 90) * Math.PI / 180;
            const lineLength = radius - 20;
            const endX = position().x + Math.cos(rads) * lineLength;
            const endY = position().y + Math.sin(rads) * lineLength;

            return (
              <line
                x1={position().x}
                y1={position().y}
                x2={endX}
                y2={endY}
                stroke={amber(0.8)}
                stroke-width="2"
                stroke-linecap="round"
              />
            );
          })()}
        </Show>
      </svg>

      {/* Menu items */}
      <For each={menuItems()}>
        {(item, index) => {
          const rads = (item.angle - 90) * Math.PI / 180;
          const x = () => position().x + Math.cos(rads) * radius;
          const y = () => position().y + Math.sin(rads) * radius;
          const styles = () => getItemStyles(index(), item);
          const isSelected = () => selectedIndex() === index();

          return (
            <div
              class="fixed z-[9999] flex items-center gap-2 px-3 py-2 rounded-none transition-all duration-75 whitespace-nowrap pointer-events-none"
              style={{
                left: `${x()}px`,
                top: `${y()}px`,
                transform: `translate(-50%, -50%) scale(${isSelected() && !item.disabled ? 1.1 : 1})`,
                background: styles().bg,
                border: `2px solid ${styles().border}`,
                color: styles().color,
                'backdrop-filter': 'blur(12px)',
                'box-shadow': styles().shadow,
                opacity: item.disabled ? 0.4 : 1,
              }}
            >
              <Icon name={item.icon} class="w-4 h-4" />
              <span class="text-[12px] font-medium">{item.label}</span>
              <span
                class="text-[10px] font-mono opacity-60"
              >
                {item.shortcut}
              </span>
              <Show when={item.badge}>
                <span
                  class="ml-1 px-1.5 py-0.5 rounded-none text-[9px] font-bold"
                  style={{ background: 'var(--amber-500)', color: 'var(--pasture-900)' }}
                >
                  {item.badge}
                </span>
              </Show>
            </div>
          );
        }}
      </For>
    </Show>
  );
};

export default RadialMenu;
