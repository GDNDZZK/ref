// Shared lightbox controller provided by ChatView so ImageThumbs can navigate across all images in a session.

import { inject } from 'vue'
import type { InjectionKey } from 'vue'
import type { ImageRef } from '../model/blocks.ts'
import { formatBytes } from '../model/format.ts'

export interface LightboxController {
  open(image: ImageRef): void
}

export const imageLightboxKey: InjectionKey<LightboxController> = Symbol('imageLightbox')

export function useImageLightbox(): LightboxController | undefined {
  return inject(imageLightboxKey, undefined)
}

export function imageCaption(image: ImageRef): string {
  return [image.name ?? image.mime, image.size === null ? '' : formatBytes(image.size)]
    .filter(Boolean)
    .join(' · ')
}
