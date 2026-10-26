/**
 * Whether a key event is the clear-composer chord: `Mod+Shift+K`.
 *
 * Both `meta` and `ctrl` count, so the same physical chord works on a Mac and
 * elsewhere without asking the platform. Nothing else in the shell claims it.
 */
export function clearComposerShortcut(
  event: Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'shiftKey' | 'altKey' | 'repeat'>
): boolean {
  if (!(event.metaKey || event.ctrlKey) || !event.shiftKey || event.altKey || event.repeat) {
    return false
  }
  return event.key.toLowerCase() === 'k'
}
