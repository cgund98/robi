import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js'

import { getSettings, putSetting, SETTING_KEYS } from '../../api/settings'
import styles from './Settings.module.css'

const NETWORKS = [
  { value: 'on', label: 'Sandboxed' },
  { value: 'off', label: 'Open' }
] as const

const MECHANISMS = [
  { value: 'os', label: 'OS' },
  { value: 'container', label: 'Container' }
] as const

export function SandboxSettings() {
  const [mechanism, setMechanism] = createSignal('os')
  const [projectFiles, setProjectFiles] = createSignal(false)
  const [network, setNetwork] = createSignal('on')
  const [image, setImage] = createSignal('')
  const [runtime, setRuntime] = createSignal('')
  const [memoryMb, setMemoryMb] = createSignal('')
  const [cpus, setCpus] = createSignal('')
  const [pidsLimit, setPidsLimit] = createSignal('')
  const [error, setError] = createSignal<string | null>(null)
  const saved = {
    image: '',
    runtime: '',
    memoryMb: '',
    cpus: '',
    pidsLimit: ''
  }

  createEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const [
          mechanismSetting,
          projectFilesSetting,
          networkSetting,
          imageSetting,
          runtimeSetting,
          memorySetting,
          cpusSetting,
          pidsSetting
        ] = await getSettings([
          SETTING_KEYS.sandboxMechanism,
          SETTING_KEYS.sandboxProjectFiles,
          SETTING_KEYS.sandboxNetwork,
          SETTING_KEYS.sandboxImage,
          SETTING_KEYS.sandboxRuntime,
          SETTING_KEYS.sandboxMemoryMb,
          SETTING_KEYS.sandboxCpus,
          SETTING_KEYS.sandboxPidsLimit
        ])
        if (cancelled) {
          return
        }
        const nextImage = imageSetting.value ?? ''
        const nextRuntime = runtimeSetting.value ?? ''
        const nextMemory = memorySetting.value ?? ''
        const nextCpus = cpusSetting.value ?? ''
        const nextPids = pidsSetting.value ?? ''
        saved.image = nextImage
        saved.runtime = nextRuntime
        saved.memoryMb = nextMemory
        saved.cpus = nextCpus
        saved.pidsLimit = nextPids
        setMechanism(mechanismSetting.value === 'container' ? 'container' : 'os')
        setProjectFiles(projectFilesSetting.value === 'on')
        setNetwork(networkSetting.value === 'off' ? 'off' : 'on')
        setImage(nextImage)
        setRuntime(nextRuntime)
        setMemoryMb(nextMemory)
        setCpus(nextCpus)
        setPidsLimit(nextPids)
      } catch (err: unknown) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load settings')
        }
      }
    })()
    onCleanup(() => {
      cancelled = true
    })
  })

  async function saveText(
    key: string,
    value: string,
    savedKey: keyof typeof saved,
    restore: (value: string) => void
  ) {
    const trimmed = value.trim()
    if (trimmed === saved[savedKey]) {
      restore(trimmed)
      return
    }
    // An empty value is refused by the server for every one of these keys, so
    // it reverts to what is stored instead of clearing the key.
    if (trimmed.length === 0) {
      restore(saved[savedKey])
      return
    }
    setError(null)
    try {
      await putSetting(key, trimmed, false)
      saved[savedKey] = trimmed
      restore(trimmed)
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Failed to save setting')
      restore(saved[savedKey])
    }
  }

  async function saveChoice(
    key: string,
    value: string,
    current: string,
    apply: (next: string) => void
  ) {
    if (value === current) {
      return
    }
    apply(value)
    setError(null)
    try {
      await putSetting(key, value, false)
    } catch (err: unknown) {
      apply(current)
      setError(err instanceof Error ? err.message : 'Failed to save setting')
    }
  }

  async function toggleProjectFiles() {
    const next = !projectFiles()
    setProjectFiles(next)
    setError(null)
    try {
      await putSetting(SETTING_KEYS.sandboxProjectFiles, next ? 'on' : 'off', false)
    } catch (err: unknown) {
      setProjectFiles(!next)
      setError(err instanceof Error ? err.message : 'Failed to save setting')
    }
  }

  return (
    <>
      <h1 class={styles.title}>Sandbox</h1>
      <p class={styles.lead}>
        Every command runs in a sandbox. The mechanism picks which one; the rest configures the
        container when that mechanism is selected.
      </p>
      <section class={styles.section}>
        <h2 class={styles.sectionTitle}>Mechanism</h2>
        <p class={styles.sectionHint}>
          `OS` uses Seatbelt on macOS and bubblewrap on Linux. `Container` runs the command in a
          gVisor container over the Docker API and needs a running daemon. The next command uses
          this.
        </p>
        <div class={styles.card}>
          <SegmentRow
            label="Mechanism"
            hint="Neither replaces the other. `OS` stays the default."
            ariaLabel="Sandbox mechanism"
            options={MECHANISMS}
            value={mechanism()}
            onSelect={(next) =>
              void saveChoice(SETTING_KEYS.sandboxMechanism, next, mechanism(), setMechanism)
            }
          />
        </div>
      </section>
      <section class={styles.section}>
        <h2 class={styles.sectionTitle}>Container</h2>
        <p class={styles.sectionHint}>
          Used only when the mechanism is `Container`. A project may narrow these in
          `.robi/sandbox.toml` once project files are allowed; it cannot choose the mechanism.
        </p>
        <div class={styles.card}>
          <TextRow
            label="Image"
            hint="The image the container starts from, as `repo/name[:tag]`."
            ariaLabel="Sandbox image"
            placeholder="robi/agent:0.1.0"
            value={image()}
            onInput={setImage}
            onBlur={() => void saveText(SETTING_KEYS.sandboxImage, image(), 'image', setImage)}
          />
          <TextRow
            label="Runtime"
            hint="A runtime name the daemon knows, such as `runsc` or `runc`."
            ariaLabel="Sandbox runtime"
            placeholder="runsc"
            value={runtime()}
            onInput={setRuntime}
            onBlur={() =>
              void saveText(SETTING_KEYS.sandboxRuntime, runtime(), 'runtime', setRuntime)
            }
          />
          <NumberRow
            label="Memory"
            hint="Size of the container's memory limit, in MiB. From 256 to 65536."
            ariaLabel="Sandbox memory in MiB"
            unit="MiB"
            value={memoryMb()}
            onInput={setMemoryMb}
            onBlur={() =>
              void saveText(SETTING_KEYS.sandboxMemoryMb, memoryMb(), 'memoryMb', setMemoryMb)
            }
          />
          <NumberRow
            label="CPUs"
            hint="Whole CPUs the container may use. From 1 to the machine's CPU count."
            ariaLabel="Sandbox CPUs"
            unit="cores"
            value={cpus()}
            onInput={setCpus}
            onBlur={() => void saveText(SETTING_KEYS.sandboxCpus, cpus(), 'cpus', setCpus)}
          />
          <NumberRow
            label="Process limit"
            hint="Most processes the container may run at once. From 64 to 8192."
            ariaLabel="Sandbox process limit"
            unit="pids"
            value={pidsLimit()}
            onInput={setPidsLimit}
            onBlur={() =>
              void saveText(SETTING_KEYS.sandboxPidsLimit, pidsLimit(), 'pidsLimit', setPidsLimit)
            }
          />
        </div>
      </section>
      <section class={styles.section}>
        <h2 class={styles.sectionTitle}>Network</h2>
        <p class={styles.sectionHint}>
          `Sandboxed` gives the container no interface. A single command can still be approved for
          egress. `Open` gives every sandboxed command full outbound networking.
        </p>
        <div class={styles.card}>
          <SegmentRow
            label="Container network"
            hint="Machine-wide. A workspace file cannot change it, and `Open` means no approval card appears for egress, because there is nothing left to grant."
            ariaLabel="Sandbox network"
            options={NETWORKS}
            value={network()}
            onSelect={(next) =>
              void saveChoice(SETTING_KEYS.sandboxNetwork, next, network(), setNetwork)
            }
          />
        </div>
      </section>
      <section class={styles.section}>
        <h2 class={styles.sectionTitle}>Project files</h2>
        <p class={styles.sectionHint}>
          A workspace may ship `.robi/sandbox.toml` with an image, limits, services, and
          environment. Off by default: a cloned repository should not name the image its code runs
          in.
        </p>
        <div class={styles.card}>
          <div class={styles.row}>
            <div class={styles.copy}>
              <div class={styles.label}>Allow workspace sandbox file</div>
              <div class={styles.hint}>
                Applies to every workspace. The file can narrow this tab's values, and it can name
                services and environment that have no setting here.
              </div>
            </div>
            <button
              type="button"
              class={styles.switch}
              role="switch"
              aria-checked={projectFiles()}
              aria-label="Allow workspace sandbox file"
              onClick={() => void toggleProjectFiles()}
            >
              <span class={styles.knob} />
            </button>
          </div>
        </div>
      </section>
      <Show when={error()}>
        <p class={`${styles.status} ${styles.statusError}`}>{error()}</p>
      </Show>
    </>
  )
}

function SegmentRow(props: {
  label: string
  hint: string
  ariaLabel: string
  options: readonly { value: string; label: string }[]
  value: string
  onSelect: (value: string) => void
}) {
  return (
    <div class={styles.row}>
      <div class={styles.copy}>
        <div class={styles.label}>{props.label}</div>
        <div class={styles.hint}>{props.hint}</div>
      </div>
      <div class={styles.segments} role="group" aria-label={props.ariaLabel}>
        <For each={props.options}>
          {(option) => (
            <button
              type="button"
              class={`${styles.segment} ${props.value === option.value ? styles.segmentActive : ''}`}
              aria-pressed={props.value === option.value}
              onClick={() => props.onSelect(option.value)}
            >
              {option.label}
            </button>
          )}
        </For>
      </div>
    </div>
  )
}

function TextRow(props: {
  label: string
  hint: string
  ariaLabel: string
  placeholder: string
  value: string
  onInput: (value: string) => void
  onBlur: () => void
}) {
  return (
    <div class={styles.row}>
      <div class={styles.copy}>
        <div class={styles.label}>{props.label}</div>
        <div class={styles.hint}>{props.hint}</div>
      </div>
      <input
        class={`${styles.input} ${styles.control}`}
        spellcheck={false}
        aria-label={props.ariaLabel}
        placeholder={props.placeholder}
        value={props.value}
        onInput={(event) => props.onInput(event.currentTarget.value)}
        onBlur={() => props.onBlur()}
      />
    </div>
  )
}

function NumberRow(props: {
  label: string
  hint: string
  ariaLabel: string
  unit: string
  value: string
  onInput: (value: string) => void
  onBlur: () => void
}) {
  return (
    <div class={styles.row}>
      <div class={styles.copy}>
        <div class={styles.label}>{props.label}</div>
        <div class={styles.hint}>{props.hint}</div>
      </div>
      <div class={`${styles.stepper} ${styles.control}`}>
        <input
          class={`${styles.input} ${styles.number}`}
          inputMode="numeric"
          aria-label={props.ariaLabel}
          value={props.value}
          onInput={(event) => props.onInput(event.currentTarget.value)}
          onBlur={() => props.onBlur()}
        />
        <span class={styles.stepValue}>{props.unit}</span>
      </div>
    </div>
  )
}
