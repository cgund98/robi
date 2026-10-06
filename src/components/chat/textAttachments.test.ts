import { describe, expect, it } from 'vitest'

import {
  attachmentFromPicked,
  bytesToBase64,
  filesFromTransfer,
  isImageFile,
  looksLikeText,
  readFileAttachment,
  toFileInputs
} from './textAttachments'

describe('text attachments', () => {
  it('recognises images by type and extension', () => {
    expect(isImageFile(new File(['x'], 'a.png', { type: 'image/png' }))).toBe(true)
    expect(isImageFile(new File(['x'], 'shot.png', { type: '' }))).toBe(true)
    expect(isImageFile(new File(['x'], 'notes.md', { type: 'text/markdown' }))).toBe(false)
  })

  it('decides text from content, not the name', () => {
    const text = new TextEncoder().encode('2026-01-01 boot\n')
    expect(looksLikeText(text)).toBe(true)

    // A NUL byte marks binary, whatever the name is.
    expect(looksLikeText(new Uint8Array([0x66, 0x00, 0x6f]))).toBe(false)
    // Invalid UTF-8 is binary too.
    expect(looksLikeText(new Uint8Array([0xff, 0xfe, 0xfa]))).toBe(false)
    // An empty file is text.
    expect(looksLikeText(new Uint8Array([]))).toBe(true)
  })

  it('reads a .log file into an attachment, base64-encoded', async () => {
    const attachment = await readFileAttachment(
      new File(['2026-01-01 boot\n'], 'server.log', { type: '' })
    )
    expect(attachment.name).toBe('server.log')
    expect(attachment.size).toBe(16)
    expect(attachment.contentBase64).toBe(
      bytesToBase64(new TextEncoder().encode('2026-01-01 boot\n'))
    )
  })

  it('refuses a binary file', async () => {
    const binary = new File([new Uint8Array([0x00, 0x01, 0x02])], 'blob.bin')
    await expect(readFileAttachment(binary)).rejects.toThrow('not a text file')
  })

  it('sends the server shape, omitting an absent path and range', () => {
    expect(toFileInputs([{ name: 'a.txt', contentBase64: 'aGk=', size: 2 }])).toEqual([
      { name: 'a.txt', content_base64: 'aGk=' }
    ])
  })

  it('sends the absolute path and range for a picked slice', () => {
    expect(
      toFileInputs([
        {
          name: 'error.rs',
          absolutePath: '/repo/src/error.rs',
          startLine: 29,
          endLine: 34,
          contentBase64: 'Ym9vbQ==',
          size: 4
        }
      ])
    ).toEqual([
      {
        name: 'error.rs',
        absolute_path: '/repo/src/error.rs',
        start_line: 29,
        end_line: 34,
        content_base64: 'Ym9vbQ=='
      }
    ])
  })

  it('builds a path-carrying attachment from a picked file', () => {
    const attachment = attachmentFromPicked({
      name: 'index.md',
      absolutePath: '/repo/docs/src/index.md',
      contentBase64: 'aGk='
    })
    expect(attachment.absolutePath).toBe('/repo/docs/src/index.md')
    expect(attachment.size).toBe(2)
  })

  it('refuses a picked binary file', () => {
    expect(() =>
      attachmentFromPicked({
        name: 'blob.bin',
        absolutePath: '/tmp/blob.bin',
        contentBase64: 'AAEC'
      })
    ).toThrow('not a text file')
  })

  it('names a clipboard image that has no filename', () => {
    const data = {
      files: [new File(['x'], '', { type: 'image/png' })],
      items: []
    } as unknown as DataTransfer
    const [file] = filesFromTransfer(data)
    expect(file.name).toBe('pasted.png')
    expect(file.type).toBe('image/png')
  })
})
