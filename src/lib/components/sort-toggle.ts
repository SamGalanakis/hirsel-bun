/**
 * Reusable sort toggle component
 * Inspired by W&B's clean sort UI
 */

export type SortDirection = 'asc' | 'desc';

export interface SortOption {
  value: string;
  label: string;
}

export interface SortToggleConfig {
  /** Current sort field */
  field: string;
  /** Current sort direction */
  direction: SortDirection;
  /** Available sort options */
  options?: SortOption[];
  /** Callback when sort changes */
  onChange?: (field: string, direction: SortDirection) => void;
}

/**
 * Sort toggle component for use in headers
 * Usage in Alpine: x-data="sortToggle({ field: 'timestamp', direction: 'desc', onChange: (f, d) => ... })"
 */
export function sortToggle(config: Partial<SortToggleConfig> = {}) {
  return {
    field: config.field || 'timestamp',
    direction: (config.direction || 'desc') as SortDirection,
    options: config.options || [{ value: 'timestamp', label: 'Time' }],
    isOpen: false,
    _onChange: config.onChange,

    get currentLabel(): string {
      const opt = this.options.find((o: SortOption) => o.value === this.field);
      return opt ? opt.label : this.field;
    },

    get directionLabel(): string {
      return this.direction === 'desc' ? 'Latest' : 'Oldest';
    },

    get directionIcon(): string {
      return this.direction === 'desc' ? '↓' : '↑';
    },

    toggleDirection() {
      this.direction = this.direction === 'desc' ? 'asc' : 'desc';
      this.emitChange();
    },

    setField(value: string) {
      this.field = value;
      this.isOpen = false;
      this.emitChange();
    },

    toggle() {
      this.isOpen = !this.isOpen;
    },

    close() {
      this.isOpen = false;
    },

    emitChange() {
      if (this._onChange) {
        this._onChange(this.field, this.direction);
      }
      // Also dispatch a custom event for flexibility
      window.dispatchEvent(
        new CustomEvent('sort-changed', {
          detail: { field: this.field, direction: this.direction },
        }),
      );
    },
  };
}

/**
 * Simple inline sort button (no dropdown, just toggles direction)
 * Usage: x-data="sortButton({ direction: 'desc', onChange: (d) => ... })"
 */
export function sortButton(
  config: {
    direction?: SortDirection;
    label?: string;
    onChange?: (direction: SortDirection) => void;
  } = {},
) {
  return {
    direction: (config.direction || 'desc') as SortDirection,
    label: config.label || 'Time',
    _onChange: config.onChange,

    get displayLabel(): string {
      return this.direction === 'desc' ? 'Latest' : 'Oldest';
    },

    get icon(): string {
      return this.direction === 'desc' ? '↓' : '↑';
    },

    toggle() {
      this.direction = this.direction === 'desc' ? 'asc' : 'desc';
      if (this._onChange) {
        this._onChange(this.direction);
      }
    },
  };
}
