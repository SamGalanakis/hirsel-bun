/**
 * Sheep Clicker Mini-Game
 * A fun easter egg to pass time while agents work
 */
import {
  type Component,
  For,
  Show,
  createEffect,
  createSignal,
  onCleanup,
  onMount,
} from 'solid-js';
import { createStore, produce } from 'solid-js/store';

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
}

const INITIAL_UPGRADES: Upgrade[] = [
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
];

const INITIAL_ACHIEVEMENTS: Achievement[] = [
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
];

const STORAGE_KEY = 'hirsel-sheep-clicker';

function formatNumber(n: number): string {
  if (n < 1000) return Math.floor(n).toString();
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}K`;
  if (n < 1_000_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  return `${(n / 1_000_000_000).toFixed(1)}B`;
}

function getUpgradeCost(upgrade: Upgrade): number {
  return Math.floor(upgrade.baseCost * 1.15 ** upgrade.owned);
}

export const SheepClicker: Component = () => {
  const [visible, setVisible] = createSignal(false);
  const [wool, setWool] = createSignal(0);
  const [totalWool, setTotalWool] = createSignal(0);
  const [woolPerClick, setWoolPerClick] = createSignal(1);
  const [woolPerSecond, setWoolPerSecond] = createSignal(0);
  const [clickMultiplier, setClickMultiplier] = createSignal(1);
  const [comboCount, setComboCount] = createSignal(0);
  const [sheepBounce, setSheepBounce] = createSignal(false);
  const [floatingNumbers, setFloatingNumbers] = createSignal<FloatingNumber[]>([]);

  const [upgrades, setUpgrades] = createStore<Upgrade[]>(
    structuredClone(INITIAL_UPGRADES)
  );
  const [achievements, setAchievements] = createStore<Achievement[]>(
    structuredClone(INITIAL_ACHIEVEMENTS)
  );

  let lastTick = Date.now();
  let comboTimer: ReturnType<typeof setTimeout> | null = null;

  const save = () => {
    const data = {
      wool: wool(),
      totalWool: totalWool(),
      woolPerClick: woolPerClick(),
      woolPerSecond: woolPerSecond(),
      clickMultiplier: clickMultiplier(),
      upgrades: upgrades.map((u) => ({ id: u.id, owned: u.owned })),
      achievements: achievements.map((a) => ({ id: a.id, unlocked: a.unlocked })),
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
  };

  const load = () => {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (!saved) return;
    try {
      const data = JSON.parse(saved);
      setWool(data.wool || 0);
      setTotalWool(data.totalWool || 0);
      setWoolPerClick(data.woolPerClick || 1);
      setWoolPerSecond(data.woolPerSecond || 0);
      setClickMultiplier(data.clickMultiplier || 1);
      if (data.upgrades) {
        for (const s of data.upgrades) {
          const idx = upgrades.findIndex((x) => x.id === s.id);
          if (idx !== -1) {
            setUpgrades(idx, 'owned', s.owned);
          }
        }
      }
      if (data.achievements) {
        for (const s of data.achievements) {
          const idx = achievements.findIndex((x) => x.id === s.id);
          if (idx !== -1) {
            setAchievements(idx, 'unlocked', s.unlocked);
          }
        }
      }
      lastTick = Date.now();
    } catch (e) {
      console.warn('Failed to load sheep clicker:', e);
    }
  };

  const checkAchievements = () => {
    const total = totalWool();
    setAchievements(
      produce((achs) => {
        for (const a of achs) {
          if (!a.unlocked && total >= a.requirement) {
            a.unlocked = true;
            window.toast?.success(`Achievement: ${a.name}`, a.icon);
          }
        }
      })
    );
  };

  const handleClick = (event: MouseEvent) => {
    const woolEarned = woolPerClick() * clickMultiplier();
    if (comboTimer) clearTimeout(comboTimer);
    setComboCount((c) => c + 1);
    comboTimer = setTimeout(() => setComboCount(0), 500);

    const comboBonus = Math.min(comboCount() * 0.1, 2);
    const earned = Math.floor(woolEarned * (1 + comboBonus));

    setWool((w) => w + earned);
    setTotalWool((t) => t + earned);

    setSheepBounce(true);
    setTimeout(() => setSheepBounce(false), 100);

    // Floating number
    const target = event.currentTarget as HTMLElement;
    const rect = target.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    const floatingId = Date.now() + Math.random();
    setFloatingNumbers((nums) => [...nums, { id: floatingId, value: earned, x, y }]);
    setTimeout(() => {
      setFloatingNumbers((nums) => nums.filter((f) => f.id !== floatingId));
    }, 1000);

    checkAchievements();
    if (Math.random() < 0.1) save();
  };

  const buyUpgrade = (upgradeId: string) => {
    const idx = upgrades.findIndex((u) => u.id === upgradeId);
    if (idx === -1) return;
    const upgrade = upgrades[idx];
    const cost = getUpgradeCost(upgrade);
    if (wool() < cost) return;

    setWool((w) => w - cost);
    setUpgrades(idx, 'owned', upgrade.owned + 1);

    if (upgrade.woolPerSecond > 0) {
      setWoolPerSecond((w) => w + upgrade.woolPerSecond);
    }
    if (upgrade.id === 'shears') {
      setWoolPerClick((w) => w + 1);
    }
    if (upgrade.id === 'spinning') {
      setClickMultiplier((m) => m * 2);
    }
    save();
  };

  const reset = () => {
    if (!confirm('Reset all progress? This cannot be undone!')) return;
    setWool(0);
    setTotalWool(0);
    setWoolPerClick(1);
    setWoolPerSecond(0);
    setClickMultiplier(1);
    setUpgrades(
      produce((ups) => {
        for (const u of ups) u.owned = 0;
      })
    );
    setAchievements(
      produce((achs) => {
        for (const a of achs) a.unlocked = false;
      })
    );
    localStorage.removeItem(STORAGE_KEY);
  };

  // Listen for shortcut
  createEffect(() => {
    const handler = (e: Event) => {
      const customEvent = e as CustomEvent<string>;
      if (customEvent.detail === 'sheep-game') {
        setVisible(true);
        load();
      }
    };
    window.addEventListener('shortcut-action', handler);
    onCleanup(() => window.removeEventListener('shortcut-action', handler));
  });

  // Handle escape key
  createEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && visible()) {
        setVisible(false);
        save();
      }
    };
    document.addEventListener('keydown', handler);
    onCleanup(() => document.removeEventListener('keydown', handler));
  });

  // Game tick for passive income
  onMount(() => {
    const interval = setInterval(() => {
      if (!visible()) return;
      const now = Date.now();
      const delta = (now - lastTick) / 1000;
      lastTick = now;
      if (woolPerSecond() > 0) {
        const earned = woolPerSecond() * delta;
        setWool((w) => w + earned);
        setTotalWool((t) => t + earned);
      }
    }, 100);
    onCleanup(() => clearInterval(interval));
  });

  const unlockedAchievements = () => achievements.filter((a) => a.unlocked);

  return (
    <Show when={visible()}>
      <div
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/70"
        onClick={(e) => {
          if (e.target === e.currentTarget) {
            setVisible(false);
            save();
          }
        }}
      >
        <div
          class="bg-pasture-800 rounded-xl shadow-2xl w-[700px] max-h-[85vh] overflow-hidden border border-pasture-600"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div class="p-4 border-b border-pasture-600 flex items-center justify-between bg-gradient-to-r from-pasture-700 to-pasture-800">
            <div class="flex items-center gap-3">
              <span class="text-3xl">🐑</span>
              <div>
                <h2 class="text-xl font-bold text-amber-500">Sheep Clicker</h2>
                <p class="text-xs text-wool-500">Pass time while your agents work</p>
              </div>
            </div>
            <div class="flex items-center gap-2">
              <button
                onClick={reset}
                class="text-xs px-2 py-1 rounded bg-terra/20 text-terra hover:bg-terra/30 transition-colors"
              >
                Reset
              </button>
              <button
                onClick={() => {
                  setVisible(false);
                  save();
                }}
                class="text-wool-500 hover:text-wool-300 transition-colors"
              >
                <svg class="w-6 h-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M18 6L6 18M6 6l12 12" />
                </svg>
              </button>
            </div>
          </div>

          <div class="flex h-[500px]">
            {/* Main Game Area */}
            <div class="flex-1 flex flex-col items-center justify-center p-6 relative overflow-hidden">
              {/* Wool Counter */}
              <div class="text-center mb-6">
                <div class="text-5xl font-bold text-amber-500">{formatNumber(wool())}</div>
                <div class="text-wool-400 text-lg">wool</div>
                <div class="text-wool-600 text-sm mt-1">
                  {formatNumber(woolPerClick() * clickMultiplier())}/click
                  <span class="mx-2">•</span>
                  {formatNumber(woolPerSecond())}/sec
                </div>
              </div>

              {/* Clickable Sheep */}
              <button
                onClick={handleClick}
                class="relative w-48 h-48 rounded-full bg-gradient-to-br from-wool-200 to-wool-400 shadow-lg hover:shadow-xl transition-all duration-100 flex items-center justify-center group"
                classList={{ 'scale-95': sheepBounce() }}
              >
                <svg
                  class="w-36 h-36 text-pasture-800 group-hover:scale-105 transition-transform"
                  viewBox="0 0 64 64"
                >
                  <ellipse cx="32" cy="38" rx="20" ry="14" fill="currentColor" opacity="0.3" />
                  <ellipse cx="30" cy="36" rx="18" ry="12" fill="currentColor" opacity="0.5" />
                  <ellipse cx="32" cy="34" rx="16" ry="10" fill="currentColor" opacity="0.8" />
                  <ellipse cx="48" cy="28" rx="8" ry="6" fill="currentColor" />
                  <ellipse cx="52" cy="22" rx="3" ry="4" fill="currentColor" opacity="0.8" />
                  <circle cx="50" cy="27" r="2" fill="#fff" />
                  <circle cx="50.5" cy="26.5" r="1" fill="#1a1f1c" />
                  <rect x="22" y="44" width="3" height="10" rx="1" fill="currentColor" opacity="0.7" />
                  <rect x="28" y="44" width="3" height="10" rx="1" fill="currentColor" opacity="0.7" />
                  <rect x="36" y="44" width="3" height="10" rx="1" fill="currentColor" opacity="0.7" />
                  <rect x="42" y="44" width="3" height="10" rx="1" fill="currentColor" opacity="0.7" />
                </svg>

                {/* Floating Numbers */}
                <For each={floatingNumbers()}>
                  {(num) => (
                    <div
                      class="absolute text-amber-400 font-bold text-xl pointer-events-none sheep-clicker-floating"
                      style={{ left: `${num.x}px`, top: `${num.y}px` }}
                    >
                      +{num.value}
                    </div>
                  )}
                </For>
              </button>

              {/* Combo indicator */}
              <Show when={comboCount() > 1}>
                <div class="mt-4 text-amber-400 font-bold">{comboCount()}x Combo!</div>
              </Show>

              {/* Achievement badges */}
              <div class="absolute bottom-4 left-4 right-4 text-center">
                <For each={unlockedAchievements().slice(-3)}>
                  {(achievement) => (
                    <div class="inline-block mx-1 px-2 py-1 bg-golden/20 text-golden rounded text-xs">
                      {achievement.icon} {achievement.name}
                    </div>
                  )}
                </For>
              </div>
            </div>

            {/* Upgrades Panel */}
            <div class="w-64 border-l border-pasture-600 bg-pasture-900/50 flex flex-col">
              <div class="p-3 border-b border-pasture-600">
                <h3 class="font-semibold text-wool-200">Upgrades</h3>
              </div>
              <div class="flex-1 overflow-y-auto p-2 space-y-2">
                <For each={upgrades}>
                  {(upgrade) => {
                    const cost = () => getUpgradeCost(upgrade);
                    const canAfford = () => wool() >= cost();
                    return (
                      <button
                        onClick={() => buyUpgrade(upgrade.id)}
                        disabled={!canAfford()}
                        class="w-full p-3 rounded-lg text-left transition-all duration-150"
                        classList={{
                          'bg-pasture-700 hover:bg-pasture-600 cursor-pointer': canAfford(),
                          'bg-pasture-800 opacity-50 cursor-not-allowed': !canAfford(),
                        }}
                      >
                        <div class="flex items-center gap-2">
                          <span class="text-2xl">{upgrade.icon}</span>
                          <div class="flex-1 min-w-0">
                            <div class="flex items-center justify-between">
                              <span class="font-medium text-wool-200 text-sm">{upgrade.name}</span>
                              <Show when={upgrade.owned > 0}>
                                <span class="text-xs text-wool-500">x{upgrade.owned}</span>
                              </Show>
                            </div>
                            <div class="text-xs text-wool-500 truncate">{upgrade.description}</div>
                            <div
                              class="text-xs font-medium mt-1"
                              classList={{
                                'text-sage': canAfford(),
                                'text-terra': !canAfford(),
                              }}
                            >
                              {formatNumber(cost())} wool
                            </div>
                          </div>
                        </div>
                      </button>
                    );
                  }}
                </For>
              </div>

              {/* Stats */}
              <div class="p-3 border-t border-pasture-600 text-xs text-wool-500 space-y-1">
                <div class="flex justify-between">
                  <span>Total wool:</span>
                  <span class="text-wool-300">{formatNumber(totalWool())}</span>
                </div>
                <div class="flex justify-between">
                  <span>Achievements:</span>
                  <span class="text-wool-300">
                    {unlockedAchievements().length}/{achievements.length}
                  </span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Show>
  );
};
