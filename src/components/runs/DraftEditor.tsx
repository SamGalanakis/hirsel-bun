/**
 * Draft editor component for configuring and starting runs
 */
import { invoke } from '@tauri-apps/api/core';
import {
  type Component,
  For,
  Show,
  createEffect,
  createSignal,
  onCleanup,
} from 'solid-js';
import type { RunnerEntry } from '../../lib/types';
import { useRuns } from '../../stores';

type EditorTab = 'spec' | 'eval' | 'settings';

export const DraftEditor: Component = () => {
  const runs = useRuns();

  const [spec, setSpec] = createSignal('');
  const [evalContent, setEvalContent] = createSignal('');
  const [workerScale, setWorkerScale] = createSignal('1');
  const [timeLimitMinutes, setTimeLimitMinutes] = createSignal<number | null>(30);
  const [humanInTheLoop, setHumanInTheLoop] = createSignal(true);
  const [runner, setRunner] = createSignal<string | null>(null);
  const [runners, setRunners] = createSignal<RunnerEntry[]>([]);
  const [activeTab, setActiveTab] = createSignal<EditorTab>('spec');
  const [saving, setSaving] = createSignal(false);
  const [starting, setStarting] = createSignal(false);
  const [dragOver, setDragOver] = createSignal(false);
  const [specLoaded, setSpecLoaded] = createSignal(false);
  const [evalLoaded, setEvalLoaded] = createSignal(false);

  let saveTimeout: ReturnType<typeof setTimeout> | undefined;
  let specTextareaRef: HTMLTextAreaElement | undefined;

  const detail = () => runs.runDetail();
  const runName = () => runs.selectedRun();

  // Load spec and eval content when run changes
  createEffect(() => {
    const name = runName();
    if (!name) return;

    setSpecLoaded(false);
    setEvalLoaded(false);

    // Load spec
    invoke<string>('read_spec_file', { runName: name })
      .then((content) => {
        setSpec(content || '');
        setSpecLoaded(true);
      })
      .catch(() => {
        setSpec('');
        setSpecLoaded(true);
      });

    // Load eval
    invoke<string>('read_eval_file', { runName: name })
      .then((content) => {
        setEvalContent(content || '');
        setEvalLoaded(true);
      })
      .catch(() => {
        setEvalContent('');
        setEvalLoaded(true);
      });

    // Load runners
    invoke<RunnerEntry[]>('get_runners')
      .then((r) => setRunners(r || []))
      .catch(() => setRunners([]));
  });

  // Sync settings from detail
  createEffect(() => {
    const d = detail();
    if (d) {
      setWorkerScale(d.workerScale || '1');
      setTimeLimitMinutes(d.timeLimitMinutes ?? 30);
      setHumanInTheLoop(d.humanInTheLoop ?? true);
      setRunner(d.runner);
    }
  });

  // Debounced auto-save for spec/eval
  const scheduleSave = (field: 'spec' | 'eval', value: string) => {
    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = setTimeout(() => {
      const name = runName();
      if (!name) return;

      if (field === 'spec') {
        invoke('write_spec_file', { runName: name, content: value }).catch((e) =>
          console.error('Failed to save spec:', e)
        );
      } else {
        invoke('write_eval_file', { runName: name, content: value }).catch((e) =>
          console.error('Failed to save eval:', e)
        );
      }
    }, 1000);
  };

  const handleSpecInput = (value: string) => {
    setSpec(value);
    scheduleSave('spec', value);
  };

  const handleEvalInput = (value: string) => {
    setEvalContent(value);
    scheduleSave('eval', value);
  };

  // Save draft settings
  const saveDraft = async () => {
    const name = runName();
    if (!name) return;

    setSaving(true);
    try {
      await invoke('update_draft', {
        runName: name,
        updates: {
          workerScale: workerScale(),
          timeLimitMinutes: timeLimitMinutes(),
          humanInTheLoop: humanInTheLoop(),
          runner: runner(),
        },
      });
      window.toast?.success('Draft saved');
    } catch (e) {
      console.error('Failed to save draft:', e);
      window.toast?.error(`Failed to save draft: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  // Start the run
  const startRun = async () => {
    const name = runName();
    if (!name) return;

    if (!spec().trim()) {
      window.toast?.error('Specification is required');
      setActiveTab('spec');
      return;
    }

    setStarting(true);
    try {
      // Save spec first
      await invoke('write_spec_file', { runName: name, content: spec() });
      await invoke('write_eval_file', { runName: name, content: evalContent() });

      // Start the run
      await invoke('start_draft', { runName: name });
      window.toast?.success('Run started');
      await runs.invalidateRuns();
    } catch (e) {
      console.error('Failed to start run:', e);
      window.toast?.error(`Failed to start run: ${e}`);
    } finally {
      setStarting(false);
    }
  };

  // Handle file drop
  const handleDrop = async (e: DragEvent) => {
    e.preventDefault();
    setDragOver(false);

    const name = runName();
    if (!name || !e.dataTransfer?.files.length) return;

    const file = e.dataTransfer.files[0];
    const reader = new FileReader();

    reader.onload = async () => {
      try {
        const data = new Uint8Array(reader.result as ArrayBuffer);
        await invoke('save_asset', {
          runName: name,
          filename: file.name,
          data: Array.from(data),
        });

        // Insert reference into spec
        const ref = `![${file.name}](assets/${file.name})`;
        const textarea = specTextareaRef;
        if (textarea) {
          const start = textarea.selectionStart;
          const end = textarea.selectionEnd;
          const text = spec();
          const newText = text.substring(0, start) + ref + text.substring(end);
          setSpec(newText);
          scheduleSave('spec', newText);

          // Move cursor after inserted reference
          setTimeout(() => {
            textarea.selectionStart = textarea.selectionEnd = start + ref.length;
            textarea.focus();
          }, 0);
        }

        window.toast?.success(`Asset "${file.name}" saved`);
      } catch (e) {
        console.error('Failed to save asset:', e);
        window.toast?.error(`Failed to save asset: ${e}`);
      }
    };

    reader.readAsArrayBuffer(file);
  };

  const handleDragOver = (e: DragEvent) => {
    e.preventDefault();
    setDragOver(true);
  };

  const handleDragLeave = (e: DragEvent) => {
    e.preventDefault();
    setDragOver(false);
  };

  // Open assets folder
  const openAssets = async () => {
    const name = runName();
    if (!name) return;
    try {
      await invoke('open_assets_folder', { runName: name });
    } catch (e) {
      console.error('Failed to open assets folder:', e);
    }
  };

  // Cleanup
  onCleanup(() => {
    if (saveTimeout) clearTimeout(saveTimeout);
  });

  return (
    <div class="flex-1 flex flex-col overflow-hidden">
      {/* Header */}
      <div class="flex items-center justify-between p-4 border-b border-pasture-600">
        <div class="flex items-center gap-3">
          <h2 class="text-lg font-medium text-wool-100">{runName()}</h2>
          <span class="px-2 py-0.5 text-xs rounded bg-sky-500/20 text-sky-400">
            draft
          </span>
        </div>
        <div class="flex items-center gap-2">
          <button
            class="btn-outline"
            onClick={saveDraft}
            disabled={saving() || starting()}
          >
            <Show when={saving()}>
              <span class="spinner w-4 h-4" />
            </Show>
            <Show when={!saving()}>
              <i data-lucide="save" class="w-4 h-4" />
            </Show>
            Save Draft
          </button>
          <button
            class="btn"
            onClick={startRun}
            disabled={saving() || starting()}
          >
            <Show when={starting()}>
              <span class="spinner w-4 h-4" />
            </Show>
            <Show when={!starting()}>
              <i data-lucide="play" class="w-4 h-4" />
            </Show>
            Start Run
          </button>
        </div>
      </div>

      {/* Tab bar */}
      <div class="tabs border-b border-pasture-600">
        <nav role="tablist" class="px-4">
          <button
            role="tab"
            aria-selected={activeTab() === 'spec'}
            onClick={() => setActiveTab('spec')}
            class="px-3 py-2 text-sm"
          >
            <i data-lucide="file-text" class="w-3.5 h-3.5 inline-block mr-1.5" />
            Specification
          </button>
          <button
            role="tab"
            aria-selected={activeTab() === 'eval'}
            onClick={() => setActiveTab('eval')}
            class="px-3 py-2 text-sm"
          >
            <i data-lucide="check-circle" class="w-3.5 h-3.5 inline-block mr-1.5" />
            Evaluation
          </button>
          <button
            role="tab"
            aria-selected={activeTab() === 'settings'}
            onClick={() => setActiveTab('settings')}
            class="px-3 py-2 text-sm"
          >
            <i data-lucide="settings" class="w-3.5 h-3.5 inline-block mr-1.5" />
            Settings
          </button>
        </nav>
      </div>

      {/* Tab content */}
      <div class="flex-1 overflow-auto">
        {/* Spec Tab */}
        <Show when={activeTab() === 'spec'}>
          <div
            class={`relative h-full ${dragOver() ? 'ring-2 ring-amber-500 ring-inset' : ''}`}
            onDrop={handleDrop}
            onDragOver={handleDragOver}
            onDragLeave={handleDragLeave}
          >
            <Show when={!specLoaded()}>
              <div class="flex items-center justify-center h-full">
                <div class="spinner w-8 h-8" />
              </div>
            </Show>
            <Show when={specLoaded()}>
              <textarea
                ref={specTextareaRef}
                class="w-full h-full p-4 bg-transparent resize-none font-mono text-sm focus:outline-none"
                placeholder="Describe what you want to accomplish...

You can drop images here to add them as assets."
                value={spec()}
                onInput={(e) => handleSpecInput(e.currentTarget.value)}
              />
            </Show>
            {/* Drop overlay */}
            <Show when={dragOver()}>
              <div class="absolute inset-0 bg-amber-500/10 flex items-center justify-center pointer-events-none">
                <div class="text-center">
                  <i data-lucide="upload" class="w-12 h-12 text-amber-500 mx-auto mb-2" />
                  <p class="text-sm text-amber-400">Drop file to add as asset</p>
                </div>
              </div>
            </Show>
          </div>
        </Show>

        {/* Eval Tab */}
        <Show when={activeTab() === 'eval'}>
          <Show when={!evalLoaded()}>
            <div class="flex items-center justify-center h-full">
              <div class="spinner w-8 h-8" />
            </div>
          </Show>
          <Show when={evalLoaded()}>
            <textarea
              class="w-full h-full p-4 bg-transparent resize-none font-mono text-sm focus:outline-none"
              placeholder="Define evaluation criteria...

Example:
- [ ] All tests pass
- [ ] No console errors
- [ ] Feature works as described"
              value={evalContent()}
              onInput={(e) => handleEvalInput(e.currentTarget.value)}
            />
          </Show>
        </Show>

        {/* Settings Tab */}
        <Show when={activeTab() === 'settings'}>
          <div class="p-6 max-w-xl">
            <div class="form grid gap-6">
              {/* Worker Scale */}
              <div class="grid gap-2">
                <label for="worker-scale">Worker Scale</label>
                <input
                  id="worker-scale"
                  type="text"
                  class="input"
                  value={workerScale()}
                  onInput={(e) => setWorkerScale(e.currentTarget.value)}
                  placeholder="1"
                />
                <p class="text-muted-foreground text-sm">
                  Number of workers (e.g., "1", "2", "1-3")
                </p>
              </div>

              {/* Time Limit */}
              <div class="grid gap-2">
                <label for="time-limit">Time Limit (minutes)</label>
                <input
                  id="time-limit"
                  type="number"
                  class="input"
                  value={timeLimitMinutes() ?? ''}
                  onInput={(e) => {
                    const val = e.currentTarget.value;
                    setTimeLimitMinutes(val ? Number(val) : null);
                  }}
                  placeholder="30"
                />
                <p class="text-muted-foreground text-sm">
                  Maximum run duration. Leave empty for no limit.
                </p>
              </div>

              {/* Human in the Loop */}
              <div class="flex items-center justify-between">
                <div>
                  <label class="font-medium text-wool-200">Human in the Loop</label>
                  <p class="text-muted-foreground text-sm">
                    Pause for approval on significant changes
                  </p>
                </div>
                <button
                  type="button"
                  role="switch"
                  aria-checked={humanInTheLoop()}
                  onClick={() => setHumanInTheLoop(!humanInTheLoop())}
                  class={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
                    humanInTheLoop() ? 'bg-amber-500' : 'bg-pasture-600'
                  }`}
                >
                  <span
                    class={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
                      humanInTheLoop() ? 'translate-x-6' : 'translate-x-1'
                    }`}
                  />
                </button>
              </div>

              {/* Runner */}
              <Show when={runners().length > 0}>
                <div class="grid gap-2">
                  <label for="runner">Runner</label>
                  <select
                    id="runner"
                    class="select"
                    value={runner() ?? ''}
                    onChange={(e) =>
                      setRunner(e.currentTarget.value || null)
                    }
                  >
                    <option value="">Default (local)</option>
                    <For each={runners()}>
                      {(r) => <option value={r.name}>{r.name}</option>}
                    </For>
                  </select>
                  <p class="text-muted-foreground text-sm">
                    Where to run the workers
                  </p>
                </div>
              </Show>

              {/* Assets */}
              <div class="border-t border-pasture-600 pt-6">
                <div class="flex items-center justify-between">
                  <div>
                    <label class="font-medium text-wool-200">Assets</label>
                    <p class="text-muted-foreground text-sm">
                      Images and files for this run
                    </p>
                  </div>
                  <button
                    type="button"
                    class="btn-outline"
                    onClick={openAssets}
                  >
                    <i data-lucide="folder-open" class="w-4 h-4" />
                    Open Folder
                  </button>
                </div>
              </div>
            </div>
          </div>
        </Show>
      </div>
    </div>
  );
};
