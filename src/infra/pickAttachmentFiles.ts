import { invoke, isTauri } from '@tauri-apps/api/core'

/** One file the native picker returned, with its absolute path. */
export type PickedAttachment = {
  name: string
  absolutePath: string
  contentBase64: string
}

/**
 * Open the OS file dialog and read the chosen files, in the desktop app.
 *
 * Returns `null` in a normal browser, where the caller falls back to the
 * `<input type="file">` (which cannot provide an absolute path). The desktop
 * dialog does, and it is what lets the server decide whether an attachment is
 * inside the workspace.
 */
export async function pickAttachmentFiles(): Promise<PickedAttachment[] | null> {
  if (!isTauri()) {
    return null
  }
  return invoke<PickedAttachment[]>('pick_attachment_files')
}
