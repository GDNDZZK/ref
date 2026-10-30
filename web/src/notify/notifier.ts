// The browser's Notification API behind a small interface, so the rest of the app (and its
// tests) never touch the global.

export type NotificationAvailability =
  | 'available'
  | 'insecure' // http on a host other than localhost: browsers hide notifications there
  | 'unsupported'

export type NotificationPermissionState = 'default' | 'granted' | 'denied'

export interface NotificationRequest {
  title: string
  body: string
  tag: string // a newer notification with the same tag replaces the older one
  onClick(): void
}

export interface BrowserNotifications {
  availability: NotificationAvailability
  permission(): NotificationPermissionState
  // Browsers only allow this from a user gesture (a click or key press).
  requestPermission(): Promise<NotificationPermissionState>
  show(request: NotificationRequest): void
}

// The part of the Notification constructor we use.
export interface NotificationConstructor {
  new (
    title: string,
    options?: { body?: string; tag?: string; icon?: string },
  ): {
    onclick: ((event: Event) => void) | null
    close(): void
  }
  readonly permission: NotificationPermissionState
  requestPermission(): Promise<NotificationPermissionState>
}

export interface NotificationEnvironment {
  Notification: NotificationConstructor | undefined
  isSecureContext: boolean
  iconUrl: string
}

export function browserNotifications(environment: NotificationEnvironment): BrowserNotifications {
  const { Notification: Native, isSecureContext, iconUrl } = environment
  const availability = availabilityOf(Native, isSecureContext)

  return {
    availability,
    permission: () => (availability === 'available' ? Native!.permission : 'denied'),
    requestPermission: async () => {
      if (availability !== 'available') return 'denied'
      return await Native!.requestPermission()
    },
    show({ title, body, tag, onClick }) {
      if (availability !== 'available') return
      const shown = new Native!(title, { body, tag, icon: iconUrl })
      shown.onclick = () => {
        shown.close()
        onClick()
      }
    },
  }
}

function availabilityOf(
  native: NotificationConstructor | undefined,
  isSecureContext: boolean,
): NotificationAvailability {
  if (!isSecureContext) return 'insecure'
  return native ? 'available' : 'unsupported'
}
