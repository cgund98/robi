import { createHighlighterCore, type HighlighterCore, type ThemedToken } from 'shiki/core'
import { createJavaScriptRegexEngine } from 'shiki/engine/javascript'
import css from 'shiki/langs/css.mjs'
import go from 'shiki/langs/go.mjs'
import html from 'shiki/langs/html.mjs'
import javascript from 'shiki/langs/javascript.mjs'
import json from 'shiki/langs/json.mjs'
import jsx from 'shiki/langs/jsx.mjs'
import markdown from 'shiki/langs/markdown.mjs'
import python from 'shiki/langs/python.mjs'
import rust from 'shiki/langs/rust.mjs'
import sql from 'shiki/langs/sql.mjs'
import toml from 'shiki/langs/toml.mjs'
import tsx from 'shiki/langs/tsx.mjs'
import typescript from 'shiki/langs/typescript.mjs'
import yaml from 'shiki/langs/yaml.mjs'
import githubDark from 'shiki/themes/github-dark.mjs'

export type PaintedToken = {
  text: string
  color?: string
}

const LANG_BY_EXT: Record<string, string> = {
  ts: 'typescript',
  tsx: 'tsx',
  js: 'javascript',
  jsx: 'jsx',
  rs: 'rust',
  py: 'python',
  go: 'go',
  json: 'json',
  css: 'css',
  md: 'markdown',
  html: 'html',
  toml: 'toml',
  yaml: 'yaml',
  yml: 'yaml',
  sql: 'sql'
}

let highlighterPromise: Promise<HighlighterCore> | null = null

function highlighter(): Promise<HighlighterCore> {
  highlighterPromise ??= createHighlighterCore({
    themes: [githubDark],
    langs: [
      typescript,
      tsx,
      javascript,
      jsx,
      rust,
      python,
      go,
      json,
      css,
      markdown,
      html,
      toml,
      yaml,
      sql
    ],
    engine: createJavaScriptRegexEngine()
  })
  return highlighterPromise
}

export function languageForPath(path: string): string | null {
  const name = path.split('/').pop() ?? path
  const dot = name.lastIndexOf('.')
  if (dot < 0) {
    return null
  }
  return LANG_BY_EXT[name.slice(dot + 1).toLowerCase()] ?? null
}

function splitLines(text: string): string[] {
  if (text === '') {
    return []
  }
  const lines = text.split(/\r?\n/)
  if (lines[lines.length - 1] === '') {
    lines.pop()
  }
  return lines
}

function plain(text: string): PaintedToken[][] {
  return splitLines(text).map((line) => [{ text: line }])
}

function fromTokens(tokens: ThemedToken[][]): PaintedToken[][] {
  return tokens.map((line) => line.map((token) => ({ text: token.content, color: token.color })))
}

async function paint(text: string, lang: string): Promise<PaintedToken[][]> {
  const lines = splitLines(text)
  if (lines.length === 0) {
    return []
  }
  const hl = await highlighter()
  const { tokens } = hl.codeToTokens(text, { lang, theme: 'github-dark' })
  let painted = fromTokens(tokens)
  if (
    painted.length === lines.length + 1 &&
    painted[painted.length - 1].every((token) => token.text === '')
  ) {
    painted = painted.slice(0, -1)
  }
  if (painted.length !== lines.length) {
    return plain(text)
  }
  return painted
}

export async function paintSides(
  path: string,
  baseline: string,
  current: string
): Promise<{ baseline: PaintedToken[][]; current: PaintedToken[][] }> {
  const lang = languageForPath(path)
  if (!lang) {
    return { baseline: plain(baseline), current: plain(current) }
  }
  try {
    const [before, after] = await Promise.all([paint(baseline, lang), paint(current, lang)])
    return { baseline: before, current: after }
  } catch {
    return { baseline: plain(baseline), current: plain(current) }
  }
}
