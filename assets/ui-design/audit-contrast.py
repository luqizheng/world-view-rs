"""
World View UI v0.1.1 · 对比度实测（WCAG 2.x）

为什么有这个脚本：
    v0.1 的配色是目测配的，实测发现 6 处不合格（其中 4 处我以为合格）。
    目测是设计工作里最不可靠的一环 —— 颜色必须算，不能看。

用法：
    python audit-contrast.py           # 全部检查
    python audit-contrast.py --strict  # 任一不合格即非零退出（供 CI 用）

门槛（WCAG 2.2 AA）：
    · 正文        ≥ 4.5:1
    · 非文本/UI   ≥ 3.0:1（边框、图标、图形对象）
    · 大字号      ≥ 3.0:1（≥24px，或 ≥18.66px 且粗体）
    徽标字号 10px —— 不适用大字号门槛。
"""
import sys

# ---------- WCAG 相对亮度 ----------
def _lin(c):
    c = c / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4

def lum(hexstr):
    h = hexstr.lstrip('#')
    return (0.2126 * _lin(int(h[0:2], 16))
            + 0.7152 * _lin(int(h[2:4], 16))
            + 0.0722 * _lin(int(h[4:6], 16)))

def ratio(fg, bg):
    a, b = lum(fg), lum(bg)
    hi, lo = max(a, b), min(a, b)
    return (hi + 0.05) / (lo + 0.05)

def blend(fg, bg, alpha):
    """前景以 alpha 叠在背景上的实际色值 —— 半透明底必须先合成再算。"""
    f, b = fg.lstrip('#'), bg.lstrip('#')
    return '#%02X%02X%02X' % tuple(
        round(int(f[i:i+2], 16) * alpha + int(b[i:i+2], 16) * (1 - alpha))
        for i in (0, 2, 4))

# ---------- 令牌 ----------
DARK = {
    'bg.app': '#0F1116', 'bg.map': '#0B0D11', 'bg.panel': '#191C24',
    'bg.raised': '#212530', 'bg.hover': '#2A2F3C', 'bg.active': '#323847',
    'bd.subtle': '#272C37', 'bd.strong': '#3A4152',
    'text.1': '#E9ECF3', 'text.2': '#A3ABB9', 'text.3': '#858D9C',
    'accent': '#4C8DF6', 'focus': '#F2C14E',
}
LIGHT = {
    'bg.app': '#EDEEF2', 'bg.map': '#E4E6EB', 'bg.panel': '#FBFBFD',
    'bg.raised': '#F1F2F6', 'bg.hover': '#E7E9F0', 'bg.active': '#DBDEE8',
    'bd.subtle': '#E1E4EC', 'bd.strong': '#C6CBD8',
    'text.1': '#1B1E26', 'text.2': '#525A69', 'text.3': '#6B7383',
    'accent': '#2A6FD4', 'focus': '#8A5A00',
}
WORLD = {
    'core.snow': '#E6ECEF',            # 地图上最亮的色 —— 浮层必须扛得住它
    'accent.select': '#F2C14E',
    'accent.cursor': '#E8623C',
    'ui_link.reachable': '#3F9B4E',
    'ui_link.late': '#C9971F',
    'ui_link.unreachable': '#B5453A',
}

FAILS = []

def check(label, fg, bg, need=4.5, note=''):
    r = ratio(fg, bg)
    ok = r >= need
    if not ok:
        FAILS.append((label, fg, bg, r, need))
    tag = 'PASS' if ok else '*** FAIL ***'
    print(f'  {fg} on {bg}  {r:6.2f}:1  (需 {need})  {tag:<14} {label} {note}')
    return r

def head(t):
    print('\n' + '=' * 74)
    print(t)
    print('-' * 74)

def main(strict=False):
    print('#' * 74)
    print('# World View UI v0.1.1 · WCAG 2.2 AA 对比度实测')
    print('#' * 74)

    for theme, T in (('暗色外壳', DARK), ('亮色外壳', LIGHT)):
        head(f'{theme} · 文字层级（底 = ui.bg.panel）')
        p = T['bg.panel']
        check('主文本 text.1', T['text.1'], p)
        check('次文本 text.2', T['text.2'], p)
        check('弱文本 text.3', T['text.3'], p)
        check('交互主色 accent（需 3:1，非文本）', T['accent'], p, 3.0)

    # ---------- v0.2 新增：面包屑（crumb）与钻入标记 ----------
    head('面包屑 crumb（底 = ui-bg-float .94 叠地图亮处）');
    for theme, T, light_t3, dark_t3 in (('暗色', DARK, '#8A93A6', '#858D9C'), ('亮色', LIGHT, '#5B6373', '#5B6373')):
        # ui-bg-float 在雪原 #E6ECEF 上的合成色（取与时间轴 chip 一致的 worst-case 底）
        if theme == '暗色':
            bg = blend('#14171D', WORLD['core.snow'], 0.94)   # ≈ #21242A
        else:
            bg = blend('#F8F8FB', WORLD['core.snow'], 0.93)   # ≈ #F7F7FA
        check(f'{theme} crumb-i:not([disabled])', light_t3, bg, 4.5, '上级（可点击）')
        # 当前级 = ui-text-1，已在主文本测试覆盖（虽然底不同，但 ui-text-1 对比度更高，天然通过）
        # 这里只补"上级"的色值，因为它是新增/覆盖的令牌

    head('钻入标记 drill-marker（图形对象，非文本需 3:1）');
    for theme, T in (('暗色', DARK), ('亮色', LIGHT)):
        bg = T['bg.map']
        check(f'{theme} 菱形默认（--ui-accent on ui-bg-map）', T['accent'], bg, 3.0)
        check(f'{theme} 菱形已访问（--ui-text-3 on ui-bg-map）', T['text.3'], bg, 3.0)

    head('状态色徽标（10px 文字 —— 不适用大字号门槛，按 4.5:1 计）')
    check('.bdg.ok', '#7FD593', blend(WORLD['ui_link.reachable'], DARK['bg.active'], 0.16))
    check('.bdg.warn', '#E8C065', blend(WORLD['ui_link.late'], DARK['bg.active'], 0.16))
    check('.bdg.err', '#F09A91', blend(WORLD['ui_link.unreachable'], DARK['bg.active'], 0.18))

    head('提示条 .note.*（底透明度更低，实测本就合格）')
    r = DARK['bg.raised']
    check('.note.warn', '#E8C065', blend(WORLD['ui_link.late'], r, 0.09))
    check('.note.ok', '#7FD593', blend(WORLD['ui_link.reachable'], r, 0.09))
    check('.note.err', '#F09A91', blend(WORLD['ui_link.unreachable'], r, 0.10))

    head('选中描边 focus-ring（非文本，需 3:1）')
    check('暗色 brush 描边', blend('#F2C14E', DARK['bg.raised'], 0.85), DARK['bg.raised'], 3.0)
    check('亮色 brush 描边', blend('#8A5A00', LIGHT['bg.raised'], 0.90), LIGHT['bg.raised'], 3.0)

    head('焦点环 focus（与面板底对比）')
    check('暗色 focus', DARK['focus'], DARK['bg.panel'], 3.0)
    check('亮色 focus', LIGHT['focus'], LIGHT['bg.panel'], 3.0)

    head('浮层 chip 压在地图最亮处（雪原 #E6ECEF）')
    snow = WORLD['core.snow']
    check('暗色 float .94 · 次文本', DARK['text.2'], blend('#14171D', snow, 0.94))
    check('亮色 float .93 · 次文本', LIGHT['text.2'], blend('#F8F8FB', snow, 0.93))

    head('时间轴游标（图形对象，需 3:1）')
    check('accent.cursor on 面板底', WORLD['accent.cursor'], DARK['bg.panel'], 3.0)

    head('已知坑：金色在亮色下不可直接使用')
    r1 = ratio(WORLD['accent.select'], LIGHT['bg.panel'])
    r2 = ratio(LIGHT['focus'], LIGHT['bg.panel'])
    print(f'  world.accent.select #F2C14E on 白底 = {r1:.2f}:1  → 不可作焦点环')
    print(f'  ui.focus         #8A5A00 on 白底 = {r2:.2f}:1  → 合格替身')
    if r1 >= 3.0:
        FAILS.append(('world.accent.select 亮色可用性', '#F2C14E', '白底', r1, 3.0))

    head('结果')
    if FAILS:
        print(f'  ✗ {len(FAILS)} 项不合格：')
        for label, fg, bg, r, need in FAILS:
            print(f'      {label}: {fg} on {bg} = {r:.2f}:1 (需 {need})')
    else:
        print('  ✓ 全部通过')

    if strict and FAILS:
        sys.exit(1)
    return len(FAILS)

if __name__ == '__main__':
    main(strict='--strict' in sys.argv)