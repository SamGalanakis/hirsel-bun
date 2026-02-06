/**
 * Profiling-aware invoke wrapper
 *
 * Drop-in replacement for `@tauri-apps/api/core`'s invoke().
 * When profiling is active, measures IPC round-trip time.
 * When profiling is off, passes through directly (zero overhead).
 */

import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { isProfilingEnabled, recordIpc } from './profiling';

export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isProfilingEnabled()) {
    return tauriInvoke<T>(command, args);
  }

  const start = performance.now();
  let error = false;

  try {
    return await tauriInvoke<T>(command, args);
  } catch (e) {
    error = true;
    throw e;
  } finally {
    const duration = performance.now() - start;
    recordIpc(command, start, duration, args, error);
  }
}
