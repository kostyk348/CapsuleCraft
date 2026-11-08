#!/usr/bin/env python3
"""Launch-readiness gate for the Android APK (pure python, no SDK needed).

For every packaged native library (lib/arm64-v8a/*.so) the script checks:

1. every DT_NEEDED entry is either an Android system library (present on
   the device) or is itself packaged in the APK — a missing one kills the
   app at dlopen time with an instant "app has stopped" (v2.12.3 shipped
   without libc++_shared.so, which cpal/oboe pull in, and crashed on
   launch on every device);
2. PT_LOAD alignment is 16384 bytes — Android 15+ 16 KB-page devices
   refuse to load 4 KB-aligned libraries;
3. the NativeActivity entry point (ANativeActivity_onCreate) is exported
   by the main library.

Usage: python3 scripts/check_apk_native_deps.py <app.apk> [<app2.apk> ...]
Exit codes: 0 = all APKs launch-ready, 1 = a problem was found.
"""
import struct
import sys
import zipfile

# Libraries every Android device provides out of the box (NDK/bionic +
# framework). Anything else must be packaged in the APK.
SYSTEM_LIBS = {
    "libc.so", "libdl.so", "libm.so", "liblog.so", "libandroid.so",
    "libOpenSLES.so", "libaaudio.so", "libEGL.so", "libGLESv2.so",
    "libGLESv3.so", "libvulkan.so", "libz.so", "libnativewindow.so",
    "libsync.so", "libstdc++.so", "libmediandk.so", "libcamera2ndk.so",
    "libjnigraphics.so", "libwilhelm.so", "libunwind.so",
}

MIN_PAGE_ALIGN = 16384


def dyn_needed(data: bytes):
    """DT_NEEDED entries of an ELF64 shared object."""
    e_phoff, = struct.unpack_from("<Q", data, 32)
    e_phentsize, e_phnum = struct.unpack_from("<HH", data, 54)
    dyn = None
    max_align = 0
    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        (p_type, _flags, p_offset, _vaddr, _paddr, p_filesz,
         _memsz, p_align) = struct.unpack_from("<IIQQQQQQ", data, off)
        if p_type == 1:  # PT_LOAD
            max_align = max(max_align, p_align)
        elif p_type == 2:  # PT_DYNAMIC
            dyn = (p_offset, p_filesz)
    if dyn is None:
        return [], max_align
    off, size = dyn
    strtab = None
    tags = []
    for i in range(size // 16):
        d_tag, d_val = struct.unpack_from("<qQ", data, off + i * 16)
        if d_tag == 0:
            break
        tags.append((d_tag, d_val))
        if d_tag == 5:  # DT_STRTAB
            strtab = d_val
    needed = []
    for d_tag, d_val in tags:
        if d_tag == 1 and strtab is not None:  # DT_NEEDED
            end = data.index(b"\0", strtab + d_val)
            needed.append(data[strtab + d_val:end].decode())
    return needed, max_align


def defined_symbols(data: bytes):
    """Symbols defined (not imported) by the ELF, via .dynsym/.dynstr."""
    e_shoff, = struct.unpack_from("<Q", data, 40)
    e_shentsize, e_shnum, e_shstrndx = struct.unpack_from("<HHH", data, 58)
    sections = []
    for i in range(e_shnum):
        off = e_shoff + i * e_shentsize
        (name, _typ, _flags, _addr, offset, size, link,
         _info, _align, entsize) = struct.unpack_from("<IIQQQQIIQQ", data, off)
        sections.append(dict(name=name, offset=offset, size=size, link=link,
                             entsize=entsize))
    shstr = sections[e_shstrndx]
    name_at = shstr["offset"]

    def secname(n):
        end = data.index(b"\0", name_at + n)
        return data[name_at + n:end].decode()

    syms = set()
    for s in sections:
        if secname(s["name"]) != ".dynsym":
            continue
        dynstr = sections[s["link"]]
        for i in range(s["size"] // s["entsize"]):
            off = s["offset"] + i * s["entsize"]
            st_name, _info, _other, st_shndx, _value, _size = struct.unpack_from(
                "<IBBHQQ", data, off)
            if st_shndx == 0 or st_name == 0:
                continue
            end = data.index(b"\0", dynstr["offset"] + st_name)
            syms.add(data[dynstr["offset"] + st_name:end].decode())
    return syms


def check_apk(path: str) -> bool:
    ok = True
    with zipfile.ZipFile(path) as z:
        libs = [n for n in z.namelist() if n.startswith("lib/armeabi")
                or n.startswith("lib/arm64") or n.startswith("lib/x86")]
        packaged = {n.rsplit("/", 1)[-1] for n in libs}
        print(f"== {path}: {len(libs)} native libs")
        main_lib = None
        for name in libs:
            data = z.read(name)
            short = name.rsplit("/", 1)[-1]
            needed, align = dyn_needed(data)
            if align < MIN_PAGE_ALIGN:
                print(f"  FAIL {short}: PT_LOAD alignment {align} < {MIN_PAGE_ALIGN} "
                      "(16 KB-page devices refuse to load it)")
                ok = False
            for dep in needed:
                if dep not in SYSTEM_LIBS and dep not in packaged:
                    print(f"  FAIL {short}: needs {dep} — not in the APK and not a "
                          "system library (dlopen fails at launch)")
                    ok = False
            print(f"  {short}: align={align}, deps={', '.join(needed) or '—'}")
            if defined_symbols_check(data, "ANativeActivity_onCreate"):
                main_lib = short
        if main_lib is None:
            print("  FAIL: no packaged library exports ANativeActivity_onCreate "
                  "(NativeActivity has nothing to call — instant crash)")
            ok = False
        else:
            print(f"  ANativeActivity_onCreate: exported by {main_lib}")
    return ok


def defined_symbols_check(data: bytes, symbol: str) -> bool:
    return symbol in defined_symbols(data)


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    ok = all(check_apk(p) for p in argv[1:])
    print("LAUNCH-READY" if ok else "BROKEN — see FAIL lines above")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
