# ZIP042 caller adoption

PacketA adopts qualified [Zipfast042](https://github.com/xbmc4lyfe/zipfast/commit/042aea4f8643419193f59a05b14cadd3d8650454) from [PR104](https://github.com/xbmc4lyfe/zipfast/pull/104). Its tree is `45740b10adc9630d50114dc16d62abb1dc87a754`, with sole parent0030. The [bound source inventory](zipfast-042-caller-source.json) records all310 tracked SDK blobs, executable bits, caller source/lock hashes, independent fixture vector and retained command/log digests. Earlier inventories remain historical evidence.

The targeted offline update changes only the `zipfast` and `zip-container` workspace source records. PAR stays at `e7c419e77a5ad05f8d611c3c77adaa48d6fec477`; standalone Zipview stays at `2c3d73a06b29987cea1510e903e437a482076745`. Other parsed package records are identical. The source was already cached and verified against the immutable tree; no fetch or caller rebase occurred. These changes are local working-tree changes on published caller HEADac304, identified by source hashes, rather than an assertion that that published commit contains the adoption.

## Adapter behavior

`archive_native::extract_encrypted_rar5_whole` now takes Unix mode from the retained `Entry.mode`, applying the existing executable permission mask only after a successful read. It no longer opens another RarSession catalogue solely for mode metadata. Existing prewalk, volume readiness, repair holds, path checks, overwrite/skip, error and partial-output behavior remain.

The same selected decoder now uses `read_observed`. Payload and solid-prerequisite notices produce the incumbent `Unrar: Extracting from ...` line once per volume traversal index across selected members. Header-only scans, skipped members and unknown future roles do not produce that consumption line. Filename numbering is not used as the traversal index. Borrowed paths are used synchronously as labels, with no retained borrowed references. The adapter checks cancellation before and after notices and before sink writes, including cancellation requested by logging.

Two downstream effects are observable. Member progress can precede the first observed volume notice, so that volume can replace the member's progress label until another event updates it. The handlers parse the two lines independently; there is no ordering prerequisite for integrity or repair-hold decisions. DirectUnpack records only announced volumes in its archive list. A header-only volume left unrecorded can therefore cause the incumbent caller to choose ordinary full unpack, adding work rather than treating a metadata scan as consumed payload. These effects accompany the intended announcement correction; output/progress behavior is not claimed entirely unchanged.

Notices describe provisional logical source use. They do not authenticate a filesystem object or payload, prove that all bytes were read, release a repair hold, or acknowledge consumer end. Existing checked ZIP readers remain unchanged. No persistent PartialFile, late-damage/resume API, new coding format or b003 progressive adapter is adopted here.

## Functional evidence

Before adoption,19 archive and6 unpack tests passed. A new encrypted control passed on0030 with exact plaintext, Unix permission mask, wrong-password/checksum errors, skip preservation and the incumbent partial-output behavior. The deterministic RAR5 fixture has nonzero-numbered volumes, a directory-only first volume and encrypted payload in the second. Ciphertext is an independent Python hashlib PBKDF2 plus system OpenSSL AES256-CBC known answer; the writer frames headers with an independent bitwise CRC.

Three actual behavioral regressions then failed on0030: the header-only source was announced as consumed; skipping the payload still announced both sources; and cancellation just before reading could fall through to a missing-volume error. All pass after the observer wiring. The cancellation correction restores the existing `Aborted` contract before a later source open. Wrong-password, checksum-error and partial-output records remain byte-for-byte identical across old/new runs. No broader error-format change is intended.

Final locked/offline library checks with Rust1.100, jobs3, testthreads3, incremental0 and `-D warnings` passed:

| Filter | Passed |
| --- | ---: |
| `archive_native::` |26 |
| `unpack::` |6 |
| `parchecker::live_` |49 |
| `parlive::write_tests` |20 |

The seven new archive cases include real encrypted bytes/mode/error/cleanup, header-only and skipped sources, cancellation before an open and from a payload notice, deduplication across multiple members, and held/missing-volume fallback. Successful callbacks remain provisional until the incumbent integrity checks return success. The intentional old failures, fixtures and raw logs are retained; the inventory binds their exact digests and native exits.

These are library caller checks, not a fresh linked-daemon, three-platform, full compatibility or performance qualification. The held daemon/archive were unchanged. Physical ownership, destructive mutations, consumer quiescence and source-release ordering remain separately owned I2 work; this packet does not qualify merge or deployment.
