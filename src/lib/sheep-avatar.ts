/**
 * Sheep avatar SVG generator
 * Generates parametric sheep avatars based on Noto emoji (128x128 viewBox)
 */

import type { SheepConfig, WorkerStatus } from './types';

// Hat definitions (scaled 3.2x, positioned at top of wool)
const HATS: Record<number, { svg: string; y: number }> = {
  0: { svg: '', y: 0 },
  // Crown (for leaders)
  1: { svg: `<path d="M-6 0 L-4 -6 L-2 -2 L0 -8 L2 -2 L4 -6 L6 0 Z" fill="#FFD700" stroke="#DAA520" stroke-width="0.4"/><circle cx="-2" cy="-1.5" r="0.8" fill="#E53935"/><circle cx="0" cy="-3" r="0.8" fill="#4CAF50"/><circle cx="2" cy="-1.5" r="0.8" fill="#2196F3"/>`, y: -1 },
  // Cowboy
  2: { svg: `<ellipse cx="0" cy="0" rx="8" ry="1.5" fill="#8B4513"/><path d="M-5 0 Q0 -7 5 0" fill="#A0522D"/>`, y: 0 },
  // Top hat
  3: { svg: `<ellipse cx="0" cy="0" rx="5" ry="1.2" fill="#1a1a1a"/><rect x="-3" y="-9" width="6" height="9" fill="#1a1a1a"/><rect x="-3" y="-2" width="6" height="1.2" fill="#8B0000"/>`, y: 0 },
  // Beanie
  4: { svg: `<path d="M-5 0 Q-5 -6 0 -7 Q5 -6 5 0" fill="#E91E63"/><circle cx="0" cy="-8" r="2" fill="#E91E63"/>`, y: 0 },
  // Wizard
  5: { svg: `<path d="M0 -14 L-5 0 L5 0 Z" fill="#4A148C"/><circle cx="0" cy="-6" r="0.7" fill="#FFD700"/>`, y: 0 },
  // Chef
  6: { svg: `<ellipse cx="0" cy="-5" rx="5" ry="4" fill="#FAFAFA"/><ellipse cx="-3" cy="-6" rx="3" ry="3" fill="#FAFAFA"/><ellipse cx="3" cy="-6" rx="3" ry="3" fill="#FAFAFA"/><rect x="-4" y="-1" width="8" height="2.5" fill="#FAFAFA"/>`, y: 0 },
  // Hard hat
  7: { svg: `<path d="M-5 0 Q0 -7 5 0" fill="#FFC107"/><rect x="-5" y="-1.5" width="10" height="2" fill="#FFC107"/>`, y: 0 },
  // Detective hat (reserved for eval agents)
  8: { svg: `<ellipse cx="0" cy="0" rx="7" ry="1.8" fill="#4A4A4A"/><path d="M-4.5 0 Q-4.5 -5 0 -6 Q4.5 -5 4.5 0" fill="#5D5D5D"/><rect x="-5" y="-1" width="10" height="1.5" fill="#3D3D3D"/>`, y: 0 },
};

// Glasses - positioned over eyes (left eye: 27,43, right eye: 50,47)
const GLASSES: Record<number, string> = {
  0: '',
  // Round glasses
  1: `<g>
    <circle cx="27" cy="43" r="7" fill="none" stroke="#1a1a1a" stroke-width="1.8"/>
    <circle cx="50" cy="47" r="7" fill="none" stroke="#1a1a1a" stroke-width="1.8"/>
    <path d="M34 44 Q38 42 43 46" stroke="#1a1a1a" stroke-width="1.8" fill="none"/>
    <path d="M20 42 L14 38" stroke="#1a1a1a" stroke-width="1.8"/>
  </g>`,
  // Square glasses
  2: `<g>
    <rect x="21" y="37" width="12" height="12" fill="none" stroke="#1a1a1a" stroke-width="1.8" rx="2"/>
    <rect x="44" y="41" width="12" height="12" fill="none" stroke="#1a1a1a" stroke-width="1.8" rx="2"/>
    <path d="M33 43 Q38 40 44 47" stroke="#1a1a1a" stroke-width="1.8" fill="none"/>
    <path d="M21 42 L15 38" stroke="#1a1a1a" stroke-width="1.8"/>
  </g>`,
  // Sunglasses
  3: `<g>
    <ellipse cx="27" cy="44" rx="10" ry="7" fill="#1a1a1a"/>
    <ellipse cx="50" cy="48" rx="10" ry="7" fill="#1a1a1a"/>
    <path d="M37 44 Q40 42 42 46" stroke="#1a1a1a" stroke-width="3" fill="none"/>
    <path d="M17 42 L10 37" stroke="#1a1a1a" stroke-width="2.5"/>
    <ellipse cx="27" cy="44" rx="7" ry="4" fill="#3a3a5a" opacity="0.5"/>
    <ellipse cx="50" cy="48" rx="7" ry="4" fill="#3a3a5a" opacity="0.5"/>
  </g>`,
  // Eyepatch
  4: `<g>
    <ellipse cx="50" cy="47" rx="8" ry="6" fill="#1a1a1a"/>
    <path d="M42 45 Q20 30 25 15" stroke="#1a1a1a" stroke-width="2" fill="none"/>
  </g>`,
};

// Bow ties - positioned below the face
const BOWTIES: Record<number, string> = {
  0: '',
  // Red
  1: `<g transform="translate(35, 78)">
    <path d="M0 0 L-8 -5 L-8 5 Z" fill="#D32F2F"/>
    <path d="M0 0 L8 -5 L8 5 Z" fill="#D32F2F"/>
    <circle cx="0" cy="0" r="3" fill="#B71C1C"/>
  </g>`,
  // Blue
  2: `<g transform="translate(35, 78)">
    <path d="M0 0 L-8 -5 L-8 5 Z" fill="#1976D2"/>
    <path d="M0 0 L8 -5 L8 5 Z" fill="#1976D2"/>
    <circle cx="0" cy="0" r="3" fill="#0D47A1"/>
  </g>`,
  // Gold
  3: `<g transform="translate(35, 78)">
    <path d="M0 0 L-8 -5 L-8 5 Z" fill="#FFD700"/>
    <path d="M0 0 L8 -5 L8 5 Z" fill="#FFD700"/>
    <circle cx="0" cy="0" r="3" fill="#FFA000"/>
  </g>`,
  // Pink
  4: `<g transform="translate(35, 78)">
    <path d="M0 0 L-8 -5 L-8 5 Z" fill="#E91E63"/>
    <path d="M0 0 L8 -5 L8 5 Z" fill="#E91E63"/>
    <circle cx="0" cy="0" r="3" fill="#AD1457"/>
  </g>`,
};

// Noto emoji paths (128x128 viewBox)
const LEGS_PATH = `M39.64 89.66l-12.58.56s1.88 20.65 2.16 22.71s1.03 6.1 6.66 6.01s5.44-5.73 5.35-7.04c-.09-1.31-.56-9.95-.56-9.95l8.82 2.16s.09 12.58.19 14.36s1.31 5.54 6.57 5.35s5.73-4.32 5.73-5.35c0-1.03.66-14.45.66-14.45l10.89-.94s-.15 3.94 0 6.66c.09 1.69 2.72 4.5 7.41 4.22c4.69-.28 6.38-2.53 6.29-3.85c-.09-1.31.38-11.92.38-11.92l5.07.56s-.38 11.64-.28 12.58c.09.94 2.16 4.32 7.32 4.22c5.16-.09 6.46-2.45 6.57-3.57c.19-1.97.94-13.23 1.22-16.05c.28-2.82.38-13.7.38-13.7l-68.25 7.43z`;
const BACK_EAR_PATH = `M106.65 42.45s1.27-.28 3.1.42s5.63 6.9 7.46 7.88c1.83.99 4.65 1.69 5.35 2.25c1.74 1.39 1.41 8.87-2.25 11.26c-3.66 2.39-8.17 3.1-8.17 3.1l-9.43-17.03l3.94-7.88z`;
const FACE_PATH = `M47.52 4.58C31.12 1.27 25 15.85 25 15.85s-11.83-4.5-15.77 9.01c-3.2 10.98 9.01 17.18 9.01 17.18S4.58 43.25 5.01 57.75c.27 9.15 6.48 11.45 6.48 11.45s-5.72 7.44-.84 17.6c5.07 10.56 16.61 10 16.61 10s1.42 9.49 14.5 13.23c11.83 3.38 17.74-1.55 17.74-1.55s4.93 4.36 13.51 1.97c7.08-1.97 8.17-8.02 8.17-8.02s8.16 3.93 17.03-.28c8.31-3.94 9.29-12.39 9.29-12.39s11.54-3.66 11.83-18.72c.26-13.79-8.31-19.01-8.31-19.01s4.36-10.7-5.49-18.72c-8.08-6.58-15.2-3.52-15.2-3.52s-.99-5.49-7.18-6.9s-9.43 1.97-9.43 1.97s-.59-3.68-4.08-6.05c-3.94-2.67-7.6-1.13-7.6-1.13s.12-10.14-14.52-13.1z`;
const WOOL_PATH = `M48.37 7.68c-15.86-2.96-21.12 11.28-22.24 11.4c-1.27.14-9.24-6.08-13.94 5.91c-4.09 10.43 10.56 16.2 10.13 17.89c-.37 1.47-14.79 1.05-14.41 14.31c.24 8.37 6.86 10.45 7.04 11.73c.14.99-6.3 7.02-1.69 16.24c4.41 8.82 14.99 8.02 16.61 9.1c1.63 1.09-.19 7.41 10.32 11.83c12.83 5.38 17.62-1.26 19.05-1.5c1.84-.31 2.63 5.07 12.2 3.38c7.86-1.39 7.1-8.04 8.54-8.73c1.44-.69 6.1 5 16.14.94c7.88-3.19 7.98-10.23 8.82-11.4c1.1-1.52 10.62-4.39 11.12-18.16c.52-14.27-7.83-17.25-8.02-18.4c-.19-1.13 3.38-9.57-4.5-16.14c-7.32-6.1-12.95-1.88-14.08-2.63c-.5-.33-.19-6.1-7.04-7.6c-5.68-1.24-9.39 3-10 2.67c-1.36-.73-1.06-4.09-4.26-6.26c-3.61-2.45-6.87-.5-7.85-1.06c-.84-.5 1.73-10.97-11.94-13.52z`;
const EAR_DETAIL_PATH = `M28.47 33.82s-4.94 3.24-10.14 4.04c-5.54.84-13.89-1.6-14.45.38s2.82 7.13 8.26 8.92c5.04 1.65 7.13.09 7.13.09s1.08 1.92.42 4.74s-2.72 7.17-2.3 11.5c.47 4.79 4.34 11.72 13.42 13.61c11.26 2.35 19.37-3.51 21.68-8.73c2.86-6.48.99-11.03 4.46-14.12c3.47-3.1 7.93-3.43 11.78-3.24c3.85.19 7.6-1.92 8.73-2.96c1.13-1.03 1.69-3.05 1.41-3.43c-.28-.38-2.91-.19-5.63-1.5c-2.72-1.31-7.32-3.94-7.32-3.94s-3.24 3.33-8.02 2.67c-4.79-.66-6.34-5.77-6.34-5.77s-4.97 7.6-13.7 5.63c-7.44-1.69-9.39-7.89-9.39-7.89z`;
const EYE_R_PATH = `M53.28 48.73c-.63 2.6-3.22 3.5-5.31 2.49c-1.89-.91-2.81-3.18-1.94-5.36s3.16-3.31 5.1-2.53c1.94.78 2.7 3.12 2.15 5.4z`;
const EYE_L_PATH = `M29.66 44.47c-.62 2.48-2.9 3.91-4.79 2.99c-1.69-.82-2.49-2.96-1.65-5.04c.84-2.08 2.83-2.94 4.58-2.24s2.41 2.12 1.86 4.29z`;
const NOSE_PATH = `M31.85 61.37s6.34-3.1 7.04-1.22c.7 1.88-1.88 3.33-2.91 3.99c-1.03.66-3.38 1.22-3.38 1.22s.19 3.66 2.06 3.75c1.88.09 3.24-2.49 4.46-1.17c1.22 1.31-.47 4.04-3.38 4.04s-4.5-2.06-4.5-2.06s-1.91 1.13-3.57.7c-2.58-.66-3-3.57-2.16-4.22s1.45 1.41 2.91 1.31c1.45-.09 1.88-2.67 1.88-2.67s-1.69-.8-3.1-2.49c-.81-.97-2.61-3.36-1.69-4.32c1.51-1.56 6.34 3.14 6.34 3.14z`;

// Status colors for glow effect
const STATUS_COLORS: Record<WorkerStatus, string> = {
  idle: '#64748b',     // slate
  working: '#4CAF50',  // green
  waiting: '#FF9800',  // orange
  awaiting: '#2196F3', // blue
  paused: '#9E9E9E',   // gray
  error: '#f44336',    // red
};

/** Options for customizing sheep appearance beyond SheepConfig */
export interface SheepOptions {
  /** Wool color override (default: white #FEFEFE) */
  woolColor?: string;
  /** Status for glow effect */
  status?: WorkerStatus;
}

/**
 * Generate sheep SVG from config
 */
export function generateSheepSvg(
  config: SheepConfig,
  size = 64,
  statusOrOptions?: WorkerStatus | SheepOptions
): string {
  // Handle both old API (status string) and new API (options object)
  const options: SheepOptions = typeof statusOrOptions === 'string'
    ? { status: statusOrOptions }
    : (statusOrOptions || {});

  const wool = options.woolColor || '#FEFEFE';
  const skin = '#3D3D3D';
  const dark = '#2b2b2b';
  const legColor = '#b1b1b1';

  // Parametric variations
  const bodyScale = 1 + config.bodyWidth * 0.03;
  const woolScale = 1 + config.fluffiness * 0.02;
  const earShift = config.earPosition * 2;
  const legStretch = 1 + config.legLength * 0.05;
  const rotation = config.bodyWidth * 1.5;

  // Hue shift (disabled by default)
  const hueFilter = config.hueShift > 0 ? `filter="url(#hue${config.hueShift})"` : '';
  const hueFilterDef = config.hueShift > 0
    ? `<defs><filter id="hue${config.hueShift}"><feColorMatrix type="hueRotate" values="${config.hueShift}"/></filter></defs>`
    : '';

  const hat = HATS[config.hat] || HATS[0];
  const hatRotation = -5 + (config.bodyWidth * 4) + (config.earPosition * 3);
  const hatSvg = hat.svg
    ? `<g transform="translate(46, ${16 + hat.y}) scale(3.2) rotate(${hatRotation})">${hat.svg}</g>`
    : '';

  const glassesSvg = GLASSES[config.glasses] || '';
  const bowtieSvg = BOWTIES[config.bowtie] || '';

  // Status glow
  const statusColor = options.status ? STATUS_COLORS[options.status] : null;
  const statusGlow = statusColor
    ? `<ellipse cx="64" cy="70" rx="55" ry="50" fill="${statusColor}" opacity="0.25" filter="url(#blur)"/>
       <defs><filter id="blur"><feGaussianBlur stdDeviation="8"/></filter></defs>`
    : '';

  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" width="${size}" height="${size}">
    ${hueFilterDef}
    ${statusGlow}
    <g transform="translate(128, 0) scale(-1, 1)">
      <g transform="rotate(${rotation}, 64, 64)">
        <g transform="translate(0, ${(legStretch - 1) * -10}) scale(1, ${legStretch})">
          <path fill="${legColor}" d="${LEGS_PATH}"/>
        </g>
        <g transform="translate(${earShift}, 0)">
          <path fill="${legColor}" d="${BACK_EAR_PATH}"/>
        </g>
        <g transform="scale(${bodyScale})" transform-origin="64 64" ${hueFilter}>
          <path fill="${skin}" d="${FACE_PATH}"/>
        </g>
        <g transform="scale(${woolScale})" transform-origin="64 50">
          <path fill="${wool}" d="${WOOL_PATH}"/>
        </g>
        <path fill="${legColor}" d="${EAR_DETAIL_PATH}"/>
        ${bowtieSvg}
        <path fill="${dark}" d="${EYE_R_PATH}"/>
        <path fill="${dark}" d="${EYE_L_PATH}"/>
        ${glassesSvg}
        <path fill="${dark}" d="${NOSE_PATH}"/>
        ${hatSvg}
      </g>
    </g>
  </svg>`;
}

/**
 * Get sheep SVG for a worker
 */
export function getWorkerSheepSvg(
  worker: { sheepConfig: SheepConfig; status?: WorkerStatus },
  size = 32
): string {
  return generateSheepSvg(worker.sheepConfig, size, worker.status);
}

/**
 * Get hat name by ID
 */
export function getHatName(id: number): string {
  return ['None', 'Crown', 'Cowboy', 'Top Hat', 'Beanie', 'Wizard', 'Chef', 'Hard Hat', 'Detective'][id] || 'Unknown';
}

/**
 * Gyp (Border Collie) avatar - references the SVG file directly
 */
export function generateCollieSvg(size = 64): string {
  return `<img src="/gyp.svg" width="${size}" height="${size}" alt="Gyp" style="object-fit: contain;" />`;
}

// Alias for backwards compatibility
export const generateAgentSheepSvg = generateCollieSvg;

// Export for use in global scope
declare global {
  interface Window {
    generateSheepSvg: typeof generateSheepSvg;
    getWorkerSheepSvg: typeof getWorkerSheepSvg;
    generateCollieSvg: typeof generateCollieSvg;
    generateAgentSheepSvg: typeof generateAgentSheepSvg;
  }
}

if (typeof window !== 'undefined') {
  window.generateSheepSvg = generateSheepSvg;
  window.getWorkerSheepSvg = getWorkerSheepSvg;
  window.generateCollieSvg = generateCollieSvg;
  window.generateAgentSheepSvg = generateAgentSheepSvg;
}
