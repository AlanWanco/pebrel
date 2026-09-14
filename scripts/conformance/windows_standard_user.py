"""Run native conformance as an ordinary user on elevated Windows runners.

Use a restricted copy of the current user token, retaining the same desktop,
profile and environment. Product administrator isolation stays enabled.
"""

from __future__ import annotations

import ctypes
from ctypes import wintypes as wt
import os
import subprocess
import sys


class SidAndAttributes(ctypes.Structure):
    _fields_ = [("Sid", wt.LPVOID), ("Attributes", wt.DWORD)]


class StartupInfo(ctypes.Structure):
    _fields_ = [
        ("cb", wt.DWORD), ("lpReserved", wt.LPWSTR), ("lpDesktop", wt.LPWSTR),
        ("lpTitle", wt.LPWSTR), ("dwX", wt.DWORD), ("dwY", wt.DWORD),
        ("dwXSize", wt.DWORD), ("dwYSize", wt.DWORD), ("dwXCountChars", wt.DWORD),
        ("dwYCountChars", wt.DWORD), ("dwFillAttribute", wt.DWORD),
        ("dwFlags", wt.DWORD), ("wShowWindow", wt.WORD), ("cbReserved2", wt.WORD),
        ("lpReserved2", ctypes.POINTER(wt.BYTE)), ("hStdInput", wt.HANDLE),
        ("hStdOutput", wt.HANDLE), ("hStdError", wt.HANDLE),
    ]


class ProcessInfo(ctypes.Structure):
    _fields_ = [("hProcess", wt.HANDLE), ("hThread", wt.HANDLE),
                ("dwProcessId", wt.DWORD), ("dwThreadId", wt.DWORD)]


class WindowsTokens:
    def __init__(self):
        if os.name != "nt":
            raise RuntimeError("the ordinary-user launcher requires Windows")
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.security = ctypes.WinDLL("advapi32", use_last_error=True)
        signatures = [
            (self.kernel, "GetCurrentProcess", wt.HANDLE, []),
            (self.kernel, "GetStdHandle", wt.HANDLE, [wt.DWORD]),
            (self.kernel, "CloseHandle", wt.BOOL, [wt.HANDLE]),
            (self.kernel, "LocalFree", wt.HANDLE, [wt.HANDLE]),
            (self.kernel, "WaitForSingleObject", wt.DWORD, [wt.HANDLE, wt.DWORD]),
            (self.kernel, "GetExitCodeProcess", wt.BOOL,
             [wt.HANDLE, ctypes.POINTER(wt.DWORD)]),
            (self.security, "OpenProcessToken", wt.BOOL,
             [wt.HANDLE, wt.DWORD, ctypes.POINTER(wt.HANDLE)]),
            (self.security, "GetTokenInformation", wt.BOOL,
             [wt.HANDLE, wt.DWORD, wt.LPVOID, wt.DWORD, ctypes.POINTER(wt.DWORD)]),
            (self.security, "SetTokenInformation", wt.BOOL,
             [wt.HANDLE, wt.DWORD, wt.LPVOID, wt.DWORD]),
            (self.security, "GetLengthSid", wt.DWORD, [wt.LPVOID]),
            (self.security, "ConvertStringSidToSidW", wt.BOOL,
             [wt.LPCWSTR, ctypes.POINTER(wt.LPVOID)]),
            (self.security, "CreateRestrictedToken", wt.BOOL,
             [wt.HANDLE, wt.DWORD, wt.DWORD, wt.LPVOID, wt.DWORD, wt.LPVOID,
              wt.DWORD, wt.LPVOID, ctypes.POINTER(wt.HANDLE)]),
            (self.security, "CreateProcessAsUserW", wt.BOOL,
             [wt.HANDLE, wt.LPCWSTR, wt.LPWSTR, wt.LPVOID, wt.LPVOID, wt.BOOL,
              wt.DWORD, wt.LPVOID, wt.LPCWSTR, ctypes.POINTER(StartupInfo),
              ctypes.POINTER(ProcessInfo)]),
        ]
        for library, name, result, arguments in signatures:
            function = getattr(library, name)
            function.restype, function.argtypes = result, arguments

    @staticmethod
    def require(success):
        if not success:
            raise ctypes.WinError(ctypes.get_last_error())

    def current_token(self):
        token = wt.HANDLE()
        # QUERY | DUPLICATE | ASSIGN_PRIMARY, for a restricted copy of ourselves.
        self.require(self.security.OpenProcessToken(
            self.kernel.GetCurrentProcess(), 0x000B, ctypes.byref(token)))
        return token

    def elevated(self, token):
        value, size = wt.DWORD(), wt.DWORD()
        self.require(self.security.GetTokenInformation(
            token, 20, ctypes.byref(value), ctypes.sizeof(value), ctypes.byref(size)))
        return bool(value.value)

    def restrict(self, token):
        restricted, sid = wt.HANDLE(), wt.LPVOID()
        # LUA_TOKEN removes administrator membership and elevated privileges.
        self.require(self.security.CreateRestrictedToken(
            token, 0x4, 0, None, 0, None, 0, None, ctypes.byref(restricted)))
        try:
            self.require(self.security.ConvertStringSidToSidW(
                "S-1-16-8192", ctypes.byref(sid)))  # Medium integrity.
            label = SidAndAttributes(sid, 0x20)  # SE_GROUP_INTEGRITY.
            self.require(self.security.SetTokenInformation(
                restricted, 25, ctypes.byref(label),
                ctypes.sizeof(label) + self.security.GetLengthSid(sid)))
            if self.elevated(restricted):
                raise RuntimeError("restricted token is still elevated; refusing conformance")
            return restricted
        except BaseException:
            self.kernel.CloseHandle(restricted)
            raise
        finally:
            if sid:
                self.kernel.LocalFree(sid)

    def run(self, token, arguments):
        command = ctypes.create_unicode_buffer(subprocess.list2cmdline(arguments))
        startup = StartupInfo()
        startup.cb, startup.dwFlags = ctypes.sizeof(startup), 0x100
        startup.hStdInput = self.kernel.GetStdHandle(-10)
        startup.hStdOutput = self.kernel.GetStdHandle(-11)
        startup.hStdError = self.kernel.GetStdHandle(-12)
        process = ProcessInfo()
        self.require(self.security.CreateProcessAsUserW(
            token, arguments[0], command, None, None, True, 0, None, os.getcwd(),
            ctypes.byref(startup), ctypes.byref(process)))
        try:
            if self.kernel.WaitForSingleObject(process.hProcess, 0xFFFFFFFF) != 0:
                raise ctypes.WinError(ctypes.get_last_error())
            code = wt.DWORD()
            self.require(self.kernel.GetExitCodeProcess(process.hProcess, ctypes.byref(code)))
            return code.value
        finally:
            self.kernel.CloseHandle(process.hThread)
            self.kernel.CloseHandle(process.hProcess)


def run_python(arguments):
    api = WindowsTokens()
    token, restricted = api.current_token(), None
    try:
        command = [sys.executable, *arguments]
        if not api.elevated(token):
            return subprocess.run(command, check=False).returncode
        restricted = api.restrict(token)
        print("Running native conformance with a verified non-elevated token", flush=True)
        return api.run(restricted, command)
    finally:
        if restricted:
            api.kernel.CloseHandle(restricted)
        api.kernel.CloseHandle(token)


if __name__ == "__main__":
    if len(sys.argv) < 2:
        raise SystemExit("usage: windows_standard_user.py <python script> [arguments...]")
    raise SystemExit(run_python(sys.argv[1:]))
