import type { OverlayStyle } from '../types/ipc'

/** 欢迎信息行的色相偏移量（度）：从「用户名颜色」派生它的颜色 */
export const WELCOME_HUE_SHIFT = 30

/**
 * 描边 → 四向 text-shadow（硬边，宽度近似）。
 *
 * 弹幕行与礼物行共用：两区跟随同一份弹幕样式，各写一份会导致字号/描边不一致。
 */
export function rowShadow(s: OverlayStyle): string {
  if (!s.outline) return 'none'
  const w = Math.min(5, Math.max(0, Math.round(s.outline_width)))
  const shadows: string[] = []
  for (let i = 1; i <= w; i++) {
    shadows.push(
      `0 ${i}px 0 ${s.outline_color}`,
      `0 -${i}px 0 ${s.outline_color}`,
      `${i}px 0 0 ${s.outline_color}`,
      `-${i}px 0 0 ${s.outline_color}`,
    )
  }
  return shadows.join(', ')
}

/**
 * 色相偏移指定度数，亮度与饱和度不变。
 *
 * 欢迎信息行靠它从「用户名颜色」派生自己的颜色：颜色跟着主播的主题色走，又和弹幕
 * 用户名不同。刻意**只动色相**——亮度决定可读性，弹幕窗背景多是游戏画面，降亮度会糊。
 * 不用 CSS `filter: hue-rotate()`：那会给每个元素开一个合成层，而弹幕窗本来就怕图层多。
 *
 * 只认 `#RRGGBB`（颜色选择器只产出这个），其它输入原样返回；灰阶没有色相可转，同样原样返回。
 */
export function shiftHue(hex: string, degrees: number): string {
  const m = /^#([\da-f]{6})$/i.exec(hex.trim())
  if (!m) return hex
  const n = Number.parseInt(m[1], 16)
  const r = ((n >> 16) & 0xff) / 255
  const g = ((n >> 8) & 0xff) / 255
  const b = (n & 0xff) / 255

  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const l = (max + min) / 2
  const d = max - min
  if (d === 0) return hex

  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min)
  let h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4
  h = (h / 6 + degrees / 360 + 1) % 1

  const q = l < 0.5 ? l * (1 + s) : l + s - l * s
  const p = 2 * l - q
  const channel = (t: number) => {
    let x = t
    if (x < 0) x += 1
    if (x > 1) x -= 1
    const v = x < 1 / 6 ? p + (q - p) * 6 * x : x < 1 / 2 ? q : x < 2 / 3 ? p + (q - p) * (2 / 3 - x) * 6 : p
    return Math.round(Math.min(1, Math.max(0, v)) * 255)
  }
  const byte = (v: number) => v.toString(16).padStart(2, '0')
  return `#${byte(channel(h + 1 / 3))}${byte(channel(h))}${byte(channel(h - 1 / 3))}`
}
