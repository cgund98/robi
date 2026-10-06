/** @jsxImportSource solid-js */
import styles from './Settings.module.css'
import { formatUiScale, UI_SCALE_DEFAULT, uiScale } from '../../state/uiScaleStore'

export function DisplaySettings() {
  return (
    <section class={styles.section}>
      <h2 class={styles.sectionTitle}>Display</h2>
      <div class={styles.card}>
        <div class={styles.row}>
          <div class={styles.copy}>
            <div class={styles.label}>UI scale</div>
            <div class={styles.hint}>
              Makes everything larger or smaller. Cmd/Ctrl + and − step it, Cmd/Ctrl 0 resets.
              Remembered on this device.
            </div>
          </div>
          <div class={styles.stepper} role="group" aria-label="UI scale">
            <button
              type="button"
              class={styles.stepButton}
              aria-label="Decrease UI scale"
              onClick={() => uiScale.zoomOut()}
            >
              −
            </button>
            <span class={styles.stepValue}>{formatUiScale(uiScale.scale)}</span>
            <button
              type="button"
              class={styles.stepButton}
              aria-label="Increase UI scale"
              onClick={() => uiScale.zoomIn()}
            >
              +
            </button>
            <button
              type="button"
              class={styles.stepReset}
              disabled={uiScale.scale === UI_SCALE_DEFAULT}
              onClick={() => uiScale.reset()}
            >
              Reset
            </button>
          </div>
        </div>
      </div>
    </section>
  )
}
