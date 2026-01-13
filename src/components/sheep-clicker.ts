/**
 * Sheep Clicker Mini-Game
 *
 * A fun cookie-clicker style game with sheep to pass time while agents work.
 * Activated by pressing 'g' (game) key.
 */

/**
 * Upgrade definitions
 */
interface Upgrade {
  id: string;
  name: string;
  description: string;
  baseCost: number;
  woolPerSecond: number;
  icon: string;
  owned: number;
}

/**
 * Achievement definitions
 */
interface Achievement {
  id: string;
  name: string;
  description: string;
  requirement: number;
  icon: string;
  unlocked: boolean;
}

/**
 * Floating number animation
 */
interface FloatingNumber {
  id: number;
  value: number;
  x: number;
  y: number;
  opacity: number;
}

/**
 * Sheep clicker component data
 */
export interface SheepClickerData {
  isOpen: boolean;
  wool: number;
  totalWool: number;
  woolPerClick: number;
  woolPerSecond: number;
  sheepCount: number;
  clickMultiplier: number;
  upgrades: Upgrade[];
  achievements: Achievement[];
  floatingNumbers: FloatingNumber[];
  lastTick: number;
  tickInterval: ReturnType<typeof setInterval> | null;
  sheepBounce: boolean;
  comboCount: number;
  comboTimer: ReturnType<typeof setTimeout> | null;
}

/**
 * Alpine.js component factory for the sheep clicker game
 */
export function sheepClicker(): SheepClickerData & {
  init(): void;
  destroy(): void;
  toggle(): void;
  open(): void;
  close(): void;
  clickSheep(event: MouseEvent): void;
  buyUpgrade(upgradeId: string): void;
  canAfford(cost: number): boolean;
  getUpgradeCost(upgrade: Upgrade): number;
  formatNumber(n: number): string;
  checkAchievements(): void;
  tick(): void;
  save(): void;
  load(): void;
  reset(): void;
} {
  return {
    isOpen: false,
    wool: 0,
    totalWool: 0,
    woolPerClick: 1,
    woolPerSecond: 0,
    sheepCount: 1,
    clickMultiplier: 1,
    upgrades: [
      {
        id: 'lamb',
        name: 'Lamb',
        description: 'A cute lamb helps gather wool',
        baseCost: 15,
        woolPerSecond: 0.1,
        icon: '🐑',
        owned: 0,
      },
      {
        id: 'sheepdog',
        name: 'Sheepdog',
        description: 'Herds sheep more efficiently',
        baseCost: 100,
        woolPerSecond: 1,
        icon: '🐕',
        owned: 0,
      },
      {
        id: 'shepherd',
        name: 'Shepherd',
        description: 'A wise shepherd tends the flock',
        baseCost: 500,
        woolPerSecond: 5,
        icon: '🧑‍🌾',
        owned: 0,
      },
      {
        id: 'pasture',
        name: 'Green Pasture',
        description: 'Lush grass makes happy sheep',
        baseCost: 2000,
        woolPerSecond: 20,
        icon: '🌿',
        owned: 0,
      },
      {
        id: 'barn',
        name: 'Cozy Barn',
        description: 'Shelter increases wool production',
        baseCost: 10000,
        woolPerSecond: 100,
        icon: '🏠',
        owned: 0,
      },
      {
        id: 'shears',
        name: 'Golden Shears',
        description: '+1 wool per click',
        baseCost: 500,
        woolPerSecond: 0,
        icon: '✂️',
        owned: 0,
      },
      {
        id: 'spinning',
        name: 'Spinning Wheel',
        description: 'Doubles click power',
        baseCost: 5000,
        woolPerSecond: 0,
        icon: '🎡',
        owned: 0,
      },
      {
        id: 'agent',
        name: 'AI Agent',
        description: 'An automated wool gatherer',
        baseCost: 50000,
        woolPerSecond: 500,
        icon: '🤖',
        owned: 0,
      },
    ],
    achievements: [
      {
        id: 'first_wool',
        name: 'First Fleece',
        description: 'Collect your first wool',
        requirement: 1,
        icon: '🎉',
        unlocked: false,
      },
      {
        id: 'wool_100',
        name: 'Woolly Start',
        description: 'Collect 100 wool',
        requirement: 100,
        icon: '⭐',
        unlocked: false,
      },
      {
        id: 'wool_1000',
        name: 'Wool Gatherer',
        description: 'Collect 1,000 wool',
        requirement: 1000,
        icon: '🌟',
        unlocked: false,
      },
      {
        id: 'wool_10000',
        name: 'Wool Baron',
        description: 'Collect 10,000 wool',
        requirement: 10000,
        icon: '💫',
        unlocked: false,
      },
      {
        id: 'wool_100000',
        name: 'Wool Tycoon',
        description: 'Collect 100,000 wool',
        requirement: 100000,
        icon: '👑',
        unlocked: false,
      },
      {
        id: 'wool_million',
        name: 'Wool Millionaire',
        description: 'Collect 1,000,000 wool',
        requirement: 1000000,
        icon: '🏆',
        unlocked: false,
      },
    ],
    floatingNumbers: [],
    lastTick: Date.now(),
    tickInterval: null,
    sheepBounce: false,
    comboCount: 0,
    comboTimer: null,

    /**
     * Initialize the game
     */
    init(): void {
      this.load();

      // Start tick interval for passive income
      this.tickInterval = setInterval(() => {
        this.tick();
      }, 100);

      // Listen for 'g' key to toggle game
      document.addEventListener('keydown', ((e: KeyboardEvent) => {
        if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
        if (e.key === 'g') {
          this.toggle();
        }
      }) as EventListener);
    },

    /**
     * Cleanup
     */
    destroy(): void {
      if (this.tickInterval) {
        clearInterval(this.tickInterval);
        this.tickInterval = null;
      }
      this.save();
    },

    /**
     * Toggle game visibility
     */
    toggle(): void {
      this.isOpen = !this.isOpen;
      if (this.isOpen) {
        this.load();
      } else {
        this.save();
      }
    },

    /**
     * Open the game
     */
    open(): void {
      this.isOpen = true;
      this.load();
    },

    /**
     * Close the game
     */
    close(): void {
      this.isOpen = false;
      this.save();
    },

    /**
     * Handle sheep click
     */
    clickSheep(event: MouseEvent): void {
      // Calculate wool earned
      const woolEarned = this.woolPerClick * this.clickMultiplier;

      // Update combo
      if (this.comboTimer) clearTimeout(this.comboTimer);
      this.comboCount++;
      this.comboTimer = setTimeout(() => {
        this.comboCount = 0;
      }, 500);

      // Bonus for combo
      const comboBonus = Math.min(this.comboCount * 0.1, 2); // Max 2x bonus
      const totalWool = Math.floor(woolEarned * (1 + comboBonus));

      this.wool += totalWool;
      this.totalWool += totalWool;

      // Animate sheep bounce
      this.sheepBounce = true;
      setTimeout(() => {
        this.sheepBounce = false;
      }, 100);

      // Add floating number
      const target = event.currentTarget as HTMLElement;
      const rect = target.getBoundingClientRect();
      const x = event.clientX - rect.left;
      const y = event.clientY - rect.top;

      const floatingId = Date.now() + Math.random();
      this.floatingNumbers.push({
        id: floatingId,
        value: totalWool,
        x,
        y,
        opacity: 1,
      });

      // Remove floating number after animation
      setTimeout(() => {
        this.floatingNumbers = this.floatingNumbers.filter((f) => f.id !== floatingId);
      }, 1000);

      // Check achievements
      this.checkAchievements();

      // Save periodically
      if (Math.random() < 0.1) {
        this.save();
      }
    },

    /**
     * Buy an upgrade
     */
    buyUpgrade(upgradeId: string): void {
      const upgrade = this.upgrades.find((u) => u.id === upgradeId);
      if (!upgrade) return;

      const cost = this.getUpgradeCost(upgrade);
      if (this.wool < cost) return;

      this.wool -= cost;
      upgrade.owned++;

      // Apply upgrade effects
      if (upgrade.woolPerSecond > 0) {
        this.woolPerSecond += upgrade.woolPerSecond;
      }

      // Special upgrades
      if (upgrade.id === 'shears') {
        this.woolPerClick += 1;
      }
      if (upgrade.id === 'spinning') {
        this.clickMultiplier *= 2;
      }

      this.save();
    },

    /**
     * Check if player can afford a cost
     */
    canAfford(cost: number): boolean {
      return this.wool >= cost;
    },

    /**
     * Get the current cost of an upgrade (increases with each purchase)
     */
    getUpgradeCost(upgrade: Upgrade): number {
      return Math.floor(upgrade.baseCost * Math.pow(1.15, upgrade.owned));
    },

    /**
     * Format large numbers nicely
     */
    formatNumber(n: number): string {
      if (n < 1000) return Math.floor(n).toString();
      if (n < 1000000) return (n / 1000).toFixed(1) + 'K';
      if (n < 1000000000) return (n / 1000000).toFixed(1) + 'M';
      return (n / 1000000000).toFixed(1) + 'B';
    },

    /**
     * Check and unlock achievements
     */
    checkAchievements(): void {
      for (const achievement of this.achievements) {
        if (!achievement.unlocked && this.totalWool >= achievement.requirement) {
          achievement.unlocked = true;
          // Could show a toast notification here
          console.log(`Achievement unlocked: ${achievement.name}`);
        }
      }
    },

    /**
     * Game tick for passive income
     */
    tick(): void {
      if (!this.isOpen) return;

      const now = Date.now();
      const delta = (now - this.lastTick) / 1000; // seconds
      this.lastTick = now;

      if (this.woolPerSecond > 0) {
        const earned = this.woolPerSecond * delta;
        this.wool += earned;
        this.totalWool += earned;
      }
    },

    /**
     * Save game to localStorage
     */
    save(): void {
      const saveData = {
        wool: this.wool,
        totalWool: this.totalWool,
        woolPerClick: this.woolPerClick,
        woolPerSecond: this.woolPerSecond,
        clickMultiplier: this.clickMultiplier,
        upgrades: this.upgrades.map((u) => ({ id: u.id, owned: u.owned })),
        achievements: this.achievements.map((a) => ({ id: a.id, unlocked: a.unlocked })),
      };
      localStorage.setItem('hirsel-sheep-clicker', JSON.stringify(saveData));
    },

    /**
     * Load game from localStorage
     */
    load(): void {
      const saved = localStorage.getItem('hirsel-sheep-clicker');
      if (!saved) return;

      try {
        const data = JSON.parse(saved);
        this.wool = data.wool || 0;
        this.totalWool = data.totalWool || 0;
        this.woolPerClick = data.woolPerClick || 1;
        this.woolPerSecond = data.woolPerSecond || 0;
        this.clickMultiplier = data.clickMultiplier || 1;

        // Restore upgrade owned counts
        if (data.upgrades) {
          for (const saved of data.upgrades) {
            const upgrade = this.upgrades.find((u) => u.id === saved.id);
            if (upgrade) {
              upgrade.owned = saved.owned;
            }
          }
        }

        // Restore achievement unlocked states
        if (data.achievements) {
          for (const saved of data.achievements) {
            const achievement = this.achievements.find((a) => a.id === saved.id);
            if (achievement) {
              achievement.unlocked = saved.unlocked;
            }
          }
        }

        this.lastTick = Date.now();
      } catch (e) {
        console.warn('Failed to load sheep clicker save:', e);
      }
    },

    /**
     * Reset the game
     */
    reset(): void {
      if (!confirm('Reset all progress? This cannot be undone!')) return;

      this.wool = 0;
      this.totalWool = 0;
      this.woolPerClick = 1;
      this.woolPerSecond = 0;
      this.clickMultiplier = 1;

      for (const upgrade of this.upgrades) {
        upgrade.owned = 0;
      }
      for (const achievement of this.achievements) {
        achievement.unlocked = false;
      }

      localStorage.removeItem('hirsel-sheep-clicker');
    },
  };
}

/**
 * Register the component with Alpine.js
 */
export function registerSheepClickerComponent(): void {
  if (typeof window !== 'undefined') {
    (window as unknown as Record<string, unknown>).sheepClicker = sheepClicker;
  }
}

// Auto-register if Alpine is already loaded
if (typeof window !== 'undefined' && typeof Alpine !== 'undefined') {
  registerSheepClickerComponent();
}

// Declare Alpine global for TypeScript
declare const Alpine: {
  store: (name: string) => Record<string, unknown> | undefined;
};
