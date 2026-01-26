/**
 * SVG definitions for reusable graphics
 */
import { type Component } from 'solid-js';

export const SvgDefinitions: Component = () => {
  return (
    <svg class="hidden" aria-hidden="true">
      <defs>
        {/* Single sheep icon */}
        <symbol id="sheep-icon" viewBox="0 0 64 64">
          <ellipse cx="32" cy="38" rx="20" ry="14" fill="currentColor" opacity="0.3" />
          <ellipse cx="30" cy="36" rx="18" ry="12" fill="currentColor" opacity="0.5" />
          <ellipse cx="32" cy="34" rx="16" ry="10" fill="currentColor" opacity="0.8" />
          <ellipse cx="48" cy="28" rx="8" ry="6" fill="currentColor" />
          <ellipse cx="52" cy="22" rx="3" ry="4" fill="currentColor" opacity="0.8" />
          <circle cx="50" cy="27" r="2" fill="#1a1f1c" />
          <rect x="22" y="44" width="3" height="10" rx="1" fill="currentColor" opacity="0.7" />
          <rect x="28" y="44" width="3" height="10" rx="1" fill="currentColor" opacity="0.7" />
          <rect x="36" y="44" width="3" height="10" rx="1" fill="currentColor" opacity="0.7" />
          <rect x="42" y="44" width="3" height="10" rx="1" fill="currentColor" opacity="0.7" />
        </symbol>

        {/* Sleeping sheep (for idle/empty states) */}
        <symbol id="sheep-sleeping" viewBox="0 0 64 64">
          <ellipse cx="32" cy="40" rx="20" ry="12" fill="currentColor" opacity="0.3" />
          <ellipse cx="30" cy="38" rx="18" ry="10" fill="currentColor" opacity="0.5" />
          <ellipse cx="32" cy="36" rx="16" ry="8" fill="currentColor" opacity="0.8" />
          <ellipse cx="46" cy="34" rx="7" ry="5" fill="currentColor" />
          <ellipse cx="50" cy="30" rx="2" ry="3" fill="currentColor" opacity="0.8" />
          <path d="M47 33 L49 33" stroke="#1a1f1c" stroke-width="2" stroke-linecap="round" />
          <text x="52" y="20" font-size="8" fill="currentColor" opacity="0.6" font-weight="bold">z</text>
          <text x="55" y="14" font-size="6" fill="currentColor" opacity="0.4" font-weight="bold">z</text>
          <rect x="20" y="44" width="3" height="6" rx="1" fill="currentColor" opacity="0.7" />
          <rect x="26" y="44" width="3" height="6" rx="1" fill="currentColor" opacity="0.7" />
          <rect x="38" y="44" width="3" height="6" rx="1" fill="currentColor" opacity="0.7" />
          <rect x="44" y="44" width="3" height="6" rx="1" fill="currentColor" opacity="0.7" />
        </symbol>

        {/* Flock of sheep (for "no runs" state) */}
        <symbol id="sheep-flock" viewBox="0 0 128 64">
          {/* Back sheep (smaller, faded) */}
          <g transform="translate(10, 8) scale(0.6)" opacity="0.4">
            <use href="#sheep-sleeping" />
          </g>
          <g transform="translate(60, 4) scale(0.5)" opacity="0.3">
            <use href="#sheep-sleeping" />
          </g>
          {/* Front sheep */}
          <g transform="translate(30, 10) scale(0.8)">
            <use href="#sheep-sleeping" />
          </g>
        </symbol>

        {/* Loading sheep (animated in CSS) */}
        <symbol id="sheep-walking" viewBox="0 0 64 64">
          <ellipse cx="32" cy="36" rx="18" ry="12" fill="currentColor" opacity="0.3" />
          <ellipse cx="30" cy="34" rx="16" ry="10" fill="currentColor" opacity="0.5" />
          <ellipse cx="32" cy="32" rx="14" ry="8" fill="currentColor" opacity="0.8" />
          <ellipse cx="46" cy="26" rx="7" ry="5" fill="currentColor" />
          <ellipse cx="50" cy="21" rx="2" ry="3" fill="currentColor" opacity="0.8" />
          <circle cx="48" cy="25" r="1.5" fill="#1a1f1c" />
          <rect class="sheep-leg-back" x="20" y="40" width="3" height="12" rx="1" fill="currentColor" opacity="0.7" />
          <rect class="sheep-leg-front" x="26" y="40" width="3" height="12" rx="1" fill="currentColor" opacity="0.7" />
          <rect class="sheep-leg-back" x="36" y="40" width="3" height="12" rx="1" fill="currentColor" opacity="0.7" />
          <rect class="sheep-leg-front" x="42" y="40" width="3" height="12" rx="1" fill="currentColor" opacity="0.7" />
        </symbol>

        {/* Alert sheep (for errors) */}
        <symbol id="sheep-alert" viewBox="0 0 64 64">
          <ellipse cx="32" cy="38" rx="18" ry="12" fill="currentColor" opacity="0.3" />
          <ellipse cx="30" cy="36" rx="16" ry="10" fill="currentColor" opacity="0.5" />
          <ellipse cx="32" cy="34" rx="14" ry="8" fill="currentColor" opacity="0.8" />
          <ellipse cx="46" cy="24" rx="7" ry="5" fill="currentColor" />
          <ellipse cx="49" cy="17" rx="2" ry="4" fill="currentColor" opacity="0.8" />
          <ellipse cx="44" cy="18" rx="2" ry="3" fill="currentColor" opacity="0.8" />
          <circle cx="48" cy="23" r="2" fill="#1a1f1c" />
          <circle cx="48.5" cy="22.5" r="0.5" fill="#fff" />
          <text x="54" y="14" font-size="10" fill="#d9534f" font-weight="bold">!</text>
          <rect x="22" y="44" width="3" height="10" rx="1" fill="currentColor" opacity="0.7" />
          <rect x="28" y="44" width="3" height="10" rx="1" fill="currentColor" opacity="0.7" />
          <rect x="36" y="44" width="3" height="10" rx="1" fill="currentColor" opacity="0.7" />
          <rect x="42" y="44" width="3" height="10" rx="1" fill="currentColor" opacity="0.7" />
        </symbol>
      </defs>
    </svg>
  );
};
