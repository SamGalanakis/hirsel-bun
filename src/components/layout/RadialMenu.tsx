/**
 * RadialMenu - Blender-style pie menu
 *
 * Opens at mouse cursor. Move mouse in any direction to highlight items,
 * click to select. Tab to cycle, Escape to cancel.
 *
 * Triggered by: ` (tilde/backtick) - configurable in settings
 */
import { type Component, Show, For, createSignal, createEffect, onCleanup } from 'solid-js';
import { useApp, useProject } from '../../stores';
import { useDelta } from '../../stores/delta-context';
import { Icon } from '../shared';

export const RadialMenu: Component = () => {
  const app = useApp();
  const project = useProject();
  const delta = useDelta();

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
  const calculateSelectedItem = (offsetX: number, offsetY: number): { index: number | null; angle: number | null } => {
    const distance = Math.sqrt(offsetX * offsetX + offsetY * offsetY);
    if (distance < deadZone) {
      return { index: null, angle: null }; // In dead zone, nothing selected
    }

    // Calculate angle from center (0° = right, counter-clockwise)
    let angle = Math.atan2(-offsetY, offsetX) * (180 / Math.PI);
    // Convert to 0° = up, clockwise
    angle = (90 - angle + 360) % 360;

    // Find closest item
    const items = menuItems();
    let closestIndex = 0;
    let closestDiff = 360;

    items.forEach((item, index) => {
      let diff = Math.abs(item.angle - angle);
      if (diff > 180) diff = 360 - diff;
      if (diff < closestDiff) {
        closestDiff = diff;
        closestIndex = index;
      }
    });

    return { index: closestIndex, angle };
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
      setSelectedIndex(result.index);
      setCurrentAngle(result.angle);
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

  // Compute diff badge
  const diffBadge = () => {
    if (!delta.hasDiff()) return undefined;
    const diff = delta.diff();
    const total =
      (diff?.newNodes.length || 0) +
      (diff?.modifiedNodes.length || 0) +
      (diff?.deletedNodes.length || 0);
    return total > 0 ? total : undefined;
  };

  // Check if deliver is available
  const canDeliver = () => {
    return delta.liveTree() !== null && delta.liveTree()!.length > 0;
  };

  // Action handlers
  const handleDispatch = () => {
    app.closeRadialMenu();
    window.dispatchEvent(new CustomEvent('radial-dispatch'));
  };

  const handleDeliver = () => {
    app.closeRadialMenu();
    window.dispatchEvent(new CustomEvent('radial-deliver'));
  };

  const handleDocs = () => {
    app.closeRadialMenu();
    project.setDocsOpen(!project.docsOpen());
  };

  const handleChat = () => {
    app.closeRadialMenu();
    project.setActiveThread('meadow');
    project.setSheepfoldOpen(true);
  };

  const handleIde = () => {
    app.closeRadialMenu();
    window.dispatchEvent(new CustomEvent('radial-open-ide'));
  };

  const handleSettings = () => {
    app.closeRadialMenu();
    if (project.selectedProject()) {
      project.setShowProjectSettings(true);
    } else {
      app.setShowSettings(true);
    }
  };

  // 6 route-level actions evenly spaced (60° intervals)
  // Starting from top (0°) going clockwise
  const menuItems = () => [
    { id: 'deliver', angle: 0, icon: 'package', label: 'Deliver', shortcut: '8', badge: undefined, variant: canDeliver() ? 'success' : 'default', disabled: !canDeliver() || delta.deliveryPending(), onClick: handleDeliver },
    { id: 'chat', angle: 60, icon: 'message-circle', label: 'Chat', shortcut: '9', badge: undefined, variant: 'default', disabled: false, onClick: handleChat },
    { id: 'settings', angle: 120, icon: 'settings', label: 'Settings', shortcut: '3', badge: undefined, variant: 'default', disabled: false, onClick: handleSettings },
    { id: 'ide', angle: 180, icon: 'folder-open', label: 'IDE', shortcut: '2', badge: undefined, variant: 'default', disabled: !delta.projectRun(), onClick: handleIde },
    { id: 'docs', angle: 240, icon: 'book-open', label: 'Docs', shortcut: '1', badge: undefined, variant: project.docsOpen() ? 'primary' : 'default', disabled: false, onClick: handleDocs },
    { id: 'dispatch', angle: 300, icon: 'rocket', label: 'Dispatch', shortcut: '7', badge: diffBadge(), variant: delta.hasDiff() ? 'primary' : 'default', disabled: !delta.hasDiff() || delta.dispatchPending(), onClick: handleDispatch },
  ];

  const getItemStyles = (index: number, item: ReturnType<typeof menuItems>[0]) => {
    const isSelected = selectedIndex() === index;
    const isDisabled = item.disabled;

    if (isDisabled) {
      return {
        bg: 'rgba(25, 25, 25, 0.95)',
        border: 'rgba(40, 40, 40, 0.8)',
        color: 'var(--wool-700)',
        shadow: '0 2px 8px rgba(0, 0, 0, 0.2)',
      };
    }

    if (isSelected) {
      switch (item.variant) {
        case 'primary':
          return {
            bg: 'rgba(212, 165, 116, 0.4)',
            border: 'rgba(212, 165, 116, 0.9)',
            color: 'var(--amber-300)',
            shadow: '0 0 20px rgba(212, 165, 116, 0.5), 0 4px 16px rgba(0, 0, 0, 0.3)',
          };
        case 'success':
          return {
            bg: 'rgba(125, 153, 112, 0.4)',
            border: 'rgba(125, 153, 112, 0.9)',
            color: 'var(--sage)',
            shadow: '0 0 20px rgba(125, 153, 112, 0.5), 0 4px 16px rgba(0, 0, 0, 0.3)',
          };
        default:
          return {
            bg: 'rgba(70, 70, 70, 0.95)',
            border: 'rgba(140, 140, 140, 0.9)',
            color: 'var(--wool-50)',
            shadow: '0 0 16px rgba(255, 255, 255, 0.1), 0 4px 16px rgba(0, 0, 0, 0.3)',
          };
      }
    }

    // Not selected
    switch (item.variant) {
      case 'primary':
        return {
          bg: 'rgba(212, 165, 116, 0.15)',
          border: 'rgba(212, 165, 116, 0.4)',
          color: 'var(--amber-400)',
          shadow: '0 2px 8px rgba(0, 0, 0, 0.2)',
        };
      case 'success':
        return {
          bg: 'rgba(125, 153, 112, 0.15)',
          border: 'rgba(125, 153, 112, 0.4)',
          color: 'var(--sage)',
          shadow: '0 2px 8px rgba(0, 0, 0, 0.2)',
        };
      default:
        return {
          bg: 'rgba(35, 35, 35, 0.95)',
          border: 'rgba(55, 55, 55, 0.8)',
          color: 'var(--wool-300)',
          shadow: '0 2px 8px rgba(0, 0, 0, 0.2)',
        };
    }
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
          class="absolute w-12 h-12 rounded-full"
          style={{
            left: '50%',
            top: '50%',
            transform: 'translate(-50%, -50%)',
            background: 'radial-gradient(circle, rgba(212, 165, 116, 0.15) 0%, transparent 70%)',
          }}
        />
        {/* Center ring */}
        <div
          class="relative w-7 h-7 rounded-full flex items-center justify-center"
          style={{
            background: 'rgba(25, 25, 25, 0.98)',
            border: '2px solid rgba(212, 165, 116, 0.7)',
            'box-shadow': '0 0 15px rgba(212, 165, 116, 0.4), inset 0 1px 2px rgba(255, 255, 255, 0.1)',
          }}
        >
          <div
            class="w-2 h-2 rounded-full"
            style={{
              background: 'rgba(212, 165, 116, 0.9)',
              'box-shadow': '0 0 6px rgba(212, 165, 116, 0.8)',
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
            <stop offset="0%" stop-color="rgba(212, 165, 116, 0.3)" />
            <stop offset="100%" stop-color="rgba(212, 165, 116, 0.05)" />
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

            const startRad = (startAngle - 90) * Math.PI / 180;
            const endRad = (endAngle - 90) * Math.PI / 180;
            const innerR = 18;
            const outerR = radius - 10;

            const x1 = position().x + Math.cos(startRad) * innerR;
            const y1 = position().y + Math.sin(startRad) * innerR;
            const x2 = position().x + Math.cos(startRad) * outerR;
            const y2 = position().y + Math.sin(startRad) * outerR;
            const x3 = position().x + Math.cos(endRad) * outerR;
            const y3 = position().y + Math.sin(endRad) * outerR;
            const x4 = position().x + Math.cos(endRad) * innerR;
            const y4 = position().y + Math.sin(endRad) * innerR;

            const path = `
              M ${x1} ${y1}
              L ${x2} ${y2}
              A ${outerR} ${outerR} 0 0 1 ${x3} ${y3}
              L ${x4} ${y4}
              A ${innerR} ${innerR} 0 0 0 ${x1} ${y1}
              Z
            `;

            return (
              <path
                d={path}
                fill="url(#selectedSliceGradient)"
                stroke="rgba(212, 165, 116, 0.5)"
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
                stroke="rgba(212, 165, 116, 0.8)"
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
              class="fixed z-[9999] flex items-center gap-2 px-3 py-2 rounded-lg transition-all duration-75 whitespace-nowrap pointer-events-none"
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
                  class="ml-1 px-1.5 py-0.5 rounded text-[9px] font-bold"
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
