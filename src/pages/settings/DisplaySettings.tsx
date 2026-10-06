import { formatUiScale, UI_SCALE_DEFAULT, useUiScaleStore } from '../../state/uiScaleStore'
import styles from './Settings.module.css'

/** The UI scale stepper. Lives at the top of General so it is easy to find. */
export function DisplaySettings() {
  const scale = useUiScaleStore((state) => state.scale)
  const zoomIn = useUiScaleStore((state) => state.zoomIn)
  const zoomOut = useUiScaleStore((state) => state.zoomOut)
  const reset = useUiScaleStore((state) => state.reset)

  return (
    <section className={styles.section}>
      <h2 className={styles.sectionTitle}>Display</h2>
      <div className={styles.card}>
        <div className={styles.row}>
          <div className={styles.copy}>
            <div className={styles.label}>UI scale</div>
            <div className={styles.hint}>
              Makes everything larger or smaller. Cmd/Ctrl + and − step it, Cmd/Ctrl 0 resets.
              Remembered on this device.
            </div>
          </div>
          <div className={styles.stepper} role="group" aria-label="UI scale">
            <button
              type="button"
              className={styles.stepButton}
              aria-label="Decrease UI scale"
              onClick={zoomOut}
            >
              −
            </button>
            <span className={styles.stepValue}>{formatUiScale(scale)}</span>
            <button
              type="button"
              className={styles.stepButton}
              aria-label="Increase UI scale"
              onClick={zoomIn}
            >
              +
            </button>
            <button
              type="button"
              className={styles.stepReset}
              disabled={scale === UI_SCALE_DEFAULT}
              onClick={reset}
            >
              Reset
            </button>
          </div>
        </div>
      </div>
    </section>
  )
}
