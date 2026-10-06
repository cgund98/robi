import { describe, expect, it } from 'vitest'

import { languageForPath, paintSides } from './highlight'

describe('paintSides', () => {
  it('colors a rust keyword from the current file', async () => {
    expect(languageForPath('src/main.rs')).toBe('rust')
    const sides = await paintSides('src/main.rs', 'fn old() {}\n', 'fn main() {}\n')
    const colors = sides.current.flatMap((line) => line.map((token) => token.color))
    expect(colors.some((color) => color && color.length > 0)).toBe(true)
    expect(sides.baseline[0]?.map((token) => token.text).join('')).toBe('fn old() {}')
  })

  it('leaves an unknown extension uncolored', async () => {
    const sides = await paintSides('notes.txt', '', 'hello\n')
    expect(sides.current).toEqual([[{ text: 'hello' }]])
  })
})
