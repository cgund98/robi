import { describe, expect, it } from 'vitest'

import { instructionWithTextFiles, isImageFile, isTextFile, readTextFile } from './textAttachments'

describe('text attachments', () => {
  it('recognises images and text by type and extension', () => {
    expect(isImageFile(new File(['x'], 'a.png', { type: 'image/png' }))).toBe(true)
    expect(isTextFile(new File(['x'], 'notes.MD'))).toBe(true)
    expect(isTextFile(new File(['x'], 'photo.png'))).toBe(false)
    expect(isTextFile(new File(['x'], 'README'))).toBe(false)
  })

  it('folds file text after the draft', () => {
    expect(instructionWithTextFiles('look at this', [{ name: 'a.rs', text: 'fn main() {}' }])).toBe(
      'look at this\n\n<file name="a.rs">\nfn main() {}\n</file>'
    )
  })

  it('sends attachments alone when the draft is empty', () => {
    expect(instructionWithTextFiles('  ', [{ name: 'a.txt', text: 'hi' }])).toBe(
      '<file name="a.txt">\nhi\n</file>'
    )
  })

  it('rejects a file that contains a NUL', async () => {
    const file = new File(['ok\0no'], 'a.txt', { type: 'text/plain' })
    await expect(readTextFile(file)).rejects.toThrow('not a text file')
  })
})
