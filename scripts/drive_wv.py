# -*- coding: utf-8 -*-
"""wv.exe 交互辅助：PostMessage 点击/按键（坐标=客户区物理像素）。"""
import ctypes, sys, time
from ctypes import wintypes

user32 = ctypes.windll.user32

def find_hwnd():
    result = []
    @ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)
    def cb(hwnd, lparam):
        buf = ctypes.create_unicode_buffer(256)
        user32.GetWindowTextW(hwnd, buf, 256)
        if 'World View' in buf.value and user32.IsWindowVisible(hwnd):
            r = wintypes.RECT()
            ctypes.windll.dwmapi.DwmGetWindowAttribute(hwnd, 9, ctypes.byref(r), ctypes.sizeof(r))
            if (r.right - r.left) > 300:
                result.append(hwnd)
        return True
    user32.EnumWindows(cb, 0)
    return result[0] if result else None

def click(hwnd, x, y):
    lparam = (int(y) << 16) | (int(x) & 0xFFFF)
    user32.PostMessageW(hwnd, 0x0200, 0, lparam)  # WM_MOUSEMOVE
    time.sleep(0.05)
    user32.PostMessageW(hwnd, 0x0201, 1, lparam)  # WM_LBUTTONDOWN MK_LBUTTON
    time.sleep(0.06)
    user32.PostMessageW(hwnd, 0x0202, 0, lparam)  # WM_LBUTTONUP

def key(hwnd, vk, ctrl=False):
    if ctrl:
        user32.PostMessageW(hwnd, 0x0100, 0x11, 0)  # WM_KEYDOWN VK_CONTROL
        time.sleep(0.03)
    user32.PostMessageW(hwnd, 0x0100, vk, 0)
    time.sleep(0.04)
    user32.PostMessageW(hwnd, 0x0101, vk, 0)  # WM_KEYUP
    if ctrl:
        time.sleep(0.03)
        user32.PostMessageW(hwnd, 0x0101, 0x11, 0)

def type_text(hwnd, text):
    for ch in text:
        user32.PostMessageW(hwnd, 0x0102, ord(ch), 0)  # WM_CHAR
        time.sleep(0.02)

if __name__ == "__main__":
    hwnd = find_hwnd()
    if not hwnd:
        print("no window"); sys.exit(1)
    cmd = sys.argv[1]
    if cmd == "click":
        click(hwnd, int(sys.argv[2]), int(sys.argv[3]))
        print(f"clicked {sys.argv[2]},{sys.argv[3]}")
    elif cmd == "key":
        key(hwnd, int(sys.argv[2], 0), ctrl=(len(sys.argv) > 3 and sys.argv[3] == "ctrl"))
        print(f"key {sys.argv[2]}")
    elif cmd == "text":
        type_text(hwnd, sys.argv[2])
        print(f"typed {sys.argv[2]}")
