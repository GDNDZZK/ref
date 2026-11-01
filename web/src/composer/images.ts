// Turning files the user pasted or dropped into image attachments.

import type { ImageData } from '../proto/generated'

// Same types and size limit as the TUI accepts.
const IMAGE_MIME_TYPES = new Set(['image/png', 'image/jpeg', 'image/gif', 'image/webp'])
const MAX_IMAGE_BYTES = 20 * 1024 * 1024

// Bytes turned into characters per call, to stay within the argument limit.
const BASE64_CHUNK_BYTES = 0x8000

export type ImageRejection = 'type' | 'size'

export interface ReadImage {
  image: ImageData
  name: string | undefined
}

export function rejectionOf(file: File): ImageRejection | undefined {
  if (!IMAGE_MIME_TYPES.has(file.type)) return 'type'
  if (file.size > MAX_IMAGE_BYTES) return 'size'
  return undefined
}

// A pasted screenshot has no real name; browsers call it "image.png".
export async function readImage(file: File): Promise<ReadImage> {
  const bytes = new Uint8Array(await file.arrayBuffer())
  return { image: { mime: file.type, data: toBase64(bytes) }, name: file.name || undefined }
}

function toBase64(bytes: Uint8Array): string {
  let binary = ''
  for (let offset = 0; offset < bytes.length; offset += BASE64_CHUNK_BYTES) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + BASE64_CHUNK_BYTES))
  }
  return btoa(binary)
}

export function dataUrl(image: ImageData): string {
  return `data:${image.mime};base64,${image.data}`
}
