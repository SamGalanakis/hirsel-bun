import { type JSX, type ParentProps } from 'solid-js';
import { useEscapeKey } from '../../hooks';

interface BaseModalProps extends ParentProps {
  onClose: () => void;
  class?: string;  // Additional classes for the modal content container
  overlayClass?: string;  // Additional classes for the overlay
  style?: JSX.CSSProperties;  // Inline styles for the modal content container
}

export function BaseModal(props: BaseModalProps): JSX.Element {
  useEscapeKey(() => props.onClose());

  const handleOverlayClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      props.onClose();
    }
  };

  return (
    <div
      class={`fixed inset-0 z-50 flex items-center justify-center bg-black/50 ${props.overlayClass || ''}`}
      onClick={handleOverlayClick}
    >
      <div class={`bg-mantle rounded-none shadow-xl ${props.class || ''}`} style={props.style}>
        {props.children}
      </div>
    </div>
  );
}
