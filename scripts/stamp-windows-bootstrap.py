#!/usr/bin/env python3
"""Copy nus icon/version resources onto CEF's bootstrap before signing.

Keep the bootstrap's manifest and executable code intact. Load the resource
source as data only; no application entrypoint or dependency is executed.
"""
import ctypes as c
from ctypes import wintypes as w
import sys


def stamp(source, destination):
    kernel = c.WinDLL('kernel32', use_last_error=True)
    names = c.WINFUNCTYPE(w.BOOL, w.HMODULE, c.c_void_p, c.c_void_p, w.LPARAM)
    languages = c.WINFUNCTYPE(w.BOOL, w.HMODULE, c.c_void_p, c.c_void_p, w.WORD, w.LPARAM)
    signatures = {
        'LoadLibraryExW': ([w.LPCWSTR, w.HANDLE, w.DWORD], w.HMODULE),
        'FreeLibrary': ([w.HMODULE], w.BOOL),
        'EnumResourceNamesW': ([w.HMODULE, c.c_void_p, names, w.LPARAM], w.BOOL),
        'EnumResourceLanguagesW': ([w.HMODULE, c.c_void_p, c.c_void_p, languages, w.LPARAM], w.BOOL),
        'FindResourceExW': ([w.HMODULE, c.c_void_p, c.c_void_p, w.WORD], w.HANDLE),
        'LoadResource': ([w.HMODULE, w.HANDLE], w.HANDLE),
        'LockResource': ([w.HANDLE], c.c_void_p),
        'SizeofResource': ([w.HMODULE, w.HANDLE], w.DWORD),
        'BeginUpdateResourceW': ([w.LPCWSTR, w.BOOL], w.HANDLE),
        'UpdateResourceW': ([w.HANDLE, c.c_void_p, c.c_void_p, w.WORD, c.c_void_p, w.DWORD], w.BOOL),
        'EndUpdateResourceW': ([w.HANDLE, w.BOOL], w.BOOL),
    }
    for name, (args, result) in signatures.items():
        fn = getattr(kernel, name)
        fn.argtypes, fn.restype = args, result

    def checked(value):
        if not value:
            raise c.WinError(c.get_last_error())
        return value

    module = checked(kernel.LoadLibraryExW(source, None, 0x22))
    resources, errors = [], []

    @languages
    def read_language(handle, kind, name, language, unused):
        try:
            found = checked(kernel.FindResourceExW(handle, kind, name, language))
            size = checked(kernel.SizeofResource(handle, found))
            data = checked(kernel.LockResource(checked(kernel.LoadResource(handle, found))))
            resources.append((kind, c.wstring_at(name) if name > 0xffff else name,
                              language, c.string_at(data, size)))
            return True
        except Exception as error:
            errors.append(error)
            return False

    @names
    def read_name(handle, kind, name, unused):
        return kernel.EnumResourceLanguagesW(handle, kind, name, read_language, 0)

    try:
        for kind in (3, 14, 16):  # RT_ICON, RT_GROUP_ICON, RT_VERSION
            checked(kernel.EnumResourceNamesW(module, kind, read_name, 0))
        if errors:
            raise errors[0]
    finally:
        kernel.FreeLibrary(module)
    update = checked(kernel.BeginUpdateResourceW(destination, False))
    try:
        for kind, name, language, data in resources:
            resource_name = c.c_wchar_p(name) if isinstance(name, str) else c.c_void_p(name)
            buffer = c.create_string_buffer(data)
            checked(kernel.UpdateResourceW(update, kind, c.cast(resource_name, c.c_void_p),
                                           language, buffer, len(data)))
    except BaseException:
        kernel.EndUpdateResourceW(update, True)
        raise
    checked(kernel.EndUpdateResourceW(update, False))


if __name__ == '__main__':
    stamp(sys.argv[1], sys.argv[2])
