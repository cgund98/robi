import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'

/** Back and forward side buttons. Primary is 0, so these stay out of clicks. */
export function historyStep(button: number): -1 | 1 | null {
  if (button === 3) {
    return -1
  }
  if (button === 4) {
    return 1
  }
  return null
}

/**
 * The webview does not map the side buttons onto its history, so the shell does.
 * Each document the docs viewer opens is its own history entry, and so is every
 * route, which means one gesture walks both.
 */
export function useMouseHistory() {
  const navigate = useNavigate()

  useEffect(() => {
    const onMouseDown = (event: MouseEvent) => {
      const step = historyStep(event.button)
      if (step === null) {
        return
      }
      event.preventDefault()
      navigate(step)
    }
    window.addEventListener('mousedown', onMouseDown)
    return () => window.removeEventListener('mousedown', onMouseDown)
  }, [navigate])
}
