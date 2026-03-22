/**
 * SVG definitions for reusable graphics
 */
import { type Component } from 'solid-js';

export const SvgDefinitions: Component = () => {
  return (
    <svg class="hidden" aria-hidden="true">
      <defs>
        {/* Nautilus spiral loader */}
        <symbol id="nautilus-loader" viewBox="0 0 64 64">
          <path
            d="M32,32 C32,28 36,24 40,24 C46,24 50,30 50,36 C50,46 40,52 30,52 C16,52 8,40 8,26 C8,8 24,-4 44,-4"
            fill="none"
            stroke="currentColor"
            stroke-width="1.5"
            opacity="0.6"
            stroke-linecap="round"
          />
        </symbol>
        {/* Architecture icon for project identity */}
        <symbol id="architect-mark" viewBox="0 0 24 24">
          <path
            d="M2 20h20M5 20V8l7-5 7 5v12M9 20v-6h6v6"
            fill="none"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </symbol>
      </defs>
    </svg>
  );
};
