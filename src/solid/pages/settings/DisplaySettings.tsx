/** @jsxImportSource solid-js */
import { createSignal } from 'solid-js'

import { readUiRenderer, uiRendererHref, type UiRenderer } from '../../../app/uiRenderer'
import styles from '../../../pages/settings/Settings.module.css'
import { formatUiScale, UI_SCALE_DEFAULT } from '../../../state/uiScaleStore'
import { uiScale } from '../../state/host'

export function DisplaySettings() {
  const [renderer] = createSignal<UiRenderer>(readUiRenderer())

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
        <div class={styles.row}>
          <div class={styles.copy}>
            <div class={styles.label}>UI renderer</div>
            <div class={styles.hint}>
              React or Solid. Remembered on this device. Switching reloads this page.
            </div>
          </div>
          <div class={styles.segments} role="group" aria-label="UI renderer">
            {(['react', 'solid'] as const).map((value) => (
              <a
                href={uiRendererHref(value)}
                rel="external"
                class={`${styles.segment} ${renderer() === value ? styles.segmentActive : ''}`}
                aria-current={renderer() === value ? 'true' : undefined}
              >
                {value === 'react' ? 'React' : 'Solid'}
              </a>
            ))}
          </div>
        </div>
      </div>
    </section>
  )
}
