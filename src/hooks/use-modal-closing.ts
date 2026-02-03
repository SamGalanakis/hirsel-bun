import type { Accessor } from 'solid-js';
import { useClickOutside } from './use-click-outside';
import { useEscapeKey } from './use-escape-key';

/**
 * Combined hook for modal/dropdown closing behavior.
 * Handles both click-outside and escape key to close the modal.
 *
 * @param ref Accessor returning the container element to monitor for outside clicks
 * @param isOpen Accessor returning whether the modal is currently open
 * @param onClose Callback to close the modal
 */
export function useModalClosing(
  ref: Accessor<HTMLElement | undefined>,
  isOpen: Accessor<boolean>,
  onClose: () => void,
): void {
  useClickOutside(ref, () => {
    if (isOpen()) onClose();
  });
  useEscapeKey(() => {
    if (isOpen()) onClose();
  });
}
