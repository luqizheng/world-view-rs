import ctypes, time

u = ctypes.windll.user32
GDI = ctypes.windll.gdi32

def find_win():
    hits = []
    EnumProc = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)
    def cb(hwnd, lparam):
        n = u.GetWindowTextLengthW(hwnd)
        if n:
            buf = ctypes.create_unicode_buffer(n+1)
            u.GetWindowTextW(hwnd, buf, n+1)
            if 'World View' in buf.value:
                hits.append(hwnd)
        return True
    u.EnumWindows(EnumProc(cb), 0)
    return hits[0] if hits else None

class BMIH(ctypes.Structure):
    _fields_ = [("biSize", ctypes.c_uint32), ("biWidth", ctypes.c_long), ("biHeight", ctypes.c_long),
                ("biPlanes", ctypes.c_uint16), ("biBitCount", ctypes.c_uint16), ("biCompression", ctypes.c_uint32),
                ("biSizeImage", ctypes.c_uint32), ("biXPelsPerMeter", ctypes.c_long), ("biYPelsPerMeter", ctypes.c_long),
                ("biClrUsed", ctypes.c_uint32), ("biClrImportant", ctypes.c_uint32)]

def grab(hwnd):
    class RECT(ctypes.Structure):
        _fields_ = [("l", ctypes.c_long), ("t", ctypes.c_long), ("r", ctypes.c_long), ("b", ctypes.c_long)]
    rc = RECT(); u.GetClientRect(hwnd, ctypes.byref(rc))
    w, h = rc.r - rc.l, rc.b - rc.t
    if w <= 0 or h <= 0:
        return None, None, None
    hdc = u.GetDC(hwnd); mem = GDI.CreateCompatibleDC(hdc)
    bmi = BMIH(40, w, -h, 1, 32, 0, w*h*4, 0, 0, 0, 0)
    pbits = ctypes.c_void_p()
    GDI.SelectObject(mem, GDI.CreateDIBSection(hdc, ctypes.byref(bmi), 0, ctypes.byref(pbits), None, 0))
    u.PrintWindow(hwnd, mem, 1 | 2)
    row = w*4
    return w, h, ctypes.string_at(pbits, row*h)

def click(hwnd, x, y):
    lp = (int(y) << 16) | (int(x) & 0xFFFF)
    u.PostMessageW(hwnd, 0x0201, 1, lp); time.sleep(0.05)
    u.PostMessageW(hwnd, 0x0202, 0, lp); time.sleep(0.5)
