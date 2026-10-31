// Set by capture.ts once it runs under `dbus-run-session`. ref-gui (and Chromium, ref) send
// desktop notifications to the session bus when a turn ends, so a capture on the user's own bus
// would pop up real notifications.
export const PRIVATE_BUS_FLAG = 'REF_CAPTURE_PRIVATE_BUS'

export function requirePrivateBus(): void {
  if (process.env[PRIVATE_BUS_FLAG] !== '1') {
    throw new Error('this must run under a private D-Bus session; start it with pnpm capture')
  }
}
