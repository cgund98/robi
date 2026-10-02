/**
 * Shell workspace until a real workspace picker / trust store exists.
 * `id` must stay a UUID — the chat-session API rejects anything else.
 */
export const SHELL_WORKSPACE = {
  id: '019a0000-0000-7000-8000-000000000001',
  label: 'acme-storefront'
} as const
