import type { MermaidConfig } from 'mermaid'

/**
 * Mermaid theme values, paired with the tokens in
 * `docs/src/design/shell/visual-style.md`. Mermaid needs literal colors, not
 * `var(...)`, so each value is the token's hex with its name beside it. Keep
 * this map the only place mermaid colors live.
 *
 * The `actor*`, `note*`, `activation*`, `signal*`, and `labelBox*` groups are
 * the sequence diagram's variables. Without them sequence diagrams fall back to
 * mermaid's light palette and sit bright on the dark shell, so they are set to
 * the same tokens as the flowchart ones rather than derived.
 */
const THEME_VARIABLES = {
  background: '#0b0b0b', // --bg-canvas
  primaryColor: '#1f1f1e', // --bg-surface
  primaryTextColor: '#f7f6f2', // --ink-bright
  primaryBorderColor: '#2a2a28', // --rule
  lineColor: '#2a2a28', // --rule
  secondaryColor: '#1a1a19', // --code-bg
  tertiaryColor: '#1f1f1e', // --bg-surface
  nodeBorder: '#2a2a28', // --rule
  nodeTextColor: '#f7f6f2', // --ink-bright
  mainBkg: '#1f1f1e', // --bg-surface
  clusterBkg: '#1a1a19', // --code-bg
  clusterBorder: '#2a2a28', // --rule
  // State composites draw an outer rect and an inner one. The inner fill
  // otherwise falls through to `background` (the page), so a state inside a
  // state is a hole. Both layers use the container token.
  compositeBackground: '#1a1a19', // --code-bg
  compositeTitleBackground: '#1a1a19', // --code-bg
  altBackground: '#1a1a19', // --code-bg
  edgeLabelBackground: '#0b0b0b', // --bg-canvas
  textColor: '#c3c2b8', // --ink
  titleColor: '#f7f6f2', // --ink-bright
  // Sequence diagram: actor boxes and their lifelines.
  actorBkg: '#1f1f1e', // --bg-surface
  actorBorder: '#2a2a28', // --rule
  actorTextColor: '#f7f6f2', // --ink-bright
  actorLineColor: '#2a2a28', // --rule
  // Sequence diagram: messages.
  signalColor: '#c3c2b8', // --ink
  signalTextColor: '#c3c2b8', // --ink
  sequenceNumberColor: '#8a8983', // --ink-muted
  // Sequence diagram: notes.
  noteBkgColor: '#1a1a19', // --code-bg
  noteBorderColor: '#2a2a28', // --rule
  noteTextColor: '#f7f6f2', // --ink-bright
  // Sequence diagram: loop / alt / opt frames and the activations inside them.
  labelBoxBkgColor: '#1f1f1e', // --bg-surface
  labelBoxBorderColor: '#2a2a28', // --rule
  labelTextColor: '#f7f6f2', // --ink-bright
  loopTextColor: '#c3c2b8', // --ink
  activationBkgColor: '#1a1a19', // --code-bg
  activationBorderColor: '#2a2a28' // --rule
} satisfies MermaidConfig['themeVariables']

const CONFIG = {
  startOnLoad: false,
  // The source is model output. Keep mermaid's sanitization on.
  securityLevel: 'strict',
  theme: 'base',
  // Mermaid's default look is `neo`, which paints a light gray drop shadow on
  // every node. On the dark shell that shadow reads as a pale fill, and it
  // stacks when a box sits inside a box. Classic is a flat fill.
  look: 'classic',
  // The shell's UI sans, so diagram text matches the rest of the transcript.
  // This is a top-level key, not a theme variable: sequence diagrams read
  // their own `actorFontFamily` / `noteFontFamily` / `messageFontFamily`,
  // which mermaid fills in from this one.
  fontFamily: 'ui-sans-serif, system-ui, sans-serif',
  // Diagram text sits a little smaller than body copy, matching the fenced
  // code blocks it replaces. `fontSize` is a number of px, not a CSS length.
  fontSize: 14,
  themeVariables: THEME_VARIABLES
} satisfies MermaidConfig

type MermaidApi = (typeof import('mermaid'))['default']

let mermaidPromise: Promise<MermaidApi> | null = null

function loadMermaid(): Promise<MermaidApi> {
  mermaidPromise ??= import('mermaid').then(async (mod) => {
    const api = mod.default
    await api.initialize(CONFIG)
    return api
  })
  return mermaidPromise
}

// Mermaid's render creates a temp element keyed by id, so two in-flight renders
// can collide. Run one at a time on a chain that never rejects.
let queue: Promise<unknown> = Promise.resolve()
let counter = 0

/**
 * Render one fenced `mermaid` block to SVG. The promise rejects when the source
 * is not a diagram mermaid can parse; the caller keeps the source on screen.
 */
export function renderMermaid(source: string): Promise<string> {
  const run = async () => {
    const mermaid = await loadMermaid()
    const id = `mermaid-${++counter}`
    const { svg } = await mermaid.render(id, source)
    if (svg === '') {
      // Some environments produce no markup. Treat it as a failed render so the
      // caller keeps the source fence instead of an empty surface.
      throw new Error('mermaid produced no svg')
    }
    return svg
  }
  const result = queue.then(run)
  queue = result.then(
    () => undefined,
    () => undefined
  )
  return result
}
