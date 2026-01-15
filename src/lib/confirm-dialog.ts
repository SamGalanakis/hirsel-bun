/**
 * Confirm Dialog - A styled replacement for native confirm()
 *
 * Uses basecoat dialog component for a consistent UI.
 * Returns a Promise<boolean> that resolves when the user responds.
 */

export interface ConfirmOptions {
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  danger?: boolean;
}

let dialogElement: HTMLDialogElement | null = null;
let resolvePromise: ((value: boolean) => void) | null = null;

/**
 * Initialize the confirm dialog (call once on app start)
 */
export function initConfirmDialog(): void {
  dialogElement = document.getElementById('confirm-dialog') as HTMLDialogElement;

  if (!dialogElement) {
    console.warn('[confirm-dialog] Dialog element not found');
    return;
  }

  // Handle cancel button
  const cancelBtn = dialogElement.querySelector('[data-confirm-cancel]');
  cancelBtn?.addEventListener('click', () => {
    dialogElement?.close();
    resolvePromise?.(false);
    resolvePromise = null;
  });

  // Handle confirm button
  const confirmBtn = dialogElement.querySelector('[data-confirm-ok]');
  confirmBtn?.addEventListener('click', () => {
    dialogElement?.close();
    resolvePromise?.(true);
    resolvePromise = null;
  });

  // Handle backdrop click (close without confirm)
  const dialog = dialogElement;
  dialog.addEventListener('click', (e) => {
    if (e.target === dialog) {
      dialog.close();
      resolvePromise?.(false);
      resolvePromise = null;
    }
  });

  // Handle escape key
  dialog.addEventListener('cancel', (e) => {
    e.preventDefault();
    dialog.close();
    resolvePromise?.(false);
    resolvePromise = null;
  });
}

/**
 * Show a confirm dialog and wait for user response
 */
export function showConfirm(options: ConfirmOptions): Promise<boolean> {
  if (!dialogElement) {
    // Fallback to native confirm if dialog not initialized
    return Promise.resolve(confirm(`${options.title}\n\n${options.message}`));
  }

  // Update dialog content
  const titleEl = dialogElement.querySelector('[data-confirm-title]');
  const messageEl = dialogElement.querySelector('[data-confirm-message]');
  const cancelBtn = dialogElement.querySelector('[data-confirm-cancel]');
  const confirmBtn = dialogElement.querySelector('[data-confirm-ok]');

  if (titleEl) titleEl.textContent = options.title;
  if (messageEl) messageEl.textContent = options.message;
  if (cancelBtn) cancelBtn.textContent = options.cancelText ?? 'Cancel';
  if (confirmBtn) {
    confirmBtn.textContent = options.confirmText ?? 'Confirm';
    // Toggle danger styling using basecoat classes
    if (options.danger) {
      confirmBtn.classList.add('btn-destructive');
      confirmBtn.classList.remove('btn', 'btn-primary');
    } else {
      confirmBtn.classList.remove('btn-destructive');
      confirmBtn.classList.add('btn');
    }
  }

  // Show dialog
  dialogElement.showModal();

  // Return promise that resolves when user responds
  return new Promise((resolve) => {
    resolvePromise = resolve;
  });
}

// Convenience wrapper for simple delete confirmations
export function confirmDelete(itemName: string, itemType = 'item'): Promise<boolean> {
  return showConfirm({
    title: `Delete ${itemType}?`,
    message: `Are you sure you want to delete "${itemName}"? This cannot be undone.`,
    confirmText: 'Delete',
    cancelText: 'Cancel',
    danger: true,
  });
}

// Export for global access
export const confirmDialog = {
  show: showConfirm,
  delete: confirmDelete,
};

// Make available globally
if (typeof window !== 'undefined') {
  // @ts-expect-error Adding confirmDialog to window for global access
  window.confirmDialog = confirmDialog;
}
