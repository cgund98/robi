/** Time-of-day line for an empty chat. Morning ends at noon, afternoon at 5pm. */
export function greetingLabel(now: Date): string {
  const hour = now.getHours()
  if (hour < 12) {
    return 'Good morning'
  }
  if (hour < 17) {
    return 'Good afternoon'
  }
  return 'Good evening'
}
