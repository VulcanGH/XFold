# XFold 0.3.13 local release candidate

The installer and its matching standalone source archive were assembled from
the current working tree with the pinned tools and cached dependencies.

| Artifact | SHA-256 |
|---|---|
| `XFold-0.3.13-windows-x64-setup.exe` | `b2a231cdf34e308fe7ec47d73dc2e3637cd71f314e4605c0e24ae5eb39bcf8ca` |
| `XFold-0.3.13-sources.zip` | `ba74eddd35a680e0cc2974cc93368f5db1b2b0361b9526c920524400a96d3ca7` |

- On Windows 11 x64 build 26300, Setup installed 0.3.8 and upgraded it to
  0.3.13 in a fresh temporary directory on the system drive. All 752 payload
  hashes matched, including the embedded source ZIP. Setup refused to
  replace an open installed copy and uninstall preserved state and exports.
  The initial install attempt in a temporary directory on the ReFS workspace
  drive returned Setup exit code 2. System-drive setup passed.
- The installed loopback UI and raw, standalone MKA, and full-MKV exports
  passed with only Windows System32 on PATH. The browser suite also passed
  queue, cancellation, settings, reload, and HTTP boundary checks.
- The extracted XFold source passed a locked, offline Cargo check with an
  empty Cargo cache and the pinned Windows GNU toolchain settings.
- A browser check exercised the update controls using simulated release and
  restart responses. The PowerShell updater rejected an unsigned file,
  pre-releases, same-version releases, and an unexpected download URL.

The candidate is unsigned. Live signed update installation and clean-VM
installation are pending. See the [release guide](../../packaging/RELEASE.md)
and [tool notices](../TOOL_NOTICES.md) for remaining publication work.
