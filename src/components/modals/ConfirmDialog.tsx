/**
 * Confirm dialog component using native dialog element
 */
import { type Component, createEffect, createSignal, onCleanup } from 'solid-js';

interface ConfirmOptions {
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  danger?: boolean;
}

// Global confirm dialog state
const [pendingResolve, setPendingResolve] = createSignal<
  ((value: boolean) => void) | null
>(null);
const [options, setOptions] = createSignal<ConfirmOptions | null>(null);

// Global API for confirm dialog
export const confirmDialog = {
  show: (opts: ConfirmOptions): Promise<boolean> => {
    return new Promise((resolve) => {
      setOptions(opts);
      setPendingResolve(() => resolve);
      const dialog = document.getElementById('confirm-dialog') as HTMLDialogElement;
      dialog?.showModal();
    });
  },
  delete: (itemName: string, itemType = 'item'): Promise<boolean> => {
    return confirmDialog.show({
      title: `Delete ${itemType}`,
      message: `Are you sure you want to delete "${itemName}"? This action cannot be undone.`,
      confirmText: 'Delete',
      cancelText: 'Cancel',
      danger: true,
    });
  },
};

// Expose globally
if (typeof window !== 'undefined') {
  window.confirmDialog = confirmDialog;
}

export const ConfirmDialog: Component = () => {
  let dialogRef: HTMLDialogElement | undefined;

  const handleConfirm = () => {
    const resolve = pendingResolve();
    if (resolve) {
      resolve(true);
      setPendingResolve(null);
    }
    dialogRef?.close();
  };

  const handleCancel = () => {
    const resolve = pendingResolve();
    if (resolve) {
      resolve(false);
      setPendingResolve(null);
    }
    dialogRef?.close();
  };

  // Handle escape key
  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && dialogRef?.open) {
        handleCancel();
      }
    };
    document.addEventListener('keydown', handler);
    onCleanup(() => document.removeEventListener('keydown', handler));
  });

  const currentOptions = () => options();

  return (
    <dialog
      ref={dialogRef}
      id="confirm-dialog"
      class="dialog w-full max-w-md"
      onClick={(e) => {
        if (e.target === dialogRef) handleCancel();
      }}
    >
      <div>
        <header>
          <h2 class="text-lg font-semibold tracking-tight text-wool-100">
            {currentOptions()?.title || 'Confirm'}
          </h2>
          <p class="text-sm text-wool-400">
            {currentOptions()?.message || 'Are you sure?'}
          </p>
        </header>
        <footer>
          <button class="btn btn-secondary" onClick={handleCancel}>
            {currentOptions()?.cancelText || 'Cancel'}
          </button>
          <button
            class={currentOptions()?.danger ? 'btn btn-destructive' : 'btn'}
            onClick={handleConfirm}
          >
            {currentOptions()?.confirmText || 'Confirm'}
          </button>
        </footer>
      </div>
    </dialog>
  );
};
