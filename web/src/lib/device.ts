// Small wrappers around phone features that may be missing.

/** Share sheet if there is one, else the clipboard. Returns what happened. */
export async function shareText(text: string, title: string): Promise<'shared' | 'copied' | 'failed'> {
  if (navigator.share) {
    try {
      await navigator.share({ title, text });
      return 'shared';
    } catch (e) {
      if (e instanceof DOMException && e.name === 'AbortError') return 'shared'; // closed the sheet
    }
  }
  try {
    await navigator.clipboard.writeText(text);
    return 'copied';
  } catch {
    return 'failed';
  }
}

export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

/** A short buzz on Android; nothing elsewhere. */
export function tick() {
  navigator.vibrate?.(8);
}

/**
 * Keep the screen on while shopping. The browser drops the lock when the
 * app goes to the background, so take it again when it comes back.
 */
export class WakeLock {
  #sentinel: WakeLockSentinel | null = null;
  #wanted = false;

  static get supported() {
    return 'wakeLock' in navigator;
  }

  constructor() {
    document.addEventListener('visibilitychange', () => {
      if (this.#wanted && document.visibilityState === 'visible') void this.#take();
    });
  }

  async #take() {
    try {
      this.#sentinel = await navigator.wakeLock.request('screen');
    } catch {
      this.#sentinel = null;
    }
  }

  async set(on: boolean) {
    this.#wanted = on;
    if (on) await this.#take();
    else {
      await this.#sentinel?.release();
      this.#sentinel = null;
    }
  }
}

export const wakeLock = new WakeLock();
