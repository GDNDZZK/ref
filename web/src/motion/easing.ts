// Qt easing curves, ported from Qt 6.12 (qtbase/src/3rdparty/easing/easing.cpp and the
// wrappers in src/corelib/tools/qeasingcurve.cpp). Parameters are Qt's defaults unless the
// QML animation sets them. Every curve maps progress [0, 1] to effective progress;
// values may leave [0, 1] (overshoot).

export type EasingFn = (progress: number) => number

const QT_DEFAULT_ELASTIC_AMPLITUDE = 1
const QT_DEFAULT_ELASTIC_PERIOD = 0.3
const QT_DEFAULT_OVERSHOOT = 1.70158

// OutExpo is scaled by 1.001 so that it reaches 1 before t = 1; Qt pins t = 1 to exactly 1.
const OUT_EXPO_SCALE = 1.001
const OUT_EXPO_RATE = 10

function clampProgress(t: number): number {
  return Math.min(1, Math.max(0, t))
}

const linear: EasingFn = (t) => clampProgress(t)

const inQuad: EasingFn = (t) => {
  const p = clampProgress(t)
  return p * p
}

const inCubic: EasingFn = (t) => {
  const p = clampProgress(t)
  return p * p * p
}

const inOutSine: EasingFn = (t) => -0.5 * (Math.cos(Math.PI * clampProgress(t)) - 1)

const outExpo: EasingFn = (t) => {
  const p = clampProgress(t)
  return p === 1 ? 1 : OUT_EXPO_SCALE * (1 - Math.pow(2, -OUT_EXPO_RATE * p))
}

export function outBack(overshoot: number = QT_DEFAULT_OVERSHOOT): EasingFn {
  return (t) => {
    const p = clampProgress(t)
    // Qt pins the ends: the polynomial is not exact there.
    if (p <= 0) return 0
    if (p >= 1) return 1
    const shifted = p - 1
    return shifted * shifted * ((overshoot + 1) * shifted + overshoot) + 1
  }
}

// Exponentially decaying sine around 1. An amplitude below 1 is raised to 1 (Qt does the same).
export function outElastic(
  amplitude: number = QT_DEFAULT_ELASTIC_AMPLITUDE,
  period: number = QT_DEFAULT_ELASTIC_PERIOD,
): EasingFn {
  const target = 1
  const reachesTarget = amplitude >= target
  const a = reachesTarget ? amplitude : target
  const phase = reachesTarget ? (period / (2 * Math.PI)) * Math.asin(target / a) : period / 4

  return (t) => {
    const p = clampProgress(t)
    if (p === 0) return 0
    if (p === 1) return target
    return a * Math.pow(2, -10 * p) * Math.sin(((p - phase) * (2 * Math.PI)) / period) + target
  }
}

// The curves that take no parameters, by Qt's QEasingCurve::Type name with a lower-case first letter.
export const plainEasings = {
  linear,
  inQuad,
  inCubic,
  inOutSine,
  outExpo,
  outBack: outBack(),
  outElastic: outElastic(),
} as const satisfies Record<string, EasingFn>

export type EasingName = keyof typeof plainEasings

export function isEasingName(name: string): name is EasingName {
  return Object.hasOwn(plainEasings, name)
}

// `Easing.OutExpo` in QML -> `outExpo`.
export function easingNameFromQml(qmlName: string): EasingName {
  const name = qmlName.charAt(0).toLowerCase() + qmlName.slice(1)
  if (!isEasingName(name)) throw new Error(`Easing.${qmlName} is not ported`)
  return name
}
