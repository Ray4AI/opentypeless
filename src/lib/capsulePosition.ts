import { getCurrentWindow, LogicalPosition } from '@tauri-apps/api/window'

/// localStorage key for the capsule window position, in logical coordinates.
const CAPSULE_POSITION_KEY = 'capsule_window_position'

export interface CapsuleWindowPosition {
  x: number
  y: number
}

export function loadCapsulePosition(): CapsuleWindowPosition | null {
  try {
    const raw = localStorage.getItem(CAPSULE_POSITION_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw) as Partial<CapsuleWindowPosition>
    if (
      typeof parsed.x !== 'number' ||
      typeof parsed.y !== 'number' ||
      !Number.isFinite(parsed.x) ||
      !Number.isFinite(parsed.y) ||
      parsed.x < -10_000 ||
      parsed.y < -10_000 ||
      parsed.x > 100_000 ||
      parsed.y > 100_000
    ) {
      return null
    }
    return { x: Math.round(parsed.x), y: Math.round(parsed.y) }
  } catch {
    return null
  }
}

export function saveCapsulePosition(position: CapsuleWindowPosition): void {
  try {
    localStorage.setItem(CAPSULE_POSITION_KEY, JSON.stringify(position))
  } catch {
    // localStorage unavailable — position simply won't persist.
  }
}

/// Read the capsule window's current position and persist it.
/// Called after a drag ends. Failures are silent — persistence is best-effort.
export async function rememberCapsulePosition(): Promise<void> {
  try {
    const win = getCurrentWindow()
    const pos = await win.outerPosition()
    const scale = (await win.scaleFactor()) || 1
    const logical = {
      x: Math.round(pos.x / scale),
      y: Math.round(pos.y / scale),
    }
    saveCapsulePosition(logical)
  } catch {
    // ignore — best-effort persistence
  }
}

/// Apply the persisted capsule position, if any. Called once when the capsule
/// window first mounts, before it is shown.
export async function restoreCapsulePosition(): Promise<boolean> {
  const position = loadCapsulePosition()
  if (!position) return false
  try {
    const win = getCurrentWindow()
    await win.setPosition(new LogicalPosition(position.x, position.y))
    return true
  } catch {
    return false
  }
}
