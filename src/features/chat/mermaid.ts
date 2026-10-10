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
  // A parse failure otherwise draws a wide error diagram into the page. The
  // caller logs it and keeps the source fence.
  suppressErrorRendering: true,
  themeVariables: THEME_VARIABLES
} satisfies MermaidConfig

/**
 * Mermaid's `useMaxWidth` writes `width="100%"` and no height. WebKit then
 * falls back to the SVG default of 150px and clips the rest, which cuts off a
 * sequence diagram under the participant boxes. Copy the viewBox size onto
 * the root so the diagram keeps its aspect ratio; the stylesheet caps it with
 * `max-width: 100%` and `height: auto`.
 */
export function withIntrinsicSize(svg: string): string {
  const root = new DOMParser().parseFromString(svg, 'image/svg+xml').documentElement
  if (root.nodeName.toLowerCase() !== 'svg') {
    return svg
  }
  const parts = root
    .getAttribute('viewBox')
    ?.trim()
    .split(/[\s,]+/)
    .map(Number)
  if (!parts || parts.length !== 4 || parts.some((part) => !Number.isFinite(part))) {
    return svg
  }
  const width = parts[2]
  const height = parts[3]
  if (!(width > 0) || !(height > 0)) {
    return svg
  }
  const widthAttr = root.getAttribute('width')
  const heightAttr = root.getAttribute('height')
  if (widthAttr === null || widthAttr.endsWith('%')) {
    root.setAttribute('width', String(width))
  }
  if (heightAttr === null || heightAttr.endsWith('%')) {
    root.setAttribute('height', String(height))
  }
  return root.outerHTML
}

const SEQUENCE_STATEMENT =
  /^(?:end|alt|else|opt|loop|par|and|note|rect|break|critical|option|autonumber|box|participant|actor|activate|deactivate|create|destroy|title|link|links)\b|^[^:\n]+?(?:<<->>|<<-->>|->>|-->>|->|-->)/

const RESERVED_ACTOR =
  /^(?:end|alt|else|opt|loop|par|and|note|rect|break|critical|option|autonumber|box|participant|actor|activate|deactivate|create|destroy|title|link|links|over)$/i

const SEQUENCE_ARROW = /<<->>|<<-->>|->>|-->>|->|-->/

function quoteActor(name: string): string {
  return RESERVED_ACTOR.test(name) ? `"${name}"` : name
}

/**
 * `loop` is a sequence block, and the lexer matches it case-insensitively, so
 * an actor id `Loop` is read as that keyword and the diagram fails to parse.
 * Quote reserved ids where they name an actor. The alias after `as`, and the
 * message after `:`, stay as written.
 */
function quoteReservedActors(source: string): string {
  return source
    .split('\n')
    .map((line) => {
      const signal = line.search(SEQUENCE_ARROW)
      const colon = line.indexOf(':')
      const head = signal >= 0 && colon > signal ? line.slice(0, colon) : line
      const tail = signal >= 0 && colon > signal ? line.slice(colon) : ''
      let next = head.replace(
        /^(\s*(?:participant|actor|create|destroy|activate|deactivate)\s+)([A-Za-z_][\w]*)/i,
        (_, prefix: string, name: string) => prefix + quoteActor(name)
      )
      next = next.replace(
        new RegExp(`([A-Za-z_][\\w]*)(\\s*)(${SEQUENCE_ARROW.source})`, 'g'),
        (_, name: string, space: string, arrow: string) => quoteActor(name) + space + arrow
      )
      next = next.replace(
        new RegExp(`(${SEQUENCE_ARROW.source})(\\s*)([A-Za-z_][\\w]*)`, 'g'),
        (_, arrow: string, space: string, name: string) => arrow + space + quoteActor(name)
      )
      next = next.replace(/^(\s*note\s+over\s+)([^:\n]*)/i, (_, prefix: string, actors: string) => {
        return prefix + actors.replace(/\b([A-Za-z_][\w]*)\b/g, (word) => quoteActor(word))
      })
      return next + tail
    })
    .join('\n')
}

/**
 * Mermaid ends a sequence statement at `;`. A semicolon inside a note or
 * message, as in `unknown;<br/>`, splits the line and the remainder fails to
 * parse, so the fence stays on screen as source. A semicolon that actually
 * starts the next statement is left alone. One that sits in the prose becomes
 * a fullwidth semicolon, which reads the same and is not a statement break.
 *
 * An actor id that is a sequence keyword (`Loop`, `end`, `alt`, …) is quoted
 * so it stays an actor. The message text is left alone.
 */
export function sequenceSource(source: string): string {
  if (!/^\s*(?:%%[^\n]*\n\s*)*sequenceDiagram\b/.test(source)) {
    return source
  }
  const quoted = quoteReservedActors(source)
  return quoted.replace(/;(?=[^\n]*\S)/g, (semi, index: number) => {
    const rest = quoted.slice(index + semi.length).replace(/^[ \t]+/, '')
    return SEQUENCE_STATEMENT.test(rest) ? semi : '\uFF1B'
  })
}

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
    let svg: string
    try {
      const rendered = await mermaid.render(id, sequenceSource(source))
      svg = rendered.svg
    } catch (err) {
      console.error('mermaid diagram failed to render', err)
      throw err
    }
    if (svg === '') {
      // Some environments produce no markup. Treat it as a failed render so the
      // caller keeps the source fence instead of an empty surface.
      console.error('mermaid diagram failed to render', 'produced no svg')
      throw new Error('mermaid produced no svg')
    }
    return withIntrinsicSize(svg)
  }
  const result = queue.then(run)
  queue = result.then(
    () => undefined,
    () => undefined
  )
  return result
}
