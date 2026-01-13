/**
 * Sheep Clicker Mini-Game Alpine component
 * A fun easter egg to pass time while agents work
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

interface Achievement {
  id: string;
  name: string;
  description: string;
  requirement: number;
  icon: string;
  unlocked: boolean;
}

interface FloatingNumber {
  id: number;
  value: number;
  x: number;
  y: number;
  opacity: number;
}

/**
 * Sheep clicker game component
 */
export function sheepClickerGame() {
  return {
    isOpen: false,
    wool: 0,
    totalWool: 0,
    woolPerClick: 1,
    woolPerSecond: 0,
    clickMultiplier: 1,
    upgrades: [
      { id: 'lamb', name: 'Lamb', description: 'A cute lamb helps gather wool', baseCost: 15, woolPerSecond: 0.1, icon: '🐑', owned: 0 },
      { id: 'sheepdog', name: 'Sheepdog', description: 'Herds sheep more efficiently', baseCost: 100, woolPerSecond: 1, icon: '🐕', owned: 0 },
      { id: 'shepherd', name: 'Shepherd', description: 'A wise shepherd tends the flock', baseCost: 500, woolPerSecond: 5, icon: '🧑‍🌾', owned: 0 },
      { id: 'pasture', name: 'Green Pasture', description: 'Lush grass makes happy sheep', baseCost: 2000, woolPerSecond: 20, icon: '🌿', owned: 0 },
      { id: 'barn', name: 'Cozy Barn', description: 'Shelter increases wool production', baseCost: 10000, woolPerSecond: 100, icon: '🏠', owned: 0 },
      { id: 'shears', name: 'Golden Shears', description: '+1 wool per click', baseCost: 500, woolPerSecond: 0, icon: '✂️', owned: 0 },
      { id: 'spinning', name: 'Spinning Wheel', description: 'Doubles click power', baseCost: 5000, woolPerSecond: 0, icon: '🎡', owned: 0 },
      { id: 'agent', name: 'AI Agent', description: 'An automated wool gatherer', baseCost: 50000, woolPerSecond: 500, icon: '🤖', owned: 0 },
    ] as Upgrade[],
    achievements: [
      { id: 'first_wool', name: 'First Fleece', description: 'Collect your first wool', requirement: 1, icon: '🎉', unlocked: false },
      { id: 'wool_100', name: 'Woolly Start', description: 'Collect 100 wool', requirement: 100, icon: '⭐', unlocked: false },
      { id: 'wool_1000', name: 'Wool Gatherer', description: 'Collect 1,000 wool', requirement: 1000, icon: '🌟', unlocked: false },
      { id: 'wool_10000', name: 'Wool Baron', description: 'Collect 10,000 wool', requirement: 10000, icon: '💫', unlocked: false },
      { id: 'wool_100000', name: 'Wool Tycoon', description: 'Collect 100,000 wool', requirement: 100000, icon: '👑', unlocked: false },
      { id: 'wool_million', name: 'Wool Millionaire', description: 'Collect 1,000,000 wool', requirement: 1000000, icon: '🏆', unlocked: false },
    ] as Achievement[],
    floatingNumbers: [] as FloatingNumber[],
    lastTick: Date.now(),
    _tickInterval: null as ReturnType<typeof setInterval> | null,
    sheepBounce: false,
    comboCount: 0,
    _comboTimer: null as ReturnType<typeof setTimeout> | null,

    init() {
      this.load();
      this._tickInterval = setInterval(() => this.tick(), 100);
      document.addEventListener('keydown', (e: KeyboardEvent) => {
        const target = e.target as HTMLElement;
        if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;
        if (e.key === 'g') this.toggle();
        if (e.key === 'Escape' && this.isOpen) this.close();
      });
    },

    destroy() {
      if (this._tickInterval) clearInterval(this._tickInterval);
      this.save();
    },

    toggle() {
      this.isOpen = !this.isOpen;
      if (this.isOpen) this.load();
      else this.save();
    },

    open() {
      this.isOpen = true;
      this.load();
    },

    close() {
      this.isOpen = false;
      this.save();
    },

    clickSheep(event: MouseEvent) {
      const woolEarned = this.woolPerClick * this.clickMultiplier;
      if (this._comboTimer) clearTimeout(this._comboTimer);
      this.comboCount++;
      this._comboTimer = setTimeout(() => {
        this.comboCount = 0;
      }, 500);
      const comboBonus = Math.min(this.comboCount * 0.1, 2);
      const totalWool = Math.floor(woolEarned * (1 + comboBonus));
      this.wool += totalWool;
      this.totalWool += totalWool;
      this.sheepBounce = true;
      setTimeout(() => {
        this.sheepBounce = false;
      }, 100);
      const target = event.currentTarget as HTMLElement;
      const rect = target.getBoundingClientRect();
      const x = event.clientX - rect.left;
      const y = event.clientY - rect.top;
      const floatingId = Date.now() + Math.random();
      this.floatingNumbers.push({ id: floatingId, value: totalWool, x, y, opacity: 1 });
      setTimeout(() => {
        this.floatingNumbers = this.floatingNumbers.filter(f => f.id !== floatingId);
      }, 1000);
      this.checkAchievements();
      if (Math.random() < 0.1) this.save();
    },

    buyUpgrade(upgradeId: string) {
      const upgrade = this.upgrades.find(u => u.id === upgradeId);
      if (!upgrade) return;
      const cost = this.getUpgradeCost(upgrade);
      if (this.wool < cost) return;
      this.wool -= cost;
      upgrade.owned++;
      if (upgrade.woolPerSecond > 0) this.woolPerSecond += upgrade.woolPerSecond;
      if (upgrade.id === 'shears') this.woolPerClick += 1;
      if (upgrade.id === 'spinning') this.clickMultiplier *= 2;
      this.save();
    },

    canAfford(cost: number): boolean {
      return this.wool >= cost;
    },

    getUpgradeCost(upgrade: Upgrade): number {
      return Math.floor(upgrade.baseCost * Math.pow(1.15, upgrade.owned));
    },

    formatNumber(n: number): string {
      if (n < 1000) return Math.floor(n).toString();
      if (n < 1000000) return (n / 1000).toFixed(1) + 'K';
      if (n < 1000000000) return (n / 1000000).toFixed(1) + 'M';
      return (n / 1000000000).toFixed(1) + 'B';
    },

    checkAchievements() {
      for (const a of this.achievements) {
        if (!a.unlocked && this.totalWool >= a.requirement) {
          a.unlocked = true;
          window.toast?.success(`Achievement: ${a.name}`, a.icon);
        }
      }
    },

    tick() {
      if (!this.isOpen) return;
      const now = Date.now();
      const delta = (now - this.lastTick) / 1000;
      this.lastTick = now;
      if (this.woolPerSecond > 0) {
        const earned = this.woolPerSecond * delta;
        this.wool += earned;
        this.totalWool += earned;
      }
    },

    save() {
      const data = {
        wool: this.wool,
        totalWool: this.totalWool,
        woolPerClick: this.woolPerClick,
        woolPerSecond: this.woolPerSecond,
        clickMultiplier: this.clickMultiplier,
        upgrades: this.upgrades.map(u => ({ id: u.id, owned: u.owned })),
        achievements: this.achievements.map(a => ({ id: a.id, unlocked: a.unlocked })),
      };
      localStorage.setItem('hirsel-sheep-clicker', JSON.stringify(data));
    },

    load() {
      const saved = localStorage.getItem('hirsel-sheep-clicker');
      if (!saved) return;
      try {
        const data = JSON.parse(saved);
        this.wool = data.wool || 0;
        this.totalWool = data.totalWool || 0;
        this.woolPerClick = data.woolPerClick || 1;
        this.woolPerSecond = data.woolPerSecond || 0;
        this.clickMultiplier = data.clickMultiplier || 1;
        if (data.upgrades) {
          for (const s of data.upgrades) {
            const u = this.upgrades.find(x => x.id === s.id);
            if (u) u.owned = s.owned;
          }
        }
        if (data.achievements) {
          for (const s of data.achievements) {
            const a = this.achievements.find(x => x.id === s.id);
            if (a) a.unlocked = s.unlocked;
          }
        }
        this.lastTick = Date.now();
      } catch (e) {
        console.warn('Failed to load sheep clicker:', e);
      }
    },

    reset() {
      if (!confirm('Reset all progress? This cannot be undone!')) return;
      this.wool = 0;
      this.totalWool = 0;
      this.woolPerClick = 1;
      this.woolPerSecond = 0;
      this.clickMultiplier = 1;
      for (const u of this.upgrades) u.owned = 0;
      for (const a of this.achievements) a.unlocked = false;
      localStorage.removeItem('hirsel-sheep-clicker');
    },
  };
}
