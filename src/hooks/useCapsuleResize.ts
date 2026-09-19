import { useEffect, useRef } from 'react'
import { useAppStore, type PipelineState } from '../stores/appStore'
import { loadCapsulePosition, saveCapsulePosition } from '../lib/capsulePosition'

/// Cached capsule position (logical coordinates), kept outside React so
/// repeated resizes can reuse the last known position without re-reading
/// localStorage on every state change.
let cachedCapsulePosition: { x: number; y: number } | null = null

/// Whether the capsule's persisted position has been applied in this session.
let capsulePositionRestored = false

interface CapsuleSize {
  width: number
  height: number
}

export interface CapsuleVisibilityInput {
  capsuleAutoHide: boolean
  contextMenuOpen: boolean
  translationTargetMenuOpen?: boolean
  capsuleExpanded: boolean
  hasError: boolean
  pipelineState: PipelineState
}

export function getCapsuleVisibility({
  capsuleAutoHide,
  contextMenuOpen,
  translationTargetMenuOpen = false,
  capsuleExpanded,
  hasError,
  pipelineState,
}: CapsuleVisibilityInput): boolean {
  return (
    !capsuleAutoHide ||
    contextMenuOpen ||
    translationTargetMenuOpen ||
    capsuleExpanded ||
    hasError ||
    pipelineState !== 'idle'
  )
}

export function getCapsuleFocusable(): boolean {
  return false
}

function getSizeForState(
  state: PipelineState,
  expanded: boolean,
  hasError: boolean,
  contextMenuOpen: boolean,
  translationTargetMenuOpen = false,
): CapsuleSize {
  if (translationTargetMenuOpen) return { width: 360, height: 180 }
  if (contextMenuOpen) return { width: 220, height: 220 }
  if (hasError) return { width: 200, height: 36 }
  if (expanded) return { width: 220, height: 90 }
  switch (state) {
    case 'idle':
      return { width: 36, height: 36 }
    case 'preparing':
      return { width: 180, height: 36 }
    case 'recording':
    case 'transcribing':
    case 'polishing':
      return { width: 200, height: 36 }
    case 'outputting':
      return { width: 144, height: 36 }
    case 'ask_recording':
    case 'ask_thinking':
      return { width: 168, height: 36 }
    default:
      return { width: 36, height: 36 }
  }
}

export function useCapsuleResize() {
  const pipelineState = useAppStore((s) => s.pipelineState)
  const capsuleExpanded = useAppStore((s) => s.capsuleExpanded)
  const pipelineError = useAppStore((s) => s.pipelineError)
  const contextMenuOpen = useAppStore((s) => s.contextMenuOpen)
  const translationTargetMenuOpen = useAppStore((s) => s.translationTargetMenuOpen)
  const setContextMenuReady = useAppStore((s) => s.setContextMenuReady)
  const capsuleAutoHide = useAppStore((s) => s.config.capsule_auto_hide)
  const initialized = useRef(false)
  const prevWindowSize = useRef<{ width: number; height: number } | null>(null)

  // Restore the persisted capsule position once per session, before the first
  // resize positions the window. Drag-end persistence updates the same store.
  useEffect(() => {
    if (capsulePositionRestored) return
    capsulePositionRestored = true
    cachedCapsulePosition = loadCapsulePosition()
  }, [])

  // Track window moves so resizes can preserve the user's chosen position.
  // Persisting is debounced — onMoved fires continuously during a drag.
  useEffect(() => {
    let cancelled = false
    let unlisten: (() => void) | undefined
    let saveTimer: ReturnType<typeof setTimeout> | null = null
    import('@tauri-apps/api/window')
      .then(({ getCurrentWindow }) => {
        if (cancelled) return
        const win = getCurrentWindow()
        const handler = () => {
          if (cancelled) return
          void win
            .outerPosition()
            .then((pos) => win.scaleFactor().then((scale) => ({ pos, scale })))
            .then(({ pos, scale }) => {
              cachedCapsulePosition = {
                x: Math.round(pos.x / (scale || 1)),
                y: Math.round(pos.y / (scale || 1)),
              }
              if (saveTimer) clearTimeout(saveTimer)
              saveTimer = setTimeout(() => {
                saveTimer = null
                if (cachedCapsulePosition) {
                  saveCapsulePosition(cachedCapsulePosition)
                }
              }, 300)
            })
            .catch(() => {})
        }
        return win.onMoved(handler)
      })
      .then((result) => {
        if (typeof result === 'function') unlisten = result
      })
      .catch(() => {})
    return () => {
      cancelled = true
      if (saveTimer) clearTimeout(saveTimer)
      try {
        unlisten?.()
      } catch {
        // stale handle after dev HMR
      }
    }
  }, [])

  const hasError = pipelineError !== null

  useEffect(() => {
    const size = getSizeForState(
      pipelineState,
      capsuleExpanded,
      hasError,
      contextMenuOpen,
      translationTargetMenuOpen,
    )
    const windowWidth = size.width + 24
    const windowHeight = size.height + 24
    const shouldShow = getCapsuleVisibility({
      capsuleAutoHide,
      contextMenuOpen,
      translationTargetMenuOpen,
      capsuleExpanded,
      hasError,
      pipelineState,
    })

    import('@tauri-apps/api/window')
      .then(async ({ getCurrentWindow, LogicalSize, LogicalPosition, currentMonitor }) => {
        const win = getCurrentWindow()
        await win.setFocusable(getCapsuleFocusable()).catch(() => {})

        // Re-assert always-on-top every time the capsule (re)appears. Some
        // window managers / fullscreen apps can bury a previously pinned
        // window, and a hidden-then-shown window may lose its topmost flag.
        if (shouldShow) {
          await win.setAlwaysOnTop(true).catch(() => {})
        }

        if (!initialized.current) {
          // First mount: restore the persisted position when available, else
          // position at bottom-center of screen, then show.
          await win.setSize(new LogicalSize(windowWidth, windowHeight)).catch(() => {})
          try {
            const monitor = await currentMonitor()
            if (monitor) {
              if (capsulePositionRestored && cachedCapsulePosition) {
                await win
                  .setPosition(
                    new LogicalPosition(cachedCapsulePosition.x, cachedCapsulePosition.y),
                  )
                  .catch(() => {})
              } else {
                const sw = monitor.size.width / monitor.scaleFactor
                const sh = monitor.size.height / monitor.scaleFactor
                const x = Math.round(sw / 2 - windowWidth / 2)
                const y = Math.round(sh - windowHeight - 80)
                await win.setPosition(new LogicalPosition(x, y)).catch(() => {})
              }
            }
          } catch {
            /* ignore – monitor info unavailable */
          }
          if (shouldShow) {
            await win.show().catch(() => {})
          } else {
            await win.hide().catch(() => {})
          }
          initialized.current = true
          prevWindowSize.current = { width: windowWidth, height: windowHeight }
          return
        }

        // Subsequent resizes: left edge + vertical center stay fixed.
        // Since content is always padded 12px each side, the capsule at x=12
        // is identical to a centered capsule — so the mic icon never moves.
        const prev = prevWindowSize.current
        if (prev) {
          const pos = await win.outerPosition().catch(() => null)
          if (pos) {
            const monitor = await currentMonitor()
            const scale = monitor?.scaleFactor ?? 1
            const oldLeftX = pos.x / scale
            const oldCenterY = pos.y / scale + prev.height / 2
            const newX = Math.round(oldLeftX)
            const newY = Math.round(oldCenterY - windowHeight / 2)
            await win.setPosition(new LogicalPosition(newX, newY)).catch(() => {})
            await win.setSize(new LogicalSize(windowWidth, windowHeight)).catch(() => {})
          } else {
            await win.setSize(new LogicalSize(windowWidth, windowHeight)).catch(() => {})
          }
        } else {
          await win.setSize(new LogicalSize(windowWidth, windowHeight)).catch(() => {})
        }

        prevWindowSize.current = { width: windowWidth, height: windowHeight }

        // Signal that the window has finished resizing for context menu
        if (contextMenuOpen) {
          setContextMenuReady(true)
        }

        if (shouldShow) {
          await win.show().catch(() => {})
        } else {
          await win.hide().catch(() => {})
        }
      })
      .catch(() => {})
  }, [
    pipelineState,
    capsuleExpanded,
    hasError,
    contextMenuOpen,
    translationTargetMenuOpen,
    capsuleAutoHide,
    setContextMenuReady,
  ])

  return getSizeForState(
    pipelineState,
    capsuleExpanded,
    hasError,
    contextMenuOpen,
    translationTargetMenuOpen,
  )
}
