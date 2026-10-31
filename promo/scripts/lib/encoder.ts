import { spawn, type ChildProcess } from 'node:child_process'
import { once } from 'node:events'

export interface EncoderOptions {
  output: string
  fps: number
}

const X264_CRF = 16 // visually lossless for flat UI content
const X264_PRESET = 'medium'

// ffmpeg reading PNG frames from stdin, writing H.264 / yuv420p tagged as BT.709.
export class Encoder {
  private readonly process: ChildProcess
  private readonly exited: Promise<number | null>

  constructor({ output, fps }: EncoderOptions) {
    const args = [
      ...['-y', '-hide_banner', '-loglevel', 'error'],
      ...['-f', 'image2pipe', '-framerate', String(fps), '-c:v', 'png', '-i', '-'],
      ...['-vf', 'scale=out_color_matrix=bt709:out_range=tv,format=yuv420p'],
      ...['-c:v', 'libx264', '-preset', X264_PRESET, '-crf', String(X264_CRF)],
      ...['-colorspace', 'bt709', '-color_primaries', 'bt709', '-color_trc', 'bt709'],
      ...['-r', String(fps), '-movflags', '+faststart', output],
    ]
    this.process = spawn('ffmpeg', args, { stdio: ['pipe', 'inherit', 'inherit'] })
    this.exited = once(this.process, 'exit').then(([code]) => code as number | null)
  }

  async write(frame: Buffer): Promise<void> {
    const stdin = this.process.stdin!
    if (stdin.write(frame)) return
    await once(stdin, 'drain')
  }

  async finish(): Promise<void> {
    this.process.stdin!.end()
    const code = await this.exited
    if (code !== 0) throw new Error(`ffmpeg exited with ${code}`)
  }

  // Only ever stops the ffmpeg this object started.
  abort(): void {
    this.process.kill()
  }
}
