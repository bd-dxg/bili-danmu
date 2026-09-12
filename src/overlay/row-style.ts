import type { OverlayStyle } from '../types/ipc'

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
