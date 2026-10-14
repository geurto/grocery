// Running timers, kept across screens and reloads (per device). When one
// goes off: vibrate, beep, and a notification if the app is in the
// background and notifications are allowed.

import { load, save } from './storage';
import { extend, ringing, type Timer } from './timers';

const TICK_MS = 250;
const BEEP_EVERY_MS = 2500;
/** Stop beeping after this long; the timer keeps showing. */
const BEEP_FOR_MS = 2 * 60_000;

class Kitchen {
  timers = $state<Timer[]>(load('timers', []));
  now = $state(Date.now());

  #interval: ReturnType<typeof setInterval> | undefined;
  #alerted = new Set<string>();
  #lastBeep = 0;
  #audio: AudioContext | undefined;

  constructor() {
    this.#schedule();
    // Phones pause intervals in the background; catch up on return.
    document.addEventListener('visibilitychange', () => this.#tick());
  }

  get ringing(): Timer[] {
    return this.timers.filter((t) => ringing(t, this.now));
  }

  start(timer: Omit<Timer, 'id' | 'endsAt'>, seconds: number) {
    this.#unlockAudio();
    void askForNotifications();
    const now = Date.now();
    this.now = now;
    this.timers.push({ ...timer, id: `${now}-${Math.random().toString(36).slice(2, 7)}`, endsAt: now + seconds * 1000 });
    this.#changed();
  }

  add(id: string, seconds: number) {
    this.#unlockAudio();
    this.timers = this.timers.map((t) => (t.id === id ? extend(t, seconds * 1000, Date.now()) : t));
    this.#alerted.delete(id);
    this.#changed();
  }

  stop(id: string) {
    this.timers = this.timers.filter((t) => t.id !== id);
    this.#alerted.delete(id);
    this.#changed();
  }

  #changed() {
    save('timers', this.timers);
    this.#tick();
    this.#schedule();
  }

  #schedule() {
    if (this.timers.length && !this.#interval) {
      this.#interval = setInterval(() => this.#tick(), TICK_MS);
    } else if (!this.timers.length && this.#interval) {
      clearInterval(this.#interval);
      this.#interval = undefined;
    }
  }

  #tick() {
    const now = Date.now();
    this.now = now;
    let beep = false;
    for (const t of this.timers) {
      if (!ringing(t, now)) continue;
      if (!this.#alerted.has(t.id)) {
        this.#alerted.add(t.id);
        notify(t);
        beep = true;
      } else if (now - t.endsAt < BEEP_FOR_MS && now - this.#lastBeep >= BEEP_EVERY_MS) {
        beep = true;
      }
    }
    if (beep) {
      this.#lastBeep = now;
      navigator.vibrate?.([300, 120, 300, 120, 500]);
      this.#beep();
    }
  }

  /** Browsers only allow sound after a tap; the tap that starts a timer counts. */
  #unlockAudio() {
    try {
      this.#audio ??= new AudioContext();
      void this.#audio.resume();
    } catch {
      // No Web Audio: vibration and the banner still work.
    }
  }

  #beep() {
    const ctx = this.#audio;
    if (!ctx || ctx.state !== 'running') return;
    for (let i = 0; i < 3; i++) {
      const at = ctx.currentTime + i * 0.25;
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.frequency.value = 880;
      gain.gain.setValueAtTime(0.0001, at);
      gain.gain.exponentialRampToValueAtTime(0.4, at + 0.01);
      gain.gain.exponentialRampToValueAtTime(0.0001, at + 0.18);
      osc.connect(gain).connect(ctx.destination);
      osc.start(at);
      osc.stop(at + 0.2);
    }
  }
}

async function askForNotifications() {
  if (!('Notification' in window) || Notification.permission !== 'default') return;
  try {
    await Notification.requestPermission();
  } catch {
    // Older Safari: callback style only; not worth supporting.
  }
}

function notify(t: Timer) {
  if (document.visibilityState === 'visible') return; // the banner shows it
  if (!('Notification' in window) || Notification.permission !== 'granted') return;
  const options = { body: t.label, tag: t.id, requireInteraction: true };
  // Android only shows notifications made through the service worker.
  navigator.serviceWorker?.getRegistration().then(
    (reg) => {
      if (reg) void reg.showNotification('Timer done', options);
      else new Notification('Timer done', options);
    },
    () => {},
  );
}

export const kitchen = new Kitchen();
