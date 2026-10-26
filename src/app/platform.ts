/**
 * The OS the app is running on, when that changes behavior.
 *
 * Read from the user agent because the webview does not expose the platform
 * any other way to the renderer. One function per question, so the window
 * chrome and the keyboard shortcuts cannot disagree.
 */
export function isMacOs(): boolean {
  return /mac/i.test(navigator.platform || navigator.userAgent)
}
