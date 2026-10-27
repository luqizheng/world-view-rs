# -*- coding: utf-8 -*-
"""抓 wv.exe 客户区帧：PrintWindow → PNG（验收配方，坐标逻辑像素=物理÷scale）。"""
import ctypes, sys
from ctypes import wintypes
from PIL import Image

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32

def find_hwnd(title_part):
    result = []
    @ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)
    def cb(hwnd, lparam):
        buf = ctypes.create_unicode_buffer(256)
        user32.GetWindowTextW(hwnd, buf, 256)
        if title_part in buf.value and user32.GetWindowLongW(hwnd, -16) & 0x10000000:
            # 只要可见的主顶层窗
            if user32.IsWindowVisible(hwnd):
                r = wintypes.RECT()
                ctypes.windll.dwmapi.DwmGetWindowAttribute(hwnd, 9, ctypes.byref(r), ctypes.sizeof(r))
                if (r.right - r.left) > 300:
                    result.append(hwnd)
        return True
    user32.EnumWindows(cb, 0)
    return result[0] if result else None

def client_size(hwnd):
    rect = wintypes.RECT()
    ctypes.windll.dwmapi.DwmGetWindowAttribute(
        hwnd, 9, ctypes.byref(rect), ctypes.sizeof(rect))  # DWMWA_EXTENDED_FRAME_BOUNDS
    return rect.right - rect.left, rect.bottom - rect.top

def capture(hwnd, out):
    w, h = client_size(hwnd)
    hdc = user32.GetDC(hwnd)
    mem = gdi32.CreateCompatibleDC(hdc)
    bmp = gdi32.CreateCompatibleBitmap(hdc, w, h)
    gdi32.SelectObject(mem, bmp)
    # PW_CLIENTONLY=1 | PW_RENDERFULLCONTENT=2
    user32.PrintWindow(hwnd, mem, 3)
    class BMPINFOHEADER(ctypes.Structure):
        _fields_ = [("biSize", ctypes.c_uint32), ("biWidth", ctypes.c_int32), ("biHeight", ctypes.c_int32), ("biPlanes", ctypes.c_uint16), ("biBitCount", ctypes.c_uint16), ("biCompression", ctypes.c_uint32), ("biSizeImage", ctypes.c_uint32), ("biXPelsPerMeter", ctypes.c_int32), ("biYPelsPerMeter", ctypes.c_int32), ("biClrUsed", ctypes.c_uint32), ("biClrImportant", ctypes.c_uint32)]
    bi = BMPINFOHEADER()
    bi.biSize = ctypes.sizeof(BMPINFOHEADER)
    bi.biWidth = w
    bi.biHeight = -h
    bi.biPlanes = 1
    bi.biBitCount = 32
    buf = ctypes.create_string_buffer(w * h * 4)
    gdi32.GetDIBits(mem, bmp, 0, h, buf, ctypes.byref(bi), 0)
    img = Image.frombuffer("RGBA", (w, h), buf.raw, "raw", "BGRA", 0, 1)
    img.save(out)
    gdi32.DeleteObject(bmp); gdi32.DeleteDC(mem); user32.ReleaseDC(hwnd, hdc)
    print(f"saved {out} {w}x{h}")

if __name__ == "__main__":
    hwnd = find_hwnd("World View")
    if not hwnd:
        print("window not found"); sys.exit(1)
    capture(hwnd, sys.argv[1] if len(sys.argv) > 1 else "/tmp/wv-shot.png")
