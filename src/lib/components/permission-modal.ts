/**
 * Permission Modal component
 *
 * Shows a modal when an AI agent requests permission to perform an action.
 * Used by the direct-chat component for non-hirsel permissions.
 */

import type { PendingPermission, PermissionOption } from '../types';

/**
 * Permission modal Alpine component
 *
 * Usage:
 * <div x-data="permissionModal()" x-show="isOpen">
 *   <template x-if="permission">
 *     <div class="modal">
 *       <h3 x-text="permission.title"></h3>
 *       <template x-for="option in permission.options">
 *         <button @click="respond(option.optionId)" x-text="option.label"></button>
 *       </template>
 *     </div>
 *   </template>
 * </div>
 */
export function permissionModal() {
  return {
    isOpen: false,
    permission: null as PendingPermission | null,
    _resolveCallback: null as ((optionId: string) => void) | null,

    /**
     * Show the modal with a permission request
     * Returns a promise that resolves with the selected option ID
     */
    show(permission: PendingPermission): Promise<string> {
      this.permission = permission;
      this.isOpen = true;

      return new Promise((resolve) => {
        this._resolveCallback = resolve;
      });
    },

    /**
     * Respond to the permission request
     */
    respond(optionId: string) {
      if (this._resolveCallback) {
        this._resolveCallback(optionId);
        this._resolveCallback = null;
      }

      this.isOpen = false;
      this.permission = null;
    },

    /**
     * Close without responding (will use first non-allow option or deny)
     */
    dismiss() {
      if (this.permission && this._resolveCallback) {
        // Find a deny/skip option
        const denyOption = this.permission.options.find(
          (o) => o.kind !== 'AllowAlways' && o.kind !== 'AllowOnce'
        );
        const optionId = denyOption?.optionId || this.permission.options[0]?.optionId || 'deny';
        this._resolveCallback(optionId);
        this._resolveCallback = null;
      }

      this.isOpen = false;
      this.permission = null;
    },

    /**
     * Get icon for permission option based on kind
     */
    getOptionIcon(kind: string): string {
      const icons: Record<string, string> = {
        AllowOnce: '\u2713',      // ✓
        AllowAlways: '\u2713\u2713', // ✓✓
        Deny: '\u2717',           // ✗
        Skip: '\u27A1',           // ➡
      };
      return icons[kind] || '';
    },

    /**
     * Get button class for permission option based on kind
     */
    getOptionClass(kind: string): string {
      const classes: Record<string, string> = {
        AllowOnce: 'btn-primary',
        AllowAlways: 'btn-success',
        Deny: 'btn-danger',
        Skip: 'btn-secondary',
      };
      return classes[kind] || 'btn-secondary';
    },
  };
}
